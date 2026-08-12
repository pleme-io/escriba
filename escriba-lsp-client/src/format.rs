//! The formatting runner — the courier's `textDocument/formatting` carrier.
//!
//! # Why the LSP and not the formatter binary
//!
//! escriba's catalog declares both: `defformatter` names an external command
//! (blue's is `blue fmt --write $FILE`) and `deflsp` names a server. This runner
//! implements the SERVER route, and that choice is not arbitrary.
//!
//! An external formatter with `$FILE` formats what is **on disk**. An editor
//! formats what is **in the buffer**, which is routinely not the same text — the
//! whole reason to press the key is that you just changed something. Handing a
//! `--write` formatter the path means either writing the buffer out first (so
//! "format" silently becomes "save", and formatting a file you were not ready to
//! save corrupts your intent) or formatting stale bytes and pasting the result
//! over newer ones.
//!
//! `textDocument/formatting` takes the text and returns edits. It needs no
//! save, works on a buffer that has never been written, and cannot format the
//! wrong revision — [`Freight::Format`] carries the text by value and the
//! courier's anchor fences the reply against the revision it was computed from.
//!
//! The external route stays available for languages with no server; when it
//! lands it belongs beside this, sharing the same `Freight`.
//!
//! # The comment-deletion hazard, and who is holding it
//!
//! blue's formatter has two entry points and only one of them is safe for a
//! user's buffer: `format_source_lossless` re-interleaves comments, while
//! `format_forms`/`format_source` drop every one of them (blue's own CLAUDE.md
//! records a measured 1010 bytes → 707 with six comments deleted, twice, in two
//! different callers). Measured 2026-08-12: blue's LSP `textDocument/formatting`
//! handler routes through `format_source_lossless`
//! (`blue-lang-lsp/src/analysis.rs:228`), with two gates of its own pinning it.
//!
//! That is blue's invariant to hold, not escriba's — which is precisely the
//! argument for asking the server rather than reimplementing. escriba cannot
//! know which of a formatter's entry points a `defformatter` command line
//! reaches; a server that advertises `documentFormattingProvider` has already
//! made the choice on its own side.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use escriba_core::{Edit, Position, Range};
use escriba_madoguchi::Negai;
use escriba_madoguchi::errand::{Errand, Freight, Parcel, Runner};

use crate::runner::language_of;
use crate::{ServerConfig, ServerRegistry};

/// How long to wait for a server to return edits.
///
/// Shorter than the diagnostics deadline on purpose: formatting is a
/// SYNCHRONOUS gesture — the operator pressed a key and is watching — and a
/// twenty-second stall while nothing happens reads as a hung editor. A server
/// that cannot format a file in five seconds has a problem worth reporting.
const FORMAT_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs one formatting conversation per errand.
pub struct FormatRunner {
    registry: ServerRegistry,
}

impl FormatRunner {
    #[must_use]
    pub fn new(registry: ServerRegistry) -> Self {
        Self { registry }
    }
}

impl Default for FormatRunner {
    fn default() -> Self {
        Self::new(ServerRegistry::default_set())
    }
}

impl Runner for FormatRunner {
    fn start(&self, errand: Errand, cancel: Arc<AtomicBool>, reply: Sender<Parcel>) {
        let Freight::Format {
            buffer,
            path,
            text,
            language,
        } = errand.freight.clone()
        else {
            let _ = reply.send(Parcel {
                id: errand.id,
                slip: Negai::Message("format runner received the wrong freight".into()),
            });
            return;
        };
        let id = errand.id;
        let anchor = errand.anchor.into_anchor();

        // Same resolution order as diagnostics: the editor's answer, then the
        // fallback. Unlike diagnostics, a miss here is worth SAYING — the
        // operator pressed a key and is entitled to know why nothing happened.
        let Some(language) = language.or_else(|| language_of(&path).map(str::to_owned)) else {
            let _ = reply.send(Parcel {
                id,
                slip: Negai::Message("format: no language for this file".into()),
            });
            return;
        };
        let Some(cfg) = self.registry.for_language(&language).cloned() else {
            let _ = reply.send(Parcel {
                id,
                slip: Negai::Message(no_server(&language)),
            });
            return;
        };

        let on_fail = reply.clone();
        let spawned = std::thread::Builder::new()
            .name("escriba-fmt".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = reply.send(Parcel {
                            id,
                            slip: Negai::Message(describe("format: no reactor", &e)),
                        });
                        return;
                    }
                };

                let slip = match rt.block_on(format_document(&cfg, &path, &text)) {
                    Ok(formatted) => {
                        if cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        // Already formatted ⇒ say nothing and edit nothing. An
                        // `Edit` here would still bump the buffer's revision and
                        // land an undo step for a change of zero characters, so
                        // `u` after a no-op format would appear to do nothing.
                        if formatted == text {
                            Negai::Message("already formatted".into())
                        } else {
                            Negai::ErrandReply {
                                anchor,
                                then: Box::new(Negai::Edit {
                                    buffer,
                                    edit: Edit::replace(whole_of(&text), formatted),
                                }),
                            }
                        }
                    }
                    Err(m) => Negai::Message(m),
                };
                let _ = reply.send(Parcel { id, slip });
            });

        if let Err(e) = spawned {
            let _ = on_fail.send(Parcel {
                id,
                slip: Negai::Message(describe("format: could not start the thread", &e)),
            });
        }
    }
}

