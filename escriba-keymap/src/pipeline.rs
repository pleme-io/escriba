//! The vim KEY LAYER as a pure, host-agnostic pipeline.
//!
//! ## What this is
//!
//! Everything between "a key arrived" and "these actions should run": the
//! operand captures (`di(`, `fx`, `` `a ``, `rZ`), multi-key sequence
//! resolution (`gg`, `<leader>ff`), count accumulation (`5j`, `3d2w`), and the
//! operator-pending machine (`d` + motion → [`Action::ApplyOperator`]).
//!
//! It lived inside `escriba-runtime`'s `EditorState::on_key` until 2026-09-26,
//! where it was reachable only by an editor. Every app that wants vim keys —
//! the `arnes` agent TUI, the `frost` shell — would have had to re-derive it,
//! and each re-derivation is a new place for `zt`, ``d`a`` or `3f.` to break.
//! One implementation, here; `escriba-runtime` is now a host of it like any
//! other.
//!
//! ## What it is NOT
//!
//! The pipeline never touches text, cursors, buffers, registers or marks. It
//! turns keys into [`Action`]s; executing them is the host's job. Mode CHANGES
//! are likewise returned as actions (`ChangeMode`, `EnterInsert`, …) and the
//! host applies them to its [`ModalState`] — the pipeline only READS the mode
//! (to decide whether a key is an operand, a sequence step or literal text)
//! and writes the COUNT prefix, which lives in `ModalState` by design.
//!
//! ## Two layers, one implementation
//!
//! - [`KeyPipeline::resolve_key`] — the key layer: captures, sequences,
//!   counts, keymap lookup. Yields `(Action, count)` units that have NOT yet
//!   passed the operator machine.
//! - [`KeyPipeline::compose`] — the operator machine, one unit at a time.
//! - [`KeyPipeline::feed`] — `resolve_key` then `compose` on every unit: the
//!   one-call surface most hosts want.
//!
//! The split exists because the operator machine is ALSO fed by actions that
//! never came from a key (an editor's splash menu, a picker choice, a lisp
//! effect), and because a host may need to veto a unit BEFORE the machine sees
//! it — escriba refuses to submit an uncompilable search pattern while `d/` is
//! armed, and the machine, being a pure `(State, Event) -> (State, effects)`
//! reducer, cannot observe that. A host with no such concern calls `feed`.

use escriba_core::{Action, Mode, Motion, TextObject};
use escriba_mode::{ModalState, OpState, OperatorPending};

use crate::{Key, Keymap};

/// A character search: which character, which direction, and whether it stops
/// ON it (`f`/`F`) or just BEFORE it (`t`/`T`).
///
/// The same value serves the pending operand and the `;`/`,` memory
/// ([`KeyPipeline::last_find`]), so the thing repeated is the thing that ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindSpec {
    pub ch: char,
    pub backward: bool,
    pub till: bool,
}

/// What the next keystroke means after `m`, `` ` `` or `'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkKey {
    /// `m{a-z}` — set.
    Set,
    /// `` `{a-z} `` — jump to the exact position.
    GotoExact,
    /// `'{a-z}` — jump to the line's first non-blank.
    GotoLine,
}

/// What an operand capture did with a key.
enum Claim {
    /// The key was swallowed: it armed a capture, or cancelled one.
    Consumed,
    /// The key completed a gesture. `times` is meaningful only for a
    /// [`OperandCount::SelfCounted`] capture.
    Compose { action: Action, times: u32 },
}

/// How a captured operand's composed action reaches the operator machine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OperandCount {
    /// The capture computed its own repeats; emit the action that many times
    /// at count 1. Only the object path does this (`2diw` is the OPERATOR's
    /// count, read off the armed machine).
    SelfCounted,
    /// Drain the pending count and emit one unit carrying it, so `3fx` /
    /// `3ra` / ``3`a`` repeat on the one path every other motion uses.
    Drained,
}

/// One step of the operand-capture chain.
struct OperandCapture {
    /// Stable label — what [`operand_capture_order`] reports.
    name: &'static str,
    claim: fn(&mut KeyPipeline, &ModalState, Key) -> Option<Claim>,
    count: OperandCount,
}

