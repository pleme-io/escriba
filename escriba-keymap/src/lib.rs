//! `escriba-keymap` — mode-aware keybinding dispatch.

extern crate self as escriba_keymap;

use std::collections::HashMap;

use escriba_core::{Action, CountedAction, Mode, Motion};
use escriba_mode::ModalState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Default, Clone)]
pub struct Keymap {
    bindings: HashMap<(Mode, Key), Binding>,
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

    #[must_use]
    pub fn dispatch(&self, state: &ModalState, key: &Key) -> CountedAction {
        if state.mode == Mode::Normal {
            if let Key::Char(c) = key {
                if c.is_ascii_digit() && *c != '0' {
                    return CountedAction::once(Action::Pending);
                }
                if *c == '0' && state.pending_count.is_some() {
                    return CountedAction::once(Action::Pending);
                }
            }
        }
        if state.mode == Mode::Insert {
            if let Key::Char(c) = key {
                return CountedAction::once(Action::InsertChar(*c));
            }
            if matches!(key, Key::Enter) {
                return CountedAction::once(Action::InsertChar('\n'));
            }
        }
        if state.mode == Mode::Command {
            if let Key::Char(c) = key {
                return CountedAction::once(Action::InsertChar(*c));
            }
        }
        if let Some(b) = self.lookup(state.mode, key) {
            return CountedAction::repeated(state.pending_count.unwrap_or(1), b.action.clone());
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
}
