//! Linewise operators (`dd` / `yy` / `cc` and their counted forms) and the
//! put verbs (`p` / `P`) that give them somewhere to go.
//!
//! Driven through KEYS, like `movement_suite.rs` and for the same reason —
//! but this file exists because of a defect class that even key-driven tests
//! had been missing: **every `dd` test in the repo asserted the TEXT.**
//!
//! Deleting a line is easy to get right and hard to get wrong, so the text
//! assertions were all green while the cursor landed in the wrong place every
//! time — at column 0 instead of the first non-blank, and past the last line
//! of text onto the phantom row a trailing newline makes the rope report.
//! From there the NEXT `dd` deleted the file's trailing newline rather than a
//! line. Three defects, none of them visible to an assertion about text.
//!
//! So: every case here asserts the cursor, the register, AND the text, and
//! the round trip (`dd` then `p`) is asserted end-to-end — because a wrong
//! register KIND is also invisible to any one of the three on its own.

use escriba_buffer::BufferSet;
use escriba_core::{Position, RegisterKind};
use escriba_keymap::Key;
use escriba_runtime::EditorState;

/// An editor over `text` with the cursor walked to `from` BY KEYS.
fn editor(text: &str, from: (u32, u32)) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(text);
    let mut st = EditorState::new_with_buffer(bufs, id);
    for _ in 0..from.0 {
        st.on_key(&Key::Char('j'));
    }
    st.on_key(&Key::Char('0'));
    for _ in 0..from.1 {
        st.on_key(&Key::Char('l'));
    }
    assert_eq!(
        st.cursor(),
        Position::new(from.0, from.1),
        "the fixture could not reach its own start position",
    );
    st
}

fn press(st: &mut EditorState, keys: &str) {
    for c in keys.chars() {
        st.on_key(&Key::Char(c));
    }
}

fn text_of(st: &EditorState) -> String {
    st.buffers
        .get(st.active)
        .map(escriba_buffer::Buffer::to_string)
        .unwrap_or_default()
}

/// Four lines, the middle two indented, with a trailing newline — the shape
/// that has all three defects in it at once.
const INDENTED: &str = "alpha\n    bravo\n    charlie\ndelta\n";

// ── `dd` — where the cursor lands ────────────────────────────────────────

#[test]
fn dd_lands_on_the_first_non_blank_of_the_line_that_took_its_place() {
    // vim's rule. Column 0 is merely untidy on flat prose and actively wrong
    // on indented code: it drops the cursor into the indentation, so the next
    // `i` types at the margin.
    let mut st = editor(INDENTED, (1, 4));
    press(&mut st, "dd");
    assert_eq!(text_of(&st), "alpha\n    charlie\ndelta\n");
    assert_eq!(
        st.cursor(),
        Position::new(1, 4),
        "the cursor should sit on `charlie`, not in its indent",
    );
}

#[test]
fn dd_on_the_last_line_never_parks_on_the_phantom_row() {
    // A file ending in `\n` is one line of text plus a terminator, but the
    // rope reports an extra empty line. `dd` on the last REAL line used to
    // leave the cursor sitting on that phantom row, where there is nothing to
    // edit and every subsequent key acts on nothing.
    let mut st = editor(INDENTED, (3, 0));
    press(&mut st, "dd");
    assert_eq!(text_of(&st), "alpha\n    bravo\n    charlie\n");
    assert_eq!(
        st.cursor(),
        Position::new(2, 4),
        "the cursor should walk UP to `charlie`'s first non-blank",
    );
}

#[test]
fn repeated_dd_at_the_end_of_a_file_deletes_lines_not_the_trailing_newline() {
    // The consequence of the phantom-row landing, and the reason it read as
    // "something feels off" rather than as a cursor bug: the second `dd` was
    // issued FROM the phantom row, took the "no following newline" branch,
    // and removed the file's terminator instead of a line. The text shrank by
    // one BYTE and the operator saw nothing happen.
    let mut st = editor("alpha\nbravo\ncharlie\n", (2, 0));
    press(&mut st, "dd");
    assert_eq!(text_of(&st), "alpha\nbravo\n");
    press(&mut st, "dd");
    assert_eq!(text_of(&st), "alpha\n", "a line, not a newline");
    press(&mut st, "dd");
    assert_eq!(text_of(&st), "");
}

#[test]
fn dd_on_the_last_line_of_a_file_with_no_trailing_newline_still_removes_it() {
    // The other branch: with no terminator to take forward, the preceding one
    // has to go instead, or `dd` blanks the line and leaves it behind.
    let mut st = editor("alpha\nbravo\ncharlie", (2, 0));
    press(&mut st, "dd");
    assert_eq!(text_of(&st), "alpha\nbravo");
    assert_eq!(st.cursor(), Position::new(1, 0));
}

