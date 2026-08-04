//! `escriba-keymap` — mode-aware keybinding dispatch.

extern crate self as escriba_keymap;

use escriba_search::{CaretMove, Direction as SearchDirection};
use std::collections::HashMap;

use escriba_core::{Action, CountedAction, Mode, Motion, Operator, TextObject};
use escriba_mode::ModalState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Key {
    Char(char),
    Esc,
    Enter,
    Tab,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Ctrl(char),
    Alt(char),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub action: Action,
    pub description: String,
}

impl Binding {
    #[must_use]
    pub fn new(action: Action, description: impl Into<String>) -> Self {
        Self {
            action,
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: HashMap<(Mode, Key), Binding>,
    /// Multi-key sequence bindings (`<leader>ff`, `gg`, `<C-w>h`).
    /// Keyed by the full key sequence; resolved by the runtime's
    /// pending-stroke loop ([`lookup_sequence`](Keymap::lookup_sequence)
    /// + [`is_sequence_prefix`](Keymap::is_sequence_prefix)).
    sequences: HashMap<(Mode, Vec<Key>), Binding>,
    /// The prefix `<leader>` resolves to at sequence-apply time.
    leader: Key,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: HashMap::new(),
            sequences: HashMap::new(),
            // blnvim's leader is comma; escriba ships blnvim-parity
            // defaults, so the prefix users press matches muscle memory.
            leader: Key::Char(','),
        }
    }
}

impl Keymap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn default_vim() -> Self {
        let mut m = Self::new();
        let nm = |m: &mut Keymap, k: Key, a: Action, d: &'static str| m.bind(Mode::Normal, k, a, d);
        nm(
            &mut m,
            Key::Char('h'),
            Action::Move(Motion::Left),
            "move left",
        );
        nm(
            &mut m,
            Key::Char('l'),
            Action::Move(Motion::Right),
            "move right",
        );
        nm(
            &mut m,
            Key::Char('j'),
            Action::Move(Motion::Down),
            "move down",
        );
        nm(&mut m, Key::Char('k'), Action::Move(Motion::Up), "move up");
        nm(
            &mut m,
            Key::Char('w'),
            Action::Move(Motion::WordStartNext),
            "word forward",
        );
        nm(
            &mut m,
            Key::Char('b'),
            Action::Move(Motion::WordStartPrev),
            "word back",
        );
        nm(
            &mut m,
            Key::Char('0'),
            Action::Move(Motion::LineStart),
            "line start",
        );
        nm(
            &mut m,
            Key::Char('$'),
            Action::Move(Motion::LineEnd),
            "line end",
        );
        nm(
            &mut m,
            Key::Char('G'),
            Action::Move(Motion::DocEnd),
            "doc end",
        );
        // Operators — `d`/`c`/`y` arm the operator-pending FSM; the next
        // motion composes (e.g. `dw`, `c$`, `y0`).
        nm(
            &mut m,
            Key::Char('d'),
            Action::Operator(Operator::Delete),
            "delete (operator)",
        );
        nm(
            &mut m,
            Key::Char('c'),
            Action::Operator(Operator::Change),
            "change (operator)",
        );
        nm(
            &mut m,
            Key::Char('y'),
            Action::Operator(Operator::Yank),
            "yank (operator)",
        );
        // Structural Lisp motions — Alt-prefixed like emacs paredit.
        nm(
            &mut m,
            Key::Alt('f'),
            Action::Move(Motion::ForwardSexp),
            "forward sexp",
        );
        nm(
            &mut m,
            Key::Alt('b'),
            Action::Move(Motion::BackwardSexp),
            "backward sexp",
        );
        nm(
            &mut m,
            Key::Alt('u'),
            Action::Move(Motion::UpList),
            "up list",
        );
        nm(
            &mut m,
            Key::Alt('d'),
            Action::Move(Motion::DownList),
            "down list",
        );
        // Mode changes.
        nm(
            &mut m,
            Key::Char('i'),
            Action::ChangeMode(Mode::Insert),
            "insert",
        );
        nm(
            &mut m,
            Key::Char('v'),
            Action::ChangeMode(Mode::Visual),
            "visual",
        );
        nm(
            &mut m,
            Key::Char('V'),
            Action::ChangeMode(Mode::VisualLine),
            "visual line",
        );
        nm(
            &mut m,
            Key::Char(':'),
            Action::ChangeMode(Mode::Command),
            "command",
        );
        nm(&mut m, Key::Char('u'), Action::Undo, "undo");
        nm(
            &mut m,
            Key::Char('.'),
            Action::RepeatLastChange,
            "repeat last change",
        );
        nm(&mut m, Key::Ctrl('r'), Action::Redo, "redo");
        // Insert → Normal on Esc.
        m.bind(
            Mode::Insert,
            Key::Esc,
            Action::ChangeMode(Mode::Normal),
            "to normal",
        );
        m.bind(
            Mode::Command,
            Key::Esc,
            Action::ChangeMode(Mode::Normal),
            "abort",
        );
        m.bind(Mode::Command, Key::Enter, Action::SubmitCommand, "submit");
        m.bind(
            Mode::Command,
            Key::Up,
            Action::PromptHistory { back: true },
            "older search",
        );
        m.bind(
            Mode::Command,
            Key::Down,
            Action::PromptHistory { back: false },
            "newer search",
        );
        m.bind(
            Mode::Command,
            Key::Backspace,
            Action::PromptBackspace,
            "erase one char",
        );
        // Caret editing inside the prompt. Without these the prompt is
        // append-only, so a typo in the middle of a pattern can only be fixed
        // by deleting everything back to it.
        m.bind(
            Mode::Command,
            Key::Left,
            Action::PromptCaret {
                to: CaretMove::Left,
            },
            "caret left",
        );
        m.bind(
            Mode::Command,
            Key::Right,
            Action::PromptCaret {
                to: CaretMove::Right,
            },
            "caret right",
        );
        m.bind(
            Mode::Command,
            Key::Home,
            Action::PromptCaret {
                to: CaretMove::Start,
            },
            "caret to start",
        );
        m.bind(
            Mode::Command,
            Key::End,
            Action::PromptCaret { to: CaretMove::End },
            "caret to end",
        );
        m.bind(
            Mode::Command,
            Key::Ctrl('w'),
            Action::PromptDeleteWord,
            "delete word before caret",
        );
        m.bind(
            Mode::Command,
            Key::Ctrl('u'),
            Action::PromptClearToStart,
            "clear to start",
        );

