//! `defkeybind` — Lisp-authored `(Mode, Key) → Action` binding.
//!
//! ```lisp
//! (defkeybind :mode "normal" :key "gh" :action "goto-home")
//! (defkeybind :mode "insert" :key "jk" :action "escape-to-normal")
//! ```
//!
//! `mode` values: `"normal"`, `"insert"`, `"visual"`, `"visual-line"`,
//! `"command"`. `key` is either a single printable char (`"a"`, `"G"`)
//! or a bracketed special (`"<Esc>"`, `"<C-r>"`, `"<Tab>"`, `"<CR>"`).
//! Multi-key sequences like `"gh"` are expanded into pending-key state
//! by the keymap at dispatch time.
//!
//! `action` is a free-form string the command dispatcher resolves at
//! apply time. Unknown actions are silently ignored (forward-compat
//! with future action names).

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defkeybind")]
pub struct KeybindSpec {
    /// Which mode the binding lives in. Unknown modes are rejected.
    pub mode: String,
    /// The key or key sequence (single char or `<Name>` form).
    pub key: String,
    /// The action name the keymap invokes on match.
    pub action: String,
    /// Optional human-readable description for the which-key popup.
    #[serde(default)]
    pub description: String,
}
