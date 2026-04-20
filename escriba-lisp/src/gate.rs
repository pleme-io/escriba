//! `defgate` — convergence gate bound to an editor event.
//!
//! Invention: no existing editor (vim / nvim / helix / zed / vscode /
//! emacs / cursor / kakoune) exposes convergence gates as a typed
//! first-class declarative primitive. They all have *hooks* that
//! run arbitrary code, and some (nvim `ftplugin`, emacs
//! `before-save-hook`) can fail the underlying event if the code
//! errors — but the semantics are ad-hoc and procedural.
//!
//! `defgate` names a **typed pre-/post-condition** wired to an
//! editor event. Gate structure mirrors the convergence-computing
//! pattern used across pleme-io (tatara, kenshi, sekiban): each
//! gate is a point with prepare / execute / verify, and the
//! outcome drives one of three typed actions — `reject`, `warn`,
//! or `auto-fix`.
//!
//! ```lisp
//! ;; Reject buffer writes that contain secrets.
//! (defgate :name "no-secrets"
//!          :on-event "BufWritePre"
//!          :command "rg -U '(password|token|key):\\s*\"?[A-Za-z0-9]{10,}' $FILE"
//!          :action "reject"
//!          :message "Likely secret committed — blocked.")
//!
//! ;; Auto-format on save when rustfmt finds drift.
//! (defgate :name "rust-format"
//!          :on-event "BufWritePre"
//!          :filetype "rust"
//!          :command "rustfmt --check $FILE"
//!          :action "auto-fix"
//!          :auto-fix "rustfmt $FILE")
//!
//! ;; Warn (don't block) when the LSP reports unresolved errors.
//! (defgate :name "lsp-clean"
//!          :on-event "BufWritePost"
//!          :source "lsp.diagnostics"
//!          :severity "error"
//!          :action "warn")
//! ```
//!
//! # Semantics
//!
//! - `on-event` — one of [`crate::KNOWN_EVENTS`]. The gate fires as
//!   a middleware around the event: if `action = "reject"` and the
//!   check fails, the event is aborted.
//! - `command` (shell mode) or `source` (typed-source mode) — exactly
//!   one must be set. Command mode runs a shell command whose exit
//!   code decides pass/fail (0 = pass). Source mode reads from a
//!   typed provider (`lsp.diagnostics`, `formatter.drift`, …).
//! - `action` — `reject` (abort the event), `warn` (log + let it
//!   through), `auto-fix` (run `:auto-fix` and retry once).
//! - `auto-fix` — shell command to run when `action = "auto-fix"`
//!   and the primary check fails. If the retry still fails, falls
//!   back to `reject` semantics (so auto-fix never silently
//!   swallows a real error).
//! - `filetype` — optional narrowing; empty means all filetypes.
//! - `severity` — when `source = "lsp.diagnostics"`, the minimum
//!   severity that triggers failure (`hint` / `info` / `warn` /
//!   `error`).
//! - `message` — user-facing reason shown on failure.
//! - `timeout-ms` — hard cap on gate execution; 0 uses the
//!   runtime default (typically 5000 ms). A timeout counts as a
//!   fail for safety.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defgate")]
pub struct GateSpec {
    /// Human-readable gate id — unique within the plan.
    pub name: String,
    /// Event the gate wraps. One of `crate::KNOWN_EVENTS`.
    pub on_event: String,
    /// Optional filetype narrowing. Empty = all filetypes.
    #[serde(default)]
    pub filetype: String,
    /// Shell command to run. Mutually exclusive with `source`.
    /// `$FILE` / `$BUFFER_TEXT` tokens are substituted at dispatch.
    #[serde(default)]
    pub command: String,
    /// Typed source — e.g. `"lsp.diagnostics"`, `"formatter.drift"`,
    /// `"ts.query"`. Mutually exclusive with `command`.
    #[serde(default)]
    pub source: String,
    /// LSP severity threshold when `source = "lsp.diagnostics"`:
    /// one of `"hint"` / `"info"` / `"warn"` / `"error"`.
    #[serde(default)]
    pub severity: String,
    /// Action on failure: `"reject"` / `"warn"` / `"auto-fix"`.
    pub action: String,
    /// Auto-fix shell command; required when `action = "auto-fix"`.
    #[serde(default)]
    pub auto_fix: String,
    /// User-facing reason shown on failure.
    #[serde(default)]
    pub message: String,
    /// Timeout cap for the gate run, in milliseconds. 0 = runtime
    /// default.
    #[serde(default)]
    pub timeout_ms: u64,
}

