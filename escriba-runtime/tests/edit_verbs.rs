//! The single-key edit verbs — `x X D C Y s S J gJ r`.
//!
//! Key-driven, like every other suite here. Five of these nine are pure
//! keymap entries over `Action::ApplyOperator` — they ARE the compositions
//! vim spells shorter — so most of what this file pins is that the SHORTCUT
//! and the LONG SPELLING agree. A divergence between `x` and `dl` is exactly
//! the kind of thing that goes unnoticed until someone reaches for one of
//! them in an edge case.
//!
//! `r` and `J` are the two with real executors, and both have a refusal case
//! that matters more than their success case: `5rx` on a three-character tail
//! must do NOTHING (a partial replace destroys characters you did not name),
//! and `J` on the last line must SAY so rather than silently no-op — a key
//! that quietly does nothing is indistinguishable from an unbound one, which
//! is how `<C-h>` hid for a month.

use escriba_buffer::BufferSet;
use escriba_core::Position;
use escriba_keymap::Key;
use escriba_runtime::EditorState;

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

fn after(text: &str, from: (u32, u32), keys: &str) -> String {
    let mut st = editor(text, from);
    press(&mut st, keys);
    text_of(&st)
}

// ── `x` / `X` ────────────────────────────────────────────────────────────

#[test]
fn x_deletes_the_character_under_the_cursor() {
    assert_eq!(after("abcd\n", (0, 1), "x"), "acd\n");
}

#[test]
fn x_is_dl_and_stays_that_way() {
    // The shortcut and the long spelling must not drift — they are the same
    // action, and this is what says so.
    assert_eq!(after("abcd\n", (0, 1), "x"), after("abcd\n", (0, 1), "dl"));
    assert_eq!(after("abcd\n", (0, 0), "3x"), after("abcd\n", (0, 0), "3dl"));
}

#[test]
fn x_on_an_empty_line_does_nothing_and_does_not_join() {
    // The reason `Motion::Right` had to be clamped to its line. Unclamped,
    // `dl` on an empty line crossed the terminator and pulled the next line
    // up — a delete key that silently joins lines.
    assert_eq!(after("\nbravo\n", (0, 0), "x"), "\nbravo\n");
}

#[test]
fn x_on_the_last_character_of_a_line_does_not_join_either() {
    assert_eq!(after("ab\ncd\n", (0, 1), "x"), "a\ncd\n");
}

#[test]
fn a_counted_x_deletes_that_many_and_fills_the_register_with_all_of_them() {
    let mut st = editor("abcdef\n", (0, 1));
    press(&mut st, "3x");
    assert_eq!(text_of(&st), "aef\n");
    assert_eq!(st.register().map(|r| r.text.as_str()), Some("bcd"));
}

#[test]
fn a_count_past_the_end_of_the_line_takes_what_is_there() {
    assert_eq!(after("abc\n", (0, 1), "9x"), "a\n");
}

#[test]
fn shift_x_deletes_the_character_before_the_cursor() {
    // BEFORE, not under: on `c`, `X` takes the `b`.
    assert_eq!(after("abcd\n", (0, 2), "X"), "acd\n");
    assert_eq!(after("abcdef\n", (0, 3), "2X"), "adef\n");
}

#[test]
fn shift_x_at_column_zero_does_nothing() {
    // vim's behaviour, and it falls out of `Motion::Left` saturating.
    assert_eq!(after("abcd\n", (0, 0), "X"), "abcd\n");
}

// ── `D` / `C` / `Y` ──────────────────────────────────────────────────────

#[test]
fn shift_d_deletes_through_the_end_of_the_line() {
    // Through the last character, not up to it — `$` resolves one past.
    assert_eq!(after("hello world\n", (0, 6), "D"), "hello \n");
}

#[test]
fn shift_d_is_d_dollar() {
    assert_eq!(
        after("hello world\n", (0, 6), "D"),
        after("hello world\n", (0, 6), "d$"),
    );
}

#[test]
fn shift_c_clears_to_the_end_of_the_line_and_enters_insert() {
    let mut st = editor("hello world\n", (0, 6));
    press(&mut st, "C");
    assert_eq!(text_of(&st), "hello \n");
    assert_eq!(st.modal.mode(), escriba_core::Mode::Insert, "`C` types next");
}

#[test]
fn shift_y_yanks_to_the_end_of_the_line_neovim_style() {
    // The ONE key where vim and neovim disagree: classic vim's `Y` is `yy`,
    // neovim's is `y$`. escriba's default mirrors blnvim, which is neovim.
    // Pinned rather than left to a reader's assumption.
    let mut st = editor("hello world\n", (0, 6));
    press(&mut st, "Y");
    assert_eq!(text_of(&st), "hello world\n", "a yank mutates nothing");
    let reg = st.register().expect("`Y` filled the register");
    assert_eq!(reg.text, "world");
    assert_eq!(
        reg.kind,
        escriba_core::RegisterKind::Charwise,
        "`y$` is charwise; a linewise `Y` would put a whole line back",
    );
}