/// **The operand-capture chain, in the order that matters.**
///
/// Every adjacency is a dependency with a named failure:
///
/// 1. **mark before object** — the object path claims `i`/`a` whenever an
///    operator is armed, and a mark LETTER can be either, so ``d`a`` lost its
///    `a` to it. They do not fight over the FIRST key (the mark path arms only
///    while `pending_object` is clear, so `di'` still reaches the object
///    path); they fight over the SECOND, and the gesture already half-typed
///    must win.
/// 2. **object before find** — `di(` must not read as `d`, then `i` (insert),
///    then a literal `(`.
/// 3. **find before replace** — no live conflict; `f`/`t` and `r` arm on
///    disjoint keys and neither can be pending while the other is. Ordered
///    for stability rather than necessity, and said so rather than implying a
///    constraint that is not there.
/// 4. **all four before the sequence stepper and the keymap** — this is the
///    whole point. Each capture also declines while `pending_keys` is
///    non-empty, so a LATER key of a gesture (`zt`'s `t`) belongs to the
///    sequence rather than arming a till-find.
static OPERAND_CHAIN: &[OperandCapture] = &[
    OperandCapture {
        name: "mark",
        claim: KeyPipeline::claim_mark,
        count: OperandCount::Drained,
    },
    OperandCapture {
        name: "object",
        claim: KeyPipeline::claim_object,
        count: OperandCount::SelfCounted,
    },
    OperandCapture {
        name: "find",
        claim: KeyPipeline::claim_find,
        count: OperandCount::Drained,
    },
    OperandCapture {
        name: "replace",
        claim: KeyPipeline::claim_replace,
        count: OperandCount::Drained,
    },
];

/// The operand-capture chain's order, by name — the gate
/// `escriba-runtime/tests/operand_capture_order.rs` pins it.
#[must_use]
pub fn operand_capture_order() -> Vec<&'static str> {
    OPERAND_CHAIN.iter().map(|c| c.name).collect()
}

/// Outcome of the multi-key sequence stepper.
enum SeqStep {
    /// The key began or extended a live sequence prefix; hold it.
    Pending,
    /// The key completed a bound sequence.
    Resolved(Action),
    /// Not a sequence key; hand it to single-key dispatch.
    Passthrough,
}

/// The vim key layer: keymap + every piece of half-typed-gesture state.
///
/// See the module docs for the contract. Construct with
/// [`KeyPipeline::default_vim`] (or [`KeyPipeline::new`] over a customised
/// [`Keymap`]), then hand every key to [`KeyPipeline::feed`].
#[derive(Debug, Clone)]
pub struct KeyPipeline {
    keymap: Keymap,
    /// Keys accumulated for an in-progress multi-key sequence — e.g. holding
    /// `[,, f]` while waiting for the final key of `<leader>ff`. Empty when
    /// not mid-sequence.
    pending_keys: Vec<Key>,
    /// The operator-pending machine (`d`/`c`/`y` then a motion → `dw`/`c$`),
    /// standing on the fleet `zenmai` Mealy-machine primitive.
    op: zenmai::Stateful<OperatorPending>,
    /// `Some(around)` means an operator + `i`/`a` were pressed and the NEXT
    /// key names the object. Held at the KEY layer because the machine sees
    /// `Action`s and this decision needs the KEY: `a` and every bracket are
    /// unbound in Normal, so they all arrive as `Action::Pending` with the
    /// character already discarded. vim has a whole operator-pending keymap
    /// for the same reason.
    pending_object: Option<bool>,
    /// `f`/`F`/`t`/`T` was pressed and the NEXT key is the character to
    /// search for. It must be claimed before the sequence stepper or `f` then
    /// `f` would resolve as a bound `ff` sequence.
    pending_find: Option<FindSpec>,
    /// `r` was pressed and the NEXT key is the replacement. `rw` must not read
    /// as `r` then *move a word*.
    pending_replace: bool,
    /// `m`, `` ` `` or `'` was pressed and the NEXT key is the mark letter.
    pending_mark: Option<MarkKey>,
    /// The last resolved character search — what `;` and `,` repeat. Memory,
    /// not pending state: [`Self::reset`] keeps it.
    last_find: Option<FindSpec>,
}

impl Default for KeyPipeline {
    fn default() -> Self {
        Self::default_vim()
    }
}

