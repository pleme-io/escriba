//! `defformatter` — Lisp-authored per-filetype formatter binding.
//!
//! Parallel to conform.nvim's per-ft formatter list. The rc tells
//! the runtime "for buffers of filetype `X`, run formatter `Y`
//! (with args `Z`) on `BufWritePre`".
//!
//! ```lisp
//! (defformatter :filetype "rust"       :command "rustfmt"
//!               :args ("--edition" "2024"))
//! (defformatter :filetype "python"     :command "ruff"
//!               :args ("format" "-"))
//! (defformatter :filetype "typescript" :command "prettier"
//!               :args ("--stdin-filepath" "$FILE"))
//! ```
//!
//! The `$FILE` token in `args` is substituted with the active
//! buffer's path at dispatch time; stdin is otherwise piped to the
//! formatter and stdout replaces the buffer.
//!
//! `auto-on-save` (default true) controls whether the formatter is
//! wired into the BufWritePre hook automatically. Off means "user
//! must run `:format` explicitly" — matches conform's `format_on_save`
//! knob.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defformatter")]
pub struct FormatterSpec {
    /// Filetype this formatter applies to (matches
    /// [`MajorModeSpec::name`](crate::MajorModeSpec)).
    pub filetype: String,
    /// Executable name — resolved against `$PATH`.
    pub command: String,
    /// Extra args. Supports `$FILE` substitution.
    #[serde(default)]
    pub args: Vec<String>,
    /// Opt-out of format-on-save. The default (`false`) means the
    /// formatter wires into `BufWritePre` automatically. Set
    /// `:manual-only #t` to require an explicit `:Format` command.
    #[serde(default)]
    pub manual_only: bool,
    /// Timeout in milliseconds — avoids hanging on a stuck
    /// formatter. 0 means "use runtime default" (typically 2000).
    #[serde(default)]
    pub timeout_ms: u64,
}
