//! `defdap` — Lisp-authored DAP debugger-adapter binding.
//!
//! DAP (Debug Adapter Protocol) is to debuggers what LSP is to
//! language servers. `defdap` parallels [`LspServerSpec`](crate::LspServerSpec)
//! for debugger setup — absorbs nvim-dap's adapter config into a
//! typed form.
//!
//! ```lisp
//! (defdap :name "lldb"
//!         :command "lldb-dap"
//!         :filetypes ("rust" "c" "cpp")
//!         :port 0)
//!
//! (defdap :name "debugpy"
//!         :command "python"
//!         :args ("-m" "debugpy.adapter")
//!         :filetypes ("python"))
//!
//! (defdap :name "delve"
//!         :command "dlv"
//!         :args ("dap" "-l" "127.0.0.1:38697")
//!         :filetypes ("go")
//!         :port 38697)
//! ```
//!
//! The runtime launches the adapter on demand (first `:Debug`
//! invocation or `<leader>db` keypress) — lazy by default.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defdap")]
pub struct DapAdapterSpec {
    /// Human-readable adapter id — unique within the plan.
    pub name: String,
    /// Executable to launch (`"lldb-dap"`, `"dlv"`, `"python"`).
    /// Runtime resolves against `$PATH`.
    pub command: String,
    /// Extra arguments to pass to the adapter.
    #[serde(default)]
    pub args: Vec<String>,
    /// Filetypes that trigger this adapter. Matches
    /// [`MajorModeSpec`](crate::MajorModeSpec) `:name`.
    #[serde(default)]
    pub filetypes: Vec<String>,
    /// TCP port the adapter listens on. `0` = stdio transport
    /// (preferred for lldb-dap / debugpy). Non-zero = TCP,
    /// common for `delve`.
    #[serde(default)]
    pub port: u32,
    /// Optional JSON-encoded init-configuration (passed to the
    /// adapter's `initialize` request). Advanced setups only.
    #[serde(default)]
    pub init_configuration: String,
    /// Wait this many ms for the adapter to come up on TCP port
    /// before giving up. Ignored for stdio. 0 = runtime default.
    #[serde(default)]
    pub startup_timeout_ms: u64,
}

/// Canonical well-known adapters — informational, not restrictive.
pub const KNOWN_ADAPTERS: &[&str] = &[
    "lldb",       // rust / c / c++
    "codelldb",   // vscode-lldb alternative
    "debugpy",    // python
    "delve",      // go
    "node2",      // node/js (deprecated, use js-debug)
    "js-debug",   // typescript / javascript (vscode)
    "chrome",     // chrome / edge web
    "firefox",    // firefox web
    "dart",       // dart / flutter
    "ruby-debug", // ruby
    "php",        // php (xdebug)
    "coreclr",    // .net
];

#[must_use]
pub fn is_known_adapter(name: &str) -> bool {
    KNOWN_ADAPTERS.iter().any(|a| *a == name)
}
