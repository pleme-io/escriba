//! `textDocument/publishDiagnostics` → [`escriba_shirube::Finding`].
//!
//! # Why this is an adapter and not a subsystem
//!
//! `escriba-shirube` already models "a located thing worth navigating to" and
//! already paints it in the gutter, lists it, and steps through it with
//! `]x`/`[x`. Its own docs name diagnostics as one of the seven producers it was
//! built for, and [`Origin::Lsp`] has been sitting in the enum waiting. So the
//! LSP hookup is a **source**, exactly as designed — no second diagnostics
//! plane, no second gutter, no second navigation verb.
//!
//! # The one genuinely hard part: two different columns
//!
//! LSP counts columns in **UTF-16 code units**. `escriba_core::Position` counts
//! them in **`char`s**. Those are the same number on every ASCII file and differ
//! by one per astral-plane character, so a bridge that assigns one to the other
//! is correct in testing and wrong for any user with an emoji in a comment —
//! every squiggle after it on the line lands one column left.
//!
//! [`zahyou`] keeps the two as distinct types precisely so that mistake cannot
//! be made silently here; the conversion is [`zahyou::Lines::to_char`] and it
//! needs the document text, which is why [`to_findings`] takes it.
//!
//! # Staleness is the caller's, deliberately
//!
//! Columns are only meaningful against the text they were computed for. A
//! server publishes for version N while the buffer may already be at N+1, and
//! converting N's UTF-16 columns against N+1's text yields positions that are
//! confidently wrong. This module therefore converts against the text it is
//! *given* and reports the version it was told — it does not guess. Deciding
//! whether that version is still current is shirube's job, and it already has
//! the machinery: a stale [`escriba_shirube::ResultList`] reads as **empty**
//! rather than as its old contents.

use escriba_core::{Position as CorePosition, Range as CoreRange};
use escriba_shirube::{Finding, Origin, Severity, Site};
use serde::{Deserialize, Serialize};

use crate::{Lines, Position};

/// The `params` of a `textDocument/publishDiagnostics` notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishDiagnostics {
    pub uri: String,
    /// Present since LSP 3.15. Absent means the server did not say, which is
    /// **not** the same as "current" — an `Option` so a caller cannot mistake
    /// silence for a freshness claim.
    #[serde(default)]
    pub version: Option<i32>,
    #[serde(default)]
    pub diagnostics: Vec<WireDiagnostic>,
}

/// One diagnostic as it arrives on the wire.
///
/// Only the fields escriba renders are modelled; `serde` ignores the rest, so a
/// server sending `relatedInformation` or `tags` is not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireDiagnostic {
    pub range: WireRange,
    /// LSP severity: 1=Error, 2=Warning, 3=Information, 4=Hint. **Optional in
    /// the spec**, and the spec says the client decides when it is missing.
    #[serde(default)]
    pub severity: Option<u8>,
    #[serde(default)]
    pub code: Option<serde_json::Value>,
    #[serde(default)]
    pub source: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireRange {
    pub start: Position,
    pub end: Position,
}

/// LSP severity → shirube severity.
///
/// A missing severity becomes [`Severity::Error`]. The spec leaves it to the
/// client, and under-reporting is the worse failure: a real error rendered as a
/// hint is one the reader scrolls past, whereas a hint rendered as an error is
/// merely annoying. An unknown number is treated the same way rather than
/// silently dropped.
fn severity_of(raw: Option<u8>) -> Severity {
    match raw {
        Some(2) => Severity::Warning,
        Some(3) => Severity::Info,
        Some(4) => Severity::Hint,
        _ => Severity::Error,
    }
}

