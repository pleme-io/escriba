//! The picker — a filtered list of candidates that holds keys while open.
//!
//! ## What is escriba's here and what is not
//!
//! The narrowing machine is **`egaku::FuzzyPicker<T>`**, a fleet library: a
//! typed `PickerEvent → PickerEffect<T>` state machine with its own scoring
//! and no rendering dependencies. escriba does not own that and must not
//! reimplement it — the fleet already carries three keymap implementations
//! because things got rewritten instead of consumed.
//!
//! What escriba owns is the two ends nobody else can supply:
//!
//! - **the SOURCE** — where candidates come from, and what accepting one
//!   MEANS. `Choice` is that answer, and it is deliberately a closed enum
//!   rather than a string, so a picker whose accept nothing handles cannot be
//!   constructed.
//! - **the key translation** — escriba's `Key` into `PickerEvent`.
//!
//! ## Why a closed `Choice` rather than a callback
//!
//! A callback would let a source decide what happens on accept, which sounds
//! flexible and is how the editor would acquire a second dispatch path. An
//! accepted pick lowers into a `Negai` and goes through the one interpreter
//! like everything else; the enum is what forces that.

use egaku::picker::{FuzzyPicker, PickerEffect, PickerEvent};

/// Re-exported so consumers build items without depending on egaku directly.
/// escriba's crates speak to the fleet library through THIS module; a second
/// import path is how two versions of a type end up in one workspace.
pub use egaku::picker::PickerItem;
use escriba_core::BufferId;

/// What accepting a row MEANS.
///
/// Closed on purpose. Adding a source is adding a variant, which fails the
/// interpreter to compile until the accept is handled — the same seal
/// `Negai` uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Switch to this buffer.
    Buffer(BufferId),
    /// Run this command by name.
    Command(String),
    /// Open `path` at its start — a file, with no particular line.
    OpenFile(std::path::PathBuf),
    /// Open `path` and put the cursor on `line` (0-based).
    ///
    /// The buffer may not be open yet, which is exactly why this is a PATH
    /// and not a `BufferId`: a grep hit names a place in the project, not a
    /// place in the editor's current state.
    Location { path: std::path::PathBuf, line: u32 },
}

/// Which source a picker is showing. Used for its title, and to keep the
/// operator oriented about what they are picking FROM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Buffers,
    Commands,
    /// Every binding: key, what it runs, what it does. The searchable keymap
    /// — "how do I do X" rather than "run X".
    Help,
    /// Matches for a pattern across the project.
    Grep,
    /// Files under the working directory.
    Files,
    /// Directories that look like project roots.
    Project,
    /// Located findings — diagnostics, conflicts, grep results published as
    /// a list. The `trouble.*` family's view.
    Findings,
}

impl Source {
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Buffers => "Buffers",
            Self::Commands => "Commands",
            Self::Help => "Help",
            Self::Grep => "Grep",
            Self::Files => "Files",
            Self::Project => "Project",
            Self::Findings => "Diagnostics",
        }
    }
}

/// An open picker: the fleet's narrowing machine plus escriba's source.
#[derive(Debug)]
pub struct Picker {
    inner: FuzzyPicker<Choice>,
    source: Source,
}

/// What a keypress did to an open picker.
///
/// Total over the outcomes so a caller cannot forget the "still open"
/// case — the splash's three-arm enum widened by exactly the thing a picker
/// needs and a one-shot screen does not: it holds keys for MANY presses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Consumed {
    /// No picker is open; the key was not ours.
    NotShowing,
    /// The picker took the key and is still open.
    Held,
    /// The picker took the key and closed without committing.
    Dismissed,
    /// A row was committed; the picker is closed.
    Chose(Choice),
}

impl Picker {
    /// Open a picker over `items`.
    #[must_use]
    pub fn open(source: Source, items: Vec<PickerItem<Choice>>) -> Self {
        let mut inner = FuzzyPicker::new(items);
        let _ = inner.on_event(PickerEvent::Open);
        Self { inner, source }
    }

    #[must_use]
    pub const fn source(&self) -> Source {
        self.source
    }

