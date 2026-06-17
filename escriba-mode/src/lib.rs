//! `escriba-mode` — modal state machine, typed as a sum so illegal mode
//! field combinations are *unrepresentable*.
//!
//! Phase 1: vim-ish Normal/Insert/Visual/VisualLine/Command. The earlier
//! design was a product type — `{ mode, pending_count, pending_operator,
//! minibuffer }` — where nonsense like `mode: Insert` carrying a
//! `pending_operator: Some(Delete)` was *constructible* and only kept sane
//! by a runtime guard inside `enter()`. Per the org-level ★★
//! UNREPRESENTABILITY rule, the fix is structural: model the per-mode state
//! as a SUM so each mode carries ONLY the data that is valid in that mode.
//!
//! - `Normal` carries the pending count (`5dd`) + pending operator (the `d`
//!   in `dw`) — these exist NOWHERE else.
//! - `Insert` / `VisualLine` carry nothing.
//! - `Visual` carries nothing in phase 1 (a future `anchor: Position` lands
//!   here, in the one variant where a visual anchor is meaningful).
//! - `Command` carries ONLY the accumulating minibuffer line.
//!
//! There is no way to construct a `Command` without dropping the pending
//! operator, or an `Insert` that still holds a count — the type system
//! refuses it. The only way to change mode is the typed transition methods
//! (`enter_*` / `enter` / `escape`), so the per-mode invariant is enforced
//! by construction at every transition, not re-checked by a guard.
//!
//! Typestate destination: a future revision can promote this to a
//! phantom-typestate `Modal<P>` where illegal *transitions* (not just
//! illegal field combos) are `E0599` compile errors. That is a larger
//! ripple across the keymap-dispatch and runtime borrow sites; this sum
//! type is the pragmatic first tier — illegal field combinations are
//! already truly-unrepresentable, and the transition surface is sealed
//! behind methods so the typestate promotion is a later, localized change.

extern crate self as escriba_mode;

use escriba_core::{Mode, Operator};
use serde::{Deserialize, Serialize};

/// Pending operator-pending state — only meaningful in [`ModalState::Normal`].
///
/// Holds the count prefix being accumulated (`5` in `5dd`) and the pending
/// operator (`d` in `dw`). Both live HERE and only here: no insert-mode or
/// command-mode value can carry them, because the only place the type
/// system admits them is the `Normal` variant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PendingOp {
    /// Accumulating count prefix (`None` ⇒ no count typed yet, effective 1).
    pub count: Option<u32>,
    /// Pending operator awaiting a motion (the `d` in `dw`).
    pub operator: Option<Operator>,
}

/// The modal state machine — a SUM over the five editor modes, each
/// carrying ONLY the data valid in that mode.
///
/// `#[serde(tag = "mode")]` keeps the wire shape readable (`{"mode":
/// "Normal", "pending": {…}}`) and is the parse boundary: a deserialized
/// value can only be one of the legal shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "mode")]
pub enum ModalState {
    Normal { pending: PendingOp },
    Insert,
    Visual,
    VisualLine,
    Command { minibuffer: String },
}

impl Default for ModalState {
    fn default() -> Self {
        Self::Normal {
            pending: PendingOp::default(),
        }
    }
}

