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
    Delete,
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

/// A binding that will not do what its author intended.
///
/// Recorded at BIND time rather than discovered later, because both kinds are
/// silent by construction: a reserved chord never receives its event, and an
/// overwritten binding simply stops existing. Neither produces an error at
/// the moment it happens, and both present as "that key is broken".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Collision {
    /// Something outside escriba owns this chord — the OS, the window
    /// manager, the terminal. The binding can never fire.
    ///
    /// Not arbitrable: no amount of reordering inside escriba changes it.
    Reserved {
        mode: Mode,
        key: String,
        description: String,
        /// Who took it and what for.
        why: String,
    },
    /// A later binding displaced an earlier one for the same chord.
    ///
    /// Sometimes intended — the shipped rc deliberately overrides defaults —
    /// which is why this is REPORTED rather than refused. But it is reported,
    /// because "my plugin's key stopped working" has no other explanation
    /// available to an operator.
    Displaced {
        mode: Mode,
        key: String,
        replaced: String,
        with: String,
    },
}

impl Collision {
    /// One line an operator can act on.
    #[must_use]
    pub fn report(&self) -> String {
        match self {
            Self::Reserved {
                mode,
                key,
                description,
                why,
            } => format!("{mode:?} {key} ({description}) — {why}"),
            Self::Displaced {
                mode,
                key,
                replaced,
                with,
            } => format!("{mode:?} {key} — \"{replaced}\" was replaced by \"{with}\""),
        }
    }