// ── `s` / `S` ────────────────────────────────────────────────────────────

#[test]
fn s_substitutes_the_character_under_the_cursor() {
    let mut st = editor("abcd\n", (0, 1));
    press(&mut st, "s");
    assert_eq!(text_of(&st), "acd\n");
    assert_eq!(st.modal.mode(), escriba_core::Mode::Insert);
}

#[test]
fn a_counted_s_substitutes_that_many() {
    let mut st = editor("abcdef\n", (0, 1));
    press(&mut st, "3s");
    assert_eq!(text_of(&st), "aef\n");
    assert_eq!(st.modal.mode(), escriba_core::Mode::Insert);
}

#[test]
fn shift_s_clears_the_line_but_keeps_it() {
    // The vim rule `cc`/`S` had wrong until `Extent` split capture from
    // removal: a linewise CHANGE clears the line's text and KEEPS the line,
    // because you are changing its contents rather than removing it. It used
    // to delete the line outright, so `S` on line 2 of three left two lines
    // and typed into the wrong one.
    let mut st = editor("alpha\nbravo\ncharlie\n", (1, 2));
    press(&mut st, "S");
    assert_eq!(text_of(&st), "alpha\n\ncharlie\n");
    assert_eq!(st.cursor(), Position::new(1, 0));
    assert_eq!(st.modal.mode(), escriba_core::Mode::Insert);
}

#[test]
fn shift_s_is_cc() {
    assert_eq!(
        after("alpha\nbravo\ncharlie\n", (1, 2), "S"),
        after("alpha\nbravo\ncharlie\n", (1, 2), "cc"),
    );
}

#[test]
fn a_linewise_change_still_registers_the_whole_line_with_its_terminator() {
    // The capture is unchanged by the removal split — `ccp` must put a LINE
    // back, not a line's worth of characters spliced into another.
    let mut st = editor("alpha\nbravo\ncharlie\n", (1, 0));
    press(&mut st, "S");
    let reg = st.register().expect("`S` filled the register");
    assert_eq!(reg.text, "bravo\n");
    assert_eq!(reg.kind, escriba_core::RegisterKind::Linewise);
}

#[test]
fn a_counted_shift_s_clears_n_lines_into_one_empty_line() {
    let mut st = editor("alpha\nbravo\ncharlie\ndelta\n", (1, 0));
    press(&mut st, "2S");
    assert_eq!(text_of(&st), "alpha\n\ndelta\n");
    assert_eq!(
        st.register().map(|r| r.text.as_str()),
        Some("bravo\ncharlie\n"),
    );
}

#[test]
fn shift_s_on_an_already_empty_line_still_enters_insert() {
    let mut st = editor("alpha\n\ncharlie\n", (1, 0));
    press(&mut st, "S");
    assert_eq!(text_of(&st), "alpha\n\ncharlie\n");
    assert_eq!(st.modal.mode(), escriba_core::Mode::Insert);
}

// ── `J` / `gJ` ───────────────────────────────────────────────────────────

#[test]
fn j_joins_the_next_line_with_a_single_space() {
    assert_eq!(after("alpha\nbravo\n", (0, 0), "J"), "alpha bravo\n");
}

#[test]
fn j_drops_the_next_lines_indent() {
    // The whole reason `J` exists rather than "delete the newline".
    assert_eq!(
        after("alpha\n        bravo\n", (0, 0), "J"),
        "alpha bravo\n"
    );
}

#[test]
fn j_rests_on_the_join() {
    let mut st = editor("alpha\nbravo\n", (0, 0));
    press(&mut st, "J");
    assert_eq!(
        st.cursor(),
        Position::new(0, 5),
        "on the space that replaced the newline",
    );
}

#[test]
fn j_adds_no_space_when_the_line_already_ends_in_one() {
    assert_eq!(after("alpha \nbravo\n", (0, 0), "J"), "alpha bravo\n");
}

#[test]
fn j_adds_no_space_before_a_closing_paren() {
    // vim's other exception, and the one that makes `J` usable on wrapped
    // call sites rather than something you have to clean up after.
    assert_eq!(after("foo(a,\n    )\n", (0, 0), "J"), "foo(a,)\n");
}

#[test]
fn a_counted_j_joins_that_many_lines() {
    // vim counts LINES INVOLVED, not joins performed: `3J` makes three lines
    // into one, which is two joins. `1J` and `2J` both mean one join.
    assert_eq!(
        after("a\nb\nc\nd\n", (0, 0), "3J"),
        "a b c\nd\n",
        "three lines involved",
    );
    assert_eq!(after("a\nb\nc\n", (0, 0), "2J"), "a b\nc\n");
    assert_eq!(after("a\nb\nc\n", (0, 0), "1J"), "a b\nc\n");
}

