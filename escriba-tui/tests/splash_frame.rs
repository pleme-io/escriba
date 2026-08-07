//! The start screen as the ratatui face actually paints it.
//!
//! Asserting on cells rather than on the model, for the same reason
//! `status_line_frame.rs` does: a correct model rendered into the wrong
//! cells is, to the person using the editor, a broken screen.

use escriba_buffer::BufferSet;
use escriba_core::{Action, Mode};
use escriba_keymap::Key;
use escriba_runtime::EditorState;
use escriba_ui::splash::{Splash, SplashEntry};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn splash() -> Splash {
    Splash {
        art: vec!["ESCRIBA".into()],
        tagline: "a modal editor".into(),
        entries: vec![
            SplashEntry {
                key: 'e',
                label: "start editing".into(),
                action: Action::ChangeMode(Mode::Normal),
            },
            SplashEntry {
                key: 'q',
                label: "quit".into(),
                action: Action::Quit,
            },
        ],
        facts: vec!["v0.1.34".into(), "nord".into()],
    }
}

fn frame(st: &EditorState, w: u16, h: u16) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
    term.draw(|f| escriba_tui::draw_frame(f, st)).expect("draw");
    let buf = term.backend().buffer().clone();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

fn showing() -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("SCRATCH-BUFFER-TEXT\n");
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.set_splash(splash());
    st
}

#[test]
fn the_screen_is_painted_whole() {
    let lines = frame(&showing(), 60, 20).join("\n");
    for expected in ["ESCRIBA", "a modal editor", "start editing", "quit", "nord"] {
        assert!(lines.contains(expected), "missing {expected:?}:\n{lines}");
    }
}

#[test]
fn the_screen_replaces_the_buffer_rather_than_overlaying_it() {
    let lines = frame(&showing(), 60, 20).join("\n");
    assert!(
        !lines.contains("SCRATCH-BUFFER-TEXT"),
        "the buffer must not show through the start screen:\n{lines}",
    );
    assert!(
        !lines.contains(" 1 │"),
        "no line-number gutter behind the screen:\n{lines}",
    );
}

#[test]
fn the_status_line_survives_the_screen() {
    // The screen takes the buffer pane, never the status line — an
    // operator must always be able to see what mode they are in.
    let lines = frame(&showing(), 60, 20);
    let status = lines.last().expect("a status line");
    assert!(status.contains("NORMAL"), "{status:?}");
}

#[test]
fn dismissing_the_screen_reveals_the_buffer() {
    let mut st = showing();
    st.on_key(&Key::Char('j'));
    let lines = frame(&st, 60, 20).join("\n");
    assert!(lines.contains("SCRATCH-BUFFER-TEXT"), "{lines}");
    assert!(
        !lines.contains("ESCRIBA"),
        "the screen must be gone:\n{lines}"
    );
}

#[test]
fn a_tiny_terminal_paints_no_screen_rather_than_a_broken_one() {
    // 3 rows leaves 2 for the pane — not enough for even the compact
    // wordmark plus a menu. The face must fall back, not garble.
    let lines = frame(&showing(), 20, 3);
    assert_eq!(lines.len(), 3);
    // Whatever it drew, nothing may spill past the width.
    assert!(lines.iter().all(|l| l.chars().count() == 20), "{lines:?}");
}

#[test]
fn the_screen_is_centred_not_flush_left() {
    let lines = frame(&showing(), 60, 20);
    let art = lines
        .iter()
        .find(|l| l.contains("ESCRIBA"))
        .expect("the wordmark");
    let indent = art.len() - art.trim_start().len();
    assert!(
        indent > 4,
        "wordmark should be centred, indent was {indent}"
    );
}
