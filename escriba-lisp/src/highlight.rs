//! `defhighlight` — Lisp-authored syntax-group highlighting.
//!
//! Every editor in the category has this abstraction: vim
//! `:highlight`, nvim `vim.api.nvim_set_hl()`, emacs `defface`,
//! vscode themes, zed themes. Escriba collapses them into one
//! declarative form that reads per-group colours + attrs the
//! runtime applies on top of the active [`ThemeSpec`](crate::ThemeSpec).
//!
//! ```lisp
//! (defhighlight :group "Function" :fg "#88c0d0" :bold #t)
//! (defhighlight :group "Keyword"  :fg "#81a1c1" :italic #t)
//! (defhighlight :group "Comment"  :fg "#4c566a" :italic #t)
//! ;; Link one group to another (vim `:hi link Foo Bar`).
//! (defhighlight :group "@function.call" :link "Function")
//! ;; Error state gets an explicit background.
//! (defhighlight :group "DiagnosticError" :fg "#bf616a" :bg "#2e3440" :bold #t)
//! ```
//!
//! # Fields
//!
//! - `group` — the syntax group name. Follows nvim/tree-sitter
//!   conventions — `Function`, `Keyword`, `String`, `Comment`,
//!   `@function.call`, `@variable.member`, `DiagnosticError`,
//!   `GitSignsAdd`, `NormalFloat`, etc. Unknown groups are accepted
//!   (forward-compat with plugin-registered groups).
//! - `fg` / `bg` — hex colour `"#rrggbb"` or palette ref
//!   `"nord.frost0"` (resolver is runtime-side).
//! - `bold` / `italic` / `underline` / `undercurl` /
//!   `strikethrough` / `reverse` — boolean attrs (default false).
//! - `link` — link this group to another (tagged group takes its
//!   colours from the named link). `link` is mutually exclusive
//!   with colour fields — the runtime honours link if set.
//!
//! The runtime applies these *after* the [`ThemeSpec`](crate::ThemeSpec)
//! preset has populated the baseline, so users can customize on
//! top of a preset without reauthoring the full palette.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defhighlight")]
pub struct HighlightSpec {
    /// Syntax group name (`Function`, `Keyword`, `@variable`, …).
    pub group: String,
    /// Foreground — `"#rrggbb"` or palette ref.
    #[serde(default)]
    pub fg: String,
    /// Background — `"#rrggbb"` or palette ref.
    #[serde(default)]
    pub bg: String,
    /// Bold attribute.
    #[serde(default)]
    pub bold: bool,
    /// Italic attribute.
    #[serde(default)]
    pub italic: bool,
    /// Straight underline attribute.
    #[serde(default)]
    pub underline: bool,
    /// Squiggly underline attribute (for diagnostics).
    #[serde(default)]
    pub undercurl: bool,
    /// Strikethrough attribute.
    #[serde(default)]
    pub strikethrough: bool,
    /// Reverse video attribute (swap fg / bg).
    #[serde(default)]
    pub reverse: bool,
    /// Link to another group — takes priority over colour fields.
    #[serde(default)]
    pub link: String,
}

impl HighlightSpec {
    /// True when this spec is a pure link to another group (no
    /// colour / attr of its own).
    #[must_use]
    pub fn is_link(&self) -> bool {
        !self.link.is_empty()
    }

    /// True when any attribute flag is set.
    #[must_use]
    pub fn has_attrs(&self) -> bool {
        self.bold
            || self.italic
            || self.underline
            || self.undercurl
            || self.strikethrough
            || self.reverse
    }
}

/// Canonical syntax groups the runtime guarantees. Extending a
/// theme with groups outside this list still works — the runtime
/// just honours them lazily when tree-sitter / lsp attach.
pub const CANONICAL_GROUPS: &[&str] = &[
    // ── Syntax ────────────────────────────────────────────────────
    "Normal",
    "Comment",
    "String",
    "Number",
    "Boolean",
    "Function",
    "Keyword",
    "Statement",
    "Conditional",
    "Repeat",
    "Operator",
    "Type",
    "Structure",
    "Identifier",
    "Constant",
    "PreProc",
    "Macro",
    "Special",
    // ── UI ────────────────────────────────────────────────────────
    "CursorLine",
    "CursorColumn",
    "LineNr",
    "SignColumn",
    "Visual",
    "VisualNOS",
    "Search",
    "IncSearch",
    "MatchParen",
    "StatusLine",
    "StatusLineNC",
    "TabLine",
    "TabLineFill",
    "TabLineSel",
    "VertSplit",
    "Pmenu",
    "PmenuSel",
    "PmenuSbar",
    "PmenuThumb",
    "NormalFloat",
    "FloatBorder",
    // ── Diagnostics ───────────────────────────────────────────────
    "DiagnosticError",
    "DiagnosticWarn",
    "DiagnosticInfo",
    "DiagnosticHint",
    // ── Git ───────────────────────────────────────────────────────
    "GitSignsAdd",
    "GitSignsChange",
    "GitSignsDelete",
    "DiffAdd",
    "DiffChange",
    "DiffDelete",
    "DiffText",
];

#[must_use]
pub fn is_canonical_group(name: &str) -> bool {
    CANONICAL_GROUPS.iter().any(|g| *g == name)
}
