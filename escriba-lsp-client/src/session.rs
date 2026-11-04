use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use escriba_core::{CompletionEntry, InlayHint, Position, Range, ServerState, TextChange};
use escriba_madoguchi::errand::{
    Document, Errand, Freight, Parcel, Question, Runner, SessionFreight,
};
use escriba_madoguchi::{ErrandId, Negai};
use escriba_shirube::Anchor;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use zahyou::CharPosition;

use crate::conn::{Connection, Reply, ReplyError, ServerCaps};
use crate::findings::{PublishDiagnostics, path_of, to_findings};
use crate::runner::{decode_tokens, list_for, root_for, tokens_params, uri_of};
use crate::tokens::Legend;
use crate::wire::Incoming;
use crate::{Lines, LspError, ServerConfig, ServerRegistry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPolicy {
    pub request_timeout: Duration,
    pub handshake_timeout: Duration,
    pub hang_after: Duration,
    pub backoff_base: Duration,
    pub backoff_cap: Duration,
    pub restart_limit: u32,
    pub healthy_after: Duration,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(20),
            hang_after: Duration::from_secs(20),
            backoff_base: Duration::from_millis(250),
            backoff_cap: Duration::from_secs(8),
            restart_limit: 5,
            healthy_after: Duration::from_secs(30),
        }
    }
}

impl SessionPolicy {
    #[must_use]
    pub fn backoff(&self, attempt: u32) -> Duration {
        let doubled = self
            .backoff_base
            .saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1)));
        doubled.min(self.backoff_cap)
    }
}

struct Posted {
    errand: Errand,
    reply: Sender<Parcel>,
}

pub struct SessionRunner {
    registry: ServerRegistry,
    policy: SessionPolicy,
    hub: OnceLock<mpsc::UnboundedSender<Posted>>,
}

impl SessionRunner {
    #[must_use]
    pub fn new(registry: ServerRegistry) -> Self {
        Self {
            registry,
            policy: SessionPolicy::default(),
            hub: OnceLock::new(),
        }
    }

    #[must_use]
    pub fn with_policy(mut self, policy: SessionPolicy) -> Self {
        self.policy = policy;
        self
    }
}

impl Runner for SessionRunner {
    fn start(&self, errand: Errand, _cancel: Arc<AtomicBool>, reply: Sender<Parcel>) {
        let hub = self
            .hub
            .get_or_init(|| spawn_hub(self.registry.clone(), self.policy.clone()));
        if let Err(mpsc::error::SendError(lost)) = hub.send(Posted { errand, reply }) {
            let _ = lost.reply.send(Parcel {
                id: lost.errand.id,
                slip: Negai::Message("lsp: the session thread is gone".to_string()),
            });
        }
    }
}

fn spawn_hub(registry: ServerRegistry, policy: SessionPolicy) -> mpsc::UnboundedSender<Posted> {
    let (tx, rx) = mpsc::unbounded_channel();
    let _ = std::thread::Builder::new()
        .name("escriba-lsp-session".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(hub(registry, policy, rx));
        });
    tx
}

struct Work {
    freight: SessionFreight,
    anchor: Anchor,
}

type SessionKey = (String, PathBuf);

async fn hub(
    registry: ServerRegistry,
    policy: SessionPolicy,
    mut posted: mpsc::UnboundedReceiver<Posted>,
) {
    let mut sessions: HashMap<SessionKey, mpsc::UnboundedSender<Work>> = HashMap::new();
    let mut tasks = tokio::task::JoinSet::new();
    while let Some(Posted { errand, reply }) = posted.recv().await {
        let id = errand.id;
        let anchor = errand.anchor.into_anchor();
        let Freight::Session(freight) = errand.freight else {
            let _ = reply.send(Parcel {
                id,
                slip: Negai::Message("lsp session received the wrong freight".to_string()),
            });
            continue;
        };
        let Some(document) = document_of(&freight) else {
            for session in sessions.values() {
                let _ = session.send(Work {
                    freight: SessionFreight::Restart,
                    anchor: anchor.clone(),
                });
            }
            continue;
        };
        let Some((cfg, root)) = route(&registry, document) else {
            if let SessionFreight::Ask { question, .. } = &freight {
                if question.is_explicit() {
                    let _ = reply.send(Parcel {
                        id,
                        slip: Negai::Message(
                            "lsp: no language server is declared for this file".to_string(),
                        ),
                    });
                }
            }
            continue;
        };
        let key = (cfg.command.clone(), root.clone());
        let session = sessions.entry(key).or_insert_with(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            let sink = Sink {
                reply: reply.clone(),
                id,
                languages: languages_of(&registry, &cfg),
                command: cfg.command.clone(),
                root: root.clone(),
            };
            tasks.spawn(run_session(cfg, root, policy.clone(), rx, sink));
            tx
        });
        let _ = session.send(Work { freight, anchor });
    }
    sessions.clear();
    let drain = async { while tasks.join_next().await.is_some() {} };
    let _ = tokio::time::timeout(Duration::from_secs(1), drain).await;
}

