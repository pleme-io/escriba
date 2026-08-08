//! `:sp` and `:vsp`, asserted from rendered CELLS.
//!
//! These drive the NATIVE command names. The vim spellings (`:sp`, `:vsp`,
//! `:close`) are rc aliases in `configs/blnvim-defaults.lisp`, which these
//! tests do not load — `escriba/tests/alias_revival.rs` covers that they
//! resolve. Driving the alias here would test the rc, not the split.
//!
//! A split is the one feature where a model test proves almost nothing: the
//! layout can be perfect while the screen shows one pane, or two panes of the
//! same file, or a separator in the wrong place.

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_keymap::Key;
use escriba_runtime::EditorState;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const W: u16 = 60;
const H: u16 = 20;

fn editor() -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(&"AAAA\n".repeat(40));
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.dismiss_splash();
    escriba_tui::render::sync_viewport(&mut st, W, H);
    st
}

fn frame(st: &EditorState) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(W, H)).expect("term");
    term.draw(|f| escriba_tui::draw_frame(f, st)).expect("draw");
    let buf = term.backend().buffer().clone();
    (0..H)
        .map(|y| (0..W).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect()
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
fn vsp_draws_a_vertical_rule_and_two_panes() {
    let mut st = editor();
    assert_eq!(st.layout.count(), 1);
    ex(&mut st, "window.vsplit");
    assert_eq!(st.layout.count(), 2, "the layout has two windows");

    let f = frame(&st);
    let rules = f.iter().filter(|l| l.contains('\u{2503}')).count();
    assert!(
        rules > 3,
        "a vertical rule must run down the screen:\n{}",
        f.join("\n")
    );
    // Two gutters on one row means two panes side by side.
    let row = f
        .iter()
        .find(|l| l.contains("AAAA"))
        .expect("text is painted");
    assert!(
        row.matches("AAAA").count() >= 2,
        "both panes must show the buffer: {row:?}",
    );
}

#[test]
fn sp_draws_a_horizontal_rule() {
    let mut st = editor();
    ex(&mut st, "window.split");
    let f = frame(&st);
    assert!(
        f.iter().any(|l| l.contains('\u{2501}')),
        "a horizontal rule must separate the panes:\n{}",
        f.join("\n"),
    );
}

#[test]
fn the_cursor_appears_in_exactly_one_pane() {
    // Two panes on ONE buffer. An unfocused pane painting a cursor would
    // claim a focus it does not have, and both panes would look active.
    let mut st = editor();
    ex(&mut st, "window.vsplit");
    let mut term = Terminal::new(TestBackend::new(W, H)).expect("term");
    term.draw(|f| escriba_tui::draw_frame(f, &st))
        .expect("draw");
    let buf = term.backend().buffer().clone();
    // The block cursor inverts its cell; count reversed cells in the panes.
    let inverted = (0..H - 1)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|(x, y)| {
            buf[(*x, *y)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        })
        .count();
    assert!(inverted <= 1, "exactly one cursor cell, got {inverted}");
}

#[test]
fn ctrl_w_navigates_between_panes() {
    let mut st = editor();
    // `<C-w>l` is bound in the shipped rc, which this fixture does not load.
    // Apply just those bindings, so the test exercises the KEY PATH rather
    // than only the command.
    let plan = escriba_lisp::apply_source(
        r#"
        (defkeybind :mode "normal" :key "<C-w>h" :action "pane.left")
        (defkeybind :mode "normal" :key "<C-w>l" :action "pane.right")
        "#,
    )
    .expect("bindings parse");
    escriba_lisp::apply_plan_to_keymap(&plan, &mut st.keymap);
    ex(&mut st, "window.vsplit");
    let after_split = st.layout.active();
    // <C-w>l — the new window went LEFT (vim's splitright=off), so the
    // original is to the right.
    st.on_key(&Key::Ctrl('w'));
    st.on_key(&Key::Char('l'));
    assert_ne!(st.layout.active(), after_split, "<C-w>l must move focus");
    st.on_key(&Key::Ctrl('w'));
    st.on_key(&Key::Char('h'));
    assert_eq!(st.layout.active(), after_split, "<C-w>h must come back");
}

#[test]
fn closing_returns_to_one_pane_and_the_rule_disappears() {
    let mut st = editor();
    ex(&mut st, "window.vsplit");
    assert_eq!(st.layout.count(), 2);
    ex(&mut st, "window.close");
    assert_eq!(st.layout.count(), 1);
    let f = frame(&st);
    assert!(
        !f.iter().any(|l| l.contains('\u{2503}')),
        "no PANE separator with one pane — the light `\u{2502}` in the gutter \
         is a different glyph and must not be confused for one:\n{}",
        f.join("\n"),
    );
}

#[test]
fn the_last_window_refuses_to_close() {
    // vim's E444. Closing the last window means "quit", which is a different
    // verb the operator did not type.
    let mut st = editor();
    ex(&mut st, "window.close");
    assert_eq!(st.layout.count(), 1, "the last window survives");
    assert!(
        st.messages.iter().any(|m| m.contains("E444")),
        "and it says why: {:?}",
        st.messages,
    );
}

#[test]
fn three_splits_all_render_without_overlap() {
    let mut st = editor();
    ex(&mut st, "window.vsplit");
    ex(&mut st, "window.vsplit");
    assert_eq!(st.layout.count(), 3);
    let f = frame(&st);
    let row = f.iter().find(|l| l.contains("AAAA")).expect("text");
    assert!(
        row.matches('\u{2503}').count() >= 2,
        "three panes need two rules: {row:?}",
    );
}

#[test]
fn a_tiny_terminal_with_splits_does_not_panic() {
    let mut st = editor();
    ex(&mut st, "window.vsplit");
    ex(&mut st, "window.split");
    for (w, h) in [(8u16, 4u16), (4, 3), (20, 6)] {
        escriba_tui::render::sync_viewport(&mut st, w, h);
        let mut term = Terminal::new(TestBackend::new(w, h)).expect("term");
        term.draw(|f| escriba_tui::draw_frame(f, &st))
            .unwrap_or_else(|e| panic!("{w}x{h} must render: {e}"));
    }
}
