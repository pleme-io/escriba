//! `comment.toggle-{line,block}` — the first consumer of `:commentstring`.
//!
//! `(defmode :commentstring "// %s")` has parsed and validated since Wave 1
//! and reached nothing. These drive it end to end: a `defmode` in the plan,
//! through the filetype table, out to an edit in the buffer.
//!
//! The property that matters most is the ROUND TRIP. Toggling twice must
//! return the original text exactly — an editor whose comment verb is not an
//! involution accumulates markers (`//// x`) or eats indentation, and both
//! are the kind of damage you notice in a diff long after you caused it.

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_keymap::Key;
use escriba_runtime::EditorState;

/// An editor holding `text` in a file named `path`, with major modes applied.
///
/// Each call gets its OWN directory. The first version shared one, and since
/// several tests use `a.rs` and cargo runs tests in PARALLEL, they read each
/// other's fixtures — two tests failed with a third's content. A shared
/// mutable path is a race whatever the language.
fn editor_with(path: &str, text: &str) -> EditorState {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir()
        .join("escriba-comment-toggle")
        .join(N.fetch_add(1, Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let file = dir.join(path);
    std::fs::write(&file, text).expect("fixture");

    let mut bufs = BufferSet::new();
    let id = bufs.open(&file).expect("open");
    let mut st = EditorState::new_with_buffer(bufs, id);

    let plan = escriba_lisp::apply_source(
        r#"
        (defmode :name "rust" :extensions ("rs") :commentstring "// %s")
        (defmode :name "lisp" :extensions ("lisp") :commentstring ";; %s")
        (defmode :name "html" :extensions ("html") :commentstring "<!-- %s -->")
        (defmode :name "plain" :extensions ("txt"))
        "#,
    )
    .expect("modes parse");
    escriba_lisp::apply_plan_to_filetypes(&plan, &mut st.filetypes);
    escriba_lisp::apply_plan_to_commands(&plan, &mut st.commands);
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
        .map(escriba_buffer::Buffer::to_string)
        .unwrap_or_default()
}

#[test]
fn commenting_a_line_uses_the_filetypes_own_syntax() {
    let mut st = editor_with("a.rs", "let x = 1;\n");
    ex(&mut st, "comment.toggle-line");
    assert_eq!(text(&st), "// let x = 1;\n");
}

#[test]
fn toggling_twice_returns_the_original_exactly() {
    // The involution. Anything less accumulates markers or eats whitespace.
    for (file, body) in [
        ("a.rs", "let x = 1;\n"),
        ("a.lisp", "(defun f ())\n"),
        ("a.html", "<p>hi</p>\n"),
    ] {
        let mut st = editor_with(file, body);
        let before = text(&st);
        ex(&mut st, "comment.toggle-line");
        assert_ne!(text(&st), before, "{file}: first toggle must comment");
        ex(&mut st, "comment.toggle-line");
        assert_eq!(text(&st), before, "{file}: second toggle must restore");
    }
}

#[test]
fn indentation_survives_the_round_trip() {
    // A marker inserted before the indent destroys the alignment the code
    // depends on, and the damage shows up in a diff long after the fact.
    let mut st = editor_with("a.rs", "        deeply.indented();\n");
    ex(&mut st, "comment.toggle-line");
    assert_eq!(
        text(&st),
        "        // deeply.indented();\n",
        "the marker goes after the indent, not before it",
    );
    ex(&mut st, "comment.toggle-line");
    assert_eq!(text(&st), "        deeply.indented();\n");
}

#[test]
fn a_block_syntax_wraps_on_both_sides() {
    let mut st = editor_with("a.html", "<p>hi</p>\n");
    ex(&mut st, "comment.toggle-line");
    assert_eq!(text(&st), "<!-- <p>hi</p> -->\n");
    ex(&mut st, "comment.toggle-line");
    assert_eq!(text(&st), "<p>hi</p>\n");
}

#[test]
fn a_filetype_with_no_comment_syntax_declines() {
    // `plain` declares no commentstring, so there is no honest way to
    // comment it. Declining says so; guessing `#` would be an invention.
    let mut st = editor_with("a.txt", "words\n");
    ex(&mut st, "comment.toggle-line");
    assert_eq!(text(&st), "words\n", "the buffer is untouched");
    assert!(
        st.messages.iter().any(|m| m.contains("no comment syntax")),
        "and it must say why: {:?}",
        st.messages,
    );
}

#[test]
fn an_unknown_filetype_declines() {
    let mut st = editor_with("a.zzz", "stuff\n");
    ex(&mut st, "comment.toggle-line");
    assert_eq!(text(&st), "stuff\n");
    assert!(
        st.messages.iter().any(|m| m.contains("no filetype")),
        "{:?}",
        st.messages,
    );
}

#[test]
fn an_empty_line_declines_rather_than_leaving_a_bare_marker() {
    // Commenting an empty line leaves `//`, which the next toggle cannot
    // tell from content — the toggle would stop being an involution.
    let mut st = editor_with("a.rs", "\nlet x = 1;\n");
    ex(&mut st, "comment.toggle-line");
    assert_eq!(text(&st), "\nlet x = 1;\n", "untouched");
    assert!(
        st.messages
            .iter()
            .any(|m| m.contains("nothing on this line")),
        "{:?}",
        st.messages,
    );
}

#[test]
fn an_already_commented_line_written_by_a_human_is_recognised() {
    // Not just our own output: `//x` with no space is what people type.
    let mut st = editor_with("a.rs", "//x\n");
    ex(&mut st, "comment.toggle-line");
    assert_eq!(
        text(&st),
        "x\n",
        "uncommenting must tolerate the missing space"
    );
}

#[test]
fn a_scratch_buffer_has_no_path_and_therefore_no_filetype() {
    // Scratch buffers are pathless, so extension-based resolution cannot
    // answer. Declining beats guessing.
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("let x = 1;\n");
    let mut st = EditorState::new_with_buffer(bufs, id);
    let plan = escriba_lisp::apply_source(
        r#"(defmode :name "rust" :extensions ("rs") :commentstring "// %s")"#,
    )
    .expect("parses");
    escriba_lisp::apply_plan_to_filetypes(&plan, &mut st.filetypes);
    ex(&mut st, "comment.toggle-line");
    assert_eq!(text(&st), "let x = 1;\n");
}
