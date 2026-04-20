//! `defft` — Lisp-authored filetype routing.
//!
//! Maps a filename extension to a tree-sitter language / major mode.
//!
//! ```lisp
//! (defft :ext "rs"    :mode "rust")
//! (defft :ext "py"    :mode "python")
//! (defft :ext "lisp"  :mode "lisp")
//! (defft :ext "md"    :mode "markdown")
//! ```
//!
//! The mapping populates the tree-sitter language resolver in
//! `escriba-ts` and the per-buffer major-mode slot in `escriba-mode`.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defft")]
pub struct FiletypeSpec {
    /// File extension, no dot (`"rs"`, not `".rs"`).
    pub ext: String,
    /// Major mode / tree-sitter language name.
    pub mode: String,
}
