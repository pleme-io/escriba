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
use escriba_memori::{CaretLine, CaretMove};
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
    Normal {
        pending: PendingOp,
    },
    Insert,
    Visual,
    VisualLine,
    Command {
        #[serde(flatten)]
        line: ExLine,
    },
}

impl Default for ModalState {
    fn default() -> Self {
        Self::Normal {
            pending: PendingOp::default(),
        }
    }
}

/// The ex-line: [`CaretLine`] plus the wire shape `escriba-api` publishes.
///
/// The editing logic is NOT here — it is `CaretLine`, in memori, because the
/// search prompt needs the identical thing and the two crates cannot see each
/// other. What is left is the one thing that IS this crate's business: the
/// published JSON says `minibuffer`, and a positioning primitive has no
/// reason to know that word.
///
/// # Wire shape
///
/// `#[serde(flatten)]`ed into [`ModalState::Command`], so the schema reads
/// `{"mode": "Command", "minibuffer": "wq", "caret": 1}` — unchanged by
/// either the field-folding or the move down to memori.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(from = "ExLineWire", into = "ExLineWire")]
pub struct ExLine(#[schemars(with = "ExLineWire")] CaretLine);

/// The serialization shadow of [`ExLine`] — and the parse boundary.
///
/// Private fields stop *code* from breaking `caret <= len`; they say nothing
/// about a document that simply asserts `caret: 99` on a two-char line.
/// `CaretLine::new` clamps, so an `ExLine` that exists is one that holds,
/// whatever it was decoded from.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct ExLineWire {
    #[serde(default)]
    minibuffer: String,
    #[serde(default)]
    caret: usize,
}

impl From<ExLineWire> for ExLine {
    fn from(w: ExLineWire) -> Self {
        Self(CaretLine::new(w.minibuffer, w.caret))
    }
}

impl From<ExLine> for ExLineWire {
    fn from(l: ExLine) -> Self {
        Self {
            minibuffer: l.0.text().to_owned(),
            caret: l.0.caret(),
        }
    }
}

impl ExLine {
    /// The text typed so far, without the leading `:`.
    #[must_use]
    pub fn text(&self) -> &str {
        self.0.text()
    }

    /// The caret, in chars from the start.
    #[must_use]
    pub const fn caret(&self) -> usize {
        self.0.caret()
    }

    /// Insert a char AT the caret and step past it.
    pub fn insert(&mut self, ch: char) {
        self.0.insert(ch);
    }

    /// Append a raw fragment and park the caret at the end.
    ///
    /// Appends rather than inserting on purpose: its caller is the command
    /// registry's `__quit__` sentinel handshake, writing a fragment the user
    /// did not type.
    pub fn push_str(&mut self, s: &str) {
        self.0.push_str(s);
    }

    /// Move the caret.
    pub fn move_caret(&mut self, to: CaretMove) {
        self.0.move_caret(to);
    }

    /// Delete the char AT the caret (`<Del>`).
    pub fn delete(&mut self) {
        self.0.delete();
    }

    /// Delete the char BEFORE the caret (`<BS>`), returning it.
    pub fn backspace(&mut self) -> Option<char> {
        self.0.backspace()
    }