const fn document_of(freight: &SessionFreight) -> Option<&Document> {
    match freight {
        SessionFreight::Sync { document, .. }
        | SessionFreight::Saved { document }
        | SessionFreight::Close { document }
        | SessionFreight::Ask { document, .. } => Some(document),
        SessionFreight::Restart => None,
    }
}

fn route(registry: &ServerRegistry, document: &Document) -> Option<(ServerConfig, PathBuf)> {
    let language = document
        .language
        .clone()
        .or_else(|| crate::runner::language_of(&document.path).map(str::to_owned))?;
    let cfg = registry.for_language(&language)?.clone();
    let path = std::path::absolute(&document.path).unwrap_or_else(|_| document.path.clone());
    let anchor_dir = path
        .parent()
        .map_or_else(|| path.clone(), Path::to_path_buf);
    let root = root_for(&anchor_dir, &cfg.root_markers);
    Some((cfg, root))
}

fn languages_of(registry: &ServerRegistry, cfg: &ServerConfig) -> Vec<String> {
    registry
        .all()
        .iter()
        .filter(|s| s.command == cfg.command)
        .map(|s| s.language.clone())
        .collect()
}

#[derive(Clone)]
struct Sink {
    reply: Sender<Parcel>,
    id: ErrandId,
    languages: Vec<String>,
    command: String,
    root: PathBuf,
}

impl Sink {
    fn post(&self, slip: Negai) {
        let _ = self.reply.send(Parcel { id: self.id, slip });
    }

    fn sealed(&self, anchor: &Anchor, then: Negai) {
        self.post(Negai::ErrandReply {
            anchor: anchor.clone(),
            then: Box::new(then),
        });
    }

    fn status(&self, state: ServerState) {
        self.post(Negai::ServerStatus(escriba_core::ServerStatus {
            languages: self.languages.clone(),
            command: self.command.clone(),
            root: self.root.clone(),
            state,
        }));
    }

    fn say(&self, what: &str) {
        let mut m = String::from("lsp: ");
        m.push_str(&self.command);
        m.push(' ');
        m.push_str(what);
        self.post(Negai::Message(m));
    }
}

struct Doc {
    uri: String,
    buffer: escriba_core::BufferId,
    language: String,
    path: PathBuf,
    text: Arc<str>,
    version: i32,
    anchor: Anchor,
    open: bool,
}

#[derive(Default)]
struct Docs {
    by_uri: HashMap<String, Doc>,
}

impl Docs {
    fn update(
        &mut self,
        document: &Document,
        text: String,
        anchor: Anchor,
        fallback: &str,
    ) -> String {
        let uri = uri_of(&document.path);
        let language = document
            .language
            .clone()
            .unwrap_or_else(|| fallback.to_string());
        let entry = self.by_uri.entry(uri.clone()).or_insert_with(|| Doc {
            uri: uri.clone(),
            buffer: document.buffer,
            language,
            path: document.path.clone(),
            text: Arc::from(""),
            version: 0,
            anchor: anchor.clone(),
            open: false,
        });
        entry.text = Arc::from(text);
        entry.version += 1;
        entry.anchor = anchor;
        uri
    }

    fn forget_server(&mut self) {
        for doc in self.by_uri.values_mut() {
            doc.open = false;
        }
    }
}

enum End {
    Stopped,
    RestartAsked,
    Missing,
    Failed(String),
}

enum Idled {
    Stopped,
    RestartAsked,
    Elapsed,
}

async fn run_session(
    cfg: ServerConfig,
    root: PathBuf,
    policy: SessionPolicy,
    mut work: mpsc::UnboundedReceiver<Work>,
    sink: Sink,
) {
    let mut docs = Docs::default();
    let mut attempts = 0u32;
    let mut generation = 0u32;
    sink.status(ServerState::Starting);
    loop {
        generation += 1;
        let began = Instant::now();
        let end = incarnate(
            &cfg, &root, &policy, &mut work, &mut docs, &sink, generation,
        )
        .await;
        docs.forget_server();
        let reason = match end {
            End::Stopped => return,
            End::RestartAsked => {
                attempts = 0;
                sink.status(ServerState::Restarting {
                    attempt: 0,
                    limit: policy.restart_limit,
                    after_ms: 0,
                    reason: "asked".to_string(),
                });
                continue;
            }
            End::Missing => {
                sink.status(ServerState::Missing {
                    command: cfg.command.clone(),
                });
                match idle(&mut work, &mut docs, &sink, &cfg, None).await {
                    Idled::Stopped => return,
                    Idled::RestartAsked | Idled::Elapsed => {
                        attempts = 0;
                        continue;
                    }
                }
            }
            End::Failed(reason) => reason,
        };
        if began.elapsed() >= policy.healthy_after {
            attempts = 0;
        }
        attempts += 1;
        if attempts > policy.restart_limit {
            sink.status(ServerState::Dead { reason });
            match idle(&mut work, &mut docs, &sink, &cfg, None).await {
                Idled::Stopped => return,
                Idled::RestartAsked | Idled::Elapsed => {
                    attempts = 0;
                    continue;
                }
            }
        }
        let delay = policy.backoff(attempts);
        sink.status(ServerState::Restarting {
            attempt: attempts,
            limit: policy.restart_limit,
            after_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            reason,
        });
        match idle(&mut work, &mut docs, &sink, &cfg, Some(delay)).await {
            Idled::Stopped => return,
            Idled::RestartAsked => attempts = 0,
            Idled::Elapsed => {}
        }
    }
}