    /// Can this binding ever fire?
    #[must_use]
    pub const fn is_fatal(&self) -> bool {
        matches!(self, Self::Reserved { .. })
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
    /// Chords the world owns. Consulted on every bind, so a binding that
    /// cannot fire is known at CONSTRUCTION rather than discovered by an
    /// operator pressing a dead key.
    reserved: awase::Reserved,
    /// Every collision seen while building this keymap, in bind order.
    collisions: Vec<Collision>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: HashMap::new(),
            sequences: HashMap::new(),
            reserved: awase::Reserved::fleet_darwin(),
            collisions: Vec::new(),
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
        m.bind(
            Mode::Command,
            Key::Delete,
            Action::PromptDelete,
            "delete char at caret",
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
        // Walk the preview without committing — `/pat` then `<C-g><C-g>` is
        // `/pat<CR>nn`, except Escape still takes you home.
        m.bind(
            Mode::Command,
            Key::Ctrl('g'),
            Action::SearchPreviewStep { forward: true },
            "preview next match",
        );
        m.bind(
            Mode::Command,
            Key::Ctrl('t'),
            Action::SearchPreviewStep { forward: false },
            "preview previous match",
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
        let binding = Binding::new(action, desc);
        self.note_collisions(mode, std::slice::from_ref(&key), &binding);
        self.bindings.insert((mode, key), binding);
    }

    /// Record anything about this bind that will surprise its author.
    ///
    /// Called on every bind, single or sequence. Detection is DEFAULT-ON and
    /// costs one hash lookup plus one conversion — a keymap that only tells
    /// you about collisions when asked is a keymap nobody asks.
    fn note_collisions(&mut self, mode: Mode, keys: &[Key], binding: &Binding) {
        let Some(first) = keys.first() else { return };
        let spelled = format!("{keys:?}");

        // (1) Does the world own it? For a sequence this is its OPENER —
        // a sequence whose first key never arrives can never begin.
        if let Some(hk) = to_hotkey(first) {
            if let Some(why) = self.reserved.refuse(&hk) {
                self.collisions.push(Collision::Reserved {
                    mode,
                    key: spelled.clone(),
                    description: binding.description.clone(),
                    why,
                });
            }
        }

        // (2) Is something already here? `HashMap::insert` returns the old
        // value and every caller dropped it, so a displaced binding left no
        // trace at all.
        let existing = if keys.len() == 1 {
            self.bindings
                .get(&(mode, first.clone()))
                .map(|b| &b.description)
        } else {
            self.sequences
                .get(&(mode, keys.to_vec()))
                .map(|b| &b.description)
        };
        if let Some(replaced) = existing {
            self.collisions.push(Collision::Displaced {
                mode,
                key: spelled,
                replaced: replaced.clone(),
                with: binding.description.clone(),
            });
        }
    }

    /// Every collision recorded while this keymap was built.
    ///
    /// Read by `--list-rc` and reported at boot. An empty slice is the
    /// claim "every binding escriba ships can actually fire".
    #[must_use]
    pub fn collisions(&self) -> &[Collision] {
        &self.collisions
    }

    /// Collisions that mean a key can NEVER fire, as opposed to one that was
    /// deliberately overridden.
    pub fn fatal_collisions(&self) -> impl Iterator<Item = &Collision> {
        self.collisions.iter().filter(|c| c.is_fatal())
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
                let binding = Binding::new(action, desc);
                self.note_collisions(mode, &keys, &binding);
                self.sequences.insert((mode, keys), binding);
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

    /// Every bound sequence in `mode` that STRICTLY extends `prefix`.
    ///
    /// [`is_sequence_prefix`](Self::is_sequence_prefix) answers the same
    /// question with a `bool` and throws away the matches it just found. Two
    /// consumers need those matches:
    ///
    /// - a **which-key popup**, which must show what continues `<leader>`;
    /// - a **reserved-chord audit**, which cannot check bindings it cannot
    ///   enumerate — and `sequences` is private, so from outside this crate
    ///   the multi-key half of the keymap was invisible.
    ///
    /// Same linear scan as `is_sequence_prefix`, so this costs nothing extra;
    /// a trie is the same later optimization for both.
    ///
    /// Pass an empty `prefix` for every sequence in the mode.
    #[must_use]
    pub fn sequences_extending(&self, mode: Mode, prefix: &[Key]) -> Vec<(&[Key], &Binding)> {
        let mut v: Vec<(&[Key], &Binding)> = self
            .sequences
            .iter()
            .filter(|((m, seq), _)| {
                *m == mode && seq.len() > prefix.len() && seq.starts_with(prefix)
            })
            .map(|((_, seq), b)| (seq.as_slice(), b))
            .collect();
        // Sorted, because a which-key popup in HashMap order is a popup that
        // reorders itself between presses.
        v.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));
        v
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

/// escriba's `Key` as the fleet's chord vocabulary.
///
/// # Why a conversion rather than a migration (yet)
///
/// `escriba_keymap::Key` folds the modifier INTO the key — `Ctrl(char)`,
/// `Alt(char)` — over 16 variants. `awase::Hotkey` carries modifiers as a
/// bitflag SET over 116 key variants. The escriba shape therefore cannot
/// express `Ctrl+Shift+P`, cannot carry `Super`, and has no F-keys at all
/// (`escriba-input` discards `KeyCode::F(_)` at the door).
///
/// Migrating the whole keymap is the destination and it touches 52 call
/// sites. This conversion is what lets the **reserved-chord audit** run
/// today, before that lands: a binding escriba cannot even ask about is a
/// binding that silently dies when the window manager takes its chord.
///
/// Returns `None` for a key with no fleet spelling — today the shifted
/// digits and punctuation (`#`, `$`, `*`), which awase's `Key` does not
/// carry. That is the honest answer, and an audit must treat an unmappable
/// key as UNAUDITED rather than as available: silently counting it as clean
/// is how `Ctrl+Space` stayed hidden.
#[must_use]
pub fn to_hotkey(key: &Key) -> Option<awase::Hotkey> {
    use awase::{Hotkey, Key as AK, Modifiers as M};
    // `from_name` takes NAMES ("space"), not literal characters. Spelling a
    // space as " " returns None — which is how `Ctrl+Space` slipped past the
    // reserved audit while being bound in Insert mode AND owned by the OS.
    let named = |c: char| match c {
        ' ' => Some(AK::Space),
        c => AK::from_name(&c.to_ascii_lowercase().to_string()),
    };
    Some(match key {
        Key::Char(c) => Hotkey::new(M::NONE, named(*c)?),
        Key::Ctrl(c) => Hotkey::new(M::CTRL, named(*c)?),
        Key::Alt(c) => Hotkey::new(M::ALT, named(*c)?),
        Key::Esc => Hotkey::new(M::NONE, AK::Escape),
        Key::Enter => Hotkey::new(M::NONE, AK::Return),
        Key::Tab => Hotkey::new(M::NONE, AK::Tab),
        Key::Backspace => Hotkey::new(M::NONE, AK::Backspace),
        Key::Delete => Hotkey::new(M::NONE, AK::Delete),
        Key::Left => Hotkey::new(M::NONE, AK::Left),
        Key::Right => Hotkey::new(M::NONE, AK::Right),
        Key::Up => Hotkey::new(M::NONE, AK::Up),
        Key::Down => Hotkey::new(M::NONE, AK::Down),
        Key::PageUp => Hotkey::new(M::NONE, AK::PageUp),
        Key::PageDown => Hotkey::new(M::NONE, AK::PageDown),
        Key::Home => Hotkey::new(M::NONE, AK::Home),
        Key::End => Hotkey::new(M::NONE, AK::End),
    })
}

#[cfg(test)]
mod fleet_vocabulary {
    use super::*;

    #[test]
    fn modifiers_survive_the_conversion() {
        let h = to_hotkey(&Key::Ctrl('w')).expect("ctrl+w maps");
        assert!(h.modifiers.contains(awase::Modifiers::CTRL));
        assert_eq!(h.key, awase::Key::W);
    }

    #[test]
    fn named_keys_map_to_their_fleet_spelling() {
        // escriba says `Esc`/`Enter`; awase says `Escape`/`Return`. The
        // fleet atlas already warns that these two spellings diverge across
        // consumers, which is exactly what a shared vocabulary settles.
        assert_eq!(
            to_hotkey(&Key::Esc).map(|h| h.key),
            Some(awase::Key::Escape)
        );
        assert_eq!(
            to_hotkey(&Key::Enter).map(|h| h.key),
            Some(awase::Key::Return)
        );
    }

    #[test]
    fn every_variant_of_escribas_key_has_a_fleet_spelling() {
        // If one did not, the reserved audit would have a blind spot exactly
        // where escriba's vocabulary is unusual — which is where a collision
        // is most likely.
        let all = [
            Key::Char('a'),
            Key::Ctrl('a'),
            Key::Alt('a'),
            Key::Esc,
            Key::Enter,
            Key::Tab,
            Key::Backspace,
            Key::Delete,
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::PageUp,
            Key::PageDown,
            Key::Home,
            Key::End,
        ];
        for k in all {
            assert!(to_hotkey(&k).is_some(), "{k:?} has no fleet spelling");
        }
    }

    #[test]
    fn sequences_can_now_be_enumerated() {
        // `sequences` was private with no accessor, so the multi-key half of
        // the keymap was invisible from outside this crate — unauditable and
        // un-displayable.
        let k = Keymap::default_vim();
        let all = k.sequences_extending(Mode::Normal, &[]);
        assert!(!all.is_empty(), "the default keymap binds sequences");
        let g = k.sequences_extending(Mode::Normal, &[Key::Char('g')]);
        assert!(
            g.iter()
                .all(|(seq, _)| seq.first() == Some(&Key::Char('g'))),
            "a prefix query returns only its own continuations",
        );
    }
}

#[cfg(test)]
mod collision_detection {
    use super::*;

    #[test]
    fn a_reserved_chord_is_recorded_at_bind_time() {
        // Not discovered later by an audit — known the moment it is written.
        let mut m = Keymap::new();
        m.bind(Mode::Normal, Key::Alt('j'), Action::Undo, "focus down?");
        let c = m.collisions();
        assert_eq!(c.len(), 1, "{c:?}");
        assert!(c[0].is_fatal(), "a chord the world owns can never fire");
        assert!(
            c[0].report().contains("window manager"),
            "{}",
            c[0].report()
        );
    }

    #[test]
    fn a_displaced_binding_is_recorded_but_not_fatal() {
        // Overriding is sometimes intended — the shipped rc deliberately
        // overrides defaults — so it is REPORTED, never refused. But "my
        // plugin's key stopped working" has no other explanation available.
        let mut m = Keymap::new();
        m.bind(Mode::Normal, Key::Char('x'), Action::Undo, "first");
        m.bind(Mode::Normal, Key::Char('x'), Action::Redo, "second");
        let c = m.collisions();
        assert_eq!(c.len(), 1);
        assert!(!c[0].is_fatal());
        let r = c[0].report();
        assert!(r.contains("first") && r.contains("second"), "{r}");
    }

    #[test]
    // OPENER is shouted because which key is checked IS the point.
    #[allow(non_snake_case)]
    fn a_sequence_whose_OPENER_is_reserved_is_caught() {
        // `alt-j` then anything can never begin, because the first key never
        // arrives. Checking only single keys would miss the whole sequence.
        let mut m = Keymap::new();
        m.bind_sequence(
            Mode::Normal,
            vec![Key::Alt('j'), Key::Char('x')],
            Action::Undo,
            "dead sequence",
        );
        assert!(m.fatal_collisions().count() == 1, "{:?}", m.collisions(),);
    }

    #[test]
    fn an_ordinary_keymap_records_nothing() {
        // The detector must be quiet when there is nothing to say, or it
        // becomes noise an operator learns to skip.
        let mut m = Keymap::new();
        m.bind(Mode::Normal, Key::Char('h'), Action::Undo, "left");
        m.bind(Mode::Normal, Key::Ctrl('w'), Action::Redo, "window prefix");
        assert!(m.collisions().is_empty(), "{:?}", m.collisions());
    }

    #[test]
    fn the_shipped_default_keymap_is_clean() {
        let m = Keymap::default_vim();
        assert!(
            m.collisions().is_empty(),
            "escriba's own defaults must not collide:\n  {}",
            m.collisions()
                .iter()
                .map(Collision::report)
                .collect::<Vec<_>>()
                .join("\n  "),
        );
    }
}
