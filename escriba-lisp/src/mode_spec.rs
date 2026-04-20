//! `defmode` — Lisp-authored major mode.
//!
//! Absorbs emacs's `define-derived-mode` and neovim's `ftdetect` +
//! `ftplugin` pair into one declarative form. A major mode here is a
//! bundle of:
//!
//! - a name (`"rust"`, `"lisp"`, `"python"`),
//! - the file extensions that activate it,
//! - the tree-sitter language id used for parse + highlight
//!   (`escriba-ts` resolves this),
//! - the comment string template (vim's `commentstring`),
//! - an indentation width,
//! - an optional `:structural-lisp` flag for paredit-grade motions.
//!
//! Richer modes (lsp-server choice, formatter command, test runner)
//! can be layered via separate def-forms that reference `:mode "rust"`
//! — keeping this one focused on static per-filetype facts.
//!
//! ```lisp
//! (defmode :name "rust"
//!          :extensions ("rs")
//!          :tree-sitter "rust"
//!          :commentstring "// %s"
//!          :indent 4)
//!
//! (defmode :name "lisp"
//!          :extensions ("lisp" "cl" "el")
//!          :tree-sitter "commonlisp"
//!          :commentstring ";; %s"
//!          :indent 2
//!          :structural-lisp #t)
//! ```
//!
//! The in-tree `defft` form stays as the lightweight "one extension,
//! one mode" mapping for cases that don't need the full bundle;
//! `defmode` is for when the editor should actually *know* the language.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defmode")]
pub struct MajorModeSpec {
    /// Short human-readable name (`"rust"`, `"python"`, `"lisp"`).
    pub name: String,
    /// File extensions (no dot) that activate this mode. `["rs"]`,
    /// `["lisp", "cl", "el"]`, etc.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Tree-sitter language identifier. Must match a grammar
    /// registered with `escriba-ts` (or its resolver); a missing
    /// grammar degrades to plain-text display, never errors.
    #[serde(default)]
    pub tree_sitter: String,
    /// `%s`-template for line comments. `"// %s"` for rust,
    /// `";; %s"` for lisp, etc. Absence means "mode has no comment
    /// support" — motions like "comment-region" become no-ops.
    #[serde(default)]
    pub commentstring: String,
    /// Indent width in spaces. 0 means "use editor default".
    #[serde(default)]
    pub indent: u32,
    /// Whether paredit-grade structural motions are enabled
    /// (`forward-sexp`, `up-list`, …). Matches the
    /// `Motion::is_structural` classification in `escriba-core`.
    #[serde(default)]
    pub structural_lisp: bool,
}