async fn idle(
    work: &mut mpsc::UnboundedReceiver<Work>,
    docs: &mut Docs,
    sink: &Sink,
    cfg: &ServerConfig,
    limit: Option<Duration>,
) -> Idled {
    let deadline = async {
        match limit {
            Some(d) => tokio::time::sleep(d).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            () = &mut deadline => return Idled::Elapsed,
            next = work.recv() => match next {
                None => return Idled::Stopped,
                Some(w) => {
                    if absorb(w, docs, sink, cfg) {
                        return Idled::RestartAsked;
                    }
                }
            },
        }
    }
}

fn absorb(work: Work, docs: &mut Docs, sink: &Sink, cfg: &ServerConfig) -> bool {
    match work.freight {
        SessionFreight::Sync { document, text } => {
            docs.update(&document, text, work.anchor, &cfg.language);
            false
        }
        SessionFreight::Close { document } => {
            docs.by_uri.remove(&uri_of(&document.path));
            false
        }
        SessionFreight::Saved { .. } => false,
        SessionFreight::Ask { question, .. } => {
            if question.is_explicit() {
                sink.say("is not answering; see the status line");
            }
            false
        }
        SessionFreight::Restart => true,
    }
}

async fn incarnate(
    cfg: &ServerConfig,
    root: &Path,
    policy: &SessionPolicy,
    work: &mut mpsc::UnboundedReceiver<Work>,
    docs: &mut Docs,
    sink: &Sink,
    generation: u32,
) -> End {
    let mut conn = match Connection::spawn_logged(cfg, root) {
        Ok(conn) => conn,
        Err(LspError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return End::Missing,
        Err(e) => return End::Failed(e.to_string()),
    };
    let pid = conn.pid();
    let mut inbox = conn.take_inbox();
    let conn = Arc::new(conn);
    let shaking = {
        let conn = Arc::clone(&conn);
        let root = root.to_path_buf();
        tokio::spawn(async move { conn.handshake(&root).await })
    };
    tokio::pin!(shaking);
    let deadline = tokio::time::sleep(policy.handshake_timeout);
    tokio::pin!(deadline);
    let caps = loop {
        tokio::select! {
            () = &mut deadline => {
                conn.kill().await;
                return End::Failed("no answer to initialize".to_string());
            }
            shaken = &mut shaking => match shaken {
                Ok(Ok(caps)) => break caps,
                Ok(Err(e)) => return End::Failed(gone_reason(&conn, &e.to_string()).await),
                Err(e) => return End::Failed(e.to_string()),
            },
            next = work.recv() => match next {
                None => return End::Stopped,
                Some(w) => {
                    if absorb(w, docs, sink, cfg) {
                        conn.kill().await;
                        return End::RestartAsked;
                    }
                }
            },
        }
    };
    sink.status(ServerState::Ready {
        generation,
        pid,
        completes: caps.completes(),
        completion_triggers: caps.completion_triggers(),
    });
    let mut live = Live::new(
        Arc::clone(&conn),
        caps,
        policy.clone(),
        sink.clone(),
        cfg.clone(),
        generation,
        pid,
    );
    for doc in docs.by_uri.values_mut() {
        live.sync(doc);
    }
    let end = live.converse(work, docs, &mut inbox).await;
    live.stop();
    match end {
        LiveEnd::Stopped => {
            let bye = async {
                let _ = conn.request("shutdown", Value::Null).await;
                let _ = conn.notify("exit", Value::Null).await;
            };
            let _ = tokio::time::timeout(Duration::from_millis(300), bye).await;
            End::Stopped
        }
        LiveEnd::RestartAsked => {
            conn.kill().await;
            End::RestartAsked
        }
        LiveEnd::Gone => End::Failed(gone_reason(&conn, "closed its connection").await),
        LiveEnd::Hung => {
            conn.kill().await;
            End::Failed("stopped reading its input".to_string())
        }
    }
}

async fn gone_reason(conn: &Connection, fallback: &str) -> String {
    conn.exit_status(Duration::from_secs(1))
        .await
        .map_or_else(|| fallback.to_string(), |status| status.to_string())
}

enum Out {
    Notify {
        method: &'static str,
        params: Value,
        replaces: Option<String>,
    },
    Request {
        method: &'static str,
        params: Value,
        replaces: Option<(String, &'static str)>,
        begun: oneshot::Sender<oneshot::Receiver<Reply>>,
    },
}

fn coalesce(batch: Vec<Out>) -> Vec<Out> {
    let mut last_change: HashMap<String, usize> = HashMap::new();
    let mut last_ask: HashMap<(String, &'static str), usize> = HashMap::new();
    for (i, out) in batch.iter().enumerate() {
        match out {
            Out::Notify {
                replaces: Some(uri),
                ..
            } => {
                last_change.insert(uri.clone(), i);
            }
            Out::Request {
                replaces: Some(key),
                ..
            } => {
                last_ask.insert(key.clone(), i);
            }
            _ => {}
        }
    }
    batch
        .into_iter()
        .enumerate()
        .filter(|(i, out)| match out {
            Out::Notify {
                replaces: Some(uri),
                ..
            } => last_change.get(uri) == Some(i),
            Out::Request {
                replaces: Some(key),
                ..
            } => last_ask.get(key) == Some(i),
            _ => true,
        })
        .map(|(_, out)| out)
        .collect()
}

async fn write_out(
    conn: Arc<Connection>,
    mut queue: mpsc::UnboundedReceiver<Out>,
    busy: Arc<Mutex<Option<Instant>>>,
) {
    while let Some(first) = queue.recv().await {
        let mut batch = vec![first];
        while let Ok(next) = queue.try_recv() {
            batch.push(next);
        }
        for out in coalesce(batch) {
            if let Ok(mut b) = busy.lock() {
                *b = Some(Instant::now());
            }
            let written = match out {
                Out::Notify { method, params, .. } => conn.notify(method, params).await,
                Out::Request {
                    method,
                    params,
                    begun,
                    ..
                } => match conn.begin_request(method, params).await {
                    Ok(rx) => {
                        let _ = begun.send(rx);
                        Ok(())
                    }
                    Err(e) => Err(e),
                },
            };
            if let Ok(mut b) = busy.lock() {
                *b = None;
            }
            if written.is_err() {
                return;
            }
        }
    }
}

enum LiveEvent {
    Answered,
    Waited(u64),
}

enum LiveEnd {
    Stopped,
    RestartAsked,
    Gone,
    Hung,
}

struct Live {
    conn: Arc<Connection>,
    caps: ServerCaps,
    legend: Option<Legend>,
    policy: SessionPolicy,
    sink: Sink,
    cfg: ServerConfig,
    generation: u32,
    pid: Option<u32>,
    out: mpsc::UnboundedSender<Out>,
    busy: Arc<Mutex<Option<Instant>>>,
    writer: tokio::task::JoinHandle<()>,
    events_tx: mpsc::UnboundedSender<LiveEvent>,
    events_rx: mpsc::UnboundedReceiver<LiveEvent>,
    slow: bool,
}

impl Live {
    fn new(
        conn: Arc<Connection>,
        caps: ServerCaps,
        policy: SessionPolicy,
        sink: Sink,
        cfg: ServerConfig,
        generation: u32,
        pid: Option<u32>,
    ) -> Self {
        let (out, queue) = mpsc::unbounded_channel();
        let busy = Arc::new(Mutex::new(None));
        let writer = tokio::spawn(write_out(Arc::clone(&conn), queue, Arc::clone(&busy)));
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let legend = Legend::from_capabilities(&caps.raw);
        Self {
            conn,
            caps,
            legend,
            policy,
            sink,
            cfg,
            generation,
            pid,
            out,
            busy,
            writer,
            events_tx,
            events_rx,
            slow: false,
        }
    }

    fn stop(&self) {
        self.writer.abort();
    }

    async fn converse(
        &mut self,
        work: &mut mpsc::UnboundedReceiver<Work>,
        docs: &mut Docs,
        inbox: &mut mpsc::Receiver<Incoming>,
    ) -> LiveEnd {
        let mut beat = tokio::time::interval(Duration::from_millis(250));
        loop {
            tokio::select! {
                next = work.recv() => match next {
                    None => return LiveEnd::Stopped,
                    Some(w) => {
                        if self.handle(w, docs) {
                            return LiveEnd::RestartAsked;
                        }
                    }
                },
                message = inbox.recv() => match message {
                    None => return LiveEnd::Gone,
                    Some(m) => self.from_server(m, docs).await,
                },
                Some(event) = self.events_rx.recv() => self.note(event),
                _ = beat.tick() => {
                    if self.writer_stuck() {
                        return LiveEnd::Hung;
                    }
                }
            }
        }
    }

    fn writer_stuck(&self) -> bool {
        self.busy
            .lock()
            .ok()
            .and_then(|b| *b)
            .is_some_and(|since| since.elapsed() >= self.policy.hang_after)
    }

    fn note(&mut self, event: LiveEvent) {
        match event {
            LiveEvent::Waited(waited_ms) => {
                if !self.slow {
                    self.slow = true;
                    self.sink.status(ServerState::Slow { waited_ms });
                }
            }
            LiveEvent::Answered => {
                if self.slow {
                    self.slow = false;
                    self.sink.status(ServerState::Ready {
                        generation: self.generation,
                        pid: self.pid,
                        completes: self.caps.completes(),
                        completion_triggers: self.caps.completion_triggers(),
                    });
                }
            }
        }
    }

    fn handle(&mut self, work: Work, docs: &mut Docs) -> bool {
        match work.freight {
            SessionFreight::Sync { document, text } => {
                let uri = docs.update(&document, text, work.anchor, &self.cfg.language);
                if let Some(doc) = docs.by_uri.get_mut(&uri) {
                    self.sync(doc);
                }
                false
            }
            SessionFreight::Saved { document } => {
                let uri = uri_of(&document.path);
                if docs.by_uri.get(&uri).is_some_and(|d| d.open) {
                    self.notify(
                        "textDocument/didSave",
                        json!({ "textDocument": { "uri": uri } }),
                        None,
                    );
                }
                false
            }
            SessionFreight::Close { document } => {
                let uri = uri_of(&document.path);
                if docs.by_uri.remove(&uri).is_some_and(|d| d.open) {
                    self.notify(
                        "textDocument/didClose",
                        json!({ "textDocument": { "uri": uri } }),
                        None,
                    );
                }
                false
            }
            SessionFreight::Ask {
                document,
                at,
                question,
            } => {
                let uri = uri_of(&document.path);
                if let Some(doc) = docs.by_uri.get(&uri) {
                    self.ask(doc, at, question, &work.anchor, docs);
                }
                false
            }
            SessionFreight::Restart => true,
        }
    }

    fn notify(&self, method: &'static str, params: Value, replaces: Option<String>) {
        let _ = self.out.send(Out::Notify {
            method,
            params,
            replaces,
        });
    }

    fn request(
        &self,
        method: &'static str,
        params: Value,
        replaces: Option<(String, &'static str)>,
    ) -> oneshot::Receiver<oneshot::Receiver<Reply>> {
        let (begun, rx) = oneshot::channel();
        let _ = self.out.send(Out::Request {
            method,
            params,
            replaces,
            begun,
        });
        rx
    }

    fn sync(&self, doc: &mut Doc) {
        if self.caps.text_document_sync.unwrap_or(0) == 0 {
            return;
        }
        if doc.open {
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": doc.uri, "version": doc.version },
                    "contentChanges": [{ "text": &*doc.text }],
                }),
                Some(doc.uri.clone()),
            );
        } else {
            doc.open = true;
            self.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": doc.uri,
                        "languageId": doc.language,
                        "version": doc.version,
                        "text": &*doc.text,
                    }
                }),
                None,
            );
        }
        self.ask_tokens(doc);
        self.ask_hints(doc);
    }

    fn ask_tokens(&self, doc: &Doc) {
        if !self.caps.semantic_tokens {
            return;
        }
        let Some(legend) = self.legend.clone() else {
            return;
        };
        let pending = self.request(
            "textDocument/semanticTokens/full",
            tokens_params(&doc.uri),
            Some((doc.uri.clone(), "tokens")),
        );
        let (sink, anchor, text, buffer) = (
            self.sink.clone(),
            doc.anchor.clone(),
            Arc::clone(&doc.text),
            doc.buffer,
        );
        let timeout = self.policy.request_timeout;
        tokio::spawn(async move {
            if let Some(Ok(value)) = settled(pending, timeout).await.flatten() {
                let tokens = decode_tokens(&value, &legend, &text);
                sink.sealed(&anchor, Negai::PublishSemanticTokens { buffer, tokens });
            }
        });
    }

    fn ask_hints(&self, doc: &Doc) {
        if !self.caps.inlay_hints() {
            return;
        }
        let lines = Lines::new(&doc.text);
        let end = lines.to_utf16(
            &doc.text,
            CharPosition {
                line: u32::try_from(lines.line_count()).unwrap_or(u32::MAX),
                character: 0,
            },
        );
        let pending = self.request(
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": doc.uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": end.line, "character": end.character },
                },
            }),
            Some((doc.uri.clone(), "hints")),
        );
        let (sink, anchor, text, buffer) = (
            self.sink.clone(),
            doc.anchor.clone(),
            Arc::clone(&doc.text),
            doc.buffer,
        );
        let timeout = self.policy.request_timeout;
        tokio::spawn(async move {
            if let Some(Ok(value)) = settled(pending, timeout).await.flatten() {
                let hints = parse_inlay_hints(&value, &text);
                sink.sealed(&anchor, Negai::PublishInlayHints { buffer, hints });
            }
        });
    }

    fn ask(&self, doc: &Doc, at: Position, question: Question, anchor: &Anchor, docs: &Docs) {
        let offered = match question {
            Question::Completion { .. } => self.caps.completes(),
            Question::Hover => self.caps.hover,
            Question::Definition => self.caps.definition,
        };
        if !offered {
            if question.is_explicit() {
                self.sink.say("does not offer that");
            }
            return;
        }
        let lines = Lines::new(&doc.text);
        let wire = lines.to_utf16(
            &doc.text,
            CharPosition {
                line: at.line,
                character: at.column,
            },
        );
        let position = json!({
            "textDocument": { "uri": doc.uri },
            "position": { "line": wire.line, "character": wire.character },
        });
        let (method, kind) = match question {
            Question::Completion { .. } => ("textDocument/completion", "completion"),
            Question::Hover => ("textDocument/hover", "hover"),
            Question::Definition => ("textDocument/definition", "definition"),
        };
        let pending = self.request(method, position, Some((doc.uri.clone(), kind)));
        let others: HashMap<String, Arc<str>> = docs
            .by_uri
            .values()
            .map(|d| (d.uri.clone(), Arc::clone(&d.text)))
            .collect();
        let (sink, sealed_with, text, buffer) = (
            self.sink.clone(),
            anchor.clone(),
            Arc::clone(&doc.text),
            doc.buffer,
        );
        let events = self.events_tx.clone();
        let timeout = self.policy.request_timeout;
        tokio::spawn(async move {
            let began = Instant::now();
            let Some(answer) = settled(pending, timeout).await else {
                let waited = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
                let _ = events.send(LiveEvent::Waited(waited));
                return;
            };
            let Some(reply) = answer else {
                return;
            };
            let _ = events.send(LiveEvent::Answered);
            let value = match reply {
                Ok(value) => value,
                Err(ReplyError::Server(e)) => {
                    if question.is_explicit() {
                        sink.say(&e);
                    }
                    return;
                }
                Err(ReplyError::Abandoned(_)) => return,
            };
            match question {
                Question::Completion { manual } => {
                    let items = parse_completion(&value, &text);
                    sink.sealed(
                        &sealed_with,
                        Negai::OfferCompletion {
                            buffer,
                            at,
                            items,
                            manual,
                        },
                    );
                }
                Question::Hover => {
                    let shown = parse_hover(&value);
                    sink.sealed(
                        &sealed_with,
                        Negai::ShowHover {
                            buffer,
                            text: shown,
                        },
                    );
                }
                Question::Definition => match parse_definition(&value, &others) {
                    Some((path, at)) => {
                        sink.sealed(&sealed_with, Negai::JumpTo { path, at });
                    }
                    None => sink.say("found no definition here"),
                },
            }
        });
    }

    async fn from_server(&mut self, message: Incoming, docs: &Docs) {
        match message {
            Incoming::Notification { method, params } => {
                if method == "textDocument/publishDiagnostics" {
                    self.published(params, docs);
                } else if method == "window/showMessage" {
                    let severe = params.get("type").and_then(Value::as_u64).unwrap_or(4) <= 2;
                    if let (true, Some(m)) = (severe, params.get("message").and_then(Value::as_str))
                    {
                        self.sink.say(m);
                    }
                }
            }
            Incoming::ServerRequest { id, method, params } => {
                let answer = if method == "workspace/configuration" {
                    let asked = params
                        .get("items")
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    Value::Array(vec![Value::Null; asked])
                } else {
                    Value::Null
                };
                let conn = Arc::clone(&self.conn);
                tokio::spawn(async move {
                    let _ = conn.respond(id, answer).await;
                });
            }
            Incoming::Response { .. } => {}
        }
    }

    fn published(&self, params: Value, docs: &Docs) {
        let Ok(published) = serde_json::from_value::<PublishDiagnostics>(params) else {
            return;
        };
        let Some(doc) = docs.by_uri.get(&published.uri) else {
            return;
        };
        if published.version.is_some_and(|v| v != doc.version) {
            return;
        }
        let findings = to_findings(&published, &doc.text, Some(doc.buffer));
        let list = document_list(&doc.language, &doc.path);
        self.sink
            .sealed(&doc.anchor, Negai::PublishFindings { list, findings });
    }
}

