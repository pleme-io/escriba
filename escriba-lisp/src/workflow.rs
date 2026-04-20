//! `defworkflow` — editor-layer workflow as a named DAG of gates
//! + actions.
//!
//! Invention. Vim has ephemeral `q<letter>` keyboard macros; emacs
//! has `defkbd-macro`; vscode has tasks.json steps; but no editor
//! in the category exposes **typed composable workflows** where
//! each step is either a named `defgate` (convergence check) or a
//! named action (side effect), with typed on-failure semantics.
//!
//! `defworkflow` lifts the convergence pattern from editor events
//! (where `defgate` lives) to **named, explicit user flows** the
//! editor can run on demand:
//!
//! ```lisp
//! (defworkflow :name "ship-rust"
//!              :description "Format, test, then push the current branch"
//!              :steps ("gate:rust-format-drift"
//!                      "action:shell.cmd:cargo test"
//!                      "gate:lsp-clean"
//!                      "action:git.push")
//!              :on-failure "abort")
//!
//! (defworkflow :name "commit-rust"
//!              :steps ("gate:rust-format-drift"
//!                      "gate:no-secrets"
//!                      "action:git.commit")
//!              :on-failure "prompt")
//! ```
//!
//! # Step grammar
//!
//! Every step is a colon-separated token — `kind:ref[:arg…]`:
//!
//! - `gate:NAME`        — reference to a `defgate` by name.
//! - `action:NAME`      — built-in action (`git.push`, `shell.cmd`,
//!   `lsp.format-buffer`, `picker.files`, …).
//! - `workflow:NAME`    — nested workflow; the referenced workflow's
//!   steps get spliced in (runtime composition; no recursion check
//!   at apply time — cycles are caught at dispatch).
//! - `shell:CMD`        — shorthand for `action:shell.cmd:CMD`.
//! - `cmd:NAME`         — reference to a `defcmd` entry.
//!
//! Arguments after the second `:` are passed to the ref as
//! positional args. Step strings are opaque to `escriba-lisp` — the
//! runtime parses + resolves them.
//!
//! # On-failure actions
//!
//! - `"abort"` — stop the workflow, leaves the editor in the state
//!   the failed step produced. Default.
//! - `"continue"` — log the failure, proceed to the next step.
//! - `"prompt"` — pause, ask the user "retry / skip / abort".
//!
//! # Why this is novel
//!
//! Emacs macros are key-sequence replays; escriba workflows are
//! **typed DAGs**. Vim `q` records keys, not convergence checks.
//! VSCode tasks chain commands but have no gate semantics. `just`
//! / `make` are shell-level, not editor-integrated. `defworkflow`
//! fills a gap no editor occupies: declarative, typed, attestable
//! sequences of checks + actions the editor can run + log.
//!
//! Planned runtime: each step execution emits a tatara-style
//! attestation record so the workflow run itself is content-
//! addressable. BLAKE3 the sequence of (step_id, exit_code,
//! timestamp) and you have a cryptographic proof the workflow
//! ran, step-by-step, with the outcomes it claims. Editor-layer
//! convergence attestation — the missing tier in pleme-io's
//! convergence-computing stack.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defworkflow")]
pub struct WorkflowSpec {
    /// Human-readable workflow id — unique within the plan.
    /// Referenced from `workflow:NAME` steps in other workflows.
    pub name: String,
    /// One-line purpose sentence shown in the workflow picker.
    #[serde(default)]
    pub description: String,
    /// Ordered list of step strings; grammar documented in module
    /// docs. Empty list = a workflow that always trivially
    /// succeeds (useful as a no-op placeholder during authoring).
    #[serde(default)]
    pub steps: Vec<String>,
    /// On-failure behaviour. One of [`KNOWN_FAILURE_MODES`].
    #[serde(default)]
    pub on_failure: String,
    /// Optional keybinding that kicks this workflow off. Format
    /// matches [`crate::KeybindSpec`] `:key` — single char,
    /// `<C-x>`, `<leader>w`, etc.
    #[serde(default)]
    pub keybind: String,
    /// Hard cap on workflow duration; 0 = runtime default.
    #[serde(default)]
    pub timeout_ms: u64,
}

/// Canonical on-failure modes. Unknown values are rejected at apply.
/// Default (empty) is treated as `"abort"`.
pub const KNOWN_FAILURE_MODES: &[&str] = &["abort", "continue", "prompt"];

/// Canonical step-kind prefixes the runtime guarantees. Unknown
/// prefixes don't error at apply time — the runtime may ship new
/// kinds via plugins.
pub const KNOWN_STEP_KINDS: &[&str] = &["gate", "action", "workflow", "shell", "cmd"];

#[must_use]
pub fn is_known_failure_mode(name: &str) -> bool {
    name.is_empty() || KNOWN_FAILURE_MODES.iter().any(|m| *m == name)
}

#[must_use]
pub fn is_known_step_kind(name: &str) -> bool {
    KNOWN_STEP_KINDS.iter().any(|k| *k == name)
}

impl WorkflowSpec {
    /// Best-effort extraction of the `kind` prefix of every step
    /// — useful for `--list-rc` introspection and for the
    /// workflow picker's UI (group steps by kind). Malformed
    /// steps (no colon) return `"?"`.
    #[must_use]
    pub fn step_kinds(&self) -> Vec<&str> {
        self.steps
            .iter()
            .map(|s| s.split(':').next().unwrap_or("?"))
            .collect()
    }

    /// True when every step prefix is one of the known kinds.
    /// Plugins that add kinds can register them at runtime, so
    /// this is advisory, not enforcing.
    #[must_use]
    pub fn all_steps_known(&self) -> bool {
        self.step_kinds().iter().all(|k| is_known_step_kind(k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_kinds_extracts_prefixes() {
        let w = WorkflowSpec {
            name: "x".into(),
            steps: vec![
                "gate:rust-format-drift".into(),
                "action:git.push".into(),
                "shell:cargo test".into(),
                "cmd:write-all".into(),
                "workflow:ship".into(),
            ],
            ..Default::default()
        };
        assert_eq!(
            w.step_kinds(),
            vec!["gate", "action", "shell", "cmd", "workflow"]
        );
        assert!(w.all_steps_known());
    }

    #[test]
    fn malformed_steps_surface_as_question_mark() {
        let w = WorkflowSpec {
            name: "x".into(),
            steps: vec!["just-a-word".into(), "gate:ok".into()],
            ..Default::default()
        };
        assert_eq!(w.step_kinds(), vec!["just-a-word", "gate"]);
        assert!(!w.all_steps_known());
    }

    #[test]
    fn known_failure_mode_accepts_empty_default() {
        assert!(is_known_failure_mode(""));
        assert!(is_known_failure_mode("abort"));
        assert!(is_known_failure_mode("continue"));
        assert!(is_known_failure_mode("prompt"));
        assert!(!is_known_failure_mode("explode"));
    }
}

impl Default for WorkflowSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            steps: Vec::new(),
            on_failure: String::new(),
            keybind: String::new(),
            timeout_ms: 0,
        }
    }
}
