//! `defstatusline` — Lisp-authored status bar composition.
//!
//! Status lines across the category are all variants of the same
//! pattern: ordered segments, each with a label + data source +
//! optional colour + optional alignment. lualine, lightline, emacs
//! mode-line, vscode status bar — same shape. This form collapses
//! them to one spec.
//!
//! ```lisp
//! (defstatusline
//!   :left ((:segment "mode"  :highlight "StatusLineMode")
//>          (:segment "branch" :highlight "StatusLineBranch")
//!          (:segment "file"   :highlight "StatusLineFile"))
//!   :center ()
//!   :right ((:segment "diagnostics")
//!           (:segment "position")
//!           (:segment "filetype")
//!           (:segment "time" :format "%H:%M")))
//! ```
//!
//! Segments are free-form strings the runtime resolves via a
//! segment provider table. Known providers: `"mode"` (current
//! modal mode), `"branch"` (git), `"file"` (buffer filename),
//! `"filetype"` (major-mode name), `"diagnostics"` (lsp), `"lsp"`
//! (active lsp clients), `"position"` (cursor row:col), `"time"`
//! (local time via `:format`), `"counts"` (word/char), `"encoding"`,
//! `"fileformat"`.
//!
//! Unknown providers render as empty strings — plugins register new
//! providers as they load.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSegment {
    /// The provider name — resolved against a runtime segment table.
    pub segment: String,
    /// Syntax-group name applied to this segment's text (maps to a
    /// [`HighlightSpec`](crate::HighlightSpec)).
    #[serde(default)]
    pub highlight: String,
    /// Format string for providers that accept one (e.g. `"time"`
    /// takes a strftime spec).
    #[serde(default)]
    pub format: String,
    /// Optional literal prefix prepended to the rendered value
    /// (`" "` for a nerd-font branch icon, `":"` separator, …).
    #[serde(default)]
    pub prefix: String,
    /// Optional literal suffix appended to the rendered value.
    #[serde(default)]
    pub suffix: String,
}

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defstatusline")]
pub struct StatusLineSpec {
    /// Left-aligned segment list.
    #[serde(default)]
    pub left: Vec<StatusSegment>,
    /// Centre-aligned segments.
    #[serde(default)]
    pub center: Vec<StatusSegment>,
    /// Right-aligned segments.
    #[serde(default)]
    pub right: Vec<StatusSegment>,
}

impl StatusLineSpec {
    /// Count of segments across every alignment slot.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.left.len() + self.center.len() + self.right.len()
    }
}

/// Canonical segment provider names. Plugins register more at
/// runtime — unknown names pass through to an empty render.
pub const KNOWN_SEGMENTS: &[&str] = &[
    "mode",
    "branch",
    "file",
    "filetype",
    "diagnostics",
    "lsp",
    "position",
    "time",
    "counts",
    "encoding",
    "fileformat",
    "separator",
];

#[must_use]
pub fn is_known_segment(name: &str) -> bool {
    KNOWN_SEGMENTS.iter().any(|s| *s == name)
}
