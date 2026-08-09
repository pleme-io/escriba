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
    state_with(DOC)
}

fn state_with(doc: &str) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(doc);
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

/// A Ctrl-chorded press. The bigger erases (`<C-w>`, `<C-u>`, `<C-h>`) are all
/// chords, so they exercise a translation hop — modifier → `Key::Ctrl(_)` —
/// that the plain-key tests above never touch.
fn press_ctrl(st: &mut EditorState, c: char) {
    st.tick(&AppEvent::Key(KeyEvent {
        key: KeyCode::Char(c),
        pressed: true,
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
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

// ── The bigger erases: `<C-w>`, `<C-u>`, `<C-h>` ──────────────────────
//
// `<BS>`/`<Del>` landed first and these three were left behind, so Insert mode
// could erase one character at a time and nothing larger. Same shape of defect
// as the one above — an implemented executor with no key on it — which is why
// these also assert through `tick`.

#[test]
fn ctrl_w_erases_the_word_before_the_caret() {
    let mut st = state();
    insert(&mut st);
    press(&mut st, KeyCode::End);
    assert_eq!(st.cursor().column, 11);

    press_ctrl(&mut st, 'w');

    assert!(
        text_of(&st).starts_with("alpha \ncharlie"),
        "got {:?}",
        text_of(&st)
    );
    assert_eq!(st.cursor(), escriba_core::Position::new(0, 6));
    assert_eq!(st.modal.mode(), Mode::Insert);
}

#[test]
fn a_second_ctrl_w_eats_the_gap_and_the_word_before_it() {
    // Whitespace first, then the word — otherwise the second press only
    // removes the space and reads as broken. Mirrors the prompt's own rule.
    let mut st = state();
    insert(&mut st);
    press(&mut st, KeyCode::End);
    press_ctrl(&mut st, 'w');
    press_ctrl(&mut st, 'w');

    assert!(
        text_of(&st).starts_with("\ncharlie"),
        "both words gone; got {:?}",
        text_of(&st)
    );
    assert_eq!(st.cursor(), escriba_core::Position::ZERO);
}

#[test]
fn ctrl_w_at_column_zero_joins_the_line_above() {
    // `word_prev` is single-line and returns the cursor unchanged here, so a
    // literal reading makes `<C-w>` a dead key at the start of a line. vim
    // erases the line break instead.
    let mut st = state();
    press(&mut st, KeyCode::Char('j'));
    insert(&mut st);
    assert_eq!(st.cursor(), escriba_core::Position::new(1, 0));

    press_ctrl(&mut st, 'w');

    assert!(
        text_of(&st).starts_with("alpha bravocharlie delta"),
        "got {:?}",
        text_of(&st)
    );
    assert_eq!(st.cursor(), escriba_core::Position::new(0, 11));
}

#[test]
fn ctrl_u_clears_what_was_typed_first_and_the_indent_second() {
    // The two-step. Collapsing it into "always column 0" would destroy
    // alignment on the FIRST press, which is the one the hands reach for.
    let mut st = state_with("    hello world\nnext\n");
    insert(&mut st);
    press(&mut st, KeyCode::End);
    assert_eq!(st.cursor().column, 15);

    press_ctrl(&mut st, 'u');
    assert!(
        text_of(&st).starts_with("    \nnext"),
        "first press stops at the first non-blank; got {:?}",
        text_of(&st)
    );
    assert_eq!(st.cursor(), escriba_core::Position::new(0, 4));

    press_ctrl(&mut st, 'u');
    assert!(
        text_of(&st).starts_with("\nnext"),
        "second press takes the indent; got {:?}",
        text_of(&st)
    );
    assert_eq!(st.cursor(), escriba_core::Position::ZERO);
}

#[test]
fn ctrl_u_at_column_zero_is_a_no_op_not_a_line_merge() {
    // `<C-u>` is line-scoped. Falling through to a join — the way `<C-w>`
    // deliberately does — would delete a line break the operator never aimed
    // at, and `<C-u>` is pressed blind far more often than `<C-w>`.
    let mut st = state();
    press(&mut st, KeyCode::Char('j'));
    insert(&mut st);
    assert_eq!(st.cursor(), escriba_core::Position::new(1, 0));

    press_ctrl(&mut st, 'u');

    assert_eq!(text_of(&st), DOC, "nothing erased");
    assert_eq!(st.cursor(), escriba_core::Position::new(1, 0));
}

#[test]
fn ctrl_h_is_backspace() {
    // Terminals send 0x08 for `<C-h>`, and which of `Backspace` / `Ctrl('h')`
    // a given one reports for the physical key is its own business. Binding
    // both is what makes the answer stop mattering to the operator.
    let mut st = state();
    insert(&mut st);
    type_str(&mut st, "QQQ");
    press_ctrl(&mut st, 'h');

    assert!(
        text_of(&st).starts_with("QQalpha bravo"),
        "got {:?}",
        text_of(&st)
    );
    assert_eq!(st.cursor().column, 2);
}

#[test]
fn the_bigger_erases_leave_the_register_alone_too() {
    // Same reasoning as `erasing_a_typo_does_not_clobber_the_register`, and it
    // has to be asserted separately: these reach the buffer through a
    // different call path, and it is the path — not the key — that decides
    // whether the unnamed register is captured.
    let mut st = state();
    press(&mut st, KeyCode::Char('y'));
    press(&mut st, KeyCode::Char('w'));
    let yanked = st.register().map(str::to_owned);
    assert!(yanked.is_some(), "precondition: `yw` filled the register");

    insert(&mut st);
    press(&mut st, KeyCode::End);
    press_ctrl(&mut st, 'w');
    press_ctrl(&mut st, 'u');

    // Assert the erase HAPPENED before asserting what it left alone. Without
    // this the test passes just as happily when `<C-w>`/`<C-u>` are unbound —
    // a key that does nothing also clobbers nothing, and the guard would be
    // green for the exact defect it exists to catch.
    assert!(
        text_of(&st).starts_with("\ncharlie"),
        "precondition: both erases ran; got {:?}",
        text_of(&st)
    );
    assert_eq!(st.register().map(str::to_owned), yanked);
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
        Key::Ctrl('w'),
        Key::Ctrl('u'),
        Key::Ctrl('h'),
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
