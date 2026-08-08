//! The live connection — the only part of the client that does I/O.
//!
//! Everything decidable already lives in [`crate::wire`] (framing, JSON-RPC
//! shapes) and [`crate::pending`] (correlation, and the three ways it hangs).
//! What remains here is a read pump, a write half, and the wiring that fails
//! every waiter when the far side goes away.
//!
//! # Generic over the streams, on purpose
//!
//! [`Connection::new`] takes any `AsyncRead` + `AsyncWrite` rather than a
//! `ChildStdout`/`ChildStdin` pair, so the whole protocol loop can be driven
//! over `tokio::io::duplex` in a unit test — a real conversation, real framing,
//! real correlation, no process. [`Connection::spawn`] is the thin adapter that
//! puts a child process on the other end.
//!
//! That split matters because of what this session already learned twice: a
//! green test over a mocked *decision* says nothing about a malformed *call*.
//! Here the call IS the thing under test.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::pending::{Abandoned, Pending, Routed};
use crate::wire::{Decoded, Incoming, Notification, Request, RequestId, classify, decode, encode};
use crate::{LspError, Result, ServerConfig};

/// A reply, or the reason there will never be one.
pub type Reply = std::result::Result<serde_json::Value, ReplyError>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplyError {
    /// The server answered with a JSON-RPC error object.
    #[error("server error: {0}")]
    Server(String),
    /// No reply is coming — see [`Abandoned`].
    #[error(transparent)]
    Abandoned(#[from] Abandoned),
}

type Waiters = Arc<Mutex<Pending<oneshot::Sender<Reply>>>>;

/// One live LSP conversation.
pub struct Connection {
    waiters: Waiters,
    tx: Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>,
    /// Server-initiated traffic — `publishDiagnostics` above all. Bounded: a
    /// server that floods must not grow the editor's memory without limit.
    pub inbox: mpsc::Receiver<Incoming>,
    /// Owns the child so dropping the `Connection` kills the server. Never
    /// read — the ownership IS the behaviour, which is why it keeps the
    /// underscore and an explicit allow rather than being "used" for show.
    #[allow(dead_code)]
    child: Option<tokio::process::Child>,
}

impl Connection {
    /// Drive a conversation over any pair of streams.
    ///
    /// Spawns the read pump. When `reader` reaches EOF — the far side closed,
    /// or a child exited — **every parked waiter is failed** rather than left
    /// to hang, which is the single most important thing this function does.
    pub fn new<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let waiters: Waiters = Arc::new(Mutex::new(Pending::new()));
        let (inbox_tx, inbox) = mpsc::channel(256);
        tokio::spawn(pump(reader, Arc::clone(&waiters), inbox_tx));
        Self {
            waiters,
            tx: Arc::new(Mutex::new(Box::new(writer))),
            inbox,
            child: None,
        }
    }

    /// Launch a server from its config and talk to it over stdio.
    ///
    /// # Errors
    /// The command could not be spawned, or the child gave us no stdio.
    pub fn spawn(cfg: &ServerConfig, root: &Path) -> Result<Self> {
        let mut child = tokio::process::Command::new(&cfg.command)
            .args(&cfg.args)
            .current_dir(root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // INHERITED, not piped. An LSP server logs to stderr, and a piped
            // stderr nobody drains fills its pipe buffer and blocks the server
            // mid-write — a hang whose cause is invisible from the client side.
            .stderr(std::process::Stdio::inherit())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::Io(std::io::Error::other("child produced no stdout")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::Io(std::io::Error::other("child produced no stdin")))?;
        let mut c = Self::new(stdout, stdin);
        c.child = Some(child);
        Ok(c)
    }

    /// Send a request and await its reply.
    ///
    /// # Errors
    /// The write failed, or the server went away before replying.
    pub async fn request(&self, method: &str, params: serde_json::Value) -> Result<Reply> {
        let (id, rx) = {
            let mut w = self.waiters.lock().await;
            let id = w.next_id();
            let (tx, rx) = oneshot::channel();
            // A monotone id cannot collide, but honour the eviction contract
            // anyway rather than assume — the assumption is what rots.
            if let Some(evicted) = w.insert(&RequestId::Number(id), tx) {
                let _ = evicted.send(Err(Abandoned::IdReused.into()));
            }
            (id, rx)
        };
        self.send(&encode(&serde_json::to_vec(&Request::new(
            id, method, params,
        ))?))
        .await?;
        // A dropped sender means the pump exited without routing us, which is
        // the server-gone path.
        Ok(rx.await.unwrap_or(Err(Abandoned::ServerGone.into())))
    }

    /// Fire a notification. There is no reply and no way to await one.
    ///
    /// # Errors
    /// The write failed.
    pub async fn notify(&self, method: &str, params: serde_json::Value) -> Result<()> {
        self.send(&encode(&serde_json::to_vec(&Notification::new(
            method, params,
        ))?))
        .await
    }

    async fn send(&self, bytes: &[u8]) -> Result<()> {
        let mut tx = self.tx.lock().await;
        tx.write_all(bytes).await?;
        tx.flush().await?;
        Ok(())
    }
}

/// Read frames until EOF, routing each one. On exit, fail every waiter.
async fn pump<R: AsyncRead + Unpin>(
    mut reader: R,
    waiters: Waiters,
    inbox: mpsc::Sender<Incoming>,
) {
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        // Drain every COMPLETE frame the buffer now holds — a single read can
        // carry several, and stopping after one would stall the rest until the
        // next byte happens to arrive.
        loop {
            match decode(&buf) {
                Ok(Decoded::Message { body, consumed }) => {
                    buf.drain(..consumed);
                    route(&body, &waiters, &inbox).await;
                }
                Ok(Decoded::Incomplete) => break,
                Err(e) => {
                    // A desynchronised stream cannot be resynchronised: the
                    // frame boundaries are lost. Ending the conversation fails
                    // every waiter, which beats reading garbage forever.
                    tracing::error!(error = %e, "lsp: malformed frame, ending the conversation");
                    buf.clear();
                    let stranded = waiters.lock().await.drain_all();
                    for w in stranded {
                        let _ = w.send(Err(Abandoned::ServerGone.into()));
                    }
                    return;
                }
            }
        }
    }
    // EOF. THE LOAD-BEARING LINE: without it every in-flight request hangs
    // forever and the editor silently stops answering.
    let stranded = waiters.lock().await.drain_all();
    tracing::debug!(
        stranded = stranded.len(),
        "lsp: server closed; failing in-flight requests"
    );
    for w in stranded {
        let _ = w.send(Err(Abandoned::ServerGone.into()));
    }
}

