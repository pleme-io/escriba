//! The picker, asserted from rendered CELLS — it must OCCLUDE.
//!
//! The start screen replaces its pane rather than floating, so it could never
//! prove occlusion. This is escriba's first real overlay.

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_keymap::Key;
use escriba_runtime::EditorState;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const W: u16 = 60;
const H: u16 = 20;

fn frame(st: &EditorState) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(W, H)).expect("term");
    term.draw(|f| escriba_tui::draw_frame(f, st)).expect("draw");
    let buf = term.backend().buffer().clone();
    (0..H)
        .map(|y| (0..W).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect()
}

fn editor() -> EditorState {
    let mut bufs = BufferSet::new();
    let a = bufs.scratch("UNIQUEBUFFERTEXT\n".repeat(30).as_str());
    bufs.scratch("second\n");
    let mut st = EditorState::new_with_buffer(bufs, a);
    st.dismiss_splash();
    escriba_tui::render::sync_viewport(&mut st, W, H);
    st
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

#[test]
fn an_open_picker_occludes_the_text_behind_it() {
    let mut st = editor();
    let before = frame(&st);
    assert!(
        before
            .iter()
            .filter(|l| l.contains("UNIQUEBUFFERTEXT"))
            .count()
            > 5,
        "precondition: the buffer fills the pane",
    );

    ex(&mut st, "picker.buffers");
    let after = frame(&st);
    let still = after
        .iter()
        .filter(|l| l.contains("UNIQUEBUFFERTEXT"))
        .count();
    let was = before
        .iter()
        .filter(|l| l.contains("UNIQUEBUFFERTEXT"))
        .count();
    assert!(
        still < was,
        "the panel must COVER what is behind it — {was} rows before, {still} \
         after:\n{}",
        after.join("\n"),
    );
}

#[test]
fn the_panel_names_its_source_and_shows_a_selection() {
    let mut st = editor();
    ex(&mut st, "picker.buffers");
    let f = frame(&st);
    assert!(
        f.iter().any(|l| l.contains("Buffers")),
        "the operator must know WHAT they are picking from:\n{}",
        f.join("\n"),
    );
    assert!(
        f.iter().any(|l| l.contains('>')),
        "a highlighted row must be visible:\n{}",
        f.join("\n"),
    );
}

#[test]
fn a_tiny_terminal_does_not_panic_or_draw_outside_its_pane() {
    // A surface that can be drawn outside its container is a panic waiting
    // for a small terminal.
    let mut bufs = BufferSet::new();
    let a = bufs.scratch("x\n");
    bufs.scratch("y\n");
    let mut st = EditorState::new_with_buffer(bufs, a);
    st.dismiss_splash();
    for (w, h) in [(10u16, 4u16), (6, 3), (20, 5)] {
        escriba_tui::render::sync_viewport(&mut st, w, h);
        ex(&mut st, "picker.buffers");
        let mut term = Terminal::new(TestBackend::new(w, h)).expect("term");
        term.draw(|f| escriba_tui::draw_frame(f, &st))
            .unwrap_or_else(|e| panic!("{w}x{h} must render, got {e}"));
        st.on_key(&Key::Esc);
    }
}
