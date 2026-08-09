//! Translate crossterm keyboard events into escriba's key abstraction.
//!
//! **One table, not two.** This module used to carry its own
//! crossterm → `escriba_keymap::Key` match while `run.rs` carried a second,
//! independent crossterm → `madori::KeyEvent` match, and the event loop
//! consulted the first as a GATE before feeding the runtime through the
//! second. Two tables that had to agree, and did not: `Delete` and the F-keys
//! were present in `run.rs`'s and absent here, so the gate dropped them before
//! the runtime ever saw one. `<Del>` was bound to `Action::DeleteForward`,
//! implemented, tested — and unreachable in the default face.
//!
//! Now there is exactly one crossterm-shaped match ([`crossterm_key_event`]),
//! and [`translate_crossterm_key`] is its composition with the `escriba-input`
//! translation the GPU face already uses. A key the two faces disagree about
//! is no longer constructible.

use crossterm::event::{KeyEvent, KeyModifiers};
use escriba_keymap::Key;
use madori::event::{KeyCode as MdKey, KeyEvent as MadoriKey, Modifiers as MadoriMods};

/// Convert a crossterm key event into the madori-shaped one every escriba
/// face speaks. The SOLE crossterm-shaped translation in the crate.
#[must_use]
pub fn crossterm_key_event(e: &KeyEvent) -> MadoriKey {
    use crossterm::event::KeyCode as CkKey;

    let modifiers = MadoriMods {
        shift: e.modifiers.contains(KeyModifiers::SHIFT),
        ctrl: e.modifiers.contains(KeyModifiers::CONTROL),
        alt: e.modifiers.contains(KeyModifiers::ALT),
        meta: e.modifiers.contains(KeyModifiers::SUPER),
    };
    let key = match e.code {
        CkKey::Enter => MdKey::Enter,
        CkKey::Esc => MdKey::Escape,
        CkKey::Backspace => MdKey::Backspace,
        CkKey::Delete => MdKey::Delete,
        CkKey::Tab => MdKey::Tab,
        CkKey::Up => MdKey::Up,
        CkKey::Down => MdKey::Down,
        CkKey::Left => MdKey::Left,
        CkKey::Right => MdKey::Right,
        CkKey::Home => MdKey::Home,
        CkKey::End => MdKey::End,
        CkKey::PageUp => MdKey::PageUp,
        CkKey::PageDown => MdKey::PageDown,
        CkKey::F(n) => MdKey::F(n),
        CkKey::Char(' ') => MdKey::Space,
        CkKey::Char(c) => MdKey::Char(c),
        _ => MdKey::Unknown,
    };
    MadoriKey {
        key,
        pressed: true,
        modifiers,
        text: None,
    }
}

/// Translate one crossterm [`KeyEvent`] into an escriba [`Key`].
///
/// `None` for a key escriba has no abstraction for — the caller may drop it.
#[must_use]
pub fn translate_crossterm_key(e: &KeyEvent) -> Option<Key> {
    escriba_input::translate_key(&crossterm_key_event(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventKind, KeyEventState};

    fn ke(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn plain_char() {
        assert_eq!(
            translate_crossterm_key(&ke(KeyCode::Char('h'), KeyModifiers::NONE)),
            Some(Key::Char('h'))
        );
    }

    #[test]
    fn ctrl_char() {
        assert_eq!(
            translate_crossterm_key(&ke(KeyCode::Char('R'), KeyModifiers::CONTROL)),
            Some(Key::Ctrl('r'))
        );
    }

    #[test]
    fn alt_char() {
        assert_eq!(
            translate_crossterm_key(&ke(KeyCode::Char('f'), KeyModifiers::ALT)),
            Some(Key::Alt('f'))
        );
    }

    #[test]
    fn named_keys() {
        assert_eq!(
            translate_crossterm_key(&ke(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Key::Esc)
        );
        assert_eq!(
            translate_crossterm_key(&ke(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Key::Enter)
        );
        assert_eq!(
            translate_crossterm_key(&ke(KeyCode::Left, KeyModifiers::NONE)),
            Some(Key::Left)
        );
    }

    #[test]
    fn the_editing_keys_this_face_used_to_swallow() {
        // The regression this module exists for. Both were reachable in the
        // GPU face and dead in this one, because the gate's table and the
        // delivery's table were separate objects.
        assert_eq!(
            translate_crossterm_key(&ke(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(Key::Backspace)
        );
        assert_eq!(
            translate_crossterm_key(&ke(KeyCode::Delete, KeyModifiers::NONE)),
            Some(Key::Delete),
            "<Del> is bound to DeleteForward and must reach the runtime"
        );
        assert_eq!(
            translate_crossterm_key(&ke(KeyCode::F(5), KeyModifiers::NONE)),
            Some(Key::F(5))
        );
    }

    #[test]
    fn an_unmapped_key_is_dropped_rather_than_guessed() {
        assert_eq!(
            translate_crossterm_key(&ke(KeyCode::CapsLock, KeyModifiers::NONE)),
            None
        );
    }
}