impl ModalState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The [`Mode`] discriminant of the current state — the projection the
    /// renderers + keymap dispatch read.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        match self {
            Self::Normal { .. } => Mode::Normal,
            Self::Insert => Mode::Insert,
            Self::Visual => Mode::Visual,
            Self::VisualLine => Mode::VisualLine,
            Self::Command { .. } => Mode::Command,
        }
    }

    // ── Typed transitions — the ONLY way to change mode ──────────────────
    //
    // Each transition drops the data that is invalid in the destination
    // mode by *construction*: entering `Insert` builds the `Insert` variant
    // which has no field to hold a pending operator, so the operator is
    // gone — not "cleared by a guard", but structurally absent.

    /// Enter [`Mode::Normal`] with no pending count/operator.
    pub fn enter_normal(&mut self) {
        *self = Self::Normal {
            pending: PendingOp::default(),
        };
    }

    /// Enter [`Mode::Insert`]. Any pending count/operator/minibuffer is
    /// dropped by construction.
    pub fn enter_insert(&mut self) {
        *self = Self::Insert;
    }

    /// Enter [`Mode::Visual`].
    pub fn enter_visual(&mut self) {
        *self = Self::Visual;
    }

    /// Enter [`Mode::VisualLine`].
    pub fn enter_visual_line(&mut self) {
        *self = Self::VisualLine;
    }

    /// Enter [`Mode::Command`] with an empty minibuffer.
    pub fn enter_command(&mut self) {
        *self = Self::Command {
            minibuffer: String::new(),
        };
    }

    /// Leave any mode back to a clean [`Mode::Normal`] — the `<Esc>`
    /// transition.
    pub fn escape(&mut self) {
        self.enter_normal();
    }

    /// Dispatch to the matching typed transition for a target [`Mode`].
    ///
    /// Kept for callers that hold a runtime `Mode` value (e.g. the keymap's
    /// `Action::ChangeMode(Mode)`); it is sugar over the `enter_*` methods
    /// and preserves the invariant the same way.
    pub fn enter(&mut self, mode: Mode) {
        match mode {
            Mode::Normal => self.enter_normal(),
            Mode::Insert => self.enter_insert(),
            Mode::Visual => self.enter_visual(),
            Mode::VisualLine => self.enter_visual_line(),
            Mode::Command => self.enter_command(),
        }
    }

    // ── Pending-op surface — no-ops outside Normal ───────────────────────
    //
    // These mutate the `Normal` variant's pending state. Called in any
    // other mode they are silent no-ops: there is no field to mutate, so
    // the count/operator concept simply does not exist there — which is the
    // whole point of the sum type.

    /// Set the pending operator. No-op unless in [`Mode::Normal`].
    pub fn set_operator(&mut self, op: Operator) {
        if let Self::Normal { pending } = self {
            pending.operator = Some(op);
        }
    }

    /// Append a digit to the pending count. No-op unless in [`Mode::Normal`].
    pub fn append_count(&mut self, digit: u32) {
        if let Self::Normal { pending } = self {
            let n = pending.count.unwrap_or(0);
            pending.count = Some(n.saturating_mul(10).saturating_add(digit));
        }
    }

    /// The current pending count (`None` ⇒ none accumulated). Always `None`
    /// outside [`Mode::Normal`].
    #[must_use]
    pub const fn pending_count(&self) -> Option<u32> {
        match self {
            Self::Normal { pending } => pending.count,
            _ => None,
        }
    }

    /// The current pending operator. Always `None` outside [`Mode::Normal`].
    #[must_use]
    pub const fn pending_operator(&self) -> Option<Operator> {
        match self {
            Self::Normal { pending } => pending.operator,
            _ => None,
        }
    }

    /// Take the pending count, leaving it cleared; defaults to 1 (and at
    /// least 1). No-op-returning-1 outside [`Mode::Normal`].
    #[must_use]
    pub fn consume_count(&mut self) -> u32 {
        match self {
            Self::Normal { pending } => pending.count.take().unwrap_or(1).max(1),
            _ => 1,
        }
    }

    /// Clear any pending count without consuming its value.
    pub fn clear_count(&mut self) {
        if let Self::Normal { pending } = self {
            pending.count = None;
        }
    }

    /// Take the pending operator, leaving it cleared.
    #[must_use]
    pub fn consume_operator(&mut self) -> Option<Operator> {
        match self {
            Self::Normal { pending } => pending.operator.take(),
            _ => None,
        }
    }

    // ── Command minibuffer surface — no-ops outside Command ──────────────

    /// Read the command-mode minibuffer (empty string outside
    /// [`Mode::Command`]).
    #[must_use]
    pub fn minibuffer(&self) -> &str {
        match self {
            Self::Command { minibuffer } => minibuffer,
            _ => "",
        }
    }

    /// Push a char onto the minibuffer. No-op unless in [`Mode::Command`].
    pub fn push_minibuffer(&mut self, ch: char) {
        if let Self::Command { minibuffer } = self {
            minibuffer.push(ch);
        }
    }

    /// Pop a char off the minibuffer. `None` unless in [`Mode::Command`].
    pub fn pop_minibuffer(&mut self) -> Option<char> {
        match self {
            Self::Command { minibuffer } => minibuffer.pop(),
            _ => None,
        }
    }

    /// Append a raw fragment to the minibuffer (used by the command
    /// registry's `__quit__` sentinel handshake). No-op outside
    /// [`Mode::Command`].
    pub fn push_minibuffer_str(&mut self, s: &str) {
        if let Self::Command { minibuffer } = self {
            minibuffer.push_str(s);
        }
    }

    /// Clear the minibuffer in place. No-op outside [`Mode::Command`].
    pub fn clear_minibuffer(&mut self) {
        if let Self::Command { minibuffer } = self {
            minibuffer.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_clean_normal() {
        let s = ModalState::new();
        assert_eq!(s.mode(), Mode::Normal);
        assert_eq!(s.pending_count(), None);
        assert_eq!(s.pending_operator(), None);
    }

    // ── Legal transitions work ───────────────────────────────────────────

    #[test]
    fn enter_transitions_set_mode() {
        let mut s = ModalState::new();
        s.enter_insert();
        assert_eq!(s.mode(), Mode::Insert);
        s.enter_visual();
        assert_eq!(s.mode(), Mode::Visual);
        s.enter_visual_line();
        assert_eq!(s.mode(), Mode::VisualLine);
        s.enter_command();
        assert_eq!(s.mode(), Mode::Command);
        s.escape();
        assert_eq!(s.mode(), Mode::Normal);
    }

    #[test]
    fn enter_by_mode_value_dispatches() {
        for m in [
            Mode::Normal,
            Mode::Insert,
            Mode::Visual,
            Mode::VisualLine,
            Mode::Command,
        ] {
            let mut s = ModalState::new();
            s.enter(m);
            assert_eq!(s.mode(), m);
        }
    }

    // ── Illegal field combinations are UNREPRESENTABLE ───────────────────

    #[test]
    fn leaving_normal_structurally_drops_pending() {
        // Set a count + operator in Normal, then enter Insert. The pending
        // data is GONE — not because a guard cleared it, but because the
        // `Insert` variant has no field that could hold it. There is no
        // expressible `ModalState::Insert { pending_operator: … }`.
        let mut s = ModalState::new();
        s.set_operator(Operator::Delete);
        s.append_count(5);
        assert_eq!(s.pending_operator(), Some(Operator::Delete));
        assert_eq!(s.pending_count(), Some(5));
        s.enter_insert();
        // The compiler refuses `Insert` carrying these; the accessors prove
        // there is no value to read.
        assert_eq!(s.pending_operator(), None);
        assert_eq!(s.pending_count(), None);
    }

    #[test]
    fn pending_ops_are_noops_outside_normal() {
        // Attempting to set an operator / count while NOT in Normal cannot
        // corrupt the state — the variants have nowhere to store them.
        let mut s = ModalState::new();
        s.enter_insert();
        s.set_operator(Operator::Yank);
        s.append_count(9);
        assert_eq!(s.pending_operator(), None);
        assert_eq!(s.pending_count(), None);
        assert_eq!(s.consume_count(), 1, "no count exists outside Normal");
        assert_eq!(s.consume_operator(), None);
    }

    #[test]
    fn minibuffer_is_noop_outside_command() {
        // An Insert-mode value cannot accumulate a command line — there is
        // no minibuffer field on the `Insert` variant.
        let mut s = ModalState::new();
        s.enter_insert();
        s.push_minibuffer('w');
        assert_eq!(s.minibuffer(), "", "insert mode has no minibuffer");
        assert_eq!(s.pop_minibuffer(), None);
    }

    #[test]
    fn entering_command_clears_prior_minibuffer() {
        let mut s = ModalState::new();
        s.enter_command();
        s.push_minibuffer('q');
        assert_eq!(s.minibuffer(), "q");
        // Re-entering command starts a fresh, empty line.
        s.enter_command();
        assert_eq!(s.minibuffer(), "");
    }

    // ── Behavioral round-trips (parity with the old product type) ────────

    #[test]
    fn normal_resets_pending_state() {
        let mut s = ModalState::new();
        s.enter_insert();
        s.set_operator(Operator::Delete);
        s.append_count(5);
        s.enter_normal();
        assert!(s.pending_count().is_none());
        assert!(s.pending_operator().is_none());
    }

    #[test]
    fn count_accumulates() {
        let mut s = ModalState::new();
        s.append_count(5);
        s.append_count(3);
        assert_eq!(s.consume_count(), 53);
        assert_eq!(s.consume_count(), 1); // default 1 when consumed again
    }

    #[test]
    fn operator_round_trip() {
        let mut s = ModalState::new();
        s.set_operator(Operator::Yank);
        assert_eq!(s.consume_operator(), Some(Operator::Yank));
        assert_eq!(s.consume_operator(), None);
    }

    #[test]
    fn minibuffer_append_pop() {
        let mut s = ModalState::new();
        s.enter_command();
        s.push_minibuffer('w');
        assert_eq!(s.minibuffer(), "w");
        assert_eq!(s.pop_minibuffer(), Some('w'));
    }

    #[test]
    fn minibuffer_str_and_clear() {
        let mut s = ModalState::new();
        s.enter_command();
        s.push_minibuffer_str("__quit__");
        assert!(s.minibuffer().contains("__quit__"));
        s.clear_minibuffer();
        assert_eq!(s.minibuffer(), "");
    }

    /// The wire shape round-trips, and a deserialized value is one of the
    /// legal variants only (the parse boundary rejects illegal shapes).
    #[test]
    fn serde_round_trip_per_variant() {
        for s in [
            ModalState::new(),
            {
                let mut n = ModalState::new();
                n.append_count(12);
                n.set_operator(Operator::Delete);
                n
            },
            ModalState::Insert,
            ModalState::Visual,
            ModalState::VisualLine,
            {
                let mut c = ModalState::new();
                c.enter_command();
                c.push_minibuffer('x');
                c
            },
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let back: ModalState = serde_json::from_str(&json).unwrap();
            assert_eq!(s, back);
        }
    }
}