async fn route(body: &[u8], waiters: &Waiters, inbox: &mpsc::Sender<Incoming>) {
    let Ok(msg) = classify(body) else {
        tracing::warn!("lsp: unclassifiable message, ignoring");
        return;
    };
    match msg {
        Incoming::Response { id, result, error } => {
            let routed = waiters.lock().await.route(&id);
            match routed {
                Routed::Delivered(tx) => {
                    let reply = error.map_or_else(
                        || Ok(result.unwrap_or(serde_json::Value::Null)),
                        |e| Err(ReplyError::Server(e.to_string())),
                    );
                    let _ = tx.send(reply);
                }
                // Dropped deliberately: a late reply must never be handed to
                // whoever happens to be waiting.
                Routed::Unmatched => tracing::debug!(?id, "lsp: reply for an unknown id, dropped"),
            }
        }
        other => {
            // Server requests and notifications go to the editor. `try_send`
            // rather than `send`: a full inbox must drop the oldest news, never
            // block the read pump, because a blocked pump stops routing REPLIES
            // and turns a slow consumer into a client-wide hang.
            if inbox.try_send(other).is_err() {
                tracing::warn!("lsp: inbox full, dropping a server message");
            }
        }
    }
}

/// LSP `initialize` params, minimal but honest about what we support.
#[must_use]
pub fn initialize_params(root: &Path) -> serde_json::Value {
    let uri = format!("file://{}", root.display());
    serde_json::json!({
        "processId": std::process::id(),
        "rootUri": uri,
        "capabilities": {
            "textDocument": {
                "synchronization": { "didSave": true },
                "publishDiagnostics": { "relatedInformation": false },
                "hover": { "contentFormat": ["plaintext"] },
            }
        },
        "workspaceFolders": [{ "uri": uri, "name": "root" }],
    })
}