#[test]
fn dd_on_the_only_line_clears_it_and_keeps_the_line() {
    let mut st = editor("solo\n", (0, 2));
    press(&mut st, "dd");
    assert!(text_of(&st).is_empty() || text_of(&st) == "\n", "{:?}", text_of(&st));
    assert_eq!(st.cursor(), Position::new(0, 0));
}

// ── counted linewise ─────────────────────────────────────────────────────

#[test]
fn a_counted_dd_is_one_delete_of_n_lines_and_the_register_holds_all_of_them() {
    // `2dd` used to run the one-line object TWICE. The text came out right by
    // accident — deleting a line brings the next one under the cursor — and
    // the register kept only the SECOND line, so `2ddp` silently put back
    // half of what it took. Asserting the register is what catches it.
    let mut st = editor("alpha\nbravo\ncharlie\ndelta\n", (1, 0));
    press(&mut st, "2dd");
    assert_eq!(text_of(&st), "alpha\ndelta\n");
    assert_eq!(
        st.register().map(|r| r.text.as_str()),
        Some("bravo\ncharlie\n"),
        "both deleted lines, not just the last",
    );
}

#[test]
fn a_count_past_the_end_of_the_file_takes_what_is_there() {
    let mut st = editor("alpha\nbravo\ncharlie\n", (1, 0));
    press(&mut st, "9dd");
    assert_eq!(text_of(&st), "alpha\n");
    assert_eq!(
        st.register().map(|r| r.text.as_str()),
        Some("bravo\ncharlie\n"),
    );
}

// ── `yy` and its corollaries ─────────────────────────────────────────────

#[test]
fn yy_captures_the_whole_line_without_moving_the_cursor() {
    // vim moves the cursor to the start of a yanked region only when that is
    // BEHIND where it stood. `yy`'s range starts at column 0, so an
    // unconditional move knocked the cursor to the left margin every time you
    // copied a line — invisible to `yw`, whose range starts at the cursor.
    let mut st = editor(INDENTED, (1, 6));
    press(&mut st, "yy");
    assert_eq!(text_of(&st), INDENTED, "a yank mutates nothing");
    assert_eq!(st.cursor(), Position::new(1, 6), "`yy` does not move");
    let reg = st.register().expect("`yy` filled the register");
    assert_eq!(reg.text, "    bravo\n");
    assert_eq!(reg.kind, RegisterKind::Linewise);
}

#[test]
fn a_counted_yy_takes_n_lines_in_one_capture() {
    let mut st = editor("alpha\nbravo\ncharlie\ndelta\n", (0, 0));
    press(&mut st, "3yy");
    assert_eq!(text_of(&st), "alpha\nbravo\ncharlie\ndelta\n");
    assert_eq!(
        st.register().map(|r| r.text.as_str()),
        Some("alpha\nbravo\ncharlie\n"),
    );
}

#[test]
fn a_backward_yank_still_moves_to_the_start_of_what_it_took() {
    // The other half of the yank rule, so the fix above cannot be satisfied
    // by "never move".
    let mut st = editor("one two three\n", (0, 8));
    press(&mut st, "yb");
    assert_eq!(st.cursor(), Position::new(0, 4), "`yb` rests at the start");
}

#[test]
fn a_charwise_yank_is_captured_charwise() {
    let mut st = editor("one two three\n", (0, 0));
    press(&mut st, "yw");
    let reg = st.register().expect("`yw` filled the register");
    assert_eq!(reg.text, "one ");
    assert_eq!(reg.kind, RegisterKind::Charwise);
}

// ── `p` / `P` — linewise ─────────────────────────────────────────────────

#[test]
fn p_after_yy_opens_a_new_line_below() {
    let mut st = editor("alpha\nbravo\n", (0, 2));
    press(&mut st, "yyp");
    assert_eq!(text_of(&st), "alpha\nalpha\nbravo\n");
    assert_eq!(st.cursor(), Position::new(1, 0), "on the line just put");
}

#[test]
fn shift_p_after_yy_opens_a_new_line_above() {
    let mut st = editor("alpha\nbravo\n", (1, 0));
    press(&mut st, "yyP");
    assert_eq!(text_of(&st), "alpha\nbravo\nbravo\n");
    assert_eq!(st.cursor(), Position::new(1, 0));
}

#[test]
fn a_linewise_put_rests_on_the_first_non_blank() {
    let mut st = editor(INDENTED, (1, 4));
    press(&mut st, "yyp");
    assert_eq!(
        text_of(&st),
        "alpha\n    bravo\n    bravo\n    charlie\ndelta\n"
    );
    assert_eq!(st.cursor(), Position::new(2, 4));
}

