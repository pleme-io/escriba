//! What the operator actually SEES on the status line, asserted against a
//! real rendered frame rather than against the model behind it.
//!
//! The model was correct the whole time this was broken: `/` dispatched
//! `Action::SearchOpen`, the search opened, `status_model()` reported
//! `PromptKind::SearchForward`, and every unit test agreed. The frame still
//! read
//!
//! ```text
//!  : COMMAND  scratch                    [1/1]  /foo 2:1
//! ```
//!
//! — the `:` sigil, the word COMMAND, and the pattern parked at the far
//! right past the match count. Character for character the status line `:`
//! produces. A correct model rendered into a misleading frame is
//! indistinguishable, to the person using the editor, from a broken model,
//! which is why this file asserts on cells.

use escriba_buffer::BufferSet;
use escriba_keymap::Key;
use escriba_runtime::EditorState;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const W: u16 = 60;
const H: u16 = 6;

/// Type `keys` into a fresh editor and return the rendered status line.
fn status_line_after(keys: &[char]) -> String {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("hello world\nfoo bar\n");
    let mut st = EditorState::new_with_buffer(bufs, id);
    for k in keys {
        st.on_key(&Key::Char(*k));
    }
    let mut term = Terminal::new(TestBackend::new(W, H)).expect("test terminal");
    term.draw(|f| escriba_tui::draw_frame(f, &st))
        .expect("draw");
    let buf = term.backend().buffer().clone();
    let mut line = String::new();
    for x in 0..W {
        line.push_str(buf[(x, H - 1)].symbol());
    }
    line
}

#[test]
fn a_forward_search_announces_itself_as_a_search() {
    let line = status_line_after(&['/', 'f', 'o', 'o']);
    assert!(
        line.contains("SEARCH"),
        "a search must say SEARCH, not COMMAND: {line:?}",
    );
    assert!(
        !line.contains("COMMAND"),
        "`/` must not read as `:` command mode: {line:?}",
    );
    assert!(
        line.contains("/foo"),
        "the pattern must be visible: {line:?}"
    );
}

#[test]
fn a_backward_search_is_a_search_too() {
    let line = status_line_after(&['?', 'b', 'a', 'r']);
    assert!(line.contains("SEARCH"), "{line:?}");
    assert!(line.contains("?bar"), "{line:?}");
}

#[test]
fn an_ex_command_still_reads_as_command() {
    // The fix must not swing the other way: `:` is genuinely the command
    // line and has to keep saying so.
    let line = status_line_after(&[':', 'w']);
    assert!(line.contains("COMMAND"), "{line:?}");
    assert!(!line.contains("SEARCH"), "{line:?}");
    assert!(line.contains(":w"), "{line:?}");
}

#[test]
fn the_prompt_sits_on_the_left_where_vim_puts_it() {
    // It used to render past the match count on the right-hand side, which
    // is why a visible prompt still went unseen.
    let line = status_line_after(&['/', 'f', 'o', 'o']);
    let prompt_at = line.find("/foo").expect("prompt on the line");
    assert!(
        prompt_at < (W / 2) as usize,
        "prompt should be in the left half, found at column {prompt_at}: {line:?}",
    );
}

#[test]
fn normal_mode_shows_the_buffer_not_a_prompt() {
    let line = status_line_after(&[]);
    assert!(line.contains("NORMAL"), "{line:?}");
    assert!(
        line.contains("scratch"),
        "the path slot is the path: {line:?}"
    );
}

/// Render after `keys` and return where the terminal cursor ended up.
///
/// `TestBackend` has no "is the cursor hidden" accessor, so the assertions
/// below are phrased against the ROW: a fresh backend's cursor sits at (0, 0)
/// and only a deliberate `set_cursor_position` puts it on the status line.
fn cursor_after(keys: &[Key]) -> (u16, u16) {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("hello world\nfoo bar\n");
    let mut st = EditorState::new_with_buffer(bufs, id);
    for k in keys {
        st.on_key(k);
    }
    let mut term = Terminal::new(TestBackend::new(W, H)).expect("test terminal");
    term.draw(|f| escriba_tui::draw_frame(f, &st)).expect("draw");
    let p = term.get_cursor_position().expect("cursor position");
    (p.x, p.y)
}

#[test]
fn the_caret_is_visible_inside_the_prompt() {
    // `prompt_caret` carried the sentence "a face draws its cursor at
    // sigil_width + prompt_caret" while no face did. Left/Right/Home/<C-w>
    // all moved the caret in the model and NOTHING on screen moved, so
    // correcting the middle of a pattern was guesswork.
    let typed = [
        Key::Char('/'),
        Key::Char('f'),
        Key::Char('o'),
        Key::Char('o'),
    ];
    let (x, y) = cursor_after(&typed);
    assert_eq!(y, H - 1, "the caret belongs on the status line");

    let line = status_line_after(&['/', 'f', 'o', 'o']);
    let slash = u16::try_from(line.find("/foo").expect("prompt on the line")).expect("column");
    assert_eq!(
        x,
        slash + 4,
        "caret sits one past the last character of `/foo`: {line:?}"
    );
}

#[test]
fn the_caret_follows_an_arrow_key() {
    let mut typed = vec![
        Key::Char('/'),
        Key::Char('f'),
        Key::Char('o'),
        Key::Char('o'),
    ];
    let (end_x, _) = cursor_after(&typed);
    typed.push(Key::Left);
    typed.push(Key::Left);
    let (moved_x, moved_y) = cursor_after(&typed);
    assert_eq!(moved_y, H - 1);
    assert_eq!(
        moved_x,
        end_x - 2,
        "two `Left` presses must move the drawn caret two columns"
    );
}

#[test]
fn normal_mode_asks_for_no_status_line_caret() {
    // The prompt caret is a PROMPT affordance. Parking a cursor on the status
    // line while editing the buffer would point at the wrong thing.
    let (_, y) = cursor_after(&[]);
    assert_ne!(y, H - 1, "no prompt, no status-line caret");
}
