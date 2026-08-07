//! The view `EditContext` could not reach, now reachable — and narrowly.
//!
//! `:noh` was special-cased inside the runtime for one reason: the command
//! context could not see `SearchState`. M1 removed the special case; M2 makes
//! the search READABLE, and readable only by a handler that asks for it.

use escriba_buffer::BufferSet;
use escriba_keymap::Key;
use escriba_madoguchi::{Snapshot, cap, caps};
use escriba_runtime::EditorState;

fn searched() -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("foo bar foo baz foo\n");
    let mut st = EditorState::new_with_buffer(bufs, id);
    for k in ['/', 'f', 'o', 'o'] {
        st.on_key(&Key::Char(k));
    }
    st.on_key(&Key::Enter);
    st
}

#[test]
fn a_handler_can_finally_read_the_search() {
    let st = searched();
    let w = st.window();
    assert_eq!(w.search().pattern(), Some("foo"));
    assert_eq!(w.search().match_count(), Some(3));
    assert!(!w.search().is_prompting());
}

#[test]
fn noh_clears_the_highlight_without_forgetting_the_pattern() {
    // The distinction `match_count` must preserve: `:noh` empties the
    // HIGHLIGHT set, and reporting that as "0 matches" would be wrong —
    // `n` still works, so the pattern is still live.
    let mut st = searched();
    assert!(!st.search.highlights().is_empty());
    for k in [':', 'n', 'o', 'h'] {
        st.on_key(&Key::Char(k));
    }
    st.on_key(&Key::Enter);

    assert!(st.search.highlights().is_empty(), ":noh clears highlights");
    let w = st.window();
    assert_eq!(
        w.search().pattern(),
        Some("foo"),
        "…and must NOT forget the pattern",
    );
    assert_eq!(
        w.search().match_count(),
        Some(3),
        "…nor report zero matches, which would make `n` look broken",
    );
}

#[test]
fn nothing_searched_is_none_not_zero() {
    // "No pattern" and "a pattern with no matches" are different answers and
    // a handler must not have to re-derive the difference.
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("x\n");
    let st = EditorState::new_with_buffer(bufs, id);
    let w = st.window();
    assert_eq!(w.search().pattern(), None);
    assert_eq!(w.search().match_count(), None);
}

#[test]
fn the_window_narrows_to_exactly_what_is_asked_for() {
    // Narrowing is a type-level act over the SAME object — no copying, no
    // hiding. A handler reading through `caps!(Search)` sees the identical
    // answer the full snapshot gives.
    let st = searched();
    let w = st.window();
    let narrow: escriba_madoguchi::View<'_, caps!(cap::Search)> = escriba_madoguchi::View::new(&w);
    assert_eq!(narrow.search().pattern(), w.search().pattern());
}
