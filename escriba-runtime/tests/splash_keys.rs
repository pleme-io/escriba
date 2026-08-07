//! The start screen's one job at the keyboard: own the first keypress,
//! then get out of the way.
//!
//! The failure this pins is the one every dashboard plugin has shipped at
//! some point — the welcome screen eats the first key. An operator who
//! opens the editor and immediately types `i` expects to be inserting, not
//! to have spent a keystroke closing a screen they were not reading.

use escriba_buffer::BufferSet;
use escriba_core::{Action, Mode};
use escriba_keymap::Key;
use escriba_runtime::EditorState;
use escriba_ui::splash::{Splash, SplashEntry};

fn splash() -> Splash {
    Splash {
        art: vec!["ESCRIBA".into()],
        tagline: "a modal editor".into(),
        entries: vec![
            SplashEntry {
                key: 'i',
                label: "insert text".into(),
                action: Action::ChangeMode(Mode::Insert),
            },
            SplashEntry {
                key: 'q',
                label: "quit".into(),
                action: Action::Quit,
            },
        ],
        facts: vec!["v0".into()],
    }
}

fn showing() -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("hello world\n");
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.set_splash(splash());
    assert!(st.splash().is_some(), "fixture must start with a screen up");
    st
}

#[test]
fn a_menu_key_runs_its_entry_and_closes_the_screen() {
    let mut st = showing();
    st.on_key(&Key::Char('i'));
    assert!(st.splash().is_none(), "the screen must close");
    assert_eq!(st.modal.mode(), Mode::Insert, "and the entry must run");
}

#[test]
fn a_non_menu_key_closes_the_screen_and_still_does_its_job() {
    // `j` is not on the menu. It must move the cursor, not be swallowed.
    let mut st = showing();
    st.on_key(&Key::Char('j'));
    assert!(st.splash().is_none());
    assert_eq!(
        st.cursor().line,
        1,
        "the first keystroke must not be eaten by the start screen",
    );
}

#[test]
fn the_screen_never_comes_back_on_its_own() {
    let mut st = showing();
    st.on_key(&Key::Char('j'));
    for k in ['k', 'l', 'h'] {
        st.on_key(&Key::Char(k));
        assert!(st.splash().is_none(), "dismissed means dismissed");
    }
}

#[test]
fn a_non_char_key_dismisses_without_selecting_anything() {
    let mut st = showing();
    st.on_key(&Key::Esc);
    assert!(st.splash().is_none());
    assert_eq!(st.modal.mode(), Mode::Normal, "Esc selected no entry");
}

#[test]
fn an_empty_screen_is_refused_rather_than_painted_blank() {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("x");
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.set_splash(Splash::default());
    assert!(
        st.splash().is_none(),
        "an empty splash must not hide a perfectly good buffer",
    );
}

#[test]
fn raising_the_screen_bumps_the_refresh_generation() {
    // The GPU face caches its shaped buffer against this generation; a
    // screen raised without bumping it would not appear until the next
    // unrelated edit.
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("x");
    let mut st = EditorState::new_with_buffer(bufs, id);
    let before = st.edit_gen();
    st.set_splash(splash());
    let raised = st.edit_gen();
    assert_ne!(before, raised, "raising must invalidate the cached frame");
    st.dismiss_splash();
    assert_ne!(raised, st.edit_gen(), "dismissing must too");
}

#[test]
fn dismissing_an_absent_screen_costs_nothing() {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("x");
    let mut st = EditorState::new_with_buffer(bufs, id);
    let before = st.edit_gen();
    st.dismiss_splash();
    assert_eq!(before, st.edit_gen(), "no repaint for a no-op");
}
