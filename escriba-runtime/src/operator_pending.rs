//! The operator-pending FSM — vim's `{count}{operator}{count}{motion}`
//! key composition.
//!
//! After the `d`/`c`/`y` key ([`Action::Operator`]) the editor *waits* for a
//! motion; the next motion key composes into an [`Action::ApplyOperator`] the
//! runtime executes (the engine built in `apply_operator`). This is a pure
//! `(State, Event) -> (State, effects)` machine, so it **stands on the fleet
//! `zenmai` primitive** (the same Mealy-machine-with-effects abstraction
//! `bolso-core` and `gaveta-client-core::Escort` use) rather than re-rolling a
//! bespoke `Option<Operator>` + scattered `if let`s in the dispatch path.
//!
//! **Counts compose here, not in a naive repeat loop.** The `Event` and
//! `Effect` are `(Action, count)`: the machine consumes the resolved action +
//! its count off the keymap, and emits the action(s) the runtime should run +
//! *how many times*. A bare motion (`5j`) passes through carrying its count; an
//! operated motion multiplies the two counts (`3d2w` = delete `3 × 2 = 6`
//! words), because vim's operator count and motion count multiply.

use escriba_core::{Action, Operator};

/// Operator-pending state. `Resting` = normal; `Awaiting { op, count }` = an
/// operator key was pressed (with `count`, default 1) and the next motion will
/// be operated over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpState {
    #[default]
    Resting,
    Awaiting {
        op: Operator,
        count: u32,
    },
    /// `d/` — an operator is armed AND a search prompt is open.
    ///
    /// Without this state the prompt's own keys (the typed pattern, Backspace,
    /// history) hit the "anything else cancels" arm and the operator
    /// evaporated on the very first character, so `d/foo<CR>` silently did
    /// nothing. The operator has to survive an arbitrary number of keystrokes
    /// here, which is exactly why it needs a state rather than a guard.
    AwaitingSearch {
        op: Operator,
        count: u32,
    },
}

/// The zenmai machine. A ZST marker; the reducer is [`Self::step`]. The
/// `(Action, u32)` event/effect pair is the action and its repetition count.
pub struct OperatorPending;

impl zenmai::Machine for OperatorPending {
    type State = OpState;
    /// The resolved action and its count (from the keymap's `CountedAction`).
    type Event = (Action, u32);
    /// The action the runtime should execute and how many times. Empty = the
    /// key was consumed (an operator began, or a pending operator was cancelled).
    type Effect = (Action, u32);

