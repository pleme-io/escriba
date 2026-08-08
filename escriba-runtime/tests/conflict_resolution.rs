//! Merge conflicts, end to end — and the claim that they need no git.
//!
//! The whole `git.*` cluster was tiered as "needs a git layer". Half of it
//! does not: a conflict is text a merge tool wrote INTO the buffer, and
//! resolving one is an edit over lines already open.

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_keymap::Key;
use escriba_runtime::EditorState;

const CONFLICTED: &str = "\
fn a() {}
<<<<<<< HEAD
let x = 1;
=======
let x = 2;
>>>>>>> branch
fn b() {}
";

fn editor() -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(CONFLICTED);
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.dismiss_splash();
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

fn text(st: &EditorState) -> String {
    st.buffers
        .get(st.active)
        .map(|b| b.to_string())
        .unwrap_or_default()
}

#[test]
fn walking_lands_on_the_conflict() {
    let mut st = editor();
    ex(&mut st, "conflict.next");
    assert_eq!(st.cursor().line, 1, "the <<<<<<< line");
}

#[test]
fn choosing_ours_removes_the_markers_and_keeps_the_first_half() {
    let mut st = editor();
    for _ in 0..3 {
        st.on_key(&Key::Char('j'));
    }
    ex(&mut st, "conflict.choose-ours");
    let t = text(&st);
    assert!(t.contains("let x = 1;"), "ours survives: {t:?}");
    assert!(!t.contains("let x = 2;"), "theirs is gone: {t:?}");
    assert!(
        !t.contains("<<<<<<<") && !t.contains("=======") && !t.contains(">>>>>>>"),
        "no markers left: {t:?}"
    );
    assert!(
        t.contains("fn a() {}") && t.contains("fn b() {}"),
        "context intact: {t:?}"
    );
}

#[test]
fn choosing_theirs_keeps_the_second_half() {
    let mut st = editor();
    for _ in 0..3 {
        st.on_key(&Key::Char('j'));
    }
    ex(&mut st, "conflict.choose-theirs");
    let t = text(&st);
    assert!(
        t.contains("let x = 2;") && !t.contains("let x = 1;"),
        "{t:?}"
    );
}

#[test]
fn choosing_both_keeps_ours_then_theirs() {
    let mut st = editor();
    for _ in 0..3 {
        st.on_key(&Key::Char('j'));
    }
    ex(&mut st, "conflict.choose-both");
    let t = text(&st);
    let ours = t.find("let x = 1;").expect("ours");
    let theirs = t.find("let x = 2;").expect("theirs");
    assert!(
        ours < theirs,
        "ours comes first, as it does in the file: {t:?}"
    );
    assert!(!t.contains("======="), "{t:?}");
}

#[test]
fn resolving_outside_a_conflict_declines_rather_than_editing() {
    // Standing outside a conflict is an ordinary place to be. An edit here
    // would corrupt whatever line the cursor happened to be on.
    let mut st = editor();
    let before = text(&st);
    ex(&mut st, "conflict.choose-ours");
    assert_eq!(text(&st), before, "the buffer is untouched");
    assert!(
        st.messages.iter().any(|m| m.contains("not inside")),
        "and it says why: {:?}",
        st.messages,
    );
}

#[test]
fn a_clean_buffer_reports_no_conflicts() {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("fn a() {}\n");
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.dismiss_splash();
    ex(&mut st, "conflict.next");
    assert!(
        st.messages.iter().any(|m| m.contains("no merge conflicts")),
        "{:?}",
        st.messages,
    );
}

#[test]
fn resolving_is_undoable() {
    // It is one edit, so one `u` restores the conflict. A resolution that
    // took three undos would be three edits pretending to be one.
    let mut st = editor();
    let before = text(&st);
    for _ in 0..3 {
        st.on_key(&Key::Char('j'));
    }
    ex(&mut st, "conflict.choose-ours");
    assert_ne!(text(&st), before);
    st.on_key(&Key::Char('u'));
    assert_eq!(text(&st), before, "one undo restores the conflict");
}
