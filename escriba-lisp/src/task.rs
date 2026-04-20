//! `deftask` — Lisp-authored runnable shell task.
//!
//! Absorbs vscode `tasks.json`, nvim `asynctasks`, jetbrains run
//! configurations, and emacs `projectile-run-command` into one typed
//! form. Sits between [`CmdSpec`](crate::CmdSpec) (palette-entry
//! referencing a typed action) and [`WorkflowSpec`](crate::WorkflowSpec)
//! (multi-step DAG of gates + actions): one shell command with
//! filetype + cwd + env scope.
//!
//! ```lisp
//! (deftask :name "cargo-test"
//!          :description "cargo test --workspace"
//!          :command "cargo"
//!          :args ("test" "--workspace")
//!          :filetype "rust"
//!          :keybind "<leader>rt")
//!
//! (deftask :name "fleet-rebuild"
//!          :description "nix run .#rebuild"
//!          :command "nix"
//!          :args ("run" ".#rebuild")
//!          :cwd "~/code/github/pleme-io/nix"
//!          :env ("RUST_LOG=warn")
//!          :background #t
//!          :keybind "<leader>rR")
//!
//! (deftask :name "rg-todos"
//!          :command "rg"
//!          :args ("-n" "TODO|FIXME" ".")
//!          :keybind "<leader>ft")
//! ```
//!
//! ## Why a dedicated form
//!
//! - `defcmd` declares a palette entry that dispatches to a typed
//!   `Action::Command { name, args }` — the runtime resolves `name`
//!   via the command registry. No shell.
//! - `defworkflow` chains multiple steps (gate:X / action:Y /
//!   workflow:Z / shell:CMD) with typed on-failure semantics.
//! - `deftask` is the middle ground: one shell invocation with
//!   filetype scope, cwd, env, optional `:background` so long runs
//!   don't block the editor. `shell:<command>` inside a workflow
//!   delegates here; a `:keybind` on a deftask is effectively a
//!   one-step workflow.
//!
//! Running: the task dispatcher captures stdout + stderr into a
//! scrollable buffer the picker opens on completion. `:background #t`
//! spawns without blocking + reports exit status via notification.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "deftask")]
pub struct TaskSpec {
    /// Human-readable task id — unique within the plan.
    pub name: String,
    /// One-line description shown in the task picker.
    #[serde(default)]
    pub description: String,
    /// Shell command to exec (first positional). Empty rejects at
    /// apply time.
    #[serde(default)]
    pub command: String,
    /// Args passed to `:command`.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory. `~` expands. Empty = current editor cwd.
    #[serde(default)]
    pub cwd: String,
    /// Environment variables as `"KEY=VALUE"` strings (matches
    /// [`TermSpec::env`](crate::TermSpec) shape for tatara-lisp
    /// ergonomics).
    #[serde(default)]
    pub env: Vec<String>,
    /// Optional filetype scope — task only shows in the picker
    /// when the active buffer's filetype matches. Empty = global.
    #[serde(default)]
    pub filetype: String,
    /// Keybind that fires the task without opening the picker.
    #[serde(default)]
    pub keybind: String,
    /// If true, run without blocking the editor; report completion
    /// via notification (OSC 9 in terminal mode). Otherwise block
    /// the focused pane until the task exits.
    #[serde(default)]
    pub background: bool,
    /// Hard cap on task duration in milliseconds. 0 = runtime default.
    #[serde(default)]
    pub timeout_ms: u64,
}

impl TaskSpec {
    /// Parse `:env` strings into `(key, value)` pairs. Mirrors
    /// [`TermSpec::env_pairs`](crate::TermSpec::env_pairs) so users
    /// who author a `deftask` + a `defterm` can share the env list
    /// format across both.
    #[must_use]
    pub fn env_pairs(&self) -> Vec<(String, String)> {
        self.env
            .iter()
            .filter_map(|s| s.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect()
    }

    /// Full command line as a single display string — `command
    /// arg1 arg2 …` — used in the picker + notification payload.
    #[must_use]
    pub fn display_command(&self) -> String {
        if self.args.is_empty() {
            self.command.clone()
        } else {
            format!("{} {}", self.command, self.args.join(" "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_pairs_splits_on_equals() {
        let t = TaskSpec {
            name: "x".into(),
            command: "ls".into(),
            env: vec![
                "RUST_LOG=warn".into(),
                "CARGO_TERM_COLOR=always".into(),
                "NO_EQ".into(), // dropped
            ],
            ..Default::default()
        };
        assert_eq!(t.env_pairs().len(), 2);
    }

    #[test]
    fn display_command_handles_empty_args() {
        let t = TaskSpec {
            name: "x".into(),
            command: "ls".into(),
            ..Default::default()
        };
        assert_eq!(t.display_command(), "ls");

        let t = TaskSpec {
            name: "x".into(),
            command: "cargo".into(),
            args: vec!["test".into(), "--workspace".into()],
            ..Default::default()
        };
        assert_eq!(t.display_command(), "cargo test --workspace");
    }
}
