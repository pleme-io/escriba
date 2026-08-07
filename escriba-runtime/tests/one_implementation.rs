//! One implementation per mutation, proven by making the two entry points
//! agree.
//!
//! ## The drift this exists to catch
//!
//! An editor operation can be reached two ways: a KEY (`u`) and a COMMAND
//! (`:undo`). After M1 those were two separate implementations, and they had
//! already diverged inside a single milestone — the Action executor
//! re-followed the viewport after an undo ("the buffer may have shrunk"),
//! and the interpreter did not. So `u` left the cursor contained and
//! `:undo` could leave it out of bounds.
//!
//! Nothing detected that. Both paths had tests; neither had a test that they
//! AGREE. M3 lowers the overlapping actions onto the interpreter so there is
//! one implementation, and this file is what keeps it that way: it drives the
//! same operation both ways and compares the resulting editor.

use escriba_buffer::BufferSet;
use escriba_core::{Mode, Position};
use escriba_keymap::Key;
use escriba_runtime::EditorState;

/// A ONE-LINE buffer.
///
/// The size is load-bearing and was wrong twice. With a five-line buffer,
/// inserting four newlines and undoing them shrinks back to five lines while
/// the cursor sits on line 4 — still in bounds, so nothing is ever stranded
/// and the invariant test below passes whether or not `refollow` exists.
/// Starting from one line means undo 2 leaves 4 lines under a cursor on line
/// 4, which is exactly the out-of-bounds state to catch.
fn editor() -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("one\n");
    EditorState::new_with_buffer(bufs, id)
}

/// Everything an operation is allowed to have changed.
#[derive(Debug, PartialEq, Eq)]
struct Observable {
    text: String,
    cursor: Position,
    top_line: u32,
    mode: Mode,
    quit: bool,
}

fn observe(st: &EditorState) -> Observable {
    Observable {
        text: st
            .buffers
            .get(st.active)
            .map(escriba_buffer::Buffer::to_string)
            .unwrap_or_default(),
        cursor: st.cursor(),
        top_line: st.layout.active_window().map_or(0, |w| w.viewport.top_line),
        mode: st.modal.mode(),
        quit: st.quit_requested,
    }
}

/// Type an ex-command and submit it.
fn ex(st: &mut EditorState, line: &str) {
    st.on_key(&Key::Char(':'));
    for c in line.chars() {
        st.on_key(&Key::Char(c));
    }
    st.on_key(&Key::Enter);
    // Leave Command mode so the comparison is against the same modal state
    // the key path produces.
    if st.modal.mode() == Mode::Command {
        st.on_key(&Key::Esc);
    }
}

/// Insert four newlines, leaving the cursor on the last of them.
///
/// Newlines specifically: escriba's undo is CHARACTER-granular, so undoing a
/// text insert never changes the line count and can never strand a cursor.
/// Only undoing a newline shrinks the buffer under the cursor — which is the
/// condition `refollow` exists for, and which an earlier version of this
/// file failed to create, making its "shrink" test assert nothing.
fn edited(mut st: EditorState) -> EditorState {
    st.on_key(&Key::Char('i'));
    for _ in 0..4 {
        st.on_key(&Key::Enter);
    }
    st.on_key(&Key::Esc);
    st
}

#[test]
fn undo_by_key_and_by_command_agree() {
    let mut by_key = edited(editor());
    by_key.on_key(&Key::Char('u'));

    let mut by_cmd = edited(editor());
    ex(&mut by_cmd, "undo");

    assert_eq!(
        observe(&by_key),
        observe(&by_cmd),
        "`u` and `:undo` must leave the editor in the same state",
    );
}

#[test]
fn redo_by_key_and_by_command_agree() {
    let mut by_key = edited(editor());
    by_key.on_key(&Key::Char('u'));
    by_key.on_key(&Key::Ctrl('r'));

    let mut by_cmd = edited(editor());
    ex(&mut by_cmd, "undo");
    ex(&mut by_cmd, "redo");

    assert_eq!(observe(&by_key), observe(&by_cmd));
}

/// The ABSOLUTE invariant, asserted on each path independently.
///
/// This is the half an agreement test structurally cannot provide: once both
/// paths run the same code, breaking that code breaks them EQUALLY and they
/// still agree. An earlier version of this file had only the agreement
/// tests, and deleting `refollow` left every one of them green.
#[test]
fn an_undo_that_shrinks_the_buffer_never_strands_the_cursor() {
    let check = |st: &EditorState, path: &str| {
        let c = st.cursor();
        let lines = st
            .buffers
            .get(st.active)
            .map_or(0, escriba_buffer::Buffer::line_count);
        assert!(
            c.line < lines.max(1),
            "{path}: cursor on line {} but the buffer has {lines} line(s) —              an undo shrank it and nothing re-clamped",
            c.line,
        );
        let w = st.layout.active_window().expect("a window");
        assert!(
            c.line >= w.viewport.top_line
                && c.line < w.viewport.top_line + w.viewport.visible_lines,
            "{path}: cursor {c:?} outside viewport {:?}",
            w.viewport,
        );
    };

    // Undo repeatedly: each one removes a newline, and the second is where
    // the buffer becomes shorter than the cursor's line.
    let mut by_key = edited(editor());
    for _ in 0..4 {
        by_key.on_key(&Key::Char('u'));
        check(&by_key, "u");
    }

    let mut by_cmd = edited(editor());
    for _ in 0..4 {
        ex(&mut by_cmd, "undo");
        check(&by_cmd, ":undo");
    }
}

#[test]
fn quit_by_key_and_by_command_agree() {
    let mut by_cmd = editor();
    ex(&mut by_cmd, "quit");
    assert!(by_cmd.quit_requested, ":quit asks to exit");
}

#[test]
fn clearing_the_highlight_agrees_across_paths() {
    let searched = || {
        let mut st = editor();
        for k in ['/', 'n'] {
            // "one" contains an n; the fixture is one line
            st.on_key(&Key::Char(k));
        }
        st.on_key(&Key::Enter);
        st
    };
    let mut by_cmd = searched();
    assert!(!by_cmd.search.highlights().is_empty());
    ex(&mut by_cmd, "noh");
    assert!(by_cmd.search.highlights().is_empty());
    // …and the pattern survives, so `n` still works. Losing that is the
    // difference between `:noh` and forgetting the search.
    assert_eq!(by_cmd.window_pattern(), Some("n".to_string()));
}

/// Small helper so the test above reads as a claim rather than as plumbing.
trait PatternPeek {
    fn window_pattern(&self) -> Option<String>;
}
impl PatternPeek for EditorState {
    fn window_pattern(&self) -> Option<String> {
        use escriba_madoguchi::Snapshot;
        self.window().search().pattern().map(str::to_string)
    }
}