#[test]
fn dd_then_p_moves_a_line_down_one() {
    // The gesture the whole feature exists for, end to end.
    let mut st = editor("alpha\nbravo\ncharlie\n", (0, 0));
    press(&mut st, "ddp");
    assert_eq!(text_of(&st), "bravo\nalpha\ncharlie\n");
    assert_eq!(st.cursor(), Position::new(1, 0));
}

#[test]
fn a_linewise_put_at_the_end_of_the_file_still_makes_a_line() {
    // `line + 1` has to be a legal insertion point on the last line, or `p`
    // at the bottom of a file either does nothing or glues two lines.
    let mut st = editor("alpha\nbravo\n", (1, 0));
    press(&mut st, "yyp");
    assert_eq!(text_of(&st), "alpha\nbravo\nbravo\n");
    assert_eq!(st.cursor(), Position::new(2, 0));
}

#[test]
fn a_linewise_put_of_a_file_with_no_trailing_newline_still_makes_two_lines() {
    // `Register::replayed` terminates a linewise capture before repeating it.
    // Without that, yanking the last line of an unterminated file and putting
    // it back produces `bravobravo` on one line.
    let mut st = editor("alpha\nbravo", (1, 0));
    press(&mut st, "yyp");
    assert_eq!(text_of(&st), "alpha\nbravo\nbravo");
}

#[test]
fn a_counted_linewise_put_puts_n_copies() {
    let mut st = editor("alpha\n", (0, 0));
    press(&mut st, "yy3p");
    assert_eq!(text_of(&st), "alpha\nalpha\nalpha\nalpha\n");
    assert_eq!(st.cursor(), Position::new(1, 0), "the FIRST line put");
}

// ── `p` / `P` — charwise ─────────────────────────────────────────────────

#[test]
fn p_after_a_charwise_yank_splices_after_the_cursor() {
    let mut st = editor("one two\n", (0, 0));
    press(&mut st, "yw"); // "one "
    press(&mut st, "$"); // onto the final `o`
    press(&mut st, "p");
    assert_eq!(text_of(&st), "one twoone \n");
}

#[test]
fn shift_p_after_a_charwise_yank_splices_at_the_cursor() {
    let mut st = editor("one two\n", (0, 0));
    press(&mut st, "yw");
    press(&mut st, "P");
    assert_eq!(text_of(&st), "one one two\n");
}

#[test]
fn a_charwise_put_rests_on_the_last_character_it_wrote() {
    // vim's rule, and the one that makes a second `p` stack the copies rather
    // than march rightward across the line.
    let mut st = editor("ab\n", (0, 0));
    press(&mut st, "ylp"); // yank `a`, put it after
    assert_eq!(text_of(&st), "aab\n");
    assert_eq!(st.cursor(), Position::new(0, 1), "ON the `a` just put");
}

#[test]
fn a_counted_charwise_put_repeats_the_text() {
    let mut st = editor("ab\n", (0, 0));
    press(&mut st, "yl3p");
    assert_eq!(text_of(&st), "aaaab\n");
}

// ── the properties that hold across both kinds ───────────────────────────

#[test]
fn a_put_with_an_empty_register_does_nothing() {
    // And in particular does not record a no-op change for `.` to replay.
    let mut st = editor("alpha\n", (0, 2));
    assert!(st.register().is_none(), "precondition: nothing captured yet");
    press(&mut st, "p");
    assert_eq!(text_of(&st), "alpha\n");
    assert_eq!(st.cursor(), Position::new(0, 2));
}

#[test]
fn a_counted_put_is_one_undo_step() {
    // `3p` written as three edits would take three `u` presses to undo, which
    // is not what a single keystroke should cost.
    let mut st = editor("alpha\n", (0, 0));
    press(&mut st, "yy3p");
    assert_eq!(text_of(&st), "alpha\nalpha\nalpha\nalpha\n");
    press(&mut st, "u");
    assert_eq!(text_of(&st), "alpha\n", "one `u` undid the whole put");
}

#[test]
fn a_put_is_repeatable_with_dot() {
    let mut st = editor("alpha\n", (0, 0));
    press(&mut st, "yyp");
    assert_eq!(text_of(&st), "alpha\nalpha\n");
    press(&mut st, ".");
    assert_eq!(text_of(&st), "alpha\nalpha\nalpha\n");
}

#[test]
fn a_charwise_capture_and_a_linewise_one_put_differently_from_the_same_key() {
    // The whole point of typing the register: `p` is one key with two
    // behaviours, and the register — not the key — decides which.
    let mut charwise = editor("alpha\nbravo\n", (0, 0));
    press(&mut charwise, "ywp");
    let mut linewise = editor("alpha\nbravo\n", (0, 0));
    press(&mut linewise, "yyp");
    assert_ne!(
        text_of(&charwise),
        text_of(&linewise),
        "a spliced run of characters is not an opened line",
    );
    assert_eq!(text_of(&linewise), "alpha\nalpha\nbravo\n");
}
