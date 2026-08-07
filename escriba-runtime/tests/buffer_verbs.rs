//! `buffer.next` / `buffer.prev` / `buffer.delete` — the first three actions
//! to leave the inert inventory.
//!
//! The interesting one is delete. `EditorState::active` is a `BufferId`, not
//! an `Option<BufferId>`, so "the active buffer was closed" is not a degraded
//! state the editor limps through — it is a state where every read of the
//! active buffer returns `None` and the frame renders `<no buffer>` forever.
//! The invariant is therefore absolute: **after any close, there is an active
//! buffer that exists**, and these tests assert it after every operation
//! rather than at the end.

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_keymap::Key;
use escriba_runtime::EditorState;

fn three_buffers() -> EditorState {
    let mut bufs = BufferSet::new();
    let first = bufs.scratch("one\n");
    bufs.scratch("two\n");
    bufs.scratch("three\n");
    EditorState::new_with_buffer(bufs, first)
}

fn ex(st: &mut EditorState, line: &str) {
    st.on_key(&Key::Char(':'));
    for c in line.chars() {
        st.on_key(&Key::Char(c));
    }
    st.on_key(&Key::Enter);
    if st.modal.mode() == Mode::Command {
        st.on_key(&Key::Esc);
    }
}

/// The invariant, checked after every operation in every test below.
fn active_is_real(st: &EditorState, when: &str) {
    assert!(
        st.buffers.get(st.active).is_some(),
        "{when}: active buffer {:?} does not exist — every read returns None \
         and the frame renders <no buffer> from here on",
        st.active,
    );
    // The window must follow, or the pane paints a buffer nobody is editing.
    if let Some(w) = st.layout.active_window() {
        assert_eq!(
            w.buffer_id, st.active,
            "{when}: the window still points at a different buffer",
        );
    }
}

#[test]
fn next_and_prev_walk_the_list_and_wrap() {
    let mut st = three_buffers();
    let ids = st.buffers.ids();
    assert_eq!(ids.len(), 3);

    ex(&mut st, "buffer.next");
    assert_eq!(st.active, ids[1]);
    active_is_real(&st, "after next");

    ex(&mut st, "buffer.next");
    assert_eq!(st.active, ids[2]);

    // Wrapping matters: without it the last buffer is a dead end and `bn`
    // silently stops working, which reads as a broken key.
    ex(&mut st, "buffer.next");
    assert_eq!(st.active, ids[0], "next must wrap");

    ex(&mut st, "buffer.prev");
    assert_eq!(st.active, ids[2], "prev must wrap the other way");
    active_is_real(&st, "after wrapping prev");
}

#[test]
fn cycling_with_one_buffer_declines_rather_than_pretending() {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("only\n");
    let mut st = EditorState::new_with_buffer(bufs, id);
    ex(&mut st, "buffer.next");
    assert_eq!(st.active, id, "nowhere to go");
    assert!(
        st.messages.iter().any(|m| m.contains("only one buffer")),
        "and it must say so rather than looking like a dead key: {:?}",
        st.messages,
    );
}

#[test]
fn deleting_the_active_buffer_moves_to_the_next_one() {
    let mut st = three_buffers();
    let ids = st.buffers.ids();
    ex(&mut st, "buffer.delete");
    active_is_real(&st, "after deleting the active buffer");
    assert_eq!(st.buffers.ids().len(), 2);
    assert_eq!(
        st.active, ids[1],
        "closing walks FORWARD, so repeated closes are predictable",
    );
}

#[test]
fn deleting_the_last_buffer_leaves_a_scratch_not_a_void() {
    // The invariant's hard case. vim's `:bd` on the last buffer leaves an
    // empty buffer rather than an editor with none; the type demands the
    // same, because `active` cannot be absent.
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("only\n");
    let mut st = EditorState::new_with_buffer(bufs, id);

    ex(&mut st, "buffer.delete");
    active_is_real(&st, "after deleting the only buffer");
    assert_eq!(st.buffers.ids().len(), 1, "a fresh scratch took its place");
    assert_ne!(st.active, id, "and it is not the buffer we just closed");
    assert_eq!(
        st.buffers
            .get(st.active)
            .map(escriba_buffer::Buffer::to_string),
        Some(String::new()),
        "the replacement is empty",
    );
}

#[test]
fn closing_every_buffer_in_turn_never_strands_the_editor() {
    // The sequence that would expose an off-by-one in the "pick the next
    // active" rule: close until nothing original remains.
    let mut st = three_buffers();
    for i in 0..3 {
        ex(&mut st, "buffer.delete");
        active_is_real(&st, &format!("after close {}", i + 1));
    }
    assert_eq!(st.buffers.ids().len(), 1, "one scratch remains");
    // …and the editor still works.
    st.on_key(&Key::Char('i'));
    st.on_key(&Key::Char('x'));
    st.on_key(&Key::Esc);
    assert_eq!(
        st.buffers
            .get(st.active)
            .map(escriba_buffer::Buffer::to_string),
        Some("x".to_string()),
        "the surviving buffer is editable",
    );
}

#[test]
fn deleting_a_non_active_buffer_leaves_the_active_one_alone() {
    let mut st = three_buffers();
    let ids = st.buffers.ids();
    let active_before = st.active;
    st.interpret(escriba_madoguchi::Outcome::did(vec![
        escriba_madoguchi::Negai::CloseBuffer(ids[2]),
    ]));
    active_is_real(&st, "after closing a background buffer");
    assert_eq!(st.active, active_before, "focus must not move");
    assert_eq!(st.buffers.ids().len(), 2);
}

#[test]
fn closing_a_buffer_that_is_not_open_is_reported() {
    let mut st = three_buffers();
    st.interpret(escriba_madoguchi::Outcome::did(vec![
        escriba_madoguchi::Negai::CloseBuffer(escriba_core::BufferId(9999)),
    ]));
    assert!(
        st.messages.iter().any(|m| m.contains("no such buffer")),
        "{:?}",
        st.messages,
    );
    active_is_real(&st, "after a no-op close");
}