/// The range covering all of `text`.
///
/// Computed from the text the errand CARRIED, never from the live buffer: the
/// replacement has to span exactly what the server was shown. Reading the
/// buffer's current extent instead would be a second source of truth for the
/// same range, and they disagree the moment the operator types — which is the
/// hazard the courier's anchor exists to catch, so it must not be re-introduced
/// here where the anchor cannot see it.
fn whole_of(text: &str) -> Range {
    let lines = text.split('\n').count();
    let last_line = u32::try_from(lines.saturating_sub(1)).unwrap_or(u32::MAX);
    let last_len = text
        .rsplit('\n')
        .next()
        .map_or(0, |l| u32::try_from(l.chars().count()).unwrap_or(u32::MAX));
    Range::new(Position::new(0, 0), Position::new(last_line, last_len))
}

fn no_server(language: &str) -> String {
    let mut m = String::with_capacity(language.len() + 28);
    m.push_str("format: no server for ");
    m.push_str(language);
    m
}

fn describe(what: &str, e: &impl std::fmt::Display) -> String {
    let mut m = String::from(what);
    m.push_str(": ");
    m.push_str(&e.to_string());
    m
}

/// Ask the server to format, and reduce its edits to the finished text.
///
/// The `Connection` is dropped on every path out, which kills the child.
async fn format_document(cfg: &ServerConfig, path: &Path, text: &str) -> Result<String, String> {
    let (conn, caps, uri) = crate::runner::open_document(cfg, path, text)
        .await
        .map_err(|m| m)?;

    // READ, not assumed — the same discipline `ServerCaps` exists for. blue
    // v0.0.21 offers formatting without semantic tokens and other servers offer
    // neither; asking a server that does not format earns a `-32601` that would
    // surface as an unexplained error rather than a plain answer.
    if !caps.document_formatting {
        return Err(named("format: server does not format", &cfg.command));
    }

    let reply = tokio::time::timeout(
        FORMAT_TIMEOUT,
        conn.request(
            "textDocument/formatting",
            serde_json::json!({
                "textDocument": { "uri": uri },
                // Required by the spec even when the server ignores them. blue
                // has one formatting and no knobs — see `blue-lang-fmt`, whose
                // module docs state there is nowhere to put one — so these are
                // the spec's minimum rather than a preference escriba holds.
                "options": { "tabSize": 2, "insertSpaces": true },
            }),
        ),
    )
    .await
    .map_err(|_| {
        describe(
            "format: no reply within",
            &format_args!("{FORMAT_TIMEOUT:?}"),
        )
    })?
    .map_err(|e| describe(&named("format: request failed", &cfg.command), &e))?
    .map_err(|e| describe(&named("format: server refused", &cfg.command), &e))?;

    apply_text_edits(text, &reply)
}

/// `what (which)` — one spelling for a named failure, as in `runner.rs`.
fn named(what: &str, which: &str) -> String {
    let mut m = String::with_capacity(what.len() + which.len() + 3);
    m.push_str(what);
    m.push_str(" (");
    m.push_str(which);
    m.push(')');
    m
}

/// Fold a `TextEdit[]` reply into the finished text.
///
/// # Why this applies edits rather than taking the first one
///
/// blue answers with a single whole-document edit, and reading `[0].newText`
/// would work against it today. It is wrong in general and wrong quietly:
/// rust-analyzer and most servers return MANY small edits, and taking the first
/// would replace the document with one reformatted line. Applying them back to
/// front is the spec's own rule (edits are given against the ORIGINAL
/// document, so an earlier edit must not shift a later one's offsets).
///
/// `null` is a legal reply meaning "nothing to change" — blue sends it for an
/// unparseable document — and it must read as "no edits", never as an error or
/// as an empty document.
fn apply_text_edits(text: &str, reply: &serde_json::Value) -> Result<String, String> {
    if reply.is_null() {
        return Ok(text.to_string());
    }
    let Some(edits) = reply.as_array() else {
        return Err(String::from("format: server sent no edit list"));
    };
    let lines = zahyou::Lines::new(text);
    // Back to front, so each edit's offsets still describe the text it was
    // computed against.
    let mut ranges: Vec<(usize, usize, &str)> = Vec::with_capacity(edits.len());
    for e in edits {
        let new_text = e.get("newText").and_then(serde_json::Value::as_str);
        let (Some(new_text), Some(range)) = (new_text, e.get("range")) else {
            return Err(String::from("format: malformed edit"));
        };
        let start = offset_of(&lines, text, range.get("start"))?;
        let end = offset_of(&lines, text, range.get("end"))?;
        // A REVERSED range is refused. An out-of-range one is not checked here
        // because it cannot arrive: `zahyou::Lines::offset` clamps a position
        // past the end to the document's end, deliberately and documented ("a
        // peer asking for column 999 means end of line"). Re-checking would be
        // a second rule about the same question that can only ever disagree
        // with the fleet primitive — and it would be dead code, which is worse
        // than absent because it reads as a guard that is holding.
        if start > end {
            return Err(String::from("format: edit range is reversed"));
        }
        ranges.push((start, end, new_text));
    }
    ranges.sort_by_key(|(s, _, _)| std::cmp::Reverse(*s));
    let mut out = text.to_string();
    for (start, end, new_text) in ranges {
        out.replace_range(start..end, new_text);
    }
    Ok(out)
}