    fn step(state: &OpState, event: (Action, u32)) -> (OpState, Vec<(Action, u32)>) {
        let (action, count) = event;
        match (state, action) {
            // A stray `Pending` (mid-sequence key) never disturbs operator
            // state and runs nothing — the runtime's own sequence buffer owns it.
            (s, Action::Pending) => (*s, vec![]),

            // Resting: an operator key arms the machine (capturing its count);
            // everything else passes straight through carrying its own count.
            (OpState::Resting, Action::Operator(op)) => (OpState::Awaiting { op, count }, vec![]),
            (OpState::Resting, other) => (OpState::Resting, vec![(other, count)]),

            // Awaiting a motion: the motion composes into ApplyOperator, applied
            // `op_count × motion_count` times (the two vim counts multiply).
            (
                OpState::Awaiting {
                    op,
                    count: op_count,
                },
                Action::Move(motion),
            ) => (
                OpState::Resting,
                vec![(
                    Action::ApplyOperator { op: *op, motion },
                    op_count.saturating_mul(count).max(1),
                )],
            ),

            // Awaiting + a text OBJECT composes over its extent, not over a
            // range ending at the cursor. `dgn` deletes the match wherever it
            // is; a motion-shaped composition would delete up to it instead.
            (
                OpState::Awaiting {
                    op,
                    count: op_count,
                },
                Action::TextObject(object),
            ) => (
                OpState::Resting,
                vec![(
                    Action::ApplyOperatorObject { op: *op, object },
                    op_count.saturating_mul(count).max(1),
                )],
            ),

            // Awaiting + `/` or `?`: open the prompt and STAY ARMED. This arm
            // is the whole fix — it used to fall into the catch-all below,
            // which disarmed the operator and dropped the key.
            (
                OpState::Awaiting {
                    op,
                    count: op_count,
                },
                Action::SearchOpen(dir),
            ) => (
                OpState::AwaitingSearch {
                    op: *op,
                    count: *op_count,
                },
                vec![(Action::SearchOpen(dir), 1)],
            ),

            // Prompt editing passes through untouched while the operator
            // waits. Classified by `Action::edits_prompt`, which is TOTAL over
            // `Action`, rather than by a list here — the list is how this
            // broke once already: five prompt actions were added later and
            // none reached it, so `←` or `<C-g>` midway through `d/foo`
            // silently disarmed the operator.
            (OpState::AwaitingSearch { .. }, a) if a.edits_prompt() => (*state, vec![(a, 1)]),

            // `<CR>` resolves the operand. Emitted as ONE action rather than
            // "commit, then operate": committing MOVES the cursor, which would
            // destroy the operator's start point before the operator ran.
            (
                OpState::AwaitingSearch {
                    op,
                    count: op_count,
                },
                Action::SubmitCommand,
            ) => (
                OpState::Resting,
                vec![(Action::SearchSubmitOperated { op: *op }, *op_count)],
            ),

            // Esc disarms both the prompt and the operator.
            (OpState::AwaitingSearch { .. }, Action::ChangeMode(m)) => {
                (OpState::Resting, vec![(Action::ChangeMode(m), 1)])
            }

            // Any other key while a search operand is being typed disarms the
            // operator but still runs the key — dropping it silently is what
            // made `d/` feel broken.
            (OpState::AwaitingSearch { .. }, other) => (OpState::Resting, vec![(other, count)]),

            // Awaiting + anything-not-a-motion cancels the operator and drops
            // the key (vim: `d` then a non-motion does nothing). Covers Esc
            // (ChangeMode), operator-doubling `dd` (linewise — deferred), and
            // any other key.
            (OpState::Awaiting { .. }, _) => (OpState::Resting, vec![]),
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
        // `d` arms (count 1), `w` composes `dw` once.
        let (s, fx) =
            OperatorPending::step(&OpState::Resting, (Action::Operator(Operator::Delete), 1));
        assert_eq!(
            s,
            OpState::Awaiting {
                op: Operator::Delete,
                count: 1
            }
        );
        assert!(fx.is_empty(), "the operator key runs nothing, it waits");

        let (s, fx) = OperatorPending::step(&s, (Action::Move(Motion::WordStartNext), 1));
        assert_eq!(s, OpState::Resting);
        assert_eq!(
            fx,
            vec![(
                Action::ApplyOperator {
                    op: Operator::Delete,
                    motion: Motion::WordStartNext
                },
                1
            )]
        );
    }

    #[test]
    fn operator_count_multiplies_the_motion() {
        // `3dw` = delete 3 words: the operator's count (3) flows to the
        // composed motion as the repetition count.
        let (s, _) =
            OperatorPending::step(&OpState::Resting, (Action::Operator(Operator::Delete), 3));
        assert_eq!(
            s,
            OpState::Awaiting {
                op: Operator::Delete,
                count: 3
            }
        );
        let (_, fx) = OperatorPending::step(&s, (Action::Move(Motion::WordStartNext), 1));
        assert_eq!(
            fx,
            vec![(
                Action::ApplyOperator {
                    op: Operator::Delete,
                    motion: Motion::WordStartNext
                },
                3
            )]
        );
    }

    #[test]
    fn operator_and_motion_counts_multiply() {
        // `3d2w` = delete 3×2 = 6 words.
        let (s, _) =
            OperatorPending::step(&OpState::Resting, (Action::Operator(Operator::Delete), 3));
        let (_, fx) = OperatorPending::step(&s, (Action::Move(Motion::WordStartNext), 2));
        assert_eq!(fx[0].1, 6, "operator count × motion count");
    }

    #[test]
    fn bare_motion_passes_through_carrying_its_count() {
        // `5j` is not an operated motion — it passes through with count 5.
        let (s, fx) = OperatorPending::step(&OpState::Resting, (Action::Move(Motion::Down), 5));
        assert_eq!(s, OpState::Resting);
        assert_eq!(fx, vec![(Action::Move(Motion::Down), 5)]);
    }

    #[test]
    fn esc_cancels_a_pending_operator_and_drops_the_key() {
        let (s, fx) = OperatorPending::step(
            &OpState::Awaiting {
                op: Operator::Change,
                count: 1,
            },
            (Action::ChangeMode(Mode::Normal), 1),
        );
        assert_eq!(s, OpState::Resting, "Esc cancels the operator");
        assert!(fx.is_empty(), "the cancel key is dropped");
    }

    #[test]
    fn pending_never_disturbs_operator_state() {
        // A stray Pending while awaiting keeps the operator armed (a multi-key
        // motion like `gg` builds in the runtime's sequence buffer first).
        let (s, fx) = OperatorPending::step(
            &OpState::Awaiting {
                op: Operator::Yank,
                count: 1,
            },
            (Action::Pending, 1),
        );
        assert_eq!(
            s,
            OpState::Awaiting {
                op: Operator::Yank,
                count: 1
            }
        );
        assert!(fx.is_empty());
    }

    #[test]
    fn inert_on_a_doubled_operator_dd_deferred() {
        // `dd` (linewise) is deferred — a second operator cancels for now.
        let (s, fx) = OperatorPending::step(
            &OpState::Awaiting {
                op: Operator::Delete,
                count: 1,
            },
            (Action::Operator(Operator::Delete), 1),
        );
        assert_eq!(s, OpState::Resting);
        assert!(fx.is_empty());
    }
}
