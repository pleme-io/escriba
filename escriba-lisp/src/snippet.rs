//! `defsnippet` — Lisp-authored insert-mode snippet.
//!
//! Body uses the LSP snippet grammar so any existing corpus
//! (LuaSnip, UltiSnips compat layer, vscode snippets json) maps
//! cleanly — `${1:placeholder}`, `${2}`, `$0` for the final cursor.
//!
//! ```lisp
//! (defsnippet :trigger "fn"
//!             :body "fn ${1:name}(${2}) -> ${3} { ${0} }")
//! (defsnippet :trigger "mod-test"
//!             :body "#[cfg(test)] mod tests { ${0} }")
//! ```

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defsnippet")]
pub struct SnippetSpec {
    /// Literal trigger text — typed in insert mode, expanded on a
    /// dedicated key (tab by default).
    pub trigger: String,
    /// LSP-snippet grammar body.
    pub body: String,
    /// Optional filetype scope (matches `defft :mode`). Empty = global.
    #[serde(default)]
    pub filetype: String,
    /// Optional description for the picker preview.
    #[serde(default)]
    pub description: String,
}
