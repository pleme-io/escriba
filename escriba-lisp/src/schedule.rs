//! `defschedule` — typed declarative triggers for editor actions.
//!
//! **Invention.** No editor in the category ships typed declarative
//! cron-style triggers. emacs has `run-at-time`, nvim has
//! `vim.defer_fn`, vscode has `setInterval`, jetbrains has scheduled
//! tasks — all untyped, imperative, config-hostile calls. escriba
//! exposes temporal triggers as a first-class typed domain:
//!
//! ```lisp
//! ;; Run on a cron schedule — five-field standard cron.
//! (defschedule :name "hourly-git-pull"
//!              :description "keep the repo current at the top of every hour"
//!              :cron "0 * * * *"
//!              :command "git.pull"
//!              :filetype "any")
//!
//! ;; Fixed-period trigger.
//! (defschedule :name "refresh-diagnostics"
//!              :interval-seconds 300
//!              :workflow "diagnostics-refresh")
//!
//! ;; Fire after N seconds of user inactivity (mouse + keyboard idle).
//! (defschedule :name "autosave-on-idle"
//!              :idle-seconds 30
//!              :command "save-all"
//!              :description "save every modified buffer after 30s idle")
//!
//! ;; Fire exactly once at rc load time.
//! (defschedule :name "banner-on-start"
//!              :at-startup #t
//!              :action "picker.banner")
//!
//! ;; Manual-only — no automatic trigger, just a keybind that fires
//! ;; the associated action (useful for reusing the schedule
//! ;; dispatch logic without scheduling automatically).
//! (defschedule :name "kick-refresh"
//!              :workflow "diagnostics-refresh"
//!              :keybind "<leader>dr")
//! ```
//!
//! ## Why typed triggers
//!
//! 1. **Composable** — a schedule refers to a `defcmd` /
//!    `defworkflow` / raw `action`, so the dispatch target is already
//!    typed by the rest of the Rust+Lisp stack. The scheduler doesn't
//!    need to know about every side effect — it just resolves the
//!    ref.
//! 2. **Previewable** — `escriba doctor` can enumerate every
//!    registered schedule + its next-fire time deterministically,
//!    the same way it enumerates every other def-form via
//!    [`counts()`](crate::ApplyPlan::counts).
//! 3. **Reproducible** — the spec is content-addressable. A team's
//!    `hourly-git-pull` BLAKE3-hashes to the same token on every
//!    workstation.
//! 4. **Convergence-flavoured** — pleme-io's fleet runs FluxCD +
//!    kensa + sekiban; schedules are the editor-layer analogue. The
//!    `:at-startup #t` form is a zero-delay convergence hook.
//!
//! ## Shape constraints
//!
//! - Exactly one of `:cron` / `:interval-seconds` / `:idle-seconds`
//!   / `:at-startup` may be set. Setting two is ill-formed
//!   ([`Trigger::Invalid`]). Setting none means the schedule runs
//!   only when the keybind fires it ([`Trigger::Manual`]).
//! - Exactly one of `:command` / `:workflow` / `:action` must be
//!   set as the dispatch target. Setting zero or multiple is
//!   ill-formed ([`Dispatch::Invalid`]).
//! - `:cron` accepts the classic five-field grammar
//!   `minute hour day-of-month month day-of-week`. Full parsing
//!   lives in the runtime; the apply layer only verifies the shape
//!   has five whitespace-separated tokens.
//!
//! The apply layer validates these invariants and rejects malformed
//! specs before they land in the [`ApplyPlan`](crate::ApplyPlan).

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defschedule")]
pub struct ScheduleSpec {
    /// Human-readable schedule id — unique within the plan.
    pub name: String,
    /// Picker description shown when the manual keybind is visible.
    #[serde(default)]
    pub description: String,

    // ── Triggers (exactly one — or none, for manual-only) ───────────────
    /// Classic 5-field cron expression. The runtime parses against
    /// the same grammar `cron(8)` uses on the underlying OS.
    #[serde(default)]
    pub cron: String,
    /// Fixed-period trigger — fires every N wall-clock seconds.
    #[serde(default)]
    pub interval_seconds: u64,
    /// Idle-trigger — fires after N seconds of user inactivity
    /// (no keyboard + no mouse events). Resets on every input event.
    #[serde(default)]
    pub idle_seconds: u64,
    /// Fire exactly once at rc load time.
    #[serde(default)]
    pub at_startup: bool,

