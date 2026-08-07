//! The whole shirube path, end to end: produce → seal → navigate → paint.
//!
//! Text markers go first among the producers precisely so this test can exist
//! before the courier does — no language server, no subprocess, no async. If
//! this path is right, a diagnostic is a different SOURCE feeding the same
//! machinery, not a different machinery.

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_keymap::Key;
use escriba_runtime::EditorState;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const W: u16 = 44;
const H: u16 = 10;

fn editor(text: &str) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(text);
    EditorState::new_with_buffer(bufs, id)
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

fn frame(st: &EditorState) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(W, H)).expect("terminal");
    term.draw(|f| escriba_tui::draw_frame(f, st)).expect("draw");
    let buf = term.backend().buffer().clone();
    (0..H)
        .map(|y| (0..W).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect()
}

const SRC: &str = "fn a() {}\n// TODO: wire it\nfn b() {}\n// FIXME: broken\nfn c() {}\n";

#[test]
fn walking_markers_moves_the_cursor_and_says_what_it_found() {
    let mut st = editor(SRC);
    ex(&mut st, "todo.next");
    assert_eq!(st.cursor().line, 1, "landed on the TODO");
    assert!(
        st.messages.last().is_some_and(|m| m.contains("wire it")),
        "and reported what it is: {:?}",
        st.messages,
    );

    ex(&mut st, "todo.next");
    assert_eq!(st.cursor().line, 3, "then the FIXME");
}

#[test]
fn walking_wraps_the_way_search_does() {
    // Same stepper as `n`/`N`, so this is inherited rather than
    // re-implemented — but inherited behaviour still has to be true.
    let mut st = editor(SRC);
    ex(&mut st, "todo.next");
    ex(&mut st, "todo.next");
    ex(&mut st, "todo.next");
    assert_eq!(
        st.cursor().line,
        1,
        "forward past the last wraps to the first"
    );

    ex(&mut st, "todo.prev");
    assert_eq!(st.cursor().line, 3, "and backward wraps the other way");
}

#[test]
fn walking_is_a_jump_so_ctrl_o_comes_back() {
    // `]t` is a far jump. `<C-o>` must return from it exactly as it returns
    // from an `n` — otherwise navigation you can't undo is a trap.
    let mut st = editor(SRC);
    st.on_key(&Key::Char('j'));
    st.on_key(&Key::Char('j'));
    let before = st.cursor().line;
    assert_eq!(before, 2);

    ex(&mut st, "todo.next");
    assert_eq!(st.cursor().line, 3);
    st.on_key(&Key::Ctrl('o'));
    assert_eq!(
        st.cursor().line,
        before,
        "<C-o> returns to where we jumped from"
    );
}

#[test]
fn a_buffer_with_no_markers_declines() {
    let mut st = editor("fn a() {}\nfn b() {}\n");
    ex(&mut st, "todo.next");
    assert_eq!(st.cursor().line, 0, "nothing moved");
    assert!(
        st.messages.iter().any(|m| m.contains("no TODO markers")),
        "{:?}",
        st.messages,
    );
}

#[test]
fn the_gutter_marks_the_lines_that_have_findings() {
    let mut st = editor(SRC);
    ex(&mut st, "todo.next"); // publishes the list
    let f = frame(&st);
    // Line 2 of the buffer (index 1) carries the TODO — an Info mark.
    assert!(
        f[1].contains('\u{2022}'),
        "the TODO line should carry an Info mark: {:?}",
        f[1],
    );
    assert!(
        f[3].contains('\u{25b2}'),
        "the FIXME line should carry a Warning mark: {:?}",
        f[3],
    );
    assert!(
        !f[0].contains('\u{2022}') && !f[0].contains('\u{25b2}'),
        "a line with no finding carries no mark: {:?}",
        f[0],
    );
}

#[test]
fn the_gutter_is_one_column_wide_whether_or_not_a_line_has_a_finding() {
    // A gutter that changes width as findings arrive makes the whole file
    // jump sideways, which is worse than a coarse glyph.
    let mut st = editor(SRC);
    let before: Vec<usize> = frame(&st)
        .iter()
        .take(5)
        // CHAR index, not `find`'s byte index: the marks are multibyte, so a
        // byte offset shifts on exactly the lines this test is about.
        .map(|l| l.chars().position(|c| c == '│').unwrap_or(0))
        .collect();
    ex(&mut st, "todo.next");
    let after: Vec<usize> = frame(&st)
        .iter()
        .take(5)
        // CHAR index, not `find`'s byte index: the marks are multibyte, so a
        // byte offset shifts on exactly the lines this test is about.
        .map(|l| l.chars().position(|c| c == '│').unwrap_or(0))
        .collect();
    assert_eq!(
        before, after,
        "the text column must not move when findings arrive",
    );
}

#[test]
fn editing_the_buffer_makes_the_marks_disappear_rather_than_lie() {
    // The freshness seal, seen from the screen. An edit moves the buffer
    // revision, the list's anchor no longer holds, and the gutter goes empty
    // — which is the honest answer, because those line numbers may now point
    // anywhere.
    let mut st = editor(SRC);
    ex(&mut st, "todo.next");
    assert!(frame(&st)[1].contains('\u{2022}'), "marked before the edit");

    st.on_key(&Key::Char('i'));
    st.on_key(&Key::Char('x'));
    st.on_key(&Key::Esc);

    let f = frame(&st);
    assert!(
        !f.iter()
            .any(|l| l.contains('\u{2022}') || l.contains('\u{25b2}')),
        "a stale list must paint NOTHING rather than marks in the wrong \
         places: {f:?}",
    );
}

#[test]
fn walking_a_stale_list_refuses_instead_of_jumping_somewhere_wrong() {
    let mut st = editor(SRC);
    ex(&mut st, "todo.next");
    // Publish, then edit so the list goes stale, then walk WITHOUT re-scanning.
    st.on_key(&Key::Char('i'));
    st.on_key(&Key::Char('x'));
    st.on_key(&Key::Esc);
    let at = st.cursor().line;
    st.interpret(escriba_madoguchi::Outcome::did(vec![
        escriba_madoguchi::Negai::WalkList {
            list: "todo".to_string(),
            forward: true,
        },
    ]));
    assert_eq!(st.cursor().line, at, "the cursor did not move");
    assert!(
        st.messages.iter().any(|m| m.contains("out of date")),
        "and it said why: {:?}",
        st.messages,
    );
}