/// Per-language connections, opened lazily.
#[derive(Default)]
pub struct Connections {
    open: HashMap<String, Connection>,
}

impl Connections {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn get(&mut self, language: &str) -> Option<&mut Connection> {
        self.open.get_mut(language)
    }
    pub fn insert(&mut self, language: String, conn: Connection) {
        self.open.insert(language, conn);
    }
    #[must_use]
    pub fn languages(&self) -> Vec<&str> {
        self.open.keys().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Connection, ReplyError};
    use crate::pending::Abandoned;
    use crate::wire::{Decoded, Incoming, decode, encode};
    use tokio::io::AsyncWriteExt;

    /// Drive a real conversation over in-memory pipes: real framing, real
    /// correlation, real await — and no process, so every case below is
    /// deterministic.
    fn pair() -> (Connection, Server) {
        let (client_side, server_side) = tokio::io::duplex(64 * 1024);
        let (r, w) = tokio::io::split(client_side);
        (Connection::new(r, w), Server::new(server_side))
    }

    /// The server side of the conversation, with a PERSISTENT buffer.
    ///
    /// The buffer has to outlive the call. A helper that reads into a fresh
    /// local buffer, decodes the first frame and returns would discard whatever
    /// else that read pulled in — and since one read routinely carries several
    /// frames, the next call then blocks forever on bytes it already consumed.
    /// That is exactly the mistake `decode`'s `consumed` contract exists to
    /// prevent, and the first version of this helper made it: the batched-reply
    /// test hung rather than failed.
    struct Server {
        io: tokio::io::DuplexStream,
        buf: Vec<u8>,
    }

    impl Server {
        fn new(io: tokio::io::DuplexStream) -> Self {
            Self { io, buf: Vec::new() }
        }

        async fn read_one(&mut self) -> serde_json::Value {
            use tokio::io::AsyncReadExt;
            let mut chunk = [0u8; 4096];
            loop {
                // Drain what is already buffered BEFORE reading more.
                if let Ok(Decoded::Message { body, consumed }) = decode(&self.buf) {
                    self.buf.drain(..consumed);
                    return serde_json::from_slice(&body).unwrap();
                }
                let n = self.io.read(&mut chunk).await.unwrap();
                assert!(n > 0, "server side closed while a frame was expected");
                self.buf.extend_from_slice(&chunk[..n]);
            }
        }

        async fn write(&mut self, v: &serde_json::Value) {
            self.io.write_all(&encode(&serde_json::to_vec(v).unwrap())).await.unwrap();
        }

        async fn write_raw(&mut self, b: &[u8]) {
            self.io.write_all(b).await.unwrap();
        }
    }

    #[tokio::test]
    async fn a_request_gets_its_reply() {
        let (conn, mut server) = pair();
        let call =
            tokio::spawn(
                async move { conn.request("initialize", serde_json::json!({"x":1})).await },
            );

        let got = server.read_one().await;
        assert_eq!(got["method"], "initialize");
        let id = got["id"].clone();
        let reply = serde_json::json!({"jsonrpc":"2.0","id":id,"result":{"ok":true}});
        server.write(&reply).await;

        let out = call.await.unwrap().unwrap();
        assert_eq!(out.unwrap()["ok"], true);
    }

