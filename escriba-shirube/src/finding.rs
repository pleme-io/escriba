//! `Finding` — one located thing worth navigating to.
//!
//! A diagnostic, a git hunk, a test failure, a grep hit, a TODO, a merge
//! conflict, an LSP reference. Seven subsystems in escriba's backlog, and
//! every one of them is a place in a file with something to say about it.
//! Modelling that once is the whole bet of this crate: each producer then
//! ships a SOURCE, not a subsystem, and the gutter, the list surface and the
//! `]x`/`[x` navigation are written once rather than seven times.

use escriba_core::{BufferId, Position, Range};

/// Where a finding is.
///
/// A path AND an optional buffer id, because both are real and neither
/// subsumes the other: a diagnostic arrives for a file that may not be open
/// (so a path), and a TODO scan runs over a buffer that may never have been
/// saved (so an id). Carrying both means a producer states what it actually
/// knows instead of inventing the other half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub path: Option<std::path::PathBuf>,
    pub buffer: Option<BufferId>,
    pub range: Range,
}

impl Site {
    /// A site inside an open buffer.
    #[must_use]
    pub fn in_buffer(buffer: BufferId, range: Range) -> Self {
        Self {
            path: None,
            buffer: Some(buffer),
            range,
        }
    }

    /// A site in a file, open or not.
    #[must_use]
    pub fn in_file(path: impl Into<std::path::PathBuf>, range: Range) -> Self {
        Self {
            path: Some(path.into()),
            buffer: None,
            range,
        }
    }

    #[must_use]
    pub fn with_buffer(mut self, buffer: BufferId) -> Self {
        self.buffer = Some(buffer);
        self
    }

    /// The line this finding starts on — what the gutter marks and what
    /// navigation orders by.
    #[must_use]
    pub const fn line(&self) -> u32 {
        self.range.start.line
    }
}

/// How much a finding matters.
///
/// Ordered worst-first so `severity <= Warning` reads as "at least a
/// warning", and so a gutter showing one mark per line can show the WORST
/// rather than the last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

impl Severity {
    /// A fixed-width tag for a list row.
    ///
    /// Lives on the type rather than in a face so the three faces cannot
    /// disagree about what a severity is called — the same reason colours
    /// resolve through one `ChromePalette`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warning => "WARN ",
            Self::Info => "INFO ",
            Self::Hint => "HINT ",
        }
    }
}

/// Who produced a finding.
///
/// Kept as a typed enum rather than a free string so a list can be filtered
/// by producer without matching on prose, and so adding a producer is a
/// visible change rather than a new magic string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Scanned out of buffer text — TODO/FIXME markers, conflict markers.
    Text(&'static str),
    /// A language server.
    Lsp(String),
    /// Version control.
    Vcs,
    /// A test run.
    Test,
    /// A search.
    Search,
}

/// One located finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub site: Site,
    pub severity: Severity,
    /// One line, shown in the gutter tooltip and the list surface.
    pub message: String,
    /// Longer text, shown only when a surface has room.
    pub detail: Option<String>,
    pub origin: Origin,
}

impl Finding {
    #[must_use]
    pub fn new(site: Site, severity: Severity, message: impl Into<String>, origin: Origin) -> Self {
        Self {
            site,
            severity,
            message: message.into(),
            detail: None,
            origin,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sort key: by file, then by position.
    ///
    /// Navigation order is a property of the LIST, not of whatever order a
    /// producer happened to emit in. An LSP that returns diagnostics
    /// unordered must not make `]d` jump around.
    #[must_use]
    pub fn sort_key(&self) -> (Option<&std::path::Path>, u32, u32) {
        (
            self.site.path.as_deref(),
            self.site.range.start.line,
            self.site.range.start.column,
        )
    }
}

/// Where a finding starts, as a flat position — the coordinate navigation
/// walks in.
#[must_use]
pub fn start_of(f: &Finding) -> Position {
    f.site.range.start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_orders_worst_first() {
        // So `severity <= Warning` reads as "at least a warning", and a
        // gutter with one mark per line can show the worst rather than the
        // last one to arrive.
        assert!(Severity::Error < Severity::Warning);
        assert!(Severity::Warning < Severity::Info);
        assert!(Severity::Info < Severity::Hint);
        let mut v = vec![Severity::Hint, Severity::Error, Severity::Info];
        v.sort_unstable();
        assert_eq!(v, vec![Severity::Error, Severity::Info, Severity::Hint]);
    }

    #[test]
    fn a_site_can_be_a_buffer_a_file_or_both() {
        let r = Range::point(Position::new(3, 0));
        let b = Site::in_buffer(BufferId(1), r.clone());
        assert_eq!(b.buffer, Some(BufferId(1)));
        assert!(b.path.is_none(), "an unsaved buffer has no path to invent");

        let f = Site::in_file("/tmp/a.rs", r.clone());
        assert!(f.buffer.is_none(), "a closed file has no buffer to invent");

        let both = Site::in_file("/tmp/a.rs", r).with_buffer(BufferId(2));
        assert!(both.path.is_some() && both.buffer.is_some());
    }

    #[test]
    fn sorting_is_by_position_not_arrival() {
        // An LSP may return diagnostics in any order; `]d` must not jump
        // around because of it.
        let at = |line| {
            Finding::new(
                Site::in_buffer(BufferId(1), Range::point(Position::new(line, 0))),
                Severity::Error,
                "x",
                Origin::Lsp("rust-analyzer".into()),
            )
        };
        let mut v = vec![at(9), at(2), at(5)];
        v.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        assert_eq!(
            v.iter().map(|f| f.site.line()).collect::<Vec<_>>(),
            vec![2, 5, 9],
        );
    }
}
