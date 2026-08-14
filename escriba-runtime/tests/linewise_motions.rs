//! **vim's THIRD motion kind: linewise.**
//!
//! escriba modelled exclusive and inclusive and stopped there, so every
//! operator over a linewise motion was wrong in two ways at once and the
//! whole workspace suite stayed green through it:
//!
//! | gesture | vim | escriba until 2026-08-14 |
//! |---|---|---|
//! | `dj` on line 1 of 5 | removes lines 1–2 | removed line 1 |
//! | `dgg` on line 2 | removes lines 0–2 | removed lines 0–1 |
//! | `yj` | **Linewise** register | Charwise |
//! | `d_` | removes the line | **nothing at all** |
//!
//! The count error is one line, which reads as a plausible editor. The
//! REGISTER error is invisible entirely until a later `p`, at which point two
//! whole lines splice into the middle of a third. That is why this went
//! unnoticed: the wrong answer looks like an answer.
//!
//! Every case here asserts BOTH the text and the register kind, because the
//! kind is the half nothing else can see.

use escriba_buffer::BufferSet;
use escriba_core::{Motion, RegisterKind};
use escriba_keymap::Key;
use escriba_runtime::EditorState;

const FIVE: &str = "aaa\nbbb\nccc\nddd\neee\n";

fn editor(text: &str) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(text);
    EditorState::new_with_buffer(bufs, id)
}

