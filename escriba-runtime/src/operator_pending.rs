//! The operator-pending FSM — vim's `{operator}{motion}` key composition.
//!
//! After the `d`/`c`/`y` key ([`Action::Operator`]) the editor *waits* for a
//! motion; the next motion key composes into an [`Action::ApplyOperator`] the
//! runtime executes (the engine built in `apply_operator`). This is a pure
//! `(State, Event) -> (State, effects)` machine, so it **stands on the fleet
//! `zenmai` primitive** (the same Mealy-machine-with-effects abstraction
//! `bolso-core` and `gaveta-client-core::Escort` use) rather than re-rolling a
//! bespoke `Option<Operator>` + scattered `if let`s in the dispatch path.
//!
//! The machine's `Event` and `Effect` are both [`Action`]: it consumes the
//! resolved action coming off the keymap and emits the action(s) the runtime
//! should actually run. Most actions pass straight through; only the
//! operator-then-motion pair is rewritten.

use escriba_core::{Action, Operator};

/// Operator-pending state. `Resting` = normal; `Awaiting(op)` = an operator key
/// was pressed and the next motion will be operated over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpState {
    #[default]
    Resting,
    Awaiting(Operator),
}

/// The zenmai machine. A ZST marker; the reducer is [`Self::step`].
pub struct OperatorPending;

impl zenmai::Machine for OperatorPending {
    type State = OpState;
    type Event = Action;
    /// The action(s) the runtime should actually execute. Empty = the key was
    /// consumed (an operator began, or a pending operator was cancelled).
    type Effect = Action;

    fn step(state: &OpState, event: Action) -> (OpState, Vec<Action>) {
        match (state, event) {
            // A stray `Pending` (mid-sequence key) never disturbs operator
            // state and runs nothing — the runtime's own sequence buffer owns it.
            (s, Action::Pending) => (*s, vec![]),

            // Resting: an operator key arms the machine; everything else passes
            // straight through unchanged.
            (OpState::Resting, Action::Operator(op)) => (OpState::Awaiting(op), vec![]),
            (OpState::Resting, other) => (OpState::Resting, vec![other]),

            // Awaiting a motion: the motion composes into ApplyOperator.
            (OpState::Awaiting(op), Action::Move(motion)) => (
                OpState::Resting,
                vec![Action::ApplyOperator { op: *op, motion }],
            ),

            // Awaiting + anything-not-a-motion cancels the operator and drops
            // the key (vim: `d` then a non-motion does nothing). This covers
            // Esc (ChangeMode), operator-doubling `dd` (linewise — deferred),
            // and any other key.
            (OpState::Awaiting(_), _) => (OpState::Resting, vec![]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_core::{Mode, Motion};
    use zenmai::Machine;

    #[test]
    fn operator_then_motion_composes_apply_operator() {
        // `d` arms, `w` composes `dw`.
        let (s, fx) = OperatorPending::step(&OpState::Resting, Action::Operator(Operator::Delete));
        assert_eq!(s, OpState::Awaiting(Operator::Delete));
        assert!(fx.is_empty(), "the operator key runs nothing, it waits");

        let (s, fx) = OperatorPending::step(&s, Action::Move(Motion::WordStartNext));
        assert_eq!(s, OpState::Resting);
        assert_eq!(
            fx,
            vec![Action::ApplyOperator { op: Operator::Delete, motion: Motion::WordStartNext }]
        );
    }

    #[test]
    fn non_operator_actions_pass_straight_through() {
        let (s, fx) = OperatorPending::step(&OpState::Resting, Action::Move(Motion::Right));
        assert_eq!(s, OpState::Resting);
        assert_eq!(fx, vec![Action::Move(Motion::Right)]);
    }

    #[test]
    fn esc_cancels_a_pending_operator_and_drops_the_key() {
        let (s, fx) = OperatorPending::step(
            &OpState::Awaiting(Operator::Change),
            Action::ChangeMode(Mode::Normal),
        );
        assert_eq!(s, OpState::Resting, "Esc cancels the operator");
        assert!(fx.is_empty(), "the cancel key is dropped");
    }

    #[test]
    fn pending_never_disturbs_operator_state() {
        // A stray Pending while awaiting keeps the operator armed (a multi-key
        // motion like `gg` builds in the runtime's sequence buffer first).
        let (s, fx) = OperatorPending::step(&OpState::Awaiting(Operator::Yank), Action::Pending);
        assert_eq!(s, OpState::Awaiting(Operator::Yank));
        assert!(fx.is_empty());
    }

    #[test]
    fn inert_on_a_doubled_operator_dd_deferred() {
        // `dd` (linewise) is deferred — a second operator cancels for now.
        let (s, fx) = OperatorPending::step(
            &OpState::Awaiting(Operator::Delete),
            Action::Operator(Operator::Delete),
        );
        assert_eq!(s, OpState::Resting);
        assert!(fx.is_empty());
    }
}
