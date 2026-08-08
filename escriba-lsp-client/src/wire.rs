//! The LSP base protocol — `Content-Length` framing over a byte stream.
//!
//! Phase 2's foundation, and deliberately the half with no I/O in it. LSP's
//! base protocol is a tiny HTTP-like header block followed by a JSON body:
//!
//! ```text
//! Content-Length: 42\r\n
//! \r\n
//! {"jsonrpc":"2.0", ... }
//! ```
//!
//! Trivial to describe and unusually easy to get wrong, because every bug in
//! it is a *hang* rather than an error: read one byte too few and the client
//! waits forever for a message the server already sent. So the codec is pure
//! and the transport is thin — every nasty case below is a unit test with no
//! process, no socket and no runtime.
//!
//! **`Content-Length` counts BYTES, not characters.** That is the classic
//! implementation bug and it is silent until someone opens a file with a
//! non-ASCII identifier or a diagnostic quotes one — at which point the stream
//! desynchronises permanently, because every subsequent frame is read from the
//! wrong offset. There is a test.

use serde::{Deserialize, Serialize};

/// How much of the buffer a decode consumed, and what it produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
    /// A complete message: the JSON body, and how many bytes to drain.
    Message { body: Vec<u8>, consumed: usize },
    /// The buffer holds a valid prefix but not yet a whole message. Read more.
    ///
    /// Distinct from an error on purpose: a partial read is the NORMAL state of
    /// a stream, and treating it as a failure is how a client drops a message
    /// that was merely still arriving.
    Incomplete,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WireError {
    #[error("header block is not valid UTF-8")]
    HeaderNotUtf8,
    #[error("header line has no colon: {0:?}")]
    HeaderNoColon(String),
    #[error("Content-Length is not a number: {0:?}")]
    BadContentLength(String),
    #[error("header block ended with no Content-Length")]
    MissingContentLength,
    /// A frame claiming more than this is refused rather than buffered. An
    /// unbounded length from a misbehaving or wedged server is an OOM, and an
    /// editor that dies because a language server hiccuped is worse than one
    /// that drops the connection.
    #[error("Content-Length {0} exceeds the {1} byte ceiling")]
    ContentLengthTooLarge(usize, usize),
}

/// The largest single message accepted. rust-analyzer's initialize response on
/// a big workspace is comfortably under a megabyte; 32 MiB is far above any
/// legitimate frame and far below anything that threatens the process.
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

const SEP: &[u8] = b"\r\n\r\n";

/// Frame one JSON body for the wire.
///
/// Takes bytes rather than a `&str` so the length can never be computed from a
/// character count — the type makes the classic bug awkward to write.
#[must_use]
pub fn encode(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 32);
    out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    out.extend_from_slice(body);
    out
}

/// Try to take one message off the front of `buf`.
///
/// Never mutates: the caller drains `consumed` bytes once it has the message,
/// so a partial read leaves the buffer untouched and can simply be retried
/// after more bytes arrive.
///
/// # Errors
/// Malformed headers, an unparsable or absent `Content-Length`, or a frame
/// over [`MAX_FRAME_BYTES`].
pub fn decode(buf: &[u8]) -> Result<Decoded, WireError> {
    let Some(sep) = find(buf, SEP) else {
        return Ok(Decoded::Incomplete);
    };
    let headers = std::str::from_utf8(&buf[..sep]).map_err(|_| WireError::HeaderNotUtf8)?;

    let mut len: Option<usize> = None;
    for line in headers.split("\r\n").filter(|l| !l.is_empty()) {
        let (k, v) = line.split_once(':').ok_or_else(|| WireError::HeaderNoColon(line.to_owned()))?;
        // Only Content-Length is load-bearing. Content-Type is legal and
        // ignorable; an unknown header must NOT be an error, or a future spec
        // revision breaks every client that was strict about it.
        if k.trim().eq_ignore_ascii_case("content-length") {
            len = Some(
                v.trim().parse().map_err(|_| WireError::BadContentLength(v.trim().to_owned()))?,
            );
        }
    }
    let len = len.ok_or(WireError::MissingContentLength)?;
    if len > MAX_FRAME_BYTES {
        return Err(WireError::ContentLengthTooLarge(len, MAX_FRAME_BYTES));
    }

    let start = sep + SEP.len();
    let end = start.saturating_add(len);
    if buf.len() < end {
        return Ok(Decoded::Incomplete);
    }
    Ok(Decoded::Message { body: buf[start..end].to_vec(), consumed: end })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

// ── JSON-RPC 2.0, the layer above the frame ───────────────────────────────

/// A request id. LSP permits a number or a string; a client that assumes
/// numbers cannot talk to a server that echoes strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    Str(String),
}

