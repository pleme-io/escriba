//! `defcmd` — Lisp-authored command-palette entry.
//!
//! ```lisp
//! (defcmd :name "write-all"
//!         :description "Write every modified buffer"
//!         :action "buffer.write-all")
//! (defcmd :name "fuzzy-find-files"
//!         :description "Pick a file from the workspace"
//!         :action "picker.files")
//! ```
//!
//! `action` is a dotted symbol the command dispatcher resolves at
//! apply time (similar to the [`KeybindSpec`](crate::KeybindSpec)
//! `action` field). The shape is deliberately light — heavier
//! command argument specs still live in `escriba-command::CommandSpec`.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defcmd")]
pub struct CmdSpec {
    /// The command's human-facing name — matches what the user types
    /// into the command palette.
    pub name: String,
    /// One-line description shown in the palette next to the name.
    #[serde(default)]
    pub description: String,
    /// Dotted action symbol resolved against the command dispatcher.
    pub action: String,
}