impl KeyPipeline {
    /// A pipeline over `keymap`, with nothing half-typed.
    #[must_use]
    pub fn new(keymap: Keymap) -> Self {
        Self {
            keymap,
            pending_keys: Vec::new(),
            op: zenmai::Stateful::new(OpState::Resting),
            pending_object: None,
            pending_find: None,
            pending_replace: false,
            pending_mark: None,
            last_find: None,
        }
    }

    /// A pipeline over [`Keymap::default_vim`].
    #[must_use]
    pub fn default_vim() -> Self {
        Self::new(Keymap::default_vim())
    }

    #[must_use]
    pub const fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// The live keymap — rc / plugin binding application writes here.
    pub const fn keymap_mut(&mut self) -> &mut Keymap {
        &mut self.keymap
    }

    /// The keys held for an in-progress multi-key sequence (empty when none).
    /// What a "showcmd" indicator renders.
    #[must_use]
    pub fn pending_keys(&self) -> &[Key] {
        &self.pending_keys
    }

    /// The operator machine's state (`Resting`, `Awaiting`, `AwaitingSearch`).
    #[must_use]
    pub fn op_state(&self) -> &OpState {
        self.op.state()
    }

    /// The last completed `f`/`F`/`t`/`T` — what `;` / `,`
    /// ([`Motion::RepeatFind`]) resolve through. Resolving it needs the
    /// buffer, so that is the host's job; recording it is the key layer's.
    #[must_use]
    pub const fn last_find(&self) -> Option<FindSpec> {
        self.last_find
    }

