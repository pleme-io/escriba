//! `deftextobject` — Lisp-authored tree-sitter text objects.
//!
//! Absorbs nvim-treesitter-textobjects into a typed declarative
//! form. A text object is a named scope over the tree-sitter AST
//! that vim operators and motions can target:
//!
//! ```text
//! vim keys        what it means                   needs this object
//! -----------------------------------------------------------------
//! vif             visually select inside function   function.inner
//! daf             delete a function (w/ spacing)    function.outer
//! cip             change inside paragraph           paragraph.inner
//! vac             visually select around class      class.outer
//! ```
//!
//! Each text object bundles:
//!
//! - a **name** — the key suffix vim binds it to (`f` → function, `p`
//!   → paragraph, `c` → class, `a` → argument).
//! - a **scope** — `inner` or `outer`. Outer swallows surrounding
//!   whitespace / delimiters; inner strips them.
//! - a **query** — tree-sitter s-expression capturing the AST node
//!   the object represents.
//! - an optional **filetype** — the grammar this query binds to.
//!   Empty means "applies to every grammar that matches the query".
//!
//! ```lisp
//! (deftextobject :name "function"
//!                :scope "outer"
//!                :filetype "rust"
//!                :query "(function_item) @function.outer")
//!
//! (deftextobject :name "function"
//!                :scope "inner"
//!                :filetype "rust"
//!                :query "(function_item body: (block) @function.inner)")
//!
//! (deftextobject :name "class"
//!                :scope "outer"
//!                :filetype "python"
//!                :query "(class_definition) @class.outer")
//! ```
//!
//! The runtime composes these with the operator/motion table: when
//! the user types `vif`, the dispatcher looks up `(f, inner)` for
//! the active buffer's filetype, runs the captured query, and
//! selects the matching AST node. Compositional with vim grammar
//! — no per-plugin keybind plumbing.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "deftextobject")]
pub struct TextObjectSpec {
    /// Name the vim operator/motion table uses (`f`, `p`, `c`, `a`,
    /// `n`, `i`, …). Matches the post-`i`/`a` character in
    /// `{v,d,c,y}{i,a}X` sequences.
    pub name: String,
    /// `"inner"` (strip delimiters/whitespace) or `"outer"` (include
    /// them). Unknown values are rejected at apply time.
    pub scope: String,
    /// Tree-sitter query — an s-expression matching the AST node
    /// this object represents. Capture names typically echo
    /// `{name}.{scope}` (e.g. `@function.inner`) so consumers can
    /// cross-reference.
    pub query: String,
    /// Filetype the query binds to (matches
    /// [`MajorModeSpec`](crate::MajorModeSpec) `:name` /
    /// [`LspServerSpec`](crate::LspServerSpec) `:filetypes`).
    /// Empty = applies to every grammar the query happens to match.
    #[serde(default)]
    pub filetype: String,
    /// Human-readable description shown in which-key popups.
    #[serde(default)]
    pub description: String,
}

/// The two scope kinds a text object can carry. `inner` strips the
/// surrounding delimiters (whitespace, braces, quotes); `outer`
/// includes them. Matches vim's `i`/`a` modifier grammar.
pub const KNOWN_SCOPES: &[&str] = &["inner", "outer"];

#[must_use]
pub fn is_known_scope(name: &str) -> bool {
    KNOWN_SCOPES.iter().any(|s| *s == name)
}

/// Canonical text-object short names used across the category —
/// `f` for function, `c` for class, `p` for paragraph, `a` for
/// argument, etc. Purely advisory — any `:name` string works.
pub const CANONICAL_NAMES: &[(&str, &str)] = &[
    ("f", "function"),
    ("c", "class"),
    ("p", "paragraph"),
    ("a", "argument"),
    ("b", "block"),
    ("l", "loop"),
    ("n", "number"),
    ("i", "conditional"),
    ("A", "assignment"),
    ("r", "return"),
    ("o", "comment"),
    ("t", "call"),
];

/// True when `name` is one of the canonical single-letter aliases —
/// used for which-key rollups. Long names always work; this just
/// lets discovery UIs know the short form is "standard".
#[must_use]
pub fn is_canonical_short(name: &str) -> bool {
    CANONICAL_NAMES.iter().any(|(k, _)| *k == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_scope_classifier() {
        assert!(is_known_scope("inner"));
        assert!(is_known_scope("outer"));
        assert!(!is_known_scope("around"));
        assert!(!is_known_scope("Inner"));
    }

    #[test]
    fn canonical_names_cover_common_objects() {
        assert!(is_canonical_short("f"));
        assert!(is_canonical_short("c"));
        assert!(is_canonical_short("p"));
        assert!(!is_canonical_short("z"));
    }
}
