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
///
/// Mirrors neovim's autocmd event vocabulary so every blnvim /
/// nvim user recipe ports across directly. Events are alphabetised
/// within category bands so additions stay easy to review.
pub const KNOWN_EVENTS: &[&str] = &[
    // ── Buffer lifecycle ─────────────────────────────────────────
    "BufAdd",
    "BufDelete",
    "BufEnter",
    "BufFilePre",
    "BufFilePost",
    "BufHidden",
    "BufLeave",
    "BufModifiedSet",
    "BufNew",
    "BufNewFile",
    "BufReadCmd",
    "BufReadPost",
    "BufReadPre",
    "BufUnload",
    "BufWinEnter",
    "BufWinLeave",
    "BufWipeout",
    "BufWriteCmd",
    "BufWritePost",
    "BufWritePre",
    // ── Cursor / edit state ──────────────────────────────────────
    "CursorHold",
    "CursorHoldI",
    "CursorMoved",
    "CursorMovedI",
    "InsertChange",
    "InsertCharPre",
    "InsertEnter",
    "InsertLeave",
    "InsertLeavePre",
    "ModeChanged",
    "TextChanged",
    "TextChangedI",
    "TextYankPost",
    // ── Window / UI ──────────────────────────────────────────────
    "ColorScheme",
    "ColorSchemePre",
    "FocusGained",
    "FocusLost",
    "WinClosed",
    "WinEnter",
    "WinLeave",
    "WinNew",
    "WinResized",
    "WinScrolled",
    "TabEnter",
    "TabLeave",
    "TabClosed",
    "TabNew",
    "TabNewEntered",
    "VimEnter",
    "VimLeave",
    "VimLeavePre",
    "VimResized",
    "VimResume",
    "VimSuspend",
    // ── LSP / diagnostic ─────────────────────────────────────────
    "DiagnosticChanged",
    "LspAttach",
    "LspDetach",
    "LspNotify",
    "LspProgress",
    "LspRequest",
    "LspTokenUpdate",
    // ── Filesystem / search ──────────────────────────────────────
    "FileAppendCmd",
    "FileAppendPost",
    "FileAppendPre",
    "FileReadCmd",
    "FileReadPost",
    "FileReadPre",
    "FileType",
    "FileWriteCmd",
    "FileWritePost",
    "FileWritePre",
    "QuickFixCmdPost",
    "QuickFixCmdPre",
    "ShellCmdPost",
    "ShellFilterPost",
    "SignalUSR1",
    // ── Terminal ─────────────────────────────────────────────────
    "TermClose",
    "TermEnter",
    "TermLeave",
    "TermOpen",
    "TermRequest",
    "TermResponse",
    // ── Command-line / misc ──────────────────────────────────────
    "CmdlineChanged",
    "CmdlineEnter",
    "CmdlineLeave",
    "CmdUndefined",
    "CmdwinEnter",
    "CmdwinLeave",
    "CompleteChanged",
    "CompleteDone",
    "CompleteDonePre",
    "MenuPopup",
    "OptionSet",
    "RecordingEnter",
    "RecordingLeave",
    "SearchWrapped",
    "SessionLoadPost",
    "SessionWritePost",
    "Signal",
    "SourceCmd",
    "SourcePost",
    "SourcePre",
    "SpellFileMissing",
    "StdinReadPost",
    "StdinReadPre",
    "SwapExists",
    "Syntax",
    "UIEnter",
    "UILeave",
    "User",
];

#[must_use]
pub fn is_known_event(name: &str) -> bool {
    KNOWN_EVENTS.iter().any(|e| *e == name)
}