/// The set of valid `:action` values. Unknown values are rejected
/// at apply time.
pub const KNOWN_ACTIONS: &[&str] = &["reject", "warn", "auto-fix"];

/// The set of valid `:severity` values for LSP-diagnostic sources.
pub const KNOWN_SEVERITIES: &[&str] = &["hint", "info", "warn", "error"];

/// The set of canonical typed `:source` providers the runtime
/// understands out of the box. Custom sources can be registered by
/// plugins — unknown values pass through.
pub const KNOWN_SOURCES: &[&str] = &[
    "lsp.diagnostics",
    "formatter.drift",
    "ts.query",
    "git.status",
    "secrets.scan",
    "type.check",
];

#[must_use]
pub fn is_known_action(name: &str) -> bool {
    KNOWN_ACTIONS.iter().any(|a| *a == name)
}

#[must_use]
pub fn is_known_severity(name: &str) -> bool {
    KNOWN_SEVERITIES.iter().any(|s| *s == name)
}

#[must_use]
pub fn is_known_source(name: &str) -> bool {
    KNOWN_SOURCES.iter().any(|s| *s == name)
}

impl GateSpec {
    /// `command`-mode gates execute a shell command; `source`-mode
    /// gates read from a typed provider. A valid spec sets exactly
    /// one.
    #[must_use]
    pub fn mode(&self) -> GateMode {
        match (self.command.is_empty(), self.source.is_empty()) {
            (false, true) => GateMode::Command,
            (true, false) => GateMode::Source,
            (true, true) => GateMode::Invalid,
            (false, false) => GateMode::Invalid,
        }
    }
}

/// Which provider backs a gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateMode {
    /// Runs a shell command; exit code decides pass/fail.
    Command,
    /// Reads a typed provider (`lsp.diagnostics`, …).
    Source,
    /// Neither or both set — spec is ill-formed.
    Invalid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_classifies_spec_shape() {
        let cmd = GateSpec {
            name: "x".into(),
            on_event: "BufWritePre".into(),
            command: "rustfmt --check $FILE".into(),
            action: "reject".into(),
            ..Default::default()
        };
        assert_eq!(cmd.mode(), GateMode::Command);

        let src = GateSpec {
            name: "y".into(),
            on_event: "BufWritePost".into(),
            source: "lsp.diagnostics".into(),
            action: "warn".into(),
            ..Default::default()
        };
        assert_eq!(src.mode(), GateMode::Source);

        let bad = GateSpec {
            name: "z".into(),
            on_event: "BufWritePre".into(),
            action: "reject".into(),
            ..Default::default()
        };
        assert_eq!(bad.mode(), GateMode::Invalid);

        let both = GateSpec {
            name: "w".into(),
            on_event: "BufWritePre".into(),
            command: "x".into(),
            source: "y".into(),
            action: "reject".into(),
            ..Default::default()
        };
        assert_eq!(both.mode(), GateMode::Invalid);
    }

    #[test]
    fn known_action_classifier_is_strict() {
        assert!(is_known_action("reject"));
        assert!(is_known_action("warn"));
        assert!(is_known_action("auto-fix"));
        assert!(!is_known_action("AutoFix"));
        assert!(!is_known_action("log"));
    }
}

impl Default for GateSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            on_event: String::new(),
            filetype: String::new(),
            command: String::new(),
            source: String::new(),
            severity: String::new(),
            action: String::new(),
            auto_fix: String::new(),
            message: String::new(),
            timeout_ms: 0,
        }
    }
}
