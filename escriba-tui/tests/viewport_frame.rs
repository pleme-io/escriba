//! The viewport MODEL must match what the face can actually paint.
//!
//! The ratatui face never wrote `viewport.visible_lines`. The reasoning, in a
//! comment in the run loop, was that "ratatui auto-picks up the new size on
//! the next draw" — true of PAINTING and false of the model. So
//! `scroll_to_contain` kept using the constructor default of 40, and in any
//! terminal shorter than that the editor believed the cursor was visible
//! while it had scrolled off the screen.
//!
//! Asserted from RENDERED CELLS. A unit test on the viewport would have
//! agreed with the editor and been just as wrong.

use escriba_buffer::BufferSet;
use escriba_keymap::Key;
use escriba_runtime::EditorState;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn the_cursor_stays_on_screen_in_a_short_terminal() {
    // 20 rows: the TUI paints 18 buffer lines, but the viewport still says 40.
    const W: u16 = 40;
    const H: u16 = 20;
    let mut bufs = BufferSet::new();
    let text: String = (0..60).map(|i| format!("line {i}\n")).collect();
    let id = bufs.scratch(&text);
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.dismiss_splash();

    for _ in 0..30 {
        st.on_key(&Key::Char('j'));
    }
    assert_eq!(st.cursor().line, 30, "precondition");

    // The run loop does this before every draw; a test that skipped it
    // would be measuring a configuration the editor never runs in.
    escriba_tui::render::sync_viewport(&mut st, W, H);

    let mut term = Terminal::new(TestBackend::new(W, H)).expect("term");
    term.draw(|f| escriba_tui::draw_frame(f, &st))
        .expect("draw");
    let buf = term.backend().buffer().clone();
    let frame: Vec<String> = (0..H)
        .map(|y| (0..W).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect();

    assert!(
        frame.iter().any(|l| l.contains("line 30")),
        "the cursor line must be painted; viewport says visible_lines={} \
         while the terminal shows ~{} rows:\n{}",
        st.layout
            .active_window()
            .map_or(0, |w| w.viewport.visible_lines),
        H - 2,
        frame.join("\n"),
    );
}

#[test]
fn shrinking_the_terminal_pulls_the_cursor_back_into_view() {
    // A resize moves the WINDOW, not the cursor, so nothing re-runs
    // scroll-to-contain on its own. Without `refollow_cursor` the cursor sits
    // off-screen after a shrink until the operator happens to move it.
    let mut bufs = BufferSet::new();
    let text: String = (0..60).map(|i| format!("line {i}\n")).collect();
    let id = bufs.scratch(&text);
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.dismiss_splash();

    escriba_tui::render::sync_viewport(&mut st, 40, 50); // roomy
    for _ in 0..30 {
        st.on_key(&Key::Char('j'));
    }
    escriba_tui::render::sync_viewport(&mut st, 40, 12); // now cramped

    let mut term = Terminal::new(TestBackend::new(40, 12)).expect("term");
    term.draw(|f| escriba_tui::draw_frame(f, &st))
        .expect("draw");
    let buf = term.backend().buffer().clone();
    let frame: Vec<String> = (0..12u16)
        .map(|y| {
            (0..40u16)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect()
        })
        .collect();
    assert!(
        frame.iter().any(|l| l.contains("line 30")),
        "after shrinking, the cursor line must be pulled back into view:\n{}",
        frame.join("\n"),
    );
}

#[test]
fn the_text_column_is_not_clipped_by_a_double_gutter_subtraction() {
    // `sync_viewport` stores the FULL width; `draw_buffer` subtracts the
    // gutter. Subtracting in both places clipped every line short by a
    // gutter's worth of text — caught while writing this file.
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("abcdefghijklmnopqrstuvwxyz0123456789\n");
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.dismiss_splash();
    escriba_tui::render::sync_viewport(&mut st, 40, 12);

    let mut term = Terminal::new(TestBackend::new(40, 12)).expect("term");
    term.draw(|f| escriba_tui::draw_frame(f, &st))
        .expect("draw");
    let buf = term.backend().buffer().clone();
    let row: String = (0..40u16)
        .map(|x| buf[(x, 0)].symbol().to_string())
        .collect();
    let gutter = escriba_ui::gutter::gutter_width(1);
    let want = 40 - gutter;
    // The TEXT portion, past the gutter — not the whole row, which is of
    // course the full terminal width. Measuring the row was this test's own
    // first bug.
    let got = row
        .chars()
        .skip(gutter)
        .collect::<String>()
        .trim_end()
        .chars()
        .count();
    assert_eq!(
        got, want,
        "a {gutter}-column gutter in a 40-column terminal leaves {want} for \
         text, got {got}: {row:?}",
    );
}