    /// **The hang that matters, end to end.** The server closes with a request
    /// in flight; the caller must get an error rather than wait forever.
    #[tokio::test]
    async fn a_server_that_closes_fails_the_in_flight_request() {
        let (conn, mut server) = pair();
        let call =
            tokio::spawn(async move { conn.request("hover", serde_json::Value::Null).await });
        let _ = server.read_one().await;
        drop(server); // the server exits mid-request

        let out = tokio::time::timeout(std::time::Duration::from_secs(5), call)
            .await
            .expect("must not hang")
            .unwrap()
            .unwrap();
        assert_eq!(out, Err(ReplyError::Abandoned(Abandoned::ServerGone)));
    }

    /// A JSON-RPC error object is a REPLY, not a hang and not a panic.
    #[tokio::test]
    async fn a_server_error_object_comes_back_as_an_error_reply() {
        let (conn, mut server) = pair();
        let call = tokio::spawn(async move { conn.request("nope", serde_json::Value::Null).await });
        let got = server.read_one().await;
        let reply = serde_json::json!({
            "jsonrpc":"2.0","id":got["id"],"error":{"code":-32601,"message":"unknown"}
        });
        server.write(&reply).await;

        let out = call.await.unwrap().unwrap();
        assert!(matches!(out, Err(ReplyError::Server(_))));
    }

    /// Diagnostics are the whole point of the client. A notification must reach
    /// the editor's inbox rather than be mistaken for a reply.
    #[tokio::test]
    async fn a_notification_reaches_the_inbox() {
        let (mut conn, mut server) = pair();
        let note = serde_json::json!({
            "jsonrpc":"2.0",
            "method":"textDocument/publishDiagnostics",
            "params":{"uri":"file:///x.nix","diagnostics":[]}
        });
        server.write(&note).await;

        let got = tokio::time::timeout(std::time::Duration::from_secs(5), conn.inbox.recv())
            .await
            .expect("must not hang")
            .expect("a message");
        assert!(matches!(got, Incoming::Notification { method, .. }
            if method == "textDocument/publishDiagnostics"));
    }

    /// Two replies in ONE write must both land. A pump that handled one frame
    /// per read would strand the second until more bytes happened to arrive.
    #[tokio::test]
    async fn two_replies_in_one_write_both_get_routed() {
        let (conn, mut server) = pair();
        let a = tokio::spawn({
            let c = std::sync::Arc::new(conn);
            let c2 = std::sync::Arc::clone(&c);
            async move {
                let first = c.request("one", serde_json::Value::Null);
                let second = c2.request("two", serde_json::Value::Null);
                tokio::join!(first, second)
            }
        });

        let m1 = server.read_one().await;
        let m2 = server.read_one().await;
        let mut batch = encode(
            &serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":m1["id"],"result":1}))
                .unwrap(),
        );
        batch.extend_from_slice(&encode(
            &serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":m2["id"],"result":2}))
                .unwrap(),
        ));
        server.write_raw(&batch).await;

        let (r1, r2) = tokio::time::timeout(std::time::Duration::from_secs(5), a)
            .await
            .expect("must not hang")
            .unwrap();
        assert!(
            r1.is_ok() && r2.is_ok(),
            "both halves of a batched write must route"
        );
    }

    /// A reply for an id nobody awaits must be dropped without disturbing the
    /// real waiter — the misrouting case, proven over the live pump.
    #[tokio::test]
    async fn a_stray_reply_does_not_disturb_a_real_waiter() {
        let (conn, mut server) = pair();
        let call = tokio::spawn(async move { conn.request("real", serde_json::Value::Null).await });
        let got = server.read_one().await;

        let stray = serde_json::json!({"jsonrpc":"2.0","id":9999,"result":"nobody asked"});
        server.write(&stray).await;
        let real = serde_json::json!({"jsonrpc":"2.0","id":got["id"],"result":"mine"});
        server.write(&real).await;

        let out = tokio::time::timeout(std::time::Duration::from_secs(5), call)
            .await
            .expect("must not hang")
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(out, "mine");
    }
}