async fn settled(
    pending: oneshot::Receiver<oneshot::Receiver<Reply>>,
    timeout: Duration,
) -> Option<Option<Reply>> {
    tokio::time::timeout(timeout, async {
        let reply = pending.await.ok()?;
        reply.await.ok()
    })
    .await
    .ok()
}

#[must_use]
pub fn document_list(language: &str, path: &Path) -> String {
    let mut list = list_for(language);
    list.push(':');
    list.push_str(&path.to_string_lossy());
    list
}

fn char_position(lines: &Lines, text: &str, value: &Value) -> Option<Position> {
    let wire = crate::Position {
        line: u32::try_from(value.get("line")?.as_u64()?).ok()?,
        character: u32::try_from(value.get("character")?.as_u64()?).ok()?,
    };
    let c = lines.to_char(text, wire);
    Some(Position::new(c.line, c.character))
}

fn char_range(lines: &Lines, text: &str, value: &Value) -> Option<Range> {
    Some(Range::new(
        char_position(lines, text, value.get("start")?)?,
        char_position(lines, text, value.get("end")?)?,
    ))
}

fn change_of(lines: &Lines, text: &str, value: &Value) -> Option<TextChange> {
    let range = value
        .get("range")
        .or_else(|| value.get("replace"))
        .or_else(|| value.get("insert"))?;
    Some(TextChange {
        range: char_range(lines, text, range)?,
        text: value.get("newText")?.as_str()?.to_string(),
    })
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Object(_) => value.get("value")?.as_str().map(str::to_owned),
        _ => None,
    }
}

