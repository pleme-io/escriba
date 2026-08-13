//! The vim movement suite — the motions escriba grew on 2026-08-13.
//!
//! Driven through KEYS, not through `Action`s, because every defect this file
//! exists to catch has been a *binding* defect rather than an executor one:
//! `<Del>` was implemented and unreachable, `<C-h>` was bound and shadowed,
//! `e` resolved through `w`. A test that calls `apply(Action::Move(..))`
//! passes in all three cases.

use escriba_buffer::BufferSet;
use escriba_core::Position;
use escriba_keymap::Key;
use escriba_runtime::EditorState;

/// An editor over `text` with the cursor walked to `from` BY KEYS.
///
/// Key-driven placement rather than a `set_cursor` test hook, and the reason
/// is the same one this file exists for: a hook can put the cursor where no
/// keystroke can, and then the test proves a motion from a state the editor
/// never reaches. `j`/`l` are the two motions everything else here is
/// measured against, so if they are wrong the whole file fails loudly rather
/// than one case failing subtly.
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

/// Type `keys` into a fresh editor over `text` and report where the cursor
/// ended up. A bare char is one keypress, so `"dfx"` is `d`, `f`, `x`.
fn cursor_after(text: &str, from: (u32, u32), keys: &str) -> Position {
    let mut st = editor(text, from);
    press(&mut st, keys);
    st.cursor()
}

fn text_after(text: &str, from: (u32, u32), keys: &str) -> String {
    let mut st = editor(text, from);
    press(&mut st, keys);
    st.buffers
        .get(st.active)
        .map(escriba_buffer::Buffer::to_string)
        .unwrap_or_default()
}

// ── WORD motions (`W` / `E` / `B` / `gE`) ─────────────────────────────

#[test]
fn big_word_treats_punctuation_as_part_of_the_word() {
    // The whole point of `W`: `foo.bar` is ONE WORD and three words. If `W`
    // and `w` agree here, `W` is not implemented — it is aliased.
    assert_eq!(
        cursor_after("foo.bar baz", (0, 0), "w"),
        Position::new(0, 3)
    );
    assert_eq!(
        cursor_after("foo.bar baz", (0, 0), "W"),
        Position::new(0, 8)
    );
}

#[test]
fn big_word_end_and_back_mirror_each_other() {
    assert_eq!(
        cursor_after("foo.bar baz", (0, 0), "E"),
        Position::new(0, 6)
    );
    assert_eq!(
        cursor_after("foo.bar baz", (0, 8), "B"),
        Position::new(0, 0)
    );
}

#[test]
fn ge_goes_to_the_end_of_the_previous_word() {
    // `ge` must MOVE from a word's last character, not stand still — the same
    // always-advance rule `e` has, mirrored.
    assert_eq!(
        cursor_after("alpha beta", (0, 6), "ge"),
        Position::new(0, 4)
    );
    assert_eq!(
        cursor_after("alpha beta", (0, 9), "ge"),
        Position::new(0, 4)
    );
}

// ── character search (`f` / `F` / `t` / `T` / `;`) ────────────────────

#[test]
fn find_char_lands_on_it_and_till_stops_before() {
    assert_eq!(cursor_after("abcdef", (0, 0), "fd"), Position::new(0, 3));
    assert_eq!(cursor_after("abcdef", (0, 0), "td"), Position::new(0, 2));
    assert_eq!(cursor_after("abcdef", (0, 5), "Fb"), Position::new(0, 1));
    assert_eq!(cursor_after("abcdef", (0, 5), "Tb"), Position::new(0, 2));
}

#[test]
fn a_find_that_misses_leaves_the_cursor_alone() {
    // Not "move to the line end". A motion that cannot resolve must fail, or
    // `dfz` deletes the rest of the line on a typo.
    assert_eq!(cursor_after("abcdef", (0, 2), "fz"), Position::new(0, 2));
    assert_eq!(text_after("abcdef", (0, 2), "dfz"), "abcdef");
}

#[test]
fn semicolon_repeats_the_last_find() {
    assert_eq!(cursor_after("axbxcx", (0, 0), "fx"), Position::new(0, 1));
    assert_eq!(cursor_after("axbxcx", (0, 0), "fx;"), Position::new(0, 3));
    assert_eq!(cursor_after("axbxcx", (0, 0), "fx;;"), Position::new(0, 5));
}

#[test]
fn f_is_reached_before_the_keymap_so_its_operand_is_never_a_binding() {
    // `fw` must find a `w`, NOT move a word — the character after `f` is an
    // OPERAND. Likewise `fi` must not enter Insert mode.
    assert_eq!(cursor_after("a w b", (0, 0), "fw"), Position::new(0, 2));
    assert_eq!(cursor_after("a i b", (0, 0), "fi"), Position::new(0, 2));
}

