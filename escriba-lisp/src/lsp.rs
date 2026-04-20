//! `deflsp` — Lisp-authored LSP server binding.
//!
//! Absorbs nvim-lspconfig's per-server config + mason's installer
//! into one declarative spec. A binding here tells the runtime:
//!
//! - which server binary to launch,
//! - with which arguments,
//! - on which filetypes,
//! - what to consider the workspace root (marker files),
//! - what optional init-options to hand the server.
//!
//! ```lisp
//! (deflsp :name "rust-analyzer"
//!         :command "rust-analyzer"
//!         :filetypes ("rust")
//!         :root-markers ("Cargo.toml" "rust-project.json"))
//!
//! (deflsp :name "typescript"
//!         :command "typescript-language-server"
//!         :args ("--stdio")
//!         :filetypes ("typescript" "javascript" "tsx")
//!         :root-markers ("tsconfig.json" "package.json" "jsconfig.json"))
//! ```
//!
//! Unlike nvim-lspconfig, there's no magic server registry — every
//! server is explicit. This keeps the escriba-lisp crate zero-deps
//! and lets users layer their own servers without monkey-patching.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "deflsp")]
pub struct LspServerSpec {
    /// Human-readable server id — unique within the plan.
    pub name: String,
    /// Executable to run (`"rust-analyzer"`, `"pyright"`). The
    /// runtime resolves against `$PATH`.
    pub command: String,
    /// Extra arguments to pass to the server.
    #[serde(default)]
    pub args: Vec<String>,
    /// Filetypes that auto-attach this server. Matches
    /// [`MajorModeSpec`](crate::MajorModeSpec) `:name` values.
    #[serde(default)]
    pub filetypes: Vec<String>,
    /// Marker files / directories that establish the workspace
    /// root. First match wins (`"Cargo.toml"` beats `".git"`).
    #[serde(default)]
    pub root_markers: Vec<String>,
    /// Optional JSON-encoded init-options string (servers that
    /// take fancy configuration — `typescript-language-server`,
    /// `lua-ls`, etc.).
    #[serde(default)]
    pub init_options: String,
    /// Opt-out of auto-attach on matching filetypes. The default
    /// (`false`) means the server attaches automatically when a
    /// matching buffer opens. Set `:manual-only #t` to require a
    /// user-invoked `:LspStart`.
    #[serde(default)]
    pub manual_only: bool,
}

/// Canonical well-known servers. Purely advisory — any `command`
/// works at runtime. These are the names blnvim's mason-lspconfig
/// set ships by default, plus a few popular additions.
pub const KNOWN_SERVERS: &[&str] = &[
    "rust-analyzer",
    "typescript-language-server",
    "pyright",
    "gopls",
    "lua-language-server",
    "nil", // Nix LS
    "nixd",
    "clangd",
    "zls",
    "bash-language-server",
    "yaml-language-server",
    "jsonls",
    "tailwindcss-language-server",
    "svelte-language-server",
    "taplo", // TOML
    "marksman",
    "terraformls",
    "ansiblels",
    "dockerls",
    "graphql-language-service-cli",
    "elmls",
    "ocamllsp",
    "ruff-lsp",
    "solargraph",
    "prismals",
];

#[must_use]
pub fn is_known_server(name: &str) -> bool {
    KNOWN_SERVERS.iter().any(|s| *s == name)
}