#[must_use]
pub fn parse_completion(value: &Value, text: &str) -> Vec<CompletionEntry> {
    let items = value
        .as_array()
        .or_else(|| value.get("items").and_then(Value::as_array));
    let lines = Lines::new(text);
    items
        .map(|items| {
            items
                .iter()
                .filter_map(|item| entry_of(item, &lines, text))
                .collect()
        })
        .unwrap_or_default()
}

fn entry_of(item: &Value, lines: &Lines, text: &str) -> Option<CompletionEntry> {
    let label = item.get("label")?.as_str()?.to_string();
    let field = |key: &str| item.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(CompletionEntry {
        filter: field("filterText").unwrap_or_else(|| label.clone()),
        sort: field("sortText").unwrap_or_else(|| label.clone()),
        insert: field("insertText").unwrap_or_else(|| label.clone()),
        detail: field("detail"),
        tier: item
            .pointer("/labelDetails/description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        documentation: item.get("documentation").and_then(text_of),
        edit: item
            .get("textEdit")
            .and_then(|edit| change_of(lines, text, edit)),
        additional: item
            .get("additionalTextEdits")
            .and_then(Value::as_array)
            .map(|edits| {
                edits
                    .iter()
                    .filter_map(|edit| change_of(lines, text, edit))
                    .collect()
            })
            .unwrap_or_default(),
        label,
    })
}

#[must_use]
pub fn parse_hover(value: &Value) -> String {
    let Some(contents) = value.get("contents") else {
        return String::new();
    };
    let joined = match contents {
        Value::Array(parts) => parts
            .iter()
            .filter_map(text_of)
            .collect::<Vec<_>>()
            .join("\n"),
        other => text_of(other).unwrap_or_default(),
    };
    joined
        .lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn parse_definition(
    value: &Value,
    open: &HashMap<String, Arc<str>>,
) -> Option<(PathBuf, Position)> {
    let first = value.as_array().map_or(Some(value), |all| all.first())?;
    let uri = first
        .get("uri")
        .or_else(|| first.get("targetUri"))?
        .as_str()?;
    let start = first
        .get("range")
        .or_else(|| first.get("targetSelectionRange"))?
        .get("start")?;
    let path = path_of(uri)?;
    let at = match open.get(uri) {
        Some(text) => char_position(&Lines::new(text), text, start)?,
        None => Position::new(
            u32::try_from(start.get("line")?.as_u64()?).ok()?,
            u32::try_from(start.get("character")?.as_u64()?).ok()?,
        ),
    };
    Some((path, at))
}

#[must_use]
pub fn parse_inlay_hints(value: &Value, text: &str) -> Vec<InlayHint> {
    let lines = Lines::new(text);
    value
        .as_array()
        .map(|hints| {
            hints
                .iter()
                .filter_map(|hint| {
                    let label = match hint.get("label")? {
                        Value::String(s) => s.clone(),
                        Value::Array(parts) => parts
                            .iter()
                            .filter_map(|p| p.get("value").and_then(Value::as_str))
                            .collect::<String>(),
                        _ => return None,
                    };
                    Some(InlayHint {
                        position: char_position(&lines, text, hint.get("position")?)?,
                        label,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn changed(uri: &str, tag: i64) -> Out {
        Out::Notify {
            method: "textDocument/didChange",
            params: json!(tag),
            replaces: Some(uri.to_string()),
        }
    }

    fn tag_of(out: &Out) -> Value {
        match out {
            Out::Notify { params, .. } | Out::Request { params, .. } => params.clone(),
        }
    }

    #[test]
    fn a_batch_keeps_only_the_newest_change_per_document() {
        let batch = vec![changed("a", 1), changed("b", 2), changed("a", 3)];
        let kept: Vec<Value> = coalesce(batch).iter().map(tag_of).collect();
        assert_eq!(kept, vec![json!(2), json!(3)]);
    }

    #[test]
    fn a_newer_question_of_the_same_kind_replaces_the_older_and_ends_its_wait() {
        let ask = |tag: i64| {
            let (begun, rx) = oneshot::channel();
            (
                Out::Request {
                    method: "textDocument/completion",
                    params: json!(tag),
                    replaces: Some(("a".to_string(), "completion")),
                    begun,
                },
                rx,
            )
        };
        let (older, mut older_rx) = ask(1);
        let (newer, _newer_rx) = ask(2);
        let kept = coalesce(vec![older, newer]);
        assert_eq!(kept.iter().map(tag_of).collect::<Vec<_>>(), vec![json!(2)]);
        assert!(
            older_rx.try_recv().is_err(),
            "the replaced waiter must not be parked for an answer"
        );
    }

    #[test]
    fn backoff_doubles_and_stops_at_the_cap() {
        let policy = SessionPolicy {
            backoff_base: Duration::from_millis(100),
            backoff_cap: Duration::from_millis(350),
            ..SessionPolicy::default()
        };
        let got: Vec<u128> = (1..=4).map(|a| policy.backoff(a).as_millis()).collect();
        assert_eq!(got, vec![100, 200, 350, 350]);
    }

    #[test]
    fn a_completion_item_reads_its_tier_edit_and_additional_edits_in_chars() {
        let text = "😀 ab\n";
        let value = json!({ "isIncomplete": false, "items": [{
            "label": "abc",
            "labelDetails": { "description": "needs a use" },
            "documentation": { "kind": "markdown", "value": "docs" },
            "textEdit": {
                "range": {
                    "start": { "line": 0, "character": 3 },
                    "end": { "line": 0, "character": 5 },
                },
                "newText": "abc",
            },
            "additionalTextEdits": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 0 },
                },
                "newText": "use\n",
            }],
        }]});
        let items = parse_completion(&value, text);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].tier.as_deref(), Some("needs a use"));
        assert_eq!(items[0].documentation.as_deref(), Some("docs"));
        let edit = items[0].edit.as_ref().expect("the text edit is read");
        assert_eq!(
            edit.range,
            Range::new(Position::new(0, 2), Position::new(0, 4)),
            "the emoji is two UTF-16 units and one char"
        );
        assert_eq!(items[0].additional.len(), 1);
    }

    #[test]
    fn hover_drops_fence_lines_and_keeps_the_words() {
        let value = json!({ "contents": { "kind": "markdown", "value": "```blue\ndef f(x)\n```\n\nwords" } });
        assert_eq!(parse_hover(&value), "def f(x)\n\nwords");
        assert_eq!(parse_hover(&Value::Null), "");
    }

    #[test]
    fn definition_reads_a_location_list_and_decodes_the_path() {
        let value = json!([{
            "uri": "file:///a%20b/c.b",
            "range": {
                "start": { "line": 3, "character": 4 },
                "end": { "line": 3, "character": 9 },
            },
        }]);
        let (path, at) = parse_definition(&value, &HashMap::new()).expect("a location");
        assert_eq!(path, PathBuf::from("/a b/c.b"));
        assert_eq!(at, Position::new(3, 4));
        assert!(parse_definition(&json!([]), &HashMap::new()).is_none());
    }

    #[test]
    fn inlay_labels_may_be_a_string_or_parts() {
        let value = json!([
            { "position": { "line": 0, "character": 1 }, "label": "=> 3" },
            { "position": { "line": 1, "character": 0 }, "label": [{ "value": "a" }, { "value": "b" }] },
        ]);
        let hints = parse_inlay_hints(&value, "x\ny\n");
        assert_eq!(hints[0].label, "=> 3");
        assert_eq!(hints[1].label, "ab");
    }

    #[test]
    fn a_document_list_names_the_language_and_the_path() {
        assert_eq!(
            document_list("blue", Path::new("/a/b.b")),
            "diagnostics:blue:/a/b.b"
        );
    }

    fn a_session(
        command: &str,
    ) -> (
        Sink,
        mpsc::UnboundedSender<Work>,
        std::sync::mpsc::Receiver<Parcel>,
        tokio::task::JoinHandle<()>,
    ) {
        let (reply, parcels) = std::sync::mpsc::channel();
        let (tx, rx) = mpsc::unbounded_channel();
        let cfg = ServerConfig {
            language: "x".to_string(),
            command: command.to_string(),
            args: vec![],
            root_markers: vec![],
        };
        let sink = Sink {
            reply,
            id: ErrandId(1),
            languages: vec!["x".to_string()],
            command: command.to_string(),
            root: PathBuf::from("."),
        };
        let policy = SessionPolicy {
            backoff_base: Duration::from_millis(5),
            backoff_cap: Duration::from_millis(10),
            restart_limit: 2,
            handshake_timeout: Duration::from_secs(5),
            ..SessionPolicy::default()
        };
        let task = tokio::spawn(run_session(
            cfg,
            PathBuf::from("."),
            policy,
            rx,
            sink.clone(),
        ));
        (sink, tx, parcels, task)
    }

    fn states(parcels: &std::sync::mpsc::Receiver<Parcel>) -> Vec<ServerState> {
        parcels
            .try_iter()
            .filter_map(|p| match p.slip {
                Negai::ServerStatus(s) => Some(s.state),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn a_binary_that_is_not_there_is_the_typed_missing_state_and_no_retry_loop() {
        let (_sink, _tx, parcels, task) = a_session("escriba-test-no-such-language-server");
        tokio::time::sleep(Duration::from_millis(200)).await;
        let seen = states(&parcels);
        assert!(
            matches!(seen.last(), Some(ServerState::Missing { command }) if command.contains("no-such")),
            "{seen:?}"
        );
        assert_eq!(
            seen.len(),
            2,
            "one start and one verdict, no retries: {seen:?}"
        );
        task.abort();
    }

    #[tokio::test]
    async fn a_server_that_keeps_dying_ends_in_the_typed_dead_state_after_the_limit() {
        let (_sink, tx, parcels, task) = a_session("false");
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let seen = states(&parcels);
        let restarts = seen
            .iter()
            .filter(|s| matches!(s, ServerState::Restarting { .. }))
            .count();
        assert_eq!(restarts, 2, "{seen:?}");
        assert!(
            matches!(seen.last(), Some(ServerState::Dead { .. })),
            "{seen:?}"
        );

        tx.send(Work {
            freight: SessionFreight::Restart,
            anchor: Anchor::new(),
        })
        .expect("the dead session still takes a restart");
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(
            states(&parcels)
                .iter()
                .any(|s| matches!(s, ServerState::Restarting { .. })),
            "an asked restart leaves the dead state"
        );
        task.abort();
    }
}