    // ── Dispatch (exactly one required) ─────────────────────────────────
    /// Name of a [`defcmd`](crate::CmdSpec) registered elsewhere
    /// in the plan. Mutually exclusive with `:workflow` / `:action`.
    #[serde(default)]
    pub command: String,
    /// Name of a [`defworkflow`](crate::WorkflowSpec) registered in
    /// the plan. The scheduler drives the DAG exactly as a manual
    /// invocation would.
    #[serde(default)]
    pub workflow: String,
    /// Raw typed [`Action`](..) string — same grammar the keymap
    /// apply layer resolves (e.g., `"save"`, `"picker.files"`).
    #[serde(default)]
    pub action: String,

    // ── Scope + manual handle ──────────────────────────────────────────
    /// Optional filetype restriction — the schedule only fires when
    /// the active buffer's filetype matches. Empty = global.
    #[serde(default)]
    pub filetype: String,
    /// Optional keybind that fires the schedule's dispatch target
    /// directly, bypassing the trigger. Useful as a manual "kick"
    /// (or, when no automatic trigger is set, the only way to fire).
    #[serde(default)]
    pub keybind: String,
}

/// Which temporal trigger the spec declares.
///
/// Determined purely from which fields are populated — the apply
/// layer drops `Invalid` before the spec reaches the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// `:cron` is set.
    Cron,
    /// `:interval-seconds` is set.
    Interval,
    /// `:idle-seconds` is set.
    Idle,
    /// `:at-startup #t` is set.
    Startup,
    /// No automatic trigger — the schedule only fires via `:keybind`.
    Manual,
    /// Two or more trigger fields are set. Ill-formed.
    Invalid,
}

/// Which dispatch target the spec declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// `:command` is set.
    Command,
    /// `:workflow` is set.
    Workflow,
    /// `:action` is set.
    Action,
    /// Zero or multiple dispatch fields. Ill-formed.
    Invalid,
}

impl ScheduleSpec {
    /// Classify the trigger configuration.
    #[must_use]
    pub fn trigger(&self) -> Trigger {
        let has_cron = !self.cron.is_empty();
        let has_interval = self.interval_seconds > 0;
        let has_idle = self.idle_seconds > 0;
        let has_startup = self.at_startup;
        match (has_cron, has_interval, has_idle, has_startup) {
            (true, false, false, false) => Trigger::Cron,
            (false, true, false, false) => Trigger::Interval,
            (false, false, true, false) => Trigger::Idle,
            (false, false, false, true) => Trigger::Startup,
            (false, false, false, false) => Trigger::Manual,
            _ => Trigger::Invalid,
        }
    }

    /// Classify the dispatch target.
    #[must_use]
    pub fn dispatch(&self) -> Dispatch {
        let has_cmd = !self.command.is_empty();
        let has_wf = !self.workflow.is_empty();
        let has_action = !self.action.is_empty();
        match (has_cmd, has_wf, has_action) {
            (true, false, false) => Dispatch::Command,
            (false, true, false) => Dispatch::Workflow,
            (false, false, true) => Dispatch::Action,
            _ => Dispatch::Invalid,
        }
    }

    /// Quick structural check on `:cron` — passes when the string
    /// parses as five whitespace-separated non-empty tokens. The
    /// runtime does the full minute/hour/day/month/dow parse; this
    /// helper exists purely so `:cron "bogus"` fails at apply time
    /// instead of at the first scheduler tick.
    #[must_use]
    pub fn has_well_shaped_cron(&self) -> bool {
        let fields: Vec<&str> = self.cron.split_whitespace().collect();
        fields.len() == 5 && fields.iter().all(|f| !f.is_empty())
    }

    /// True when the schedule has an automatic trigger (i.e., it
    /// will fire without a keybind). False for `Trigger::Manual`.
    #[must_use]
    pub fn is_automatic(&self) -> bool {
        !matches!(self.trigger(), Trigger::Manual | Trigger::Invalid)
    }