    /// True while a key sequence, an operator (including `d/` with its search
    /// prompt open), an object, a find, a replace or a mark is half-typed.
    ///
    /// A bare COUNT prefix is not included: it lives in [`ModalState`]
    /// (`pending_count()`), which the host already holds.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        !self.pending_keys.is_empty()
            || self.pending_object.is_some()
            || self.pending_find.is_some()
            || self.pending_replace
            || self.pending_mark.is_some()
            || !matches!(self.op.state(), OpState::Resting)
    }

    /// Abandon everything half-typed and disarm the operator machine. The
    /// keymap and [`Self::last_find`] are kept. The count prefix lives in the
    /// host's [`ModalState`]; clear it there.
    pub fn reset(&mut self) {
        self.pending_keys.clear();
        self.op = zenmai::Stateful::new(OpState::Resting);
        self.pending_object = None;
        self.pending_find = None;
        self.pending_replace = false;
        self.pending_mark = None;
    }

    /// Feed one key; get the fully composed steps to execute, in order.
    ///
    /// Each `(Action, u32)` has passed the operator machine: `3d2w` yields one
    /// `ApplyOperator { Delete, WordStartNext }` at count 6, `ciw` one
    /// `ApplyOperatorObject`, `fx` a `Move(FindChar …)`, `gg` the resolved
    /// sequence. The count is a REPETITION count for most actions; an editor
    /// that treats some actions as absorbing their count (`3p`, `2dd` as one
    /// operation) decides that itself.
    ///
    /// Counts accumulate in `modal`; the pipeline never mutates text and never
    /// changes the mode — mode changes come back as actions.
    ///
    /// This is [`Self::resolve_key`] followed by [`Self::compose`] on every
    /// unit. A host that must be able to refuse a unit before the operator
    /// machine sees it (see the module docs) calls those two itself.
    pub fn feed(&mut self, modal: &mut ModalState, key: &Key) -> Vec<(Action, u32)> {
        let units = self.resolve_key(modal, key);
        let mut out = Vec::with_capacity(units.len());
        for (action, count) in units {
            out.extend(self.compose(&action, count));
        }
        out
    }

    /// Run the operator machine over one resolved unit.
    ///
    /// Also the entry for actions that did not come from a key — anything a
    /// host dispatches should pass here, so an armed `d` composes with it the
    /// same way it composes with a keyed motion.
    ///
    /// `|` is folded in first: it is the one motion whose count is an
    /// ARGUMENT rather than a repetition (`40|` is column 40, not "column 1,
    /// forty times"). Folding it here keeps the machine at one rule — counts
    /// repeat — with the exception living where the exception is.
    pub fn compose(&mut self, action: &Action, count: u32) -> Vec<(Action, u32)> {
        let (action, count) = match action {
            Action::Move(Motion::Column(_)) => (Action::Move(Motion::Column(count)), 1),
            a => (a.clone(), count),
        };
        self.op.dispatch((action, count))
    }

    /// The key layer alone: captures, sequences, counts, keymap. Returns the
    /// units to hand to [`Self::compose`], one at a time and in order.
    ///
    /// A repeated sequence (`3gg`) and a counted object (`2diw`) come back as
    /// that many count-1 units rather than one counted unit, because that is
    /// how they meet the operator machine: each repetition is its own event
    /// (`d3gg` composes the first and runs the other two bare).
    pub fn resolve_key(&mut self, modal: &mut ModalState, key: &Key) -> Vec<(Action, u32)> {
        // ── OPERAND CAPTURE — the keys that are ARGUMENTS, not bindings ──
        //
        // `di(`, `fx`, `` `a ``, `rZ`: in each, the second keystroke is an
        // operand of a half-typed gesture and must be claimed before the
        // sequence stepper and before the keymap, or it resolves as whatever
        // it happens to be bound to (`i` enters Insert, `w` moves a word).
        // A TABLE, because the order is the correctness property.
        for cap in OPERAND_CHAIN {
            let Some(claim) = (cap.claim)(self, modal, *key) else {
                continue;
            };
            return match claim {
                Claim::Consumed => Vec::new(),
                Claim::Compose { action, times } => match cap.count {
                    OperandCount::SelfCounted => vec![(action, 1); times.max(1) as usize],
                    OperandCount::Drained => {
                        let n = modal.pending_count().unwrap_or(1);
                        modal.clear_count();
                        vec![(action, n)]
                    }
                },
            };
        }
        // Multi-key sequence resolution: a key that begins or continues a
        // bound sequence (`<leader>ff`, `gg`) is held or resolved here before
        // the single-key path sees it.
        match self.step_sequence(modal.mode(), *key) {
            SeqStep::Pending => return Vec::new(),
            SeqStep::Resolved(action) => {
                let count = modal.pending_count().unwrap_or(1);
                modal.clear_count();
                return vec![(action, 1); count as usize];
            }
            SeqStep::Passthrough => {}
        }
        let counted = self.keymap.dispatch(modal, key);
        // Count prefixes accumulate into modal state.
        if matches!(counted.action, Action::Pending) {
            if let Key::Char(c) = key
                && let Some(d) = c.to_digit(10)
            {
                modal.append_count(d);
            }
            return Vec::new();
        }
        // The count flows through the operator machine, which owns
        // repetition: a bare motion runs count×, an operator captures its
        // count, and an operated motion multiplies the two.
        modal.clear_count();
        vec![(counted.action, counted.count)]
    }

    /// Advance the multi-key pending-stroke state machine for `key`.
    ///
    /// Sequences only apply in normal / visual modes — insert and command
    /// modes treat keys as literal text. Rules:
    /// - Mid-sequence: extend the pending prefix. Exact match → `Resolved`;
    ///   still a live prefix → `Pending`; otherwise abort the sequence and
    ///   re-process this key fresh.
    /// - Not mid-sequence: if `key` begins a bound sequence AND is not itself
    ///   a complete single binding (single bindings win, so no chord timeout
    ///   is needed) → start pending. Otherwise `Passthrough`.
    fn step_sequence(&mut self, mode: Mode, key: Key) -> SeqStep {
        if !matches!(mode, Mode::Normal | Mode::Visual | Mode::VisualLine) {
            return SeqStep::Passthrough;
        }
        if !self.pending_keys.is_empty() {
            let mut seq = self.pending_keys.clone();
            seq.push(key);
            if let Some(b) = self.keymap.lookup_sequence(mode, &seq) {
                let action = b.action.clone();
                self.pending_keys.clear();
                return SeqStep::Resolved(action);
            }
            if self.keymap.is_sequence_prefix(mode, &seq) {
                self.pending_keys = seq;
                return SeqStep::Pending;
            }
            // The key broke the in-progress sequence — abort it and let the
            // key be re-processed as a fresh stroke below.
            self.pending_keys.clear();
        }
        let start = [key];
        if self.keymap.is_sequence_prefix(mode, &start) && self.keymap.lookup(mode, &key).is_none()
        {
            self.pending_keys = start.to_vec();
            return SeqStep::Pending;
        }
        SeqStep::Passthrough
    }

    fn operator_armed(&self) -> bool {
        matches!(self.op.state(), OpState::Awaiting { .. })
    }

    /// Disarm the operator machine, discarding what it emits. `Esc` is the
    /// event the machine already treats as "cancel whatever is armed".
    fn disarm(&mut self) {
        self.op.dispatch((Action::ChangeMode(Mode::Normal), 1));
    }

    /// Claim the operand of a pending `m` / `` ` `` / `'`, or arm one.
    ///
    /// `ma` is `m` plus an OPERAND, and `a` is bound (append). Without
    /// claiming it first, `ma` would set no mark and enter Insert mode.
    ///
    /// Arms only while `pending_object` is clear, so `di'` — where `'` is a
    /// text-object delimiter rather than a mark jump — still reaches the
    /// object path. The guard states that dependency locally instead of
    /// leaving it implied by chain order.
    fn claim_mark(&mut self, modal: &ModalState, key: Key) -> Option<Claim> {
        if let Some(kind) = self.pending_mark.take() {
            let Key::Char(name) = key else {
                if self.operator_armed() {
                    self.disarm();
                }
                return Some(Claim::Consumed);
            };
            let action = match kind {
                MarkKey::Set => Action::SetMark(name),
                MarkKey::GotoExact => Action::Move(Motion::MarkExact(name)),
                MarkKey::GotoLine => Action::Move(Motion::MarkLine(name)),
            };
            return Some(Claim::Compose { action, times: 1 });
        }
        if !matches!(modal.mode(), Mode::Normal | Mode::Visual) {
            return None;
        }
        // Half-typed text object (`di` waiting for its `'`) belongs to the
        // object path; a key continuing a sequence belongs to the sequence
        // (see `claim_find` for the `zt` case that proves it).
        if self.pending_object.is_some() || !self.pending_keys.is_empty() {
            return None;
        }
        let Key::Char(c) = key else { return None };
        let kind = match c {
            'm' => MarkKey::Set,
            '`' => MarkKey::GotoExact,
            '\'' => MarkKey::GotoLine,
            _ => return None,
        };
        self.pending_mark = Some(kind);
        Some(Claim::Consumed)
    }

    /// Read one key as operator-pending OBJECT selection (`diw`, `ca(`).
    ///
    /// Returns `None` when the key is nothing to do with objects, so the
    /// ordinary path runs untouched.
    fn claim_object(&mut self, _modal: &ModalState, key: Key) -> Option<Claim> {
        let Key::Char(c) = key else {
            // Esc (or anything non-printable) abandons a half-typed object
            // rather than leaving the editor silently armed.
            if self.pending_object.take().is_some() {
                self.disarm();
                return Some(Claim::Consumed);
            }
            return None;
        };

        // Second key: it names the object.
        if let Some(around) = self.pending_object.take() {
            let object = object_for(c, around);
            let OpState::Awaiting { op, count } = *self.op.state() else {
                return Some(Claim::Consumed);
            };
            // Disarm either way: an unknown object key cancels the operator,
            // it does not leave it armed for the next unrelated keystroke.
            self.disarm();
            let Some(object) = object else {
                return Some(Claim::Consumed);
            };
            // `2diw` applies the object twice — the OPERATOR's count, which
            // only the armed machine knew.
            return Some(Claim::Compose {
                action: Action::ApplyOperatorObject { op, object },
                times: count,
            });
        }

        // First key: `i` or `a` while an operator waits.
        if matches!(c, 'i' | 'a') && self.operator_armed() {
            self.pending_object = Some(c == 'a');
            return Some(Claim::Consumed);
        }
        None
    }

    /// Claim the operand of a pending `f`/`F`/`t`/`T`, or arm one.
    ///
    /// The character is an OPERAND, not a binding: `dfx` is undecidable from
    /// actions — `x` would resolve as whatever `x` is bound to. Composition
    /// with an operator is free: the armed motion is emitted as an ordinary
    /// `Action::Move`, so the operator machine composes `dfx` exactly the way
    /// it composes `dw`.
    fn claim_find(&mut self, modal: &ModalState, key: Key) -> Option<Claim> {
        if let Some(spec) = self.pending_find.take() {
            let Key::Char(ch) = key else {
                // Esc (or any non-printable) abandons a half-typed find rather
                // than leaving the editor armed for the next keystroke.
                if self.operator_armed() {
                    self.disarm();
                }
                return Some(Claim::Consumed);
            };
            let spec = FindSpec { ch, ..spec };
            self.last_find = Some(spec);
            return Some(Claim::Compose {
                action: Action::Move(Motion::FindChar {
                    ch,
                    backward: spec.backward,
                    till: spec.till,
                }),
                times: 1,
            });
        }
        if modal.mode() != Mode::Normal && modal.mode() != Mode::Visual {
            return None;
        }
        // A key that is CONTINUING a sequence belongs to the sequence.
        //
        // Without this, `zt` was unreachable: `z` starts a pending sequence,
        // then `t` was claimed here as a till-find and the sequence never
        // completed. "An operand key outranks a binding" is right for the
        // FIRST key of a gesture and wrong for a later one. The operand branch
        // above runs before this guard, so `zt` and `ft` are both reachable.
        if !self.pending_keys.is_empty() {
            return None;
        }
        let Key::Char(c) = key else { return None };
        let (backward, till) = match c {
            'f' => (false, false),
            'F' => (true, false),
            't' => (false, true),
            'T' => (true, true),
            _ => return None,
        };
        self.pending_find = Some(FindSpec {
            ch: '\0',
            backward,
            till,
        });
        Some(Claim::Consumed)
    }

    /// Claim the operand of a pending `r`, or arm one.
    ///
    /// `r` is unbound in the keymap on purpose: a binding on it would be a
    /// table entry no keypress can reach — configured on paper, absent in
    /// behaviour, the exact trap `f`/`t` documented.
    fn claim_replace(&mut self, modal: &ModalState, key: Key) -> Option<Claim> {
        if self.pending_replace {
            self.pending_replace = false;
            let Key::Char(ch) = key else {
                // Esc (or any non-printable) abandons a half-typed `r` rather
                // than replacing with something unprintable.
                return Some(Claim::Consumed);
            };
            return Some(Claim::Compose {
                action: Action::ReplaceChar(ch),
                times: 1,
            });
        }
        if modal.mode() != Mode::Normal && modal.mode() != Mode::Visual {
            return None;
        }
        // A key CONTINUING a sequence belongs to the sequence — the `zt` rule.
        if !self.pending_keys.is_empty() {
            return None;
        }
        if key != Key::Char('r') {
            return None;
        }
        // `r` is not a motion, so `dr` is a typo — and vim treats it as one by
        // CANCELLING the operator. Falling through to the keymap instead is
        // the worse reading: `r` is unbound, so it resolves to
        // `Action::Pending`, which the machine deliberately lets leave the
        // operator armed (for the multi-key-sequence case). The next motion
        // would then delete.
        if self.operator_armed() {
            self.disarm();
            return Some(Claim::Consumed);
        }
        self.pending_replace = true;
        Some(Claim::Consumed)
    }
}

