//! Insert mode, driven by KEYSTROKES.
//!
//! Until 2026-08-09 `Esc` was the only binding `Mode::Insert` had, and
//! `Keymap::dispatch` answers `Key::Char` and `Key::Enter` before it consults
//! the table at all. Everything else — Backspace, `<Del>`, the arrows —
//! resolved to `Action::Pending` and did nothing. Insert mode could be typed
//! into and never corrected.
//!
//! The unit tests could not have caught it: `Action::Backspace`'s executor was
//! present and correct for prompts, and no test asked whether any KEY produced
//! it in Insert mode. So every assertion here goes through `tick`, which is
//! the only path that exercises the binding table.

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_runtime::EditorState;
use madori::event::{AppEvent, KeyCode, KeyEvent, Modifiers};

const DOC: &str = "alpha bravo\ncharlie delta\n";

fn state() -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(DOC);
    EditorState::new_with_buffer(bufs, id)
}

fn press(st: &mut EditorState, key: KeyCode) {
    st.tick(&AppEvent::Key(KeyEvent {
        key,
        pressed: true,
        modifiers: Modifiers::default(),
        text: None,
    }));
}

fn type_str(st: &mut EditorState, s: &str) {
    for c in s.chars() {
        press(st, KeyCode::Char(c));
    }
}

fn text_of(st: &EditorState) -> String {
    st.buffers
        .get(st.active)
        .map(escriba_buffer::Buffer::to_string)
        .unwrap_or_default()
}

/// Enter Insert at the start of the document.
fn insert(st: &mut EditorState) {
    press(st, KeyCode::Char('i'));
    assert_eq!(st.modal.mode(), Mode::Insert, "`i` must enter Insert");
}

#[test]
fn backspace_erases_the_character_just_typed() {
    let mut st = state();
    insert(&mut st);
    type_str(&mut st, "QQQ");
    press(&mut st, KeyCode::Backspace);

    assert!(
        text_of(&st).starts_with("QQalpha bravo"),
        "got {:?}",
        text_of(&st)
    );
    assert_eq!(st.cursor().column, 2, "the caret follows the deletion");
    assert_eq!(st.modal.mode(), Mode::Insert, "backspace does not leave Insert");
}

#[test]
fn delete_removes_the_character_ahead_and_leaves_the_caret_put() {
    let mut st = state();
    insert(&mut st);
    press(&mut st, KeyCode::Delete);

    assert!(text_of(&st).starts_with("lpha bravo"), "got {:?}", text_of(&st));
    assert_eq!(
        st.cursor().column,
        0,
        "forward-delete pulls text under a stationary caret"
    );
}

#[test]
fn backspace_at_column_zero_joins_the_line_above() {
    // The case `Motion::Left` cannot express — it saturates at column 0, so an
    // implementation routed through the motion path stops dead here instead of
    // removing the newline.
    let mut st = state();
    press(&mut st, KeyCode::Char('j')); // line 1
    insert(&mut st);
    assert_eq!(st.cursor(), escriba_core::Position::new(1, 0));

    press(&mut st, KeyCode::Backspace);

    assert!(
        text_of(&st).starts_with("alpha bravocharlie delta"),
        "the lines must join; got {:?}",
        text_of(&st)
    );
    assert_eq!(
        st.cursor(),
        escriba_core::Position::new(0, 11),
        "the caret lands at the seam"
    );
}

#[test]
fn delete_at_end_of_line_joins_the_line_below() {
    let mut st = state();
    insert(&mut st);
    press(&mut st, KeyCode::End);
    assert_eq!(st.cursor().column, 11);

    press(&mut st, KeyCode::Delete);

    assert!(
        text_of(&st).starts_with("alpha bravocharlie delta"),
        "got {:?}",
        text_of(&st)
    );
}

#[test]
fn backspace_at_the_start_of_the_document_is_a_no_op() {
    let mut st = state();
    insert(&mut st);
    press(&mut st, KeyCode::Backspace);

    assert_eq!(text_of(&st), DOC, "nothing to the left, nothing removed");
    assert_eq!(st.cursor(), escriba_core::Position::ZERO);
}

#[test]
fn erasing_a_typo_does_not_clobber_the_register() {
    // vim's insert-mode backspace leaves the unnamed register alone. Routing
    // it through `Operator::Delete` would capture the erased character, so
    // fixing a typo would silently overwrite what you yanked to paste.
    let mut st = state();
    press(&mut st, KeyCode::Char('y'));
    press(&mut st, KeyCode::Char('w'));
    let yanked = st.register().map(str::to_owned);
    assert!(yanked.is_some(), "precondition: `yw` filled the register");

    insert(&mut st);
    type_str(&mut st, "Z");
    press(&mut st, KeyCode::Backspace);

    assert_eq!(st.register().map(str::to_owned), yanked);
}

#[test]
fn the_arrows_move_the_caret_without_leaving_insert() {
    let mut st = state();
    insert(&mut st);
    press(&mut st, KeyCode::Right);
    press(&mut st, KeyCode::Right);
    press(&mut st, KeyCode::Down);
    assert_eq!(st.modal.mode(), Mode::Insert);
    assert_eq!(st.cursor(), escriba_core::Position::new(1, 2));

    // And typing lands where the arrows left the caret, rather than where
    // Insert was entered.
    type_str(&mut st, "X");
    assert!(text_of(&st).contains("chXarlie"), "got {:?}", text_of(&st));
}

#[test]
fn every_insert_editing_key_resolves_to_something() {
    // SET-shaped rather than one-assertion-per-key: a key added to the
    // Insert-mode table without a decision here still passes, but a key
    // REMOVED from it fails. That is the direction that matters — this whole
    // file exists because keys were silently absent.
    use escriba_core::Action;
    use escriba_keymap::{Key, Keymap};

    let km = Keymap::default_vim();
    for key in [
        Key::Backspace,
        Key::Delete,
        Key::Left,
        Key::Right,
        Key::Up,
        Key::Down,
        Key::Home,
        Key::End,
        Key::Esc,
    ] {
        let b = km.lookup(Mode::Insert, &key);
        assert!(b.is_some(), "Insert-mode {key:?} is unbound");
        assert_ne!(
            b.map(|b| b.action.clone()),
            Some(Action::Pending),
            "Insert-mode {key:?} resolves to a no-op"
        );
    }
}