    /// Canonical compact identifier the scheduler logs — kind of
    /// trigger + the numeric payload — so a packed log line reads
    /// `cron:0 * * * *` / `interval:300s` / `idle:30s` / `startup`
    /// / `manual`.
    #[must_use]
    pub fn trigger_label(&self) -> String {
        match self.trigger() {
            Trigger::Cron => format!("cron:{}", self.cron),
            Trigger::Interval => format!("interval:{}s", self.interval_seconds),
            Trigger::Idle => format!("idle:{}s", self.idle_seconds),
            Trigger::Startup => "startup".to_string(),
            Trigger::Manual => "manual".to_string(),
            Trigger::Invalid => "invalid".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare(name: &str) -> ScheduleSpec {
        ScheduleSpec { name: name.into(), ..Default::default() }
    }

    #[test]
    fn trigger_classifies_single_field_as_canonical() {
        let mut s = bare("s");
        s.cron = "0 * * * *".into();
        assert_eq!(s.trigger(), Trigger::Cron);

        let mut s = bare("s");
        s.interval_seconds = 60;
        assert_eq!(s.trigger(), Trigger::Interval);

        let mut s = bare("s");
        s.idle_seconds = 30;
        assert_eq!(s.trigger(), Trigger::Idle);

        let mut s = bare("s");
        s.at_startup = true;
        assert_eq!(s.trigger(), Trigger::Startup);

        assert_eq!(bare("s").trigger(), Trigger::Manual);
    }

    #[test]
    fn trigger_rejects_multiple_fields_as_invalid() {
        let mut s = bare("s");
        s.cron = "0 * * * *".into();
        s.interval_seconds = 60;
        assert_eq!(s.trigger(), Trigger::Invalid);

        let mut s = bare("s");
        s.idle_seconds = 30;
        s.at_startup = true;
        assert_eq!(s.trigger(), Trigger::Invalid);
    }

    #[test]
    fn dispatch_classifies_each_mode() {
        let mut s = bare("s");
        s.command = "save".into();
        assert_eq!(s.dispatch(), Dispatch::Command);

        let mut s = bare("s");
        s.workflow = "ship".into();
        assert_eq!(s.dispatch(), Dispatch::Workflow);

        let mut s = bare("s");
        s.action = "picker.files".into();
        assert_eq!(s.dispatch(), Dispatch::Action);

        // Neither -> Invalid (no dispatch target).
        assert_eq!(bare("s").dispatch(), Dispatch::Invalid);

        // Multiple -> Invalid.
        let mut s = bare("s");
        s.command = "a".into();
        s.action = "b".into();
        assert_eq!(s.dispatch(), Dispatch::Invalid);
    }

    #[test]
    fn cron_shape_check_counts_five_fields() {
        let mut s = bare("s");
        s.cron = "0 * * * *".into();
        assert!(s.has_well_shaped_cron());

        s.cron = "0 0 * *".into(); // too few
        assert!(!s.has_well_shaped_cron());

        s.cron = "0 0 * * * *".into(); // too many
        assert!(!s.has_well_shaped_cron());

        s.cron = "garbage".into();
        assert!(!s.has_well_shaped_cron());

        // Leading / trailing whitespace tolerated — split_whitespace
        // normalizes. Five valid tokens still parse.
        s.cron = "  0 * * * *  ".into();
        assert!(s.has_well_shaped_cron());
    }

    #[test]
    fn is_automatic_is_false_for_manual() {
        let mut s = bare("s");
        assert!(!s.is_automatic());
        s.cron = "0 * * * *".into();
        assert!(s.is_automatic());
        s.cron = String::new();
        s.interval_seconds = 0;
        s.idle_seconds = 0;
        s.at_startup = false;
        assert!(!s.is_automatic());
    }

    #[test]
    fn trigger_label_renders_payload_for_each_kind() {
        let mut s = bare("s");
        s.cron = "*/5 * * * *".into();
        assert_eq!(s.trigger_label(), "cron:*/5 * * * *");

        s = bare("s");
        s.interval_seconds = 120;
        assert_eq!(s.trigger_label(), "interval:120s");

        s = bare("s");
        s.idle_seconds = 45;
        assert_eq!(s.trigger_label(), "idle:45s");

        s = bare("s");
        s.at_startup = true;
        assert_eq!(s.trigger_label(), "startup");

        assert_eq!(bare("s").trigger_label(), "manual");

        let mut bad = bare("bad");
        bad.cron = "0 * * * *".into();
        bad.interval_seconds = 60;
        assert_eq!(bad.trigger_label(), "invalid");
    }
}