/// Render a `code` for display. LSP allows a string or an integer.
fn code_of(code: Option<&serde_json::Value>) -> Option<String> {
    match code {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Strip a `file://` URI down to a path.
///
/// Deliberately minimal: this handles the shape servers actually send and
/// returns `None` for anything else rather than fabricating a path. A finding
/// with no path is still perfectly usable — [`Site`] carries a buffer id too,
/// and the caller knows which buffer it asked about.
fn path_of(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file:///a/b` -> `/a/b`; a non-empty authority (`file://host/a`) is not
    // a local path and is not guessed at.
    if rest.starts_with('/') {
        Some(std::path::PathBuf::from(rest))
    } else {
        None
    }
}

/// Convert one publish notification into findings.
///
/// `text` must be the document the server's columns refer to — see the module
/// note on staleness. `buffer` is attached when the caller knows which buffer
/// this is, so the gutter can find it without a path lookup.
///
/// Findings come back **sorted** by position. LSP does not promise an order,
/// and `]d` jumping backwards up a file is the kind of thing that reads as the
/// editor being broken.
#[must_use]
pub fn to_findings(
    params: &PublishDiagnostics,
    text: &str,
    buffer: Option<escriba_core::BufferId>,
) -> Vec<Finding> {
    let lines = Lines::new(text);
    let path = path_of(&params.uri);
    let server = params
        .diagnostics
        .iter()
        .find_map(|d| d.source.clone())
        .unwrap_or_else(|| "lsp".to_string());

    let mut out: Vec<Finding> = params
        .diagnostics
        .iter()
        .map(|d| {
            let site = Site {
                path: path.clone(),
                buffer,
                range: to_core_range(&lines, text, d.range),
            };
            let f = Finding::new(
                site,
                severity_of(d.severity),
                d.message.clone(),
                Origin::Lsp(server.clone()),
            );
            match code_of(d.code.as_ref()) {
                Some(c) => f.with_detail(c),
                None => f,
            }
        })
        .collect();
    out.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    out
}

/// The unit conversion, in one place: UTF-16 columns in, `char` columns out.
fn to_core_range(lines: &Lines, text: &str, r: WireRange) -> CoreRange {
    let at = |p: Position| {
        let c = lines.to_char(text, p);
        CorePosition::new(c.line, c.character)
    };
    CoreRange {
        start: at(r.start),
        end: at(r.end),
    }
}

#[cfg(test)]
mod tests {
    use super::{PublishDiagnostics, WireDiagnostic, WireRange, path_of, severity_of, to_findings};
    use crate::Position;
    use escriba_shirube::{Origin, Severity};

    fn publish(uri: &str, ds: Vec<WireDiagnostic>) -> PublishDiagnostics {
        PublishDiagnostics {
            uri: uri.to_string(),
            version: Some(1),
            diagnostics: ds,
        }
    }

    fn diag(l0: u32, c0: u32, l1: u32, c1: u32, msg: &str) -> WireDiagnostic {
        WireDiagnostic {
            range: WireRange {
                start: Position::new(l0, c0),
                end: Position::new(l1, c1),
            },
            severity: Some(1),
            code: None,
            source: Some("sui".to_string()),
            message: msg.to_string(),
        }
    }

    #[test]
    fn a_diagnostic_becomes_a_finding_with_its_server_named() {
        let p = publish("file:///t.nix", vec![diag(0, 2, 0, 3, "boom")]);
        let f = to_findings(&p, "{ x = 1; }", None);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].message, "boom");
        assert_eq!(f[0].severity, Severity::Error);
        assert_eq!(f[0].origin, Origin::Lsp("sui".to_string()));
        assert_eq!(
            f[0].site.path.as_deref(),
            Some(std::path::Path::new("/t.nix"))
        );
    }

    /// **The bug this bridge exists to not have.** The server counts the emoji
    /// as two columns; escriba's buffer counts it as one. Passing the LSP
    /// number straight through puts every squiggle on that line one to the
    /// right — invisible unless a test contains an astral-plane character.
    #[test]
    fn an_lsp_column_is_converted_to_a_char_column() {
        let text = "# 🎉 abc";
        // The server points at "abc": UTF-16 column 5..8.
        let p = publish("file:///t.nix", vec![diag(0, 5, 0, 8, "here")]);
        let f = to_findings(&p, text, None);
        assert_eq!(f[0].site.range.start.column, 4, "chars, not the 5 LSP sent");
        assert_eq!(f[0].site.range.end.column, 7);
        // And it really does select "abc" in escriba's own coordinates.
        let line: Vec<char> = text.chars().collect();
        let picked: String = line[4..7].iter().collect();
        assert_eq!(picked, "abc");
    }

    #[test]
    fn ascii_columns_pass_through_unchanged() {
        let p = publish("file:///t.nix", vec![diag(1, 4, 1, 9, "x")]);
        let f = to_findings(&p, "let a = 1;\n    b = 2;\nin a", None);
        assert_eq!(f[0].site.range.start.column, 4);
        assert_eq!(f[0].site.range.end.column, 9);
    }

    /// LSP promises no order; `]d` jumping backwards reads as a broken editor.
    #[test]
    fn findings_come_back_sorted_by_position() {
        let p = publish(
            "file:///t.nix",
            vec![
                diag(5, 0, 5, 1, "late"),
                diag(1, 0, 1, 1, "early"),
                diag(3, 2, 3, 3, "middle"),
            ],
        );
        let f = to_findings(&p, "a\nb\nc\nd\ne\nf\n", None);
        let lines: Vec<u32> = f.iter().map(|x| x.site.range.start.line).collect();
        assert_eq!(lines, vec![1, 3, 5]);
    }

    /// Under-reporting is the worse failure: an error shown as a hint is one
    /// the reader scrolls past.
    #[test]
    fn an_absent_or_unknown_severity_is_treated_as_an_error() {
        assert_eq!(severity_of(None), Severity::Error);
        assert_eq!(severity_of(Some(99)), Severity::Error);
        assert_eq!(severity_of(Some(2)), Severity::Warning);
        assert_eq!(severity_of(Some(3)), Severity::Info);
        assert_eq!(severity_of(Some(4)), Severity::Hint);
    }

    /// An empty list is the message "everything here is fixed" and must survive
    /// the trip — a bridge that treats it as nothing to do leaves stale
    /// squiggles on a file the user just corrected.
    #[test]
    fn an_empty_publish_yields_an_empty_list_not_a_dropped_update() {
        let p = publish("file:///t.nix", vec![]);
        assert!(to_findings(&p, "{ }", None).is_empty());
    }

    #[test]
    fn an_unparseable_uri_leaves_the_path_unset_rather_than_invented() {
        assert_eq!(path_of("file:///a/b.nix"), Some("/a/b.nix".into()));
        assert_eq!(path_of("untitled:Untitled-1"), None);
        assert_eq!(
            path_of("file://host/a"),
            None,
            "an authority is not a local path"
        );
    }

    /// A server that omits `version` has not claimed the diagnostics are
    /// current, and the absence must stay visible to the caller deciding
    /// freshness.
    #[test]
    fn a_missing_version_stays_none() {
        let p: PublishDiagnostics =
            serde_json::from_str(r#"{"uri":"file:///a.nix","diagnostics":[]}"#).unwrap();
        assert_eq!(p.version, None);
    }

    /// A real server sends fields this struct does not model.
    #[test]
    fn unknown_wire_fields_are_ignored_not_fatal() {
        let raw = r#"{"uri":"file:///a.nix","version":3,"diagnostics":[
            {"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},
             "severity":2,"code":"sui/x","source":"sui","message":"m",
             "tags":[1],"relatedInformation":[],"data":{"k":"v"}}]}"#;
        let p: PublishDiagnostics = serde_json::from_str(raw).unwrap();
        let f = to_findings(&p, "abc", None);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(
            f[0].detail.as_deref(),
            Some("sui/x"),
            "the code is kept as detail"
        );
    }
}
