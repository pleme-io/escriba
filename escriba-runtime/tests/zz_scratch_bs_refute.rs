//! Scratch probe: `d` `/` `<BS>` `w` and `d` `/` `<BS>` `/` `charlie` `<CR>`.
//!
//! Uses `on_key` (post key-repeat-gate) so a second `/` in the same test is
//! not swallowed by `awase::KeyRepeatGate` — a test artifact, not the FSM.

use escriba_buffer::BufferSet;
use escriba_keymap::Key;
use escriba_runtime::EditorState;

const DOC: &str = "alpha bravo charlie\n";

fn state() -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(DOC);
    EditorState::new_with_buffer(bufs, id)
}

fn keys(st: &mut EditorState, ks: &[Key]) {
    for k in ks {
        st.on_key(k);
    }
}

fn chars(st: &mut EditorState, s: &str) {
    for c in s.chars() {
        st.on_key(&Key::Char(c));
    }
}

fn text_of(st: &EditorState) -> String {
    st.buffers
        .get(st.active)
        .map(escriba_buffer::Buffer::to_string)
        .unwrap_or_default()
}

fn report(label: &str, st: &EditorState) {
    println!(
        "{label:<28} text={:?} cursor={:?} mode={:?}",
        text_of(st),
        st.cursor(),
        st.modal.mode()
    );
}

#[test]
fn probes() {
    let mut st = state();
    chars(&mut st, "w");
    report("w", &st);

    let mut st = state();
    chars(&mut st, "dw");
    report("dw", &st);

    let mut st = state();
    chars(&mut st, "d/");
    keys(&mut st, &[Key::Backspace]);
    report("d/<BS>", &st);
    chars(&mut st, "w");
    report("d/<BS>w", &st);

    let mut st = state();
    chars(&mut st, "d/");
    keys(&mut st, &[Key::Backspace, Key::Backspace]);
    report("d/<BS><BS>", &st);
    chars(&mut st, "w");
    report("d/<BS><BS>w", &st);

    let mut st = state();
    chars(&mut st, "d/");
    keys(&mut st, &[Key::Backspace]);
    chars(&mut st, "/charlie");
    keys(&mut st, &[Key::Enter]);
    report("d/<BS>/charlie<CR>", &st);

    let mut st = state();
    chars(&mut st, "/charlie");
    keys(&mut st, &[Key::Enter]);
    report("/charlie<CR>", &st);

    let mut st = state();
    chars(&mut st, "d/charlie");
    keys(&mut st, &[Key::Enter]);
    report("d/charlie<CR>", &st);

    // `d/ab<BS><BS>` — two backspaces: first eats `b`, second eats `a`,
    // prompt still OPEN (caret 0, text empty only after the second? no:
    // after the second the text is empty and the prompt stays open until a
    // THIRD backspace). Then `w`.
    let mut st = state();
    chars(&mut st, "d/ab");
    keys(&mut st, &[Key::Backspace, Key::Backspace]);
    report("d/ab<BS><BS>", &st);
    keys(&mut st, &[Key::Enter]);
    report("d/ab<BS><BS><CR>", &st);
}