/// The text object named by `c` after `i` (`around == false`) or `a`.
fn object_for(c: char, around: bool) -> Option<TextObject> {
    let delimited = |open, close| {
        Some(TextObject::Delimited {
            open,
            close,
            around,
        })
    };
    match c {
        'w' => Some(TextObject::Word { around }),
        // vim's `b` and `B` aliases for the bracket pairs, plus the brackets
        // themselves in both directions.
        '(' | ')' | 'b' => delimited('(', ')'),
        '{' | '}' | 'B' => delimited('{', '}'),
        '[' | ']' => delimited('[', ']'),
        '<' | '>' => delimited('<', '>'),
        // Quotes: `open == close`, which is what tells the resolver not to
        // count nesting.
        '"' => delimited('"', '"'),
        '\'' => delimited('\'', '\''),
        '`' => delimited('`', '`'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_core::{InsertAt, Operator};

    fn normal() -> ModalState {
        ModalState::new()
    }

    /// Feed a string of printable keys; collect every emitted step.
    fn type_keys(p: &mut KeyPipeline, m: &mut ModalState, keys: &str) -> Vec<(Action, u32)> {
        keys.chars()
            .flat_map(|c| p.feed(m, &Key::Char(c)))
            .collect()
    }

    #[test]
    fn dw_composes_one_apply_operator() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert!(p.feed(&mut m, &Key::Char('d')).is_empty(), "d waits");
        assert!(p.is_pending());
        assert_eq!(
            p.feed(&mut m, &Key::Char('w')),
            vec![(
                Action::ApplyOperator {
                    op: Operator::Delete,
                    motion: Motion::WordStartNext
                },
                1
            )]
        );
        assert!(!p.is_pending());
    }

    #[test]
    fn three_d_two_w_is_one_operation_at_count_six() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            type_keys(&mut p, &mut m, "3d2w"),
            vec![(
                Action::ApplyOperator {
                    op: Operator::Delete,
                    motion: Motion::WordStartNext
                },
                6
            )]
        );
        assert_eq!(m.pending_count(), None, "the count was drained");
    }

    #[test]
    fn ciw_composes_one_object_operation() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            type_keys(&mut p, &mut m, "ciw"),
            vec![(
                Action::ApplyOperatorObject {
                    op: Operator::Change,
                    object: TextObject::Word { around: false }
                },
                1
            )]
        );
        assert!(!p.is_pending());
    }

    #[test]
    fn di_paren_is_an_object_not_insert() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            type_keys(&mut p, &mut m, "di("),
            vec![(
                Action::ApplyOperatorObject {
                    op: Operator::Delete,
                    object: TextObject::Delimited {
                        open: '(',
                        close: ')',
                        around: false
                    }
                },
                1
            )]
        );
    }

    #[test]
    fn a_counted_object_emits_one_unit_per_repeat() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        let steps = type_keys(&mut p, &mut m, "2daw");
        let obj = Action::ApplyOperatorObject {
            op: Operator::Delete,
            object: TextObject::Word { around: true },
        };
        assert_eq!(steps, vec![(obj.clone(), 1), (obj, 1)]);
    }

    #[test]
    fn fx_is_a_find_motion_and_is_remembered() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        let find = Action::Move(Motion::FindChar {
            ch: 'x',
            backward: false,
            till: false,
        });
        assert_eq!(type_keys(&mut p, &mut m, "fx"), vec![(find, 1)]);
        assert_eq!(
            p.last_find(),
            Some(FindSpec {
                ch: 'x',
                backward: false,
                till: false
            })
        );
    }

    #[test]
    fn a_counted_find_carries_the_count_once() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        let steps = type_keys(&mut p, &mut m, "3f.");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].1, 3, "3f. is the third dot, not the ninth");
    }

    #[test]
    fn dtx_composes_a_till_find() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            type_keys(&mut p, &mut m, "dtx"),
            vec![(
                Action::ApplyOperator {
                    op: Operator::Delete,
                    motion: Motion::FindChar {
                        ch: 'x',
                        backward: false,
                        till: true
                    }
                },
                1
            )]
        );
    }

    #[test]
    fn r_takes_its_operand_even_when_bound() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            type_keys(&mut p, &mut m, "rw"),
            vec![(Action::ReplaceChar('w'), 1)]
        );
    }

    #[test]
    fn dr_cancels_the_operator() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert!(type_keys(&mut p, &mut m, "dr").is_empty());
        assert!(!p.is_pending(), "the typo disarmed rather than arming r");
    }

    #[test]
    fn gg_resolves_the_sequence() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert!(p.feed(&mut m, &Key::Char('g')).is_empty());
        assert_eq!(p.pending_keys(), &[Key::Char('g')]);
        assert!(p.is_pending());
        assert_eq!(
            p.feed(&mut m, &Key::Char('g')),
            vec![(Action::Move(Motion::DocStart), 1)]
        );
        assert!(p.pending_keys().is_empty());
    }

    #[test]
    fn dgg_composes_through_the_sequence() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            type_keys(&mut p, &mut m, "dgg"),
            vec![(
                Action::ApplyOperator {
                    op: Operator::Delete,
                    motion: Motion::DocStart
                },
                1
            )]
        );
    }

    #[test]
    fn capital_g_is_a_single_key() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            p.feed(&mut m, &Key::Char('G')),
            vec![(Action::Move(Motion::DocEnd), 1)]
        );
    }

    #[test]
    fn digit_counts_accumulate() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert!(p.feed(&mut m, &Key::Char('1')).is_empty());
        assert!(p.feed(&mut m, &Key::Char('5')).is_empty());
        assert_eq!(m.pending_count(), Some(15));
        assert_eq!(
            p.feed(&mut m, &Key::Char('j')),
            vec![(Action::Move(Motion::Down), 15)]
        );
        assert_eq!(m.pending_count(), None);
    }

    #[test]
    fn five_j() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            type_keys(&mut p, &mut m, "5j"),
            vec![(Action::Move(Motion::Down), 5)]
        );
    }

    #[test]
    fn zero_is_a_motion_alone_and_a_digit_mid_count() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            p.feed(&mut m, &Key::Char('0')),
            vec![(Action::Move(Motion::LineStart), 1)]
        );
        assert_eq!(
            type_keys(&mut p, &mut m, "10j"),
            vec![(Action::Move(Motion::Down), 10)]
        );
    }

    #[test]
    fn esc_cancels_a_half_typed_object() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        type_keys(&mut p, &mut m, "di");
        assert!(p.is_pending());
        assert!(p.feed(&mut m, &Key::Esc).is_empty());
        assert!(!p.is_pending(), "object AND operator disarmed");
        assert_eq!(
            p.feed(&mut m, &Key::Char('w')),
            vec![(Action::Move(Motion::WordStartNext), 1)]
        );
    }

    #[test]
    fn is_pending_truth_table() {
        let cases: &[(&str, bool)] = &[
            ("", false),
            ("d", true),
            ("di", true),
            ("f", true),
            ("df", true),
            ("r", true),
            ("m", true),
            ("`", true),
            ("g", true),
            ("5", false), // a count lives in ModalState, not here
            ("dw", false),
            ("fx", false),
            ("rx", false),
            ("ma", false),
            ("gg", false),
            ("j", false),
        ];
        for (keys, want) in cases {
            let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
            type_keys(&mut p, &mut m, keys);
            assert_eq!(p.is_pending(), *want, "after {keys:?}");
        }
    }

    #[test]
    fn reset_abandons_everything_half_typed() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        type_keys(&mut p, &mut m, "fx");
        type_keys(&mut p, &mut m, "dg");
        assert!(p.is_pending());
        p.reset();
        assert!(!p.is_pending());
        assert!(p.last_find().is_some(), "memory survives a reset");
    }

    #[test]
    fn insert_mode_keys_pass_through_as_text() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            p.feed(&mut m, &Key::Char('i')),
            vec![(Action::EnterInsert(InsertAt::Caret), 1)]
        );
        // The host applies the mode change; the pipeline only reads it.
        m.enter_insert();
        for c in ['g', 'g', 'f', 'd', 'r', 'm', '5', 'i'] {
            assert_eq!(
                p.feed(&mut m, &Key::Char(c)),
                vec![(Action::InsertChar(c), 1)],
                "{c:?} in Insert is text"
            );
            assert!(!p.is_pending(), "{c:?} armed nothing in Insert");
        }
    }

    #[test]
    fn marks_capture_their_letter() {
        let (mut p, mut m) = (KeyPipeline::default_vim(), normal());
        assert_eq!(
            type_keys(&mut p, &mut m, "ma"),
            vec![(Action::SetMark('a'), 1)]
        );
        assert_eq!(
            type_keys(&mut p, &mut m, "d`a"),
            vec![(
                Action::ApplyOperator {
                    op: Operator::Delete,
                    motion: Motion::MarkExact('a')
                },
                1
            )]
        );
    }

    #[test]
    fn a_column_count_is_an_argument() {
        let mut p = KeyPipeline::default_vim();
        assert_eq!(
            p.compose(&Action::Move(Motion::Column(0)), 40),
            vec![(Action::Move(Motion::Column(40)), 1)]
        );
    }

    #[test]
    fn the_chain_order_is_mark_object_find_replace() {
        assert_eq!(
            operand_capture_order(),
            vec!["mark", "object", "find", "replace"]
        );
    }
}