    /// Empty the line AND return the caret home.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Length in chars — the caret's upper bound.
    #[must_use]
    pub fn len_chars(&self) -> usize {
        self.0.len_chars()
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
            line: ExLine::default(),
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
            Self::Command { line } => line.text(),
            _ => "",
        }
    }

    /// Push a char onto the minibuffer. No-op unless in [`Mode::Command`].
    pub fn push_minibuffer(&mut self, ch: char) {
        if let Self::Command { line } = self {
            line.insert(ch);
        }
    }

    /// Move the ex-line caret. No-op outside [`Mode::Command`].
    pub fn move_minibuffer_caret(&mut self, to: CaretMove) {
        if let Self::Command { line } = self {
            line.move_caret(to);
        }
    }

    /// Delete the character AT the caret. No-op at the end of the line.
    pub fn delete_minibuffer_at_caret(&mut self) {
        if let Self::Command { line } = self {
            line.delete();
        }
    }

    /// The ex-line caret, in chars. `0` outside [`Mode::Command`].
    #[must_use]
    pub fn minibuffer_caret(&self) -> usize {
        match self {
            Self::Command { line } => line.caret(),
            _ => 0,
        }
    }

    /// Pop a char off the minibuffer. `None` unless in [`Mode::Command`].
    pub fn pop_minibuffer(&mut self) -> Option<char> {
        match self {
            Self::Command { line } => line.backspace(),
            _ => None,
        }
    }

    /// Append a raw fragment to the minibuffer (used by the command
    /// registry's `__quit__` sentinel handshake). No-op outside
    /// [`Mode::Command`].
    pub fn push_minibuffer_str(&mut self, s: &str) {
        if let Self::Command { line } = self {
            line.push_str(s);
        }
    }

    /// Clear the minibuffer in place. No-op outside [`Mode::Command`].
    pub fn clear_minibuffer(&mut self) {
        if let Self::Command { line } = self {
            line.clear();
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

#[cfg(test)]
mod ex_line_tests {
    use super::*;

    #[test]
    fn clearing_the_ex_line_brings_the_caret_home() {
        // The defect that motivated `ExLine`: the text was emptied and the
        // caret left behind, so the next insert built a line whose caret
        // claimed a position the line did not have.
        let mut s = ModalState::new();
        s.enter_command();
        for ch in "foo".chars() {
            s.push_minibuffer(ch);
        }
        assert_eq!(s.minibuffer_caret(), 3);

        s.clear_minibuffer();
        assert_eq!(s.minibuffer(), "");
        assert_eq!(s.minibuffer_caret(), 0, "the caret is half of the value");

        s.push_minibuffer('x');
        assert_eq!(s.minibuffer(), "x");
        assert_eq!(
            s.minibuffer_caret(),
            1,
            "and one char in means caret 1, not 4"
        );
    }

    #[test]
    fn the_caret_never_exceeds_the_line_it_indexes() {
        // The invariant, exercised across every mutation the type offers.
        let mut line = ExLine::default();
        for ch in "héllo".chars() {
            line.insert(ch);
        }
        line.move_caret(CaretMove::Start);
        line.delete();
        line.backspace();
        line.move_caret(CaretMove::End);
        line.push_str("!");
        line.clear();
        line.insert('a');
        assert!(line.caret() <= line.len_chars());
        assert_eq!(line.text(), "a");
    }

    #[test]
    fn the_published_wire_shape_survives_the_extraction() {
        // `escriba-api` publishes `ModalState`'s schema, so folding two fields
        // into a struct must not be visible on the wire. This test is what
        // makes `#[serde(flatten)]` + the `minibuffer` rename load-bearing
        // rather than decorative.
        let mut s = ModalState::new();
        s.enter_command();
        s.push_minibuffer('w');
        s.push_minibuffer('q');
        s.move_minibuffer_caret(CaretMove::Left);

        let v: serde_json::Value = serde_json::to_value(&s).unwrap();
        assert_eq!(v["mode"], "Command");
        assert_eq!(v["minibuffer"], "wq", "NOT nested under a `line` key");
        assert_eq!(v["caret"], 1);
        assert_eq!(serde_json::from_value::<ModalState>(v).unwrap(), s);
    }

    #[test]
    fn a_caret_past_the_end_is_clamped_at_the_parse_boundary() {
        // Private fields stop code from breaking the invariant. They say
        // nothing about a document that simply asserts a bad caret, which is
        // what the deserialization shadow is for.
        let s: ModalState =
            serde_json::from_str(r#"{"mode":"Command","minibuffer":"ab","caret":99}"#).unwrap();
        assert_eq!(s.minibuffer(), "ab");
        assert_eq!(s.minibuffer_caret(), 2);
    }
}
