//! Translate crossterm keyboard events into `escriba_keymap::Key`.
//!
//! Mirrors the shape of `escriba-input` (which translates madori events);
//! same abstractions, different upstream source.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use escriba_keymap::Key;

/// Translate one crossterm [`KeyEvent`] into an escriba [`Key`].
#[must_use]
pub fn translate_crossterm_key(e: &KeyEvent) -> Option<Key> {
    let ctrl = e.modifiers.contains(KeyModifiers::CONTROL);
    let alt = e.modifiers.contains(KeyModifiers::ALT);

    let key = match e.code {
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Char(c) => {
            if ctrl && !alt {
                return Some(Key::Ctrl(c.to_ascii_lowercase()));
            }
            if alt && !ctrl {
                return Some(Key::Alt(c.to_ascii_lowercase()));
            }
            Key::Char(c)
        }
        _ => return None,
    };
    Some(key)
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
}