#[test]
fn a_counted_j_is_one_undo_step() {
    let mut st = editor("a\nb\nc\nd\n", (0, 0));
    press(&mut st, "3J");
    assert_eq!(text_of(&st), "a b c\nd\n");
    press(&mut st, "u");
    assert_eq!(text_of(&st), "a\nb\nc\nd\n", "one `u` undid the whole join");
}

#[test]
fn j_on_the_last_line_refuses_out_loud() {
    // A key that silently does nothing is indistinguishable from an unbound
    // one — which is exactly how the shadowed `<C-h>` hid.
    let mut st = editor("alpha\nbravo\n", (1, 0));
    press(&mut st, "J");
    assert_eq!(text_of(&st), "alpha\nbravo\n");
    assert!(
        st.messages.iter().any(|m| m.contains("E36")),
        "expected E36; got {:?}",
        st.messages,
    );
}

#[test]
fn g_j_joins_verbatim() {
    // No space, no indent stripping — the escape hatch from `J` being lossy.
    assert_eq!(
        after("alpha\n        bravo\n", (0, 0), "gJ"),
        "alpha        bravo\n",
    );
}

#[test]
fn j_does_not_touch_the_register() {
    let mut st = editor("alpha\nbravo\ncharlie\n", (0, 0));
    press(&mut st, "yy");
    let before = st.register().map(|r| r.text.clone());
    press(&mut st, "J");
    assert_eq!(text_of(&st), "alpha bravo\ncharlie\n");
    assert_eq!(st.register().map(|r| r.text.clone()), before);
}

// ── `r` ──────────────────────────────────────────────────────────────────

#[test]
fn r_replaces_the_character_under_the_cursor() {
    assert_eq!(after("abcd\n", (0, 1), "rZ"), "aZcd\n");
}

#[test]
fn r_takes_a_key_not_a_binding() {
    // `rw` must not read as `r` then *move a word*, and `ri` must not enter
    // Insert. That is why the operand is claimed at the key layer, above the
    // keymap — the same place `f`'s and `` ` ``'s operands are claimed.
    assert_eq!(after("abcd\n", (0, 1), "rw"), "awcd\n");
    assert_eq!(after("abcd\n", (0, 1), "ri"), "aicd\n");
    assert_eq!(after("abcd\n", (0, 1), "rr"), "arcd\n");
    assert_eq!(after("abcd\n", (0, 1), "rd"), "adcd\n");
}

#[test]
fn r_leaves_the_cursor_on_the_character_it_wrote() {
    let mut st = editor("abcd\n", (0, 1));
    press(&mut st, "rZ");
    assert_eq!(st.cursor(), Position::new(0, 1));
}

#[test]
fn r_does_not_enter_insert() {
    let mut st = editor("abcd\n", (0, 1));
    press(&mut st, "rZ");
    assert_eq!(st.modal.mode(), escriba_core::Mode::Normal, "`r` is not `s`");
}

#[test]
fn r_does_not_touch_the_register() {
    let mut st = editor("abcd\n", (0, 0));
    press(&mut st, "yl");
    let before = st.register().map(|r| r.text.clone());
    press(&mut st, "lrZ");
    assert_eq!(text_of(&st), "aZcd\n");
    assert_eq!(st.register().map(|r| r.text.clone()), before);
}

#[test]
fn a_counted_r_replaces_that_many() {
    let mut st = editor("abcdef\n", (0, 1));
    press(&mut st, "3rZ");
    assert_eq!(text_of(&st), "aZZZef\n");
    assert_eq!(st.cursor(), Position::new(0, 3), "on the LAST one written");
}

#[test]
fn a_counted_r_past_the_end_of_the_line_does_nothing_at_all() {
    // vim's rule, and the reason it is a rule: a PARTIAL replace silently
    // destroys characters you did not name. Refusing is the safe direction.
    assert_eq!(after("abc\n", (0, 1), "9rZ"), "abc\n");
}

#[test]
fn r_then_escape_abandons_the_gesture() {
    let mut st = editor("abcd\n", (0, 1));
    st.on_key(&Key::Char('r'));
    st.on_key(&Key::Esc);
    assert_eq!(text_of(&st), "abcd\n");
    // And the editor is not left armed for the next keystroke.
    press(&mut st, "x");
    assert_eq!(text_of(&st), "acd\n", "`x` ran as `x`, not as an operand");
}

#[test]
fn r_is_repeatable_with_dot() {
    let mut st = editor("aaaa\n", (0, 0));
    press(&mut st, "rZ");
    press(&mut st, "l.");
    assert_eq!(text_of(&st), "ZZaa\n");
}

#[test]
fn an_armed_operator_declines_r_rather_than_swallowing_it() {
    // `dr` is a typo. Claiming the `r` here would leave the operator armed
    // and make the NEXT motion delete, which is the worst of the readings.
    let mut st = editor("abcd\n", (0, 0));
    press(&mut st, "dr");
    assert_eq!(text_of(&st), "abcd\n", "the typo edited nothing");
    press(&mut st, "l");
    assert_eq!(text_of(&st), "abcd\n", "and left no operator armed");
}
