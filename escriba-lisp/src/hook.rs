//! `defhook` — Lisp-authored editor event hook.
//!
//! Events mirror Neovim's autocmd taxonomy so users coming from nvim
//! know what to reach for:
//!
//! - `BufReadPre` / `BufReadPost` — around a buffer load.
//! - `BufWritePre` / `BufWritePost` — around a buffer write.
//! - `ModeChanged` — on modal transitions (optional `:from` / `:to`).
//! - `CursorMoved` — cursor position changed.
//! - `BufEnter` / `BufLeave` — focus moving between buffers.
//! - `VimEnter` / `VimLeave` — startup / shutdown.
//!
//! The command string is the name of a command registered via
//! `defcmd` (or a built-in command); the registry resolves it at
//! dispatch time.
//!
//! ```lisp
//! (defhook :event "BufWritePost" :command "run-formatter")
//! (defhook :event "ModeChanged"  :to "insert"
//!          :command "highlight-cursor-line")
//! ```

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defhook")]
pub struct HookSpec {
    /// The autocmd-style event name.
    pub event: String,
    /// The command (registered name) to invoke when the hook fires.
    pub command: String,
    /// Optional `ModeChanged :from` qualifier.
    #[serde(default)]
    pub from: String,
    /// Optional `ModeChanged :to` qualifier.
    #[serde(default)]
    pub to: String,
    /// Optional buffer pattern qualifier (e.g., `"*.rs"` for BufReadPost).
    #[serde(default)]
    pub pattern: String,
}

/// The canonical event list — unknown events in a spec are rejected.
pub const KNOWN_EVENTS: &[&str] = &[
    "BufReadPre",
    "BufReadPost",
    "BufWritePre",
    "BufWritePost",
    "ModeChanged",
    "CursorMoved",
    "BufEnter",
    "BufLeave",
    "VimEnter",
    "VimLeave",
];

#[must_use]
pub fn is_known_event(name: &str) -> bool {
    KNOWN_EVENTS.iter().any(|e| *e == name)
}