        // ── search ────────────────────────────────────────────────────
        // `/` and `?` open the prompt; `<CR>` is the existing SubmitCommand,
        // which the runtime routes to the search when a search prompt is open.
        // That routing is typed (Option<Prompt>), not a mode flag to forget.
        nm(
            &mut m,
            Key::Char('/'),
            Action::SearchOpen(SearchDirection::Forward),
            "search forward",
        );
        nm(
            &mut m,
            Key::Char('?'),
            Action::SearchOpen(SearchDirection::Backward),
            "search backward",
        );
        // `n`/`N` are MOTIONS, not standalone jumps. Binding them to
        // `Action::Move` is what makes `dn` / `yN` compose — the
        // operator-pending machine only recognises `Action::Move` as an
        // operand. `Action::SearchRepeat` remains a valid action (a user rc or
        // the tatara-lisp binding table may name it) and the runtime routes it
        // through the same executor, so there is exactly one code path.
        nm(
            &mut m,
            Key::Char('n'),
            Action::Move(Motion::SearchNext),
            "next match",
        );
        nm(
            &mut m,
            Key::Char('N'),
            Action::Move(Motion::SearchPrev),
            "previous match",
        );

        // `gn` / `gN` — the match as an OBJECT, so `cgn` changes the whole
        // match and `.` repeats that on the next one.
        m.bind_sequence(
            Mode::Normal,
            vec![Key::Char('g'), Key::Char('n')],
            Action::TextObject(TextObject::NextMatch),
            "next match (object)",
        );
        m.bind_sequence(
            Mode::Normal,
            vec![Key::Char('g'), Key::Char('N')],
            Action::TextObject(TextObject::PrevMatch),
            "previous match (object)",
        );