#[test]
fn find_composes_with_an_operator_and_f_is_inclusive() {
    // `dfx` deletes THROUGH the x; `dtx` stops before it. Getting this
    // backwards leaves (or takes) exactly one character.
    assert_eq!(text_after("abcxdef", (0, 0), "dfx"), "def");
    assert_eq!(text_after("abcxdef", (0, 0), "dtx"), "xdef");
}

#[test]
fn a_count_repeats_a_find() {
    assert_eq!(cursor_after("axbxcx", (0, 0), "3fx"), Position::new(0, 5));
}

// ── `%` ───────────────────────────────────────────────────────────────

#[test]
fn percent_matches_across_lines_and_counts_depth() {
    let src = "fn f() {\n    if x {\n    }\n}\n";
    // On the `{` of line 0 → the `}` on line 3, NOT the one on line 2.
    assert_eq!(cursor_after(src, (0, 7), "%"), Position::new(3, 0));
    // And back.
    assert_eq!(cursor_after(src, (3, 0), "%"), Position::new(0, 7));
}

#[test]
fn percent_scans_right_for_a_bracket_the_cursor_is_not_on() {
    // vim finds the first bracket on the line rather than refusing.
    assert_eq!(
        cursor_after("let x = (1);", (0, 0), "%"),
        Position::new(0, 10)
    );
}

// ── paragraphs and sentences ──────────────────────────────────────────

#[test]
fn paragraph_motions_stop_on_blank_lines() {
    let src = "a\nb\n\nc\nd\n\ne\n";
    assert_eq!(cursor_after(src, (0, 0), "}"), Position::new(2, 0));
    assert_eq!(cursor_after(src, (0, 0), "}}"), Position::new(5, 0));
    assert_eq!(cursor_after(src, (5, 0), "{"), Position::new(2, 0));
}

#[test]
fn sentence_motions_step_between_sentences_on_one_line() {
    let src = "One. Two. Three.\n";
    assert_eq!(cursor_after(src, (0, 0), ")"), Position::new(0, 5));
    assert_eq!(cursor_after(src, (0, 0), "))"), Position::new(0, 10));
    assert_eq!(cursor_after(src, (0, 10), "("), Position::new(0, 5));
}

// ── line-local motions ────────────────────────────────────────────────

#[test]
fn caret_and_g_underscore_bracket_the_text_of_a_padded_line() {
    let src = "   hi   \n";
    assert_eq!(cursor_after(src, (0, 7), "^"), Position::new(0, 3));
    assert_eq!(cursor_after(src, (0, 0), "g_"), Position::new(0, 4));
}

#[test]
fn g_underscore_is_inclusive_and_dollar_is_not() {
    // `d$` takes the trailing blanks with it; `dg_` stops after the text.
    assert_eq!(text_after("ab cd  \n", (0, 0), "d$"), "\n");
    assert_eq!(text_after("ab cd  \n", (0, 0), "dg_"), "  \n");
}

#[test]
fn bar_is_a_one_based_column_and_clamps() {
    assert_eq!(cursor_after("abcdef", (0, 0), "4|"), Position::new(0, 3));
    // `500|` lands on the last character rather than refusing.
    let far = cursor_after("abcdef", (0, 0), "9|");
    assert_eq!(far, Position::new(0, 5));
}

#[test]
fn plus_and_minus_land_on_the_first_non_blank() {
    let src = "a\n    b\nc\n";
    assert_eq!(cursor_after(src, (0, 0), "+"), Position::new(1, 4));
    assert_eq!(cursor_after(src, (1, 4), "-"), Position::new(0, 0));
}

// ── viewport-relative ─────────────────────────────────────────────────

#[test]
fn h_m_l_land_inside_the_viewport() {
    let src: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let mut st = editor(&src, (0, 0));
    press(&mut st, "H");
    let top = st.cursor().line;
    press(&mut st, "L");
    let bottom = st.cursor().line;
    press(&mut st, "M");
    let middle = st.cursor().line;
    assert!(top < bottom, "H must be above L (got {top} / {bottom})");
    assert!(
        (top..=bottom).contains(&middle),
        "M ({middle}) must lie between H ({top}) and L ({bottom})",
    );
}

#[test]
fn an_operator_composes_with_a_paragraph_motion() {
    // `d}` from the first line takes everything up to the blank line. The
    // motion works bare and this is the composition — the two are separate
    // claims, and the composition is the one an editor is judged on.
    assert_eq!(text_after("a\nb\n\nc\n", (0, 0), "d}"), "\nc\n");
}

#[test]
fn an_operator_composes_with_percent_inclusively() {
    // `d%` on the `(` takes the pair WITH the closing bracket. Exclusive
    // would leave a stray `)` behind, which is the whole reason `%` is an
    // inclusive motion.
    assert_eq!(text_after("fn main() {\n", (0, 7), "d%"), "fn main {\n");
}
