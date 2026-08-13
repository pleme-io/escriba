//! `w`, `b`, `e` — where a word starts, where it ends, and where the cursor
//! is allowed to stand.
//!
//! Two defects motivated this file, and they look alike from a keyboard:
//!
//! 1. **`w` walked off the end of the text.** On the last word of the buffer
//!    it landed one column PAST the last character, which Normal mode has no
//!    business resting on. The position itself is right — it is the exclusive
//!    end an operator needs — so the fix is the *cursor's* rule, not the
//!    motion's, and the two are pinned apart here.
//! 2. **`w` and `e` were the same function.** `Motion::WordEndNext` resolved
//!    through `word_next`, so the only difference between them was the name.
//!
//! Both are asserted through the KEYMAP where a key exists, because a motion
//! that is right and unbound is the other half of this repo's recurring
//! failure — the model correct, the report (or the binding) missing.

use escriba_buffer::BufferSet;
use escriba_core::Position;
use escriba_keymap::Key;
use escriba_runtime::EditorState;

fn editor(text: &str) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(text);
    EditorState::new_with_buffer(bufs, id)
}

fn press(st: &mut EditorState, keys: &str) {
    for c in keys.chars() {
        st.on_key(&Key::Char(c));
    }
}

/// The reported bug: `w` onto the final word must rest ON its last character.
#[test]
fn w_onto_the_last_word_rests_on_its_last_character() {
    let mut st = editor("hello world");
    press(&mut st, "w");
    assert_eq!(st.cursor(), Position::new(0, 6), "`w` reaches `world`");
    press(&mut st, "w");
    assert_eq!(
        st.cursor(),
        Position::new(0, 10),
        "with no next word, `w` stops on `d` — column 11 is past the text",
    );
}

/// The same rule, reached by every other forward motion. Normal mode has no
/// position past the last character, so this is a property of the CURSOR and
/// not of any one motion.
#[test]
fn normal_mode_never_rests_past_the_last_character() {
    let mut st = editor("hello");
    press(&mut st, "$");
    assert_eq!(st.cursor(), Position::new(0, 4), "`$` rests on `o`");

    // An empty line is the degenerate case: column 0 is both the last
    // character's column and one past it.
    let mut empty = editor("\nsecond");
    press(&mut empty, "$");
    assert_eq!(empty.cursor(), Position::new(0, 0));
}

/// …and the operator is NOT clamped, which is the whole reason the rule
/// lives at the cursor. `dw` on the final word deletes the word, not the word
/// minus its last letter.
#[test]
fn dw_on_the_last_word_still_deletes_all_of_it() {
    let mut st = editor("hello world");
    press(&mut st, "w");
    press(&mut st, "dw");
    assert_eq!(
        st.buffers
            .get(st.active)
            .map(escriba_buffer::Buffer::to_string),
        Some("hello ".to_string()),
        "the deleted range ends after `d`, even though the cursor may not",
    );
}

/// A file ending in `\n` is one line plus a terminator. The rope reports two
/// lines, the second empty, and `w` used to walk onto it — off the end of the
/// file, onto a row with nothing on it. This is the shape the bug takes on an
/// ordinary saved file, where every line ends in a newline.
#[test]
fn w_does_not_walk_onto_the_row_after_a_trailing_newline() {
    let mut st = editor("hello world\n");
    press(&mut st, "w");
    assert_eq!(st.cursor(), Position::new(0, 6));
    press(&mut st, "w");
    assert_eq!(
        st.cursor(),
        Position::new(0, 10),
        "the terminator is not a line to move to",
    );
    press(&mut st, "e");
    assert_eq!(st.cursor(), Position::new(0, 10), "`e` stops there too");
}

/// …and a GENUINE trailing blank line is still a place to go, or `w` would
/// stop working for walking down through paragraphs at the bottom of a file.
#[test]
fn a_real_blank_line_before_the_terminator_is_still_a_word() {
    let mut st = editor("one\n\n");
    press(&mut st, "w");
    assert_eq!(st.cursor(), Position::new(1, 0), "the blank line is real");
}

/// vim's `w` stops at a class boundary; whitespace is not the only one.
#[test]
fn w_stops_at_punctuation() {
    let mut st = editor("foo.bar baz");
    for (n, expect) in [(1, 3), (2, 4), (3, 8)] {
        press(&mut st, "w");
        assert_eq!(
            st.cursor(),
            Position::new(0, expect),
            "press {n} of `w` on `foo.bar baz`",
        );
    }
}

/// …and so does `b`, or `dw` and `db` would disagree about where the same
/// word begins.
#[test]
fn b_agrees_with_w_about_where_a_word_begins() {
    let mut st = editor("foo.bar");
    press(&mut st, "$");
    assert_eq!(st.cursor(), Position::new(0, 6));
    press(&mut st, "b");
    assert_eq!(st.cursor(), Position::new(0, 4), "`b` back to `bar`");
    press(&mut st, "b");
    assert_eq!(st.cursor(), Position::new(0, 3), "`b` back to `.`");
    press(&mut st, "b");
    assert_eq!(st.cursor(), Position::new(0, 0), "`b` back to `foo`");
}

/// Crossing a line lands on the first non-blank, not on column 0 — otherwise
/// the next `w` is spent walking out of the indent.
#[test]
fn w_crosses_a_line_onto_its_first_non_blank() {
    let mut st = editor("end\n    next");
    press(&mut st, "w");
    assert_eq!(st.cursor(), Position::new(1, 4), "onto `n`, not the indent");
}

/// An empty line is a word to vim, which is what makes `w` usable for
/// walking down through paragraphs.
#[test]
fn an_empty_line_is_a_word() {
    let mut st = editor("one\n\ntwo");
    press(&mut st, "w");
    assert_eq!(
        st.cursor(),
        Position::new(1, 0),
        "`w` stops on the blank line"
    );
    press(&mut st, "w");
    assert_eq!(st.cursor(), Position::new(2, 0));
}

/// `e` is bound and is its own motion — it used to resolve through `w`.
#[test]
fn e_lands_on_the_last_character_of_a_word() {
    let mut st = editor("hello world");
    press(&mut st, "e");
    assert_eq!(st.cursor(), Position::new(0, 4), "`e` onto `o` of hello");
    press(&mut st, "e");
    assert_eq!(st.cursor(), Position::new(0, 10), "`e` onto `d` of world");
    // Always moves: standing on a word's last character, `e` takes the next.
    let mut punct = editor("foo.bar");
    press(&mut punct, "e");
    assert_eq!(punct.cursor(), Position::new(0, 2));
    press(&mut punct, "e");
    assert_eq!(
        punct.cursor(),
        Position::new(0, 3),
        "`.` is a word to `e` too"
    );
}

/// `e` is INCLUSIVE: `de` deletes through the character it names. Getting
/// this wrong leaves exactly one letter behind, on the key most often used
/// to delete a word without its trailing space.
#[test]
fn de_deletes_through_the_character_e_names() {
    let mut st = editor("hello world");
    press(&mut st, "de");
    assert_eq!(
        st.buffers
            .get(st.active)
            .map(escriba_buffer::Buffer::to_string),
        Some(" world".to_string()),
        "`de` takes all of `hello`, including the `o`",
    );
}