        // ── jumplist ──────────────────────────────────────────────────
        // The return ticket for every far jump above. Without it a committed
        // search is a one-way door.
        nm(&mut m, Key::Ctrl('o'), Action::JumpBack, "jump back");
        nm(&mut m, Key::Ctrl('i'), Action::JumpForward, "jump forward");
        nm(
            &mut m,
            Key::Char('*'),
            Action::SearchWord { reverse: false },
            "search word forward",
        );
        nm(
            &mut m,
            Key::Char('#'),
            Action::SearchWord { reverse: true },
            "search word backward",
        );
        m.bind(
            Mode::Visual,
            Key::Esc,
            Action::ChangeMode(Mode::Normal),
            "to normal",
        );
        m.bind(
            Mode::VisualLine,
            Key::Esc,
            Action::ChangeMode(Mode::Normal),
            "to normal",
        );
        m
    }

    pub fn bind(&mut self, mode: Mode, key: Key, action: Action, desc: impl Into<String>) {
        self.bindings
            .insert((mode, key), Binding::new(action, desc));
    }

    #[must_use]
    pub fn lookup(&self, mode: Mode, key: &Key) -> Option<&Binding> {
        self.bindings.get(&(mode, key.clone()))
    }

    /// The leader key — what `<leader>` resolves to when a sequence
    /// binding is applied. Defaults to `,` (blnvim parity).
    #[must_use]
    pub fn leader(&self) -> &Key {
        &self.leader
    }

    /// Override the leader key. Applied before sequence bindings so
    /// `<leader>`-prefixed specs resolve against the chosen prefix.
    pub fn set_leader(&mut self, key: Key) {
        self.leader = key;
    }

    /// Bind a multi-key SEQUENCE — `<leader>ff` →
    /// `[Char(','), Char('f'), Char('f')]`, `gg` →
    /// `[Char('g'), Char('g')]`. A length-1 sequence delegates to
    /// [`bind`](Keymap::bind) so callers never special-case it; an
    /// empty sequence is a no-op.
    pub fn bind_sequence(
        &mut self,
        mode: Mode,
        keys: Vec<Key>,
        action: Action,
        desc: impl Into<String>,
    ) {
        match keys.as_slice() {
            [] => {}
            [single] => self.bind(mode, single.clone(), action, desc),
            _ => {
                self.sequences
                    .insert((mode, keys), Binding::new(action, desc));
            }
        }
    }

    /// Exact-match lookup for a full key sequence.
    #[must_use]
    pub fn lookup_sequence(&self, mode: Mode, keys: &[Key]) -> Option<&Binding> {
        self.sequences.get(&(mode, keys.to_vec()))
    }

    /// Does any bound sequence in `mode` STRICTLY extend `prefix`
    /// (i.e. `prefix` is a proper prefix of a longer bound sequence)?
    /// Drives the runtime's pending-stroke state: a partial sequence
    /// that is still a live prefix is held pending rather than
    /// dispatched. Linear scan — fine at fleet sequence counts; a
    /// trie is a later optimization if profiling ever asks for it.
    #[must_use]
    pub fn is_sequence_prefix(&self, mode: Mode, prefix: &[Key]) -> bool {
        self.sequences
            .keys()
            .any(|(m, seq)| *m == mode && seq.len() > prefix.len() && seq.starts_with(prefix))
    }

    /// Count of bound multi-key sequences — for `--keymap` / doctor.
    #[must_use]
    pub fn sequence_len(&self) -> usize {
        self.sequences.len()
    }

    #[must_use]
    pub fn dispatch(&self, state: &ModalState, key: &Key) -> CountedAction {
        let mode = state.mode();
        if mode == Mode::Normal {
            if let Key::Char(c) = key {
                if c.is_ascii_digit() && *c != '0' {
                    return CountedAction::once(Action::Pending);
                }
                if *c == '0' && state.pending_count().is_some() {
                    return CountedAction::once(Action::Pending);
                }
            }
        }
        if mode == Mode::Insert {
            if let Key::Char(c) = key {
                return CountedAction::once(Action::InsertChar(*c));
            }
            if matches!(key, Key::Enter) {
                return CountedAction::once(Action::InsertChar('\n'));
            }
        }
        if mode == Mode::Command {
            if let Key::Char(c) = key {
                return CountedAction::once(Action::InsertChar(*c));
            }
        }
        if let Some(b) = self.lookup(mode, key) {
            return CountedAction::repeated(state.pending_count().unwrap_or(1), b.action.clone());
        }
        CountedAction::once(Action::Pending)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Sorted view over every binding — for `escriba --keymap` and palettes.
    #[must_use]
    pub fn entries_sorted(&self) -> Vec<(&Mode, &Key, &Binding)> {
        let mut v: Vec<_> = self.bindings.iter().map(|((m, k), b)| (m, k, b)).collect();
        v.sort_by(|a, b| {
            (a.0.as_str(), format!("{:?}", a.1)).cmp(&(b.0.as_str(), format!("{:?}", b.1)))
        });
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_vim_has_bindings() {
        let k = Keymap::default_vim();
        assert!(k.len() > 10);
        assert!(k.lookup(Mode::Normal, &Key::Char('h')).is_some());
        assert!(k.lookup(Mode::Insert, &Key::Esc).is_some());
        assert!(k.lookup(Mode::Normal, &Key::Alt('f')).is_some());
    }

    #[test]
    fn dispatch_normal_motion() {
        let k = Keymap::default_vim();
        let s = ModalState::new();
        let a = k.dispatch(&s, &Key::Char('h'));
        assert_eq!(a.count, 1);
        assert_eq!(a.action, Action::Move(Motion::Left));
    }

    #[test]
    fn dispatch_count_prefix_pends() {
        let k = Keymap::default_vim();
        let s = ModalState::new();
        assert!(matches!(
            k.dispatch(&s, &Key::Char('5')).action,
            Action::Pending
        ));
    }

    #[test]
    fn dispatch_insert_char() {
        let k = Keymap::default_vim();
        let mut s = ModalState::new();
        s.enter(Mode::Insert);
        let a = k.dispatch(&s, &Key::Char('a'));
        assert_eq!(a.action, Action::InsertChar('a'));
    }

    #[test]
    fn lisp_structural_motions_bound() {
        let k = Keymap::default_vim();
        assert_eq!(
            k.lookup(Mode::Normal, &Key::Alt('f')).unwrap().action,
            Action::Move(Motion::ForwardSexp)
        );
    }

    #[test]
    fn default_leader_is_comma() {
        assert_eq!(Keymap::new().leader(), &Key::Char(','));
    }

    #[test]
    fn bind_sequence_stores_multikey_and_resolves() {
        let mut k = Keymap::new();
        let seq = vec![Key::Char(','), Key::Char('f'), Key::Char('f')];
        k.bind_sequence(
            Mode::Normal,
            seq.clone(),
            Action::Command {
                name: "picker.files".into(),
                args: vec![],
            },
            "find files",
        );
        // Exact match resolves.
        let b = k.lookup_sequence(Mode::Normal, &seq).expect("seq bound");
        assert!(matches!(&b.action, Action::Command { name, .. } if name == "picker.files"));
        // Proper prefixes are live; the full sequence is NOT a prefix
        // of itself.
        assert!(k.is_sequence_prefix(Mode::Normal, &[Key::Char(',')]));
        assert!(k.is_sequence_prefix(Mode::Normal, &[Key::Char(','), Key::Char('f')]));
        assert!(!k.is_sequence_prefix(Mode::Normal, &seq));
        // Wrong mode → not a prefix.
        assert!(!k.is_sequence_prefix(Mode::Insert, &[Key::Char(',')]));
        assert_eq!(k.sequence_len(), 1);
    }

    #[test]
    fn bind_sequence_length_one_delegates_to_single() {
        let mut k = Keymap::new();
        k.bind_sequence(
            Mode::Normal,
            vec![Key::Char('x')],
            Action::Undo,
            "x is undo",
        );
        // Lands in the single-key table, not the sequence table.
        assert_eq!(k.sequence_len(), 0);
        assert!(k.lookup(Mode::Normal, &Key::Char('x')).is_some());
    }
}
