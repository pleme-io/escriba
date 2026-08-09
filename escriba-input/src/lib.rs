//! `escriba-input` — translate madori input events into escriba's key
//! abstraction.
//!
//! Two layers:
//!   - [`translate_key`] takes a `madori::KeyEvent` (press only) and returns
//!     the corresponding `escriba_keymap::Key` — Ctrl/Alt modifiers preserved,
//!     printable chars passed through.
//!   - [`EditorInput`] is the full frame-level dispatch — handles window
//!     resizes, focus changes, IME, mouse events, and key events uniformly.

use escriba_keymap::Key;
use madori::AppEvent;
use madori::event::{KeyCode, KeyEvent};

/// Translate a single `madori::KeyEvent` into an `escriba_keymap::Key`.
///
/// Returns `None` for non-printable keys with no match (modifiers-only
/// presses, unknown keycodes, releases).
#[must_use]
pub fn translate_key(event: &KeyEvent) -> Option<Key> {
    if !event.pressed {
        return None;
    }
    let mods = event.modifiers;
    let key = match event.key {
        KeyCode::Enter => Key::Enter,
        KeyCode::Escape => Key::Esc,
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
        KeyCode::Space => Key::Char(' '),
        KeyCode::Char(c) => {
            // The shorthands carry exactly ONE modifier. Anything richer —
            // two modifiers, Super, or Shift as a modifier rather than a
            // capital letter — goes through `Chord`, which carries the fleet
            // type. Before this, `shift` and `meta` were computed here and
            // then dropped for want of anywhere to put them.
            let extra = mods.meta || (mods.ctrl && mods.alt);
            if extra {
                return chord_of(c, mods).map(Key::Chord);
            }
            if mods.ctrl {
                return Some(Key::Ctrl(c.to_ascii_lowercase()));
            }
            if mods.alt {
                return Some(Key::Alt(c.to_ascii_lowercase()));
            }
            Key::Char(c)
        }
        // `Delete` is a real prompt verb (`Action::DeleteForward`), not noise —
        // dropping it here is why `<Del>` was documented but unreachable.
        KeyCode::Delete => Key::Delete,
        // F-keys used to die here — declared in an rc, never delivered.
        KeyCode::F(n) => Key::F(n),
        KeyCode::Unknown => return None,
    };
    Some(key)
}

/// Build a fleet chord from a character plus every modifier that is set.
///
/// Only reached when the single-modifier shorthands cannot express the
/// combination — see the call site.
fn chord_of(c: char, mods: madori::event::Modifiers) -> Option<awase::Hotkey> {
    use awase::Modifiers as M;
    let mut m = M::NONE;
    if mods.ctrl {
        m = m | M::CTRL;
    }
    if mods.alt {
        m = m | M::ALT;
    }
    if mods.shift {
        m = m | M::SHIFT;
    }
    if mods.meta {
        m = m | M::CMD;
    }
    let key = awase::Key::from_name(&c.to_ascii_lowercase().to_string())?;
    Some(awase::Hotkey::new(m, key))
}

/// What the runtime should do after translating an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputOutcome {
    /// A fully-translated escriba key.
    Key(Key),
    /// Window was resized.
    Resized { width: u32, height: u32 },
    /// Focus changed.
    Focus(bool),
    /// Exit requested (Cmd-Q / close button).
    Quit,
    /// Nothing translatable — ignore.
    None,
}

#[must_use]
pub fn translate_app_event(event: &AppEvent) -> InputOutcome {
    match event {
        AppEvent::Key(ke) => translate_key(ke).map_or(InputOutcome::None, InputOutcome::Key),
        AppEvent::Resized { width, height } => InputOutcome::Resized {
            width: *width,
            height: *height,
        },
        AppEvent::Focused(f) => InputOutcome::Focus(*f),
        AppEvent::CloseRequested => InputOutcome::Quit,
        // IME / Mouse / RedrawRequested — phase 1 doesn't route them.
        _ => InputOutcome::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use madori::event::Modifiers;

    fn key(kc: KeyCode, mods: Modifiers) -> KeyEvent {
        KeyEvent {
            key: kc,
            pressed: true,
            modifiers: mods,
            text: None,
        }
    }

    #[test]
    fn char_without_modifiers() {
        let k = translate_key(&key(KeyCode::Char('h'), Modifiers::default())).unwrap();
        assert_eq!(k, Key::Char('h'));
    }

    #[test]
    fn ctrl_char_becomes_ctrl_key() {
        let mods = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        let k = translate_key(&key(KeyCode::Char('R'), mods)).unwrap();
        assert_eq!(k, Key::Ctrl('r'));
    }

    #[test]
    fn alt_char_becomes_alt_key() {
        let mods = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        let k = translate_key(&key(KeyCode::Char('f'), mods)).unwrap();
        assert_eq!(k, Key::Alt('f'));
    }

    #[test]
    fn named_keys_map() {
        assert_eq!(
            translate_key(&key(KeyCode::Escape, Modifiers::default())).unwrap(),
            Key::Esc
        );
        assert_eq!(
            translate_key(&key(KeyCode::Enter, Modifiers::default())).unwrap(),
            Key::Enter
        );
        assert_eq!(
            translate_key(&key(KeyCode::Space, Modifiers::default())).unwrap(),
            Key::Char(' ')
        );
    }

    #[test]
    fn release_returns_none() {
        let mut e = key(KeyCode::Char('a'), Modifiers::default());
        e.pressed = false;
        assert!(translate_key(&e).is_none());
    }

    #[test]
    fn close_event_quits() {
        assert_eq!(
            translate_app_event(&AppEvent::CloseRequested),
            InputOutcome::Quit
        );
    }
}