/// One LSP position → a byte offset, through [`zahyou`] so the UTF-16 rule is
/// the fleet's single one rather than a second implementation here.
fn offset_of(
    lines: &zahyou::Lines,
    text: &str,
    pos: Option<&serde_json::Value>,
) -> Result<usize, String> {
    let pos = pos.ok_or_else(|| String::from("format: edit without a position"))?;
    let line = pos
        .get("line")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| String::from("format: edit without a line"))?;
    let character = pos
        .get("character")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| String::from("format: edit without a character"))?;
    let p = crate::Position {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        character: u32::try_from(character).unwrap_or(u32::MAX),
    };
    Ok(lines.offset(text, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_null_reply_leaves_the_text_alone() {
        // blue answers `null` for an unparseable document. That must read as
        // "nothing to change" — reading it as an empty document would EMPTY the
        // operator's buffer on a syntax error, which is the worst available
        // outcome for a keystroke.
        let out = apply_text_edits("x = (\n", &serde_json::Value::Null).unwrap();
        assert_eq!(out, "x = (\n");
    }

    #[test]
    fn a_single_whole_document_edit_replaces_everything() {
        // blue's actual shape: one edit spanning the document.
        let text = "x=1\ny=2\n";
        let reply = serde_json::json!([{
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 0}},
            "newText": "x = 1\ny = 2\n",
        }]);
        assert_eq!(apply_text_edits(text, &reply).unwrap(), "x = 1\ny = 2\n");
    }

    /// Several edits apply back-to-front, so an earlier one cannot shift a
    /// later one's offsets.
    ///
    /// The case that would pass while broken if this took `[0].newText`: the
    /// naive reading returns only the first replacement and silently discards
    /// the rest of the file. Two edits in ASCENDING order is the shape a
    /// front-to-back fold gets wrong, which is why they are ordered that way
    /// here.
    #[test]
    fn many_edits_apply_without_shifting_each_other() {
        let text = "aa bb cc";
        let reply = serde_json::json!([
            {
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 2}},
                "newText": "AAAA",
            },
            {
                "range": {"start": {"line": 0, "character": 6}, "end": {"line": 0, "character": 8}},
                "newText": "C",
            },
        ]);
        assert_eq!(apply_text_edits(text, &reply).unwrap(), "AAAA bb C");
    }

    /// An end position past the document clamps to its end — zahyou's rule,
    /// pinned here at the point of use.
    ///
    /// This assertion started life as `is_err()`, on the reasoning that a range
    /// outside the document is a broken server and should be refused. It was
    /// wrong: `zahyou::Lines::offset` clamps such a position on purpose, so the
    /// refusal was unreachable and would have read as a guard that was holding.
    /// Pinning the clamp instead states which rule actually governs.
    #[test]
    fn an_end_past_the_document_clamps_to_the_end() {
        let reply = serde_json::json!([{
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 0}},
            "newText": "x",
        }]);
        assert_eq!(apply_text_edits("short", &reply).unwrap(), "x");
    }

    /// A REVERSED range is refused — the one out-of-range case clamping cannot
    /// rescue, since both ends clamp independently and the order survives.
    #[test]
    fn a_reversed_edit_range_is_refused() {
        let reply = serde_json::json!([{
            "range": {"start": {"line": 5, "character": 0}, "end": {"line": 0, "character": 0}},
            "newText": "x",
        }]);
        assert!(apply_text_edits("short", &reply).is_err());
    }

    #[test]
    fn a_malformed_edit_is_refused() {
        let reply = serde_json::json!([{"newText": "x"}]);
        assert!(apply_text_edits("abc", &reply).is_err());
        let reply = serde_json::json!("not a list");
        assert!(apply_text_edits("abc", &reply).is_err());
    }

    #[test]
    fn whole_of_spans_the_text_it_was_given() {
        assert_eq!(
            whole_of("a\nbb\n"),
            Range::new(Position::new(0, 0), Position::new(2, 0)),
            "a trailing newline means a final empty line"
        );
        assert_eq!(
            whole_of("one"),
            Range::new(Position::new(0, 0), Position::new(0, 3))
        );
        assert_eq!(
            whole_of(""),
            Range::new(Position::new(0, 0), Position::new(0, 0))
        );
    }
}