/// One outbound call.
#[derive(Debug, Clone, Serialize)]
pub struct Request<'a> {
    pub jsonrpc: &'static str,
    pub id: RequestId,
    pub method: &'a str,
    pub params: serde_json::Value,
}

impl<'a> Request<'a> {
    #[must_use]
    pub fn new(id: i64, method: &'a str, params: serde_json::Value) -> Self {
        Self { jsonrpc: "2.0", id: RequestId::Number(id), method, params }
    }
}

/// A notification — no id, and therefore no reply. Keeping it a distinct type
/// means a caller cannot await a response that will never come, which is the
/// other way an LSP client hangs.
#[derive(Debug, Clone, Serialize)]
pub struct Notification<'a> {
    pub jsonrpc: &'static str,
    pub method: &'a str,
    pub params: serde_json::Value,
}

impl<'a> Notification<'a> {
    #[must_use]
    pub fn new(method: &'a str, params: serde_json::Value) -> Self {
        Self { jsonrpc: "2.0", method, params }
    }
}

/// Anything the server sends us.
///
/// One enum rather than three call sites guessing, because the three shapes are
/// distinguished only by which fields are PRESENT: a response has `id` and one
/// of result/error, a server-initiated request has `id` and `method`, a
/// notification has `method` and no `id`.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Response { id: RequestId, result: Option<serde_json::Value>, error: Option<serde_json::Value> },
    ServerRequest { id: RequestId, method: String, params: serde_json::Value },
    Notification { method: String, params: serde_json::Value },
}

