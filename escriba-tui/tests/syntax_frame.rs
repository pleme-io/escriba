//! The ratatui face paints SYNTAX — asserted from rendered cell colours.
//!
//! It had none. Not "a simpler version" — none at all: `build_ecosystem`
//! lived in `escriba-render/src/gpu.rs`, so the only face that could
//! highlight was the one that needed a GPU. Anyone editing over SSH, inside
//! tmux, or in CI saw plain text.

use escriba_buffer::BufferSet;
use escriba_runtime::EditorState;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const W: u16 = 60;
const H: u16 = 12;

fn editor(text: &str, path: &str) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(text);
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.dismiss_splash();
    if let Some(b) = st.buffers.get_mut(id) {
        b.path = Some(std::path::PathBuf::from(path));
    }
    escriba_tui::render::sync_viewport(&mut st, W, H);
    st
}

/// Distinct foreground colours on the first row, excluding the gutter.
fn row_colours(st: &EditorState, row: u16) -> Vec<ratatui::style::Color> {
    let mut term = Terminal::new(TestBackend::new(W, H)).expect("term");
    term.draw(|f| escriba_tui::draw_frame(f, st)).expect("draw");
    let buf = term.backend().buffer().clone();
    let gutter = escriba_ui::gutter::gutter_width(3) as u16;
    (gutter..W)
        .filter(|x| buf[(*x, row)].symbol() != " ")
        .map(|x| buf[(x, row)].fg)
        .collect()
}

#[test]
fn code_is_painted_in_more_than_one_colour() {
    let st = editor("fn main() { let x = \"hi\"; }\n", "probe.rs");
    let mut seen: Vec<ratatui::style::Color> = row_colours(&st, 0);
    seen.sort_by_key(|c| format!("{c:?}"));
    seen.dedup();
    assert!(
        seen.len() > 1,
        "a line of Rust must not paint in ONE colour — that is the \
         no-highlighting state: {seen:?}",
    );
}

#[test]
fn a_keyword_and_a_string_paint_differently() {
    // The specific distinction a reader depends on most.
    let st = editor("fn f() { \"str\" }\n", "probe.rs");
    let mut term = Terminal::new(TestBackend::new(W, H)).expect("term");
    term.draw(|f| escriba_tui::draw_frame(f, &st))
        .expect("draw");
    let buf = term.backend().buffer().clone();
    let row: String = (0..W).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    let fg_at = |needle: char| {
        row.chars()
            .position(|c| c == needle)
            .and_then(|i| u16::try_from(i).ok())
            .map(|x| buf[(x, 0)].fg)
    };
    let kw = fg_at('f').expect("the `fn` keyword is on screen");
    let st_ = fg_at('s').expect("the string body is on screen");
    assert_ne!(kw, st_, "keyword and string must differ: {row:?}");
}

#[test]
fn an_unknown_extension_still_renders() {
    // No grammar is not an error — the table backend covers everything, and
    // a file escriba cannot classify must still be editable.
    let st = editor("some plain text\n", "notes.zzz");
    let cols = row_colours(&st, 0);
    assert!(!cols.is_empty(), "the line must still be painted");
}