/// Walk to the start line by KEYS, never by a setter — a hook can place the
/// cursor where no keystroke can, and then the gesture is proven from a state
/// the editor never reaches.
fn at_line(text: &str, line: usize) -> EditorState {
    let mut st = editor(text);
    for _ in 0..line {
        st.on_key(&Key::Char('j'));
    }
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

fn reg(st: &EditorState) -> (RegisterKind, String) {
    let r = st.register().expect("the operator left no register");
    (r.kind, r.text.clone())
}

/// One case: from `line`, press `keys`, expect this text and this register.
fn case(keys: &str, line: usize, want_text: &str, want_reg: &str) {
    let mut st = at_line(FIVE, line);
    press(&mut st, keys);
    assert_eq!(text_of(&st), want_text, "`{keys}` from line {line}: text");
    let (kind, text) = reg(&st);
    assert_eq!(
        kind,
        RegisterKind::Linewise,
        "`{keys}` must leave a LINEWISE register — a charwise one puts back \
         spliced into the middle of a line",
    );
    assert_eq!(text, want_reg, "`{keys}` from line {line}: register text");
}

// ── the vertical pair, which is how the class is usually noticed ─────────

#[test]
fn dj_takes_two_whole_lines() {
    case("dj", 1, "aaa\nddd\neee\n", "bbb\nccc\n");
}

#[test]
fn dk_takes_two_whole_lines_reaching_upward() {
    // Same two lines as `dj` from line 1 — the span is symmetric, which is
    // why `line_span` orders its arguments rather than assuming forward.
    case("dk", 2, "aaa\nddd\neee\n", "bbb\nccc\n");
}

// ── document and screen ─────────────────────────────────────────────────

#[test]
fn dgg_takes_through_the_cursor_line_not_up_to_it() {
    // The off-by-one that makes this class look "nearly right": an exclusive
    // reading stops at the start of line 2 and leaves `ccc` behind.
    case("dgg", 2, "ddd\neee\n", "aaa\nbbb\nccc\n");
}

#[test]
fn dg_capital_takes_through_the_end() {
    case("dG", 2, "aaa\nbbb\n", "ccc\nddd\neee\n");
}

#[test]
fn dh_and_dl_are_linewise() {
    case("dH", 2, "ddd\neee\n", "aaa\nbbb\nccc\n");
    case("dL", 1, "aaa\n", "bbb\nccc\nddd\neee\n");
}

// ── `+` `-` `_` ─────────────────────────────────────────────────────────

#[test]
fn plus_and_minus_are_linewise() {
    case("d+", 1, "aaa\nddd\neee\n", "bbb\nccc\n");
    case("d-", 2, "aaa\nddd\neee\n", "bbb\nccc\n");
}

#[test]
fn underscore_deletes_the_line_and_is_not_an_alias_of_caret() {
    // `d_` did NOTHING before, and for a reason that reads as correct: `_`
    // was bound to the same motion as `^`, so the operator got the exclusive
    // range [cursor, first-non-blank) — empty whenever the cursor is already
    // at the indent, which after `j` it always is.
    case("d_", 1, "aaa\nccc\nddd\neee\n", "bbb\n");

    // …while `^` stays exclusive-charwise. Same landing character, two kinds.
    let mut st = at_line("  indented\n", 0);
    press(&mut st, "$"); // to the end
    press(&mut st, "d^");
    assert_eq!(
        text_of(&st),
        "  d\n",
        "`d^` deletes back to the indent, not the line",
    );
}

// ── the register kind, stated as its own claim ──────────────────────────

#[test]
fn a_linewise_yank_puts_back_as_lines() {
    // The end-to-end shape the wrong register kind broke. `yj` then `p` must
    // OPEN two lines below; charwise would splice `bbb\nccc\n` into the
    // middle of `ddd`.
    let mut st = at_line(FIVE, 1);
    press(&mut st, "yj");
    assert_eq!(reg(&st).0, RegisterKind::Linewise);
    press(&mut st, "jj"); // onto `ddd`
    press(&mut st, "p");
    assert_eq!(text_of(&st), "aaa\nbbb\nccc\nddd\nbbb\nccc\neee\n");
}

#[test]
fn a_linewise_yank_does_not_change_the_buffer() {
    let mut st = at_line(FIVE, 2);
    press(&mut st, "ygg");
    assert_eq!(text_of(&st), FIVE, "yank must not mutate");
    assert_eq!(reg(&st), (RegisterKind::Linewise, "aaa\nbbb\nccc\n".into()));
}

// ── counts, and the change operator ─────────────────────────────────────

#[test]
fn a_counted_linewise_motion_is_one_operation_over_n_lines() {
    // `2dj` is three lines (the cursor's plus two down), one operation — so
    // the register holds all three. The repeat-instead-of-absorb defect this
    // guards is the same one `2dd` had.
    case("2dj", 0, "ddd\neee\n", "aaa\nbbb\nccc\n");
}

#[test]
fn change_over_a_linewise_motion_keeps_a_line_to_type_on() {
    // `cj` clears two lines' TEXT and leaves one empty line — the removal /
    // capture split `Extent` exists for. `dj` removes the lines entirely.
    let mut st = at_line(FIVE, 1);
    press(&mut st, "cj");
    assert_eq!(text_of(&st), "aaa\n\nddd\neee\n");
    assert_eq!(
        st.modal.mode(),
        escriba_core::Mode::Insert,
        "`c` types next"
    );
    assert_eq!(reg(&st), (RegisterKind::Linewise, "bbb\nccc\n".into()));
}

// ── the controls: what must NOT have become linewise ─────────────────────

#[test]
fn the_charwise_motions_are_untouched() {
    let mut st = at_line(FIVE, 1);
    press(&mut st, "d$");
    assert_eq!(
        text_of(&st),
        "aaa\n\nccc\nddd\neee\n",
        "`d$` blanks the line"
    );
    assert_eq!(reg(&st).0, RegisterKind::Charwise);

    // `}` is EXCLUSIVE in vim, not linewise — the guess that would make `d}`
    // swallow the blank line that terminates the paragraph.
    let mut st2 = at_line("one\ntwo\n\nthree\n", 0);
    press(&mut st2, "d}");
    assert_eq!(text_of(&st2), "\nthree\n", "`d}}` stops AT the blank line");
    assert_eq!(reg(&st2).0, RegisterKind::Charwise);
}

/// The classifier itself, spot-checked at the boundary that matters.
///
/// `is_linewise` is an exhaustive `match`, so a NEW motion cannot default into
/// the wrong answer — the compiler asks. What it cannot ask is whether an
/// EXISTING one is classified correctly, and the `^` / `_` pair is where
/// getting it wrong is least visible, since both land on the same character.
#[test]
fn caret_and_underscore_are_different_kinds() {
    assert!(!Motion::LineFirstNonBlank.is_linewise(), "`^`");
    assert!(Motion::LinewiseDown.is_linewise(), "`_`");
    assert!(
        Motion::Down.is_linewise() && Motion::Up.is_linewise(),
        "`j`/`k`"
    );
    assert!(
        !Motion::WordStartNext.is_linewise() && !Motion::ParagraphNext.is_linewise(),
        "`w` and `}}` are charwise",
    );
    // Two spellings of a mark jump, two kinds — the reason they are two
    // variants rather than one plus a flag.
    assert!(Motion::MarkLine('a').is_linewise());
    assert!(!Motion::MarkExact('a').is_linewise());
}