/// Classify one decoded body.
///
/// # Errors
/// The body is not JSON, or is JSON that matches none of the three shapes.
pub fn classify(body: &[u8]) -> Result<Incoming, serde_json::Error> {
    let v: serde_json::Value = serde_json::from_slice(body)?;
    let id = v.get("id").and_then(|i| serde_json::from_value::<RequestId>(i.clone()).ok());
    let method = v.get("method").and_then(|m| m.as_str()).map(ToOwned::to_owned);
    let params = v.get("params").cloned().unwrap_or(serde_json::Value::Null);

    Ok(match (id, method) {
        // id + method = the server is asking US something.
        (Some(id), Some(method)) => Incoming::ServerRequest { id, method, params },
        // id, no method = a reply to one of our requests.
        (Some(id), None) => Incoming::Response {
            id,
            result: v.get("result").cloned(),
            error: v.get("error").cloned(),
        },
        // method, no id = a notification, most often publishDiagnostics.
        (None, Some(method)) => Incoming::Notification { method, params },
        (None, None) => {
            return Err(serde::de::Error::custom("neither an id nor a method"));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        classify, decode, encode, Decoded, Incoming, Notification, Request, RequestId, WireError,
        MAX_FRAME_BYTES,
    };

    #[test]
    fn a_frame_round_trips() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let framed = encode(body);
        match decode(&framed).expect("valid") {
            Decoded::Message { body: got, consumed } => {
                assert_eq!(got, body);
                assert_eq!(consumed, framed.len());
            }
            Decoded::Incomplete => panic!("a complete frame decoded as incomplete"),
        }
    }

    /// **The classic bug.** `Content-Length` is BYTES. Counting characters
    /// desynchronises the stream permanently on the first non-ASCII payload,
    /// because every later frame is then read from the wrong offset.
    #[test]
    fn content_length_counts_bytes_not_characters() {
        let body = r#"{"m":"café ✓ 日本語"}"#.as_bytes();
        assert!(body.len() > r#"{"m":"café ✓ 日本語"}"#.chars().count(), "fixture must be multibyte");
        let framed = encode(body);
        let header = String::from_utf8_lossy(&framed[..20]).to_string();
        assert!(header.contains(&body.len().to_string()), "header must carry the BYTE count");
        assert_eq!(decode(&framed).unwrap(), Decoded::Message { body: body.to_vec(), consumed: framed.len() });
    }

    /// A partial read is the normal state of a stream, not a failure.
    #[test]
    fn every_prefix_of_a_frame_is_incomplete_never_an_error() {
        let framed = encode(br#"{"jsonrpc":"2.0"}"#);
        for n in 0..framed.len() {
            assert_eq!(
                decode(&framed[..n]),
                Ok(Decoded::Incomplete),
                "prefix of length {n} must be Incomplete, not an error or a message"
            );
        }
        assert!(matches!(decode(&framed), Ok(Decoded::Message { .. })));
    }

    /// Two messages in one read is routine — a server flushes a batch. The
    /// decoder must take exactly the first and report how much to drain.
    #[test]
    fn a_buffer_holding_two_messages_yields_them_one_at_a_time() {
        let a = encode(br#"{"id":1}"#);
        let b = encode(br#"{"id":2}"#);
        let mut buf = a.clone();
        buf.extend_from_slice(&b);

        let Decoded::Message { body, consumed } = decode(&buf).unwrap() else { panic!() };
        assert_eq!(body, br#"{"id":1}"#);
        assert_eq!(consumed, a.len(), "must consume exactly the first frame");

        let Decoded::Message { body, .. } = decode(&buf[consumed..]).unwrap() else { panic!() };
        assert_eq!(body, br#"{"id":2}"#);
    }

    /// An unknown header must be ignored, not rejected — a future spec
    /// revision would otherwise break every client that was strict.
    #[test]
    fn extra_headers_are_ignored_and_the_name_is_case_insensitive() {
        let body = br#"{"ok":true}"#;
        let framed = [
            b"content-length: ".to_vec(),
            body.len().to_string().into_bytes(),
            b"\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\nX-Future: 1\r\n\r\n".to_vec(),
            body.to_vec(),
        ]
        .concat();
        assert!(matches!(decode(&framed), Ok(Decoded::Message { .. })));
    }

    #[test]
    fn a_header_block_without_content_length_is_an_error() {
        let framed = b"Content-Type: application/json\r\n\r\n{}".to_vec();
        assert_eq!(decode(&framed), Err(WireError::MissingContentLength));
    }

    #[test]
    fn a_nonnumeric_content_length_is_an_error_not_a_hang() {
        let framed = b"Content-Length: banana\r\n\r\n{}".to_vec();
        assert!(matches!(decode(&framed), Err(WireError::BadContentLength(_))));
    }

    /// An unbounded length from a wedged server must not be buffered. An
    /// editor that dies because a language server hiccuped is worse than one
    /// that drops the connection.
    #[test]
    fn an_absurd_content_length_is_refused_rather_than_allocated() {
        let framed = format!("Content-Length: {}\r\n\r\n", MAX_FRAME_BYTES + 1).into_bytes();
        assert_eq!(
            decode(&framed),
            Err(WireError::ContentLengthTooLarge(MAX_FRAME_BYTES + 1, MAX_FRAME_BYTES))
        );
    }

    // ── JSON-RPC classification ───────────────────────────────────────

    /// The three shapes differ only by which fields are present, which is
    /// exactly why one classifier beats three call sites guessing.
    #[test]
    fn the_three_incoming_shapes_are_told_apart_by_presence() {
        let r = classify(br#"{"jsonrpc":"2.0","id":7,"result":{"ok":1}}"#).unwrap();
        assert!(matches!(r, Incoming::Response { id: RequestId::Number(7), .. }));

        let sr = classify(br#"{"jsonrpc":"2.0","id":8,"method":"window/showMessageRequest"}"#).unwrap();
        assert!(matches!(sr, Incoming::ServerRequest { method, .. } if method == "window/showMessageRequest"));

        let n = classify(br#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{}}"#).unwrap();
        assert!(matches!(n, Incoming::Notification { method, .. } if method == "textDocument/publishDiagnostics"));
    }

    /// A string id is legal. A client that assumes numbers cannot talk to a
    /// server that echoes strings, and the failure looks like a hang.
    #[test]
    fn a_string_request_id_is_accepted() {
        let r = classify(br#"{"jsonrpc":"2.0","id":"abc","result":null}"#).unwrap();
        assert!(matches!(r, Incoming::Response { id: RequestId::Str(s), .. } if s == "abc"));
    }

    /// An error reply is still a Response — routing it as anything else strands
    /// the caller waiting for a reply that already arrived.
    #[test]
    fn an_error_reply_is_a_response_not_a_separate_shape() {
        let r = classify(br#"{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"nope"}}"#).unwrap();
        match r {
            Incoming::Response { id, result, error } => {
                assert_eq!(id, RequestId::Number(3));
                assert!(result.is_none());
                assert!(error.is_some());
            }
            other => panic!("expected a Response, got {other:?}"),
        }
    }

    #[test]
    fn a_body_with_neither_id_nor_method_is_rejected() {
        assert!(classify(br#"{"jsonrpc":"2.0"}"#).is_err());
    }

    /// A notification carries no id, so no caller can ever await it. The type
    /// split is what makes "awaiting a reply that never comes" unwritable.
    #[test]
    fn a_notification_serialises_without_an_id_field() {
        let n = Notification::new("textDocument/didOpen", serde_json::json!({"uri":"file:///x"}));
        let s = serde_json::to_string(&n).unwrap();
        assert!(!s.contains("\"id\""), "a notification must not carry an id: {s}");
        let r = Request::new(1, "initialize", serde_json::Value::Null);
        assert!(serde_json::to_string(&r).unwrap().contains("\"id\":1"));
    }
}
