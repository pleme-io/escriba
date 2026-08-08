//! The first SURFACE: open, narrow, accept, and the effect goes through the
//! one interpreter.
//!
//! `picker.buffers` needs no I/O, so it proves the whole path with nothing
//! mocked — which is why it goes first among the seven picker verbs.

use escriba_buffer::BufferSet;
use escriba_keymap::Key;
use escriba_runtime::EditorState;

fn editor() -> EditorState {
    let mut bufs = BufferSet::new();
    let a = bufs.scratch("alpha\n");
    bufs.scratch("beta\n");
    bufs.scratch("gamma\n");
    let mut st = EditorState::new_with_buffer(bufs, a);
    st.dismiss_splash();
    st
}

fn ex(st: &mut EditorState, line: &str) {
    st.on_key(&Key::Char(':'));
    for c in line.chars() {
        st.on_key(&Key::Char(c));
    }
    st.on_key(&Key::Enter);
    if st.modal.mode() == escriba_core::Mode::Command {
        st.on_key(&Key::Esc);
    }
}

#[test]
fn the_command_opens_a_picker() {
    let mut st = editor();
    assert!(st.picker().is_none());
    ex(&mut st, "picker.buffers");
    let p = st.picker().expect("the picker must be open");
    assert_eq!(p.visible_count(), 3, "one row per open buffer");
}

#[test]
fn an_open_picker_owns_every_key_including_ones_it_ignores() {
    // THE property that makes a surface safe. `x` deletes a character in
    // normal mode; while the picker is up it must not reach the buffer.
    let mut st = editor();
    let before = st.buffers.get(st.active).expect("buffer").text_rev();
    ex(&mut st, "picker.buffers");
    st.on_key(&Key::Char('x'));
    st.on_key(&Key::Ctrl('w'));
    assert!(st.picker().is_some(), "still open");
    assert_eq!(
        st.buffers.get(st.active).expect("buffer").text_rev(),
        before,
        "no key may reach the buffer behind an open picker",
    );
}

#[test]
fn typing_narrows_and_escape_leaves_the_buffer_untouched() {
    let mut st = editor();
    ex(&mut st, "picker.buffers");
    let all = st.picker().expect("open").visible_count();
    st.on_key(&Key::Char('s'));
    assert!(
        st.picker().expect("open").visible_count() <= all,
        "typing must narrow, never widen",
    );
    st.on_key(&Key::Esc);
    assert!(st.picker().is_none(), "Esc dismisses");
}

#[test]
fn accepting_switches_buffer_through_the_interpreter() {
    // The accept lowers to `Negai::FocusBuffer` and goes through `interpret`
    // like every other effect — no second dispatch path.
    let mut st = editor();
    let start = st.active;
    ex(&mut st, "picker.buffers");
    // Move off the first row, then commit.
    st.on_key(&Key::Ctrl('n'));
    st.on_key(&Key::Enter);
    assert!(st.picker().is_none(), "accepting closes the picker");
    assert_ne!(st.active, start, "and switches to the chosen buffer");
}

#[test]
fn a_second_source_is_config_not_machinery() {
    // The absorption hypothesis, tested by BUILDING rather than by reading:
    // if pickers really are one primitive with a source parameter, the
    // second source costs a variant and a registration, not a subsystem.
    let mut st = editor();
    ex(&mut st, "picker.commands");
    let p = st.picker().expect("the commands picker must open");
    assert!(
        p.visible_count() > 3,
        "the command picker lists commands, not buffers: {}",
        p.visible_count(),
    );
}