    #[must_use]
    pub fn query(&self) -> &str {
        self.inner.query()
    }

    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.inner.visible_count()
    }

    /// The rows to paint, as `(label, selected)`.
    ///
    /// Borrowed from the machine rather than copied, so a face cannot paint a
    /// stale view.
    #[must_use]
    pub fn rows(&self) -> Vec<(String, bool)> {
        let view = self.inner.view();
        let sel = view.selected;
        view.rows
            .iter()
            .enumerate()
            .map(|(i, it)| (it.label.clone(), i == sel))
            .collect()
    }

    /// Feed a key. `None` for a key the picker has no meaning for — which is
    /// HELD rather than passed through, because a picker that let unknown
    /// keys reach the buffer would edit the file behind the overlay.
    pub fn on_key(&mut self, key: &escriba_keymap::Key) -> Consumed {
        let Some(event) = translate(key) else {
            return Consumed::Held;
        };
        for effect in self.inner.on_event(event) {
            match effect {
                PickerEffect::Accepted { key } => return Consumed::Chose(key),
                PickerEffect::Cancelled => return Consumed::Dismissed,
                PickerEffect::Opened
                | PickerEffect::Filtered { .. }
                | PickerEffect::Moved { .. } => {}
            }
        }
        Consumed::Held
    }
}

/// escriba's `Key` → egaku's `PickerEvent`.
///
/// The bindings are the ones every picker in the category agrees on
/// (telescope, helm, fzf, Cmd-P), so muscle memory transfers: `<C-n>`/`<C-p>`
/// as well as the arrows, `<Esc>` to dismiss, `<CR>` to accept.
#[must_use]
pub fn translate(key: &escriba_keymap::Key) -> Option<PickerEvent> {
    use escriba_keymap::Key;
    Some(match key {
        Key::Char(c) => PickerEvent::Type(*c),
        Key::Backspace => PickerEvent::Backspace,
        Key::Up | Key::Ctrl('p') => PickerEvent::NavUp,
        Key::Down | Key::Ctrl('n') => PickerEvent::NavDown,
        Key::Enter => PickerEvent::Accept,
        Key::Esc | Key::Ctrl('c') => PickerEvent::Cancel,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker() -> Picker {
        Picker::open(
            Source::Buffers,
            vec![
                PickerItem::new(Choice::Buffer(BufferId(1)), "alpha.rs"),
                PickerItem::new(Choice::Buffer(BufferId(2)), "beta.rs"),
                PickerItem::new(Choice::Buffer(BufferId(3)), "gamma.txt"),
            ],
        )
    }

    #[test]
    fn typing_narrows_the_rows() {
        let mut p = picker();
        assert_eq!(p.visible_count(), 3);
        assert_eq!(p.on_key(&escriba_keymap::Key::Char('b')), Consumed::Held);
        assert!(p.visible_count() < 3, "typing must filter");
        assert_eq!(p.query(), "b");
    }

    #[test]
    fn enter_commits_the_highlighted_row() {
        let mut p = picker();
        match p.on_key(&escriba_keymap::Key::Enter) {
            Consumed::Chose(Choice::Buffer(_)) => {}
            other => panic!("Enter must commit, got {other:?}"),
        }
    }

    #[test]
    fn esc_dismisses_without_committing() {
        let mut p = picker();
        assert_eq!(p.on_key(&escriba_keymap::Key::Esc), Consumed::Dismissed);
    }

    #[test]
    fn an_unknown_key_is_held_not_passed_through() {
        // The load-bearing one. A key the picker has no meaning for must NOT
        // fall through to the buffer — an overlay that let `x` reach the
        // editor would delete a character behind itself.
        let mut p = picker();
        assert_eq!(
            p.on_key(&escriba_keymap::Key::Ctrl('w')),
            Consumed::Held,
            "an unknown key must be swallowed by the open overlay",
        );
    }

    #[test]
    fn navigation_moves_the_selection() {
        let mut p = picker();
        let first = p.rows().iter().position(|(_, sel)| *sel);
        p.on_key(&escriba_keymap::Key::Ctrl('n'));
        let second = p.rows().iter().position(|(_, sel)| *sel);
        assert_ne!(first, second, "<C-n> must move the highlight");
    }

    #[test]
    fn both_the_arrows_and_the_control_pair_navigate() {
        // Muscle memory transfers from every picker in the category; binding
        // only one of the two pairs is how a picker feels broken.
        let mut a = picker();
        a.on_key(&escriba_keymap::Key::Down);
        let via_arrow = a.rows().iter().position(|(_, s)| *s);
        let mut b = picker();
        b.on_key(&escriba_keymap::Key::Ctrl('n'));
        assert_eq!(via_arrow, b.rows().iter().position(|(_, s)| *s));
    }
}
