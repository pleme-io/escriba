//! `defplugin` — Lisp-authored plugin declaration.
//!
//! Analog of `blackmatter-nvim`'s per-plugin `default.nix` bundle,
//! distilled to a pure-Rust spec. Every escriba plugin — whether
//! it's re-implementing a blnvim behaviour (oil, gitsigns, trouble,
//! lspsaga, telescope, …) or shipping new capability — declares its
//! metadata + activation surface through this form.
//!
//! ```lisp
//! (defplugin :name "trouble"
//!            :description "Diagnostic + quickfix + loclist list UI"
//!            :category "lsp"
//!            :on-event "LspAttach"
//!            :keybinds ("<leader>xx" "<leader>xw")
//!            :lazy #t)
//!
//! (defplugin :name "oil"
//!            :description "Edit the filesystem like a buffer"
//!            :category "files"
//!            :on-command "Oil"
//!            :lazy #t)
//! ```
//!
//! # Fields
//!
//! - `name` — human-readable identifier, unique within the plan.
//! - `description` — one-line purpose sentence.
//! - `category` — coarse grouping mirroring blnvim's feature groups
//!   (`"lsp"`, `"completion"`, `"theming"`, `"telescope"`, `"files"`,
//!   `"git"`, `"treesitter"`, `"keybindings"`, `"tmux"`, `"common"`).
//! - `on-event` — optional lazy-load trigger (nvim-style autocmd
//!   event — `"BufReadPost"`, `"LspAttach"`, `"InsertEnter"`).
//! - `on-command` — optional user-command trigger (plugin loads when
//!   the user runs the named command).
//! - `on-filetype` — optional filetype trigger (lazy-load on entering
//!   a buffer of that ft).
//! - `keybinds` — optional list of keybinding triggers (plugin loads
//!   the first time any of these is pressed).
//! - `lazy` — if true, none of the above trigger? defer until the
//!   first explicit load request. Combined with any of the on-*
//!   fields, loads on the *union* of the triggers.
//! - `priority` — load-order hint (colorschemes use a high value so
//!   they win over later theming plugins). 0 = default.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defplugin")]
pub struct PluginSpec {
    /// Unique name within the plan.
    pub name: String,
    /// One-line description shown in the plugin list.
    #[serde(default)]
    pub description: String,
    /// Feature group — free-form, but canonical values are the nine
    /// from blnvim: common / completion / formatting / keybindings /
    /// lsp / telescope / theming / tmux / treesitter, plus `"files"`,
    /// `"git"`, and `"ai"` added for escriba-specific categories.
    #[serde(default)]
    pub category: String,
    /// Autocmd-style event trigger for lazy loading (`"BufReadPost"`,
    /// `"LspAttach"`, …).
    #[serde(default)]
    pub on_event: String,
    /// User-command trigger for lazy loading (plugin loads when the
    /// named ex-command is invoked).
    #[serde(default)]
    pub on_command: String,
    /// Filetype trigger for lazy loading.
    #[serde(default)]
    pub on_filetype: String,
    /// Keybinding triggers — plugin loads the first time any of
    /// these keys / sequences are pressed.
    #[serde(default)]
    pub keybinds: Vec<String>,
    /// If true, the plugin is lazy-loaded per the triggers above.
    /// If false (default), it loads at startup.
    #[serde(default)]
    pub lazy: bool,
    /// Load-order hint; higher runs first. Colorschemes typically
    /// want `:priority 1000` to beat generic plugins.
    #[serde(default)]
    pub priority: i32,
}

/// Canonical blnvim categories plus escriba additions. Accepted in
/// any casing; unknown values are allowed (forward-compat with
/// user-authored categories).
pub const KNOWN_CATEGORIES: &[&str] = &[
    "common",
    "completion",
    "formatting",
    "keybindings",
    "lsp",
    "telescope",
    "theming",
    "tmux",
    "treesitter",
    "files",
    "git",
    "ai",
];

#[must_use]
pub fn is_known_category(name: &str) -> bool {
    KNOWN_CATEGORIES
        .iter()
        .any(|c| c.eq_ignore_ascii_case(name))
}
