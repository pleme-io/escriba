//! One test per rule in the crate docs, plus the fixtures that make each
//! non-vacuous.
//!
//! Every assertion here is against real vim behaviour, not against what this
//! implementation happens to do. Where a rule has a counter-example that a
//! naive implementation gets wrong, the counter-example IS the test — the
//! point is to fail when someone simplifies the code back to the naive form.

use super::*;

/// A minimal [`TextTarget`] — a `String` and a byte caret. Enough to exercise
/// every edit path without a widget crate in the dev-dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    text: String,
    caret: usize,
}

impl Line {
    fn new(text: &str, caret: usize) -> Self {
        Self {
            text: text.to_owned(),
            caret,
        }
    }
}

impl TextTarget for Line {
    fn text(&self) -> &str {
        &self.text
    }
    fn caret(&self) -> usize {
        self.caret
    }
    fn set_caret(&mut self, at: usize) {
        self.caret = at;
    }
    fn replace(&mut self, range: Range<usize>, with: &str) {
        self.text.replace_range(range, with);
    }
}

// ── rule 2: motions always advance ───────────────────────────────────────

/// **THE FIXED-POINT GATE.** `e` implemented as "the end of the run containing
/// the caret" is a fixed point: press it twice and the second press does
/// nothing. That is a cursor that stops responding to a held key, and it is
/// the single easiest way to get `e` wrong.
#[test]
fn e_advances_every_time_it_is_pressed() {
    let t = "foo bar baz";
    let a = word_end_next(t, 0, Width::Small);
    assert_eq!(a, 2, "first `e` lands on the last char of `foo`");
    let b = word_end_next(t, a, Width::Small);
    assert_eq!(b, 6, "second `e` must MOVE, to the last char of `bar`");
    let c = word_end_next(t, b, Width::Small);
    assert_eq!(c, 10, "and again, to the last char of `baz`");
}

#[test]
fn b_retreats_every_time_it_is_pressed() {
    let t = "foo bar baz";
    let a = word_start_prev(t, 11, Width::Small);
    assert_eq!(a, 8);
    let b = word_start_prev(t, a, Width::Small);
    assert_eq!(b, 4);
    let c = word_start_prev(t, b, Width::Small);
    assert_eq!(c, 0);
}

// ── rule 3: whitespace is a gap for motions, a destination for objects ───

/// The asymmetry, both halves on one fixture. A shared class model with no
/// stated rule makes `w` stop on the space — a real and common bug.
#[test]
fn whitespace_is_crossed_by_w_and_selected_by_iw() {
    let t = "foo  bar";
    assert_eq!(
        word_start_next(t, 0, Width::Small),
        5,
        "`w` crosses the blanks and lands on `bar`, never at offset 3",
    );
    assert_eq!(
        object_span(t, 3, TextObject::Word { around: false }),
        Some(3..5),
        "`iw` ON the blanks selects the blanks — the opposite treatment",
    );
}

#[test]
fn w_stops_at_punctuation_but_big_w_does_not() {
    let t = "foo-bar baz";
    assert_eq!(
        word_start_next(t, 0, Width::Small),
        3,
        "`w` stops at the `-` — punctuation is its own class",
    );
    assert_eq!(
        word_start_next(t, 0, Width::Big),
        8,
        "`W` treats `foo-bar` as one WORD and lands on `baz`",
    );
}

// ── rule 1: exclusive vs INCLUSIVE ───────────────────────────────────────

/// **THE CLASSIC BUG.** `dw` is exclusive, `de` is INCLUSIVE. An
/// implementation that treats every operator range as `[caret, target)`
/// deletes `fo` for `de` and leaves the `o` behind.
#[test]
fn de_is_inclusive_and_dw_is_exclusive() {
    let t = "foo bar";
    assert_eq!(
        operated_span(t, 0, Operator::Delete, Motion::WordEndNext, 1),
        Some(0..3),
        "`de` takes the character `e` lands on — all three of `foo`",
    );
    assert_eq!(
        operated_span(t, 0, Operator::Delete, Motion::WordStartNext, 1),
        Some(0..4),
        "`dw` takes up to the next word's first char — `foo ` including the space",
    );
    assert!(is_inclusive(Motion::WordEndNext));
    assert!(!is_inclusive(Motion::WordStartNext));
}

/// `d$` must take the last character. Exclusive-everywhere leaves it.
#[test]
fn d_dollar_takes_the_last_character() {
    let mut l = Line::new("hello world", 6);
    let mut r = Register::default();
    let span = operated_span(&l.text, l.caret, Operator::Delete, Motion::LineEnd, 1).unwrap();
    take(&mut l, span, &mut r);
    assert_eq!(l.text, "hello ");
    assert_eq!(r.text(), "world", "the whole tail, `d` included");
}

// ── rule 4: backward ranges normalise ────────────────────────────────────

/// `db`, `d0` and `dh` all resolve LEFT of the caret. A naive `caret..target`
/// is an inverted range — a slice panic, or a typed refusal from an API that
/// checks. Either way `db` is one of four motions in the v1 alphabet.
#[test]
fn backward_operators_build_an_ordered_range() {
    let t = "foo bar baz";
    for motion in [Motion::WordStartPrev, Motion::LineStart, Motion::Left] {
        let span = operated_span(t, 8, Operator::Delete, motion, 1)
            .unwrap_or_else(|| panic!("{motion:?} resolves"));
        assert!(
            span.start <= span.end,
            "{motion:?} produced an inverted range {span:?}",
        );
        assert_eq!(
            span.end, 8,
            "the caret is the exclusive END going backwards"
        );
    }
    assert_eq!(
        operated_span(t, 8, Operator::Delete, Motion::WordStartPrev, 1),
        Some(4..8),
        "`db` from the start of `baz` takes `bar `",
    );
}

// ── rule 5: counted overrun operates over what was reached ───────────────

/// `d5w` with two words left must delete to the end, not refuse and not
/// under-delete. vim does this for `999dw` near the end of a file.
#[test]
fn a_count_that_overruns_operates_over_what_was_reached() {
    let t = "foo bar";
    let span = operated_span(t, 0, Operator::Delete, Motion::WordStartNext, 5)
        .expect("an overrunning count still resolves");
    assert_eq!(span, 0..t.len(), "everything, rather than a refusal");
}

#[test]
fn counts_multiply_through_the_motion() {
    let t = "a b c d e";
    assert_eq!(resolve(t, 0, Motion::WordStartNext, 3), Some(6));
}

// ── rule 6: cw is not ce ─────────────────────────────────────────────────

/// **THE QUIRK MOST IMPLEMENTATIONS GET WRONG**, and the folk rule `cw ≡ ce`
/// is what gets it wrong. On the LAST character of a word, `ce` advances to
/// the end of the *next* word — so `cw` implemented as `ce` eats the word
/// after the one you meant.
#[test]
fn cw_on_a_words_last_character_does_not_eat_the_next_word() {
    let t = "foo bar";
    // caret on the final `o` of `foo`.
    let cw = operated_span(t, 2, Operator::Change, Motion::WordStartNext, 1).unwrap();
    assert_eq!(cw, 2..3, "`cw` changes just the `o`");

    // The proof this is not vacuous: `ce` from the same place really does run
    // on into `bar`, which is exactly what `cw` must NOT do.
    let ce = operated_span(t, 2, Operator::Change, Motion::WordEndNext, 1).unwrap();
    assert_eq!(
        ce,
        2..7,
        "`ce` from here takes `o bar` — the wrong answer for `cw`"
    );
    assert_ne!(cw, ce, "the whole point of the quirk");
}

/// On a BLANK, the quirk does not apply — `cw` behaves like `dw`.
#[test]
fn cw_on_whitespace_behaves_like_dw() {
    let t = "foo  bar";
    assert_eq!(
        operated_span(t, 3, Operator::Change, Motion::WordStartNext, 1),
        operated_span(t, 3, Operator::Delete, Motion::WordStartNext, 1),
        "on a blank there is no current word to stay inside",
    );
}

// ── rule 7: Normal clamps, Insert does not ───────────────────────────────

#[test]
fn normal_never_rests_past_the_last_character_and_insert_may() {
    let t = "abc";
    assert_eq!(clamp(t, 3, Stance::Normal), 2, "Normal pulls back onto `c`");
    assert_eq!(
        clamp(t, 3, Stance::Insert),
        3,
        "Insert may sit past the end"
    );
    assert_eq!(clamp(t, 99, Stance::Insert), 3, "but never past the text");
}

/// The empty line is the edge case that turns a clamp into a panic.
#[test]
fn clamping_an_empty_line_is_zero_in_both_stances() {
    for s in Stance::ALL {
        assert_eq!(clamp("", 0, *s), 0);
        assert_eq!(clamp("", 5, *s), 0);
    }
}

// ── rule 8: every removal fills the register ─────────────────────────────

/// `xp` — transpose two characters — is the commonest one-line vim idiom and
/// it is silently broken if `x` does not fill the register.
#[test]
fn x_then_p_transposes_two_characters() {
    let mut l = Line::new("ab", 0);
    let mut r = Register::default();
    // `x` is delete-one-char-right.
    let span = l.caret..next_char(&l.text, l.caret);
    take(&mut l, span, &mut r);
    assert_eq!(l.text, "b");
    assert_eq!(r.text(), "a", "x MUST fill the register");
    paste(&mut l, &r, true, 1);
    assert_eq!(l.text, "ba", "xp transposes");
}

#[test]
fn yank_fills_the_register_without_removing_and_moves_to_the_start() {
    let mut l = Line::new("foo bar", 4);
    let mut r = Register::default();
    let span = operated_span(&l.text, l.caret, Operator::Yank, Motion::LineEnd, 1).unwrap();
    yank(&mut l, span, &mut r);
    assert_eq!(l.text, "foo bar", "yank removes nothing");
    assert_eq!(r.text(), "bar");
    assert_eq!(l.caret, 4, "the caret lands at the span start");
}

/// `yb` moves left because the span starts left of the caret; `yw` does not.
/// Same rule, and the asymmetry is the observable consequence.
#[test]
fn yb_moves_the_caret_and_yw_does_not() {
    let mut r = Register::default();

    let mut back = Line::new("foo bar", 4);
    let span = operated_span(
        &back.text,
        back.caret,
        Operator::Yank,
        Motion::WordStartPrev,
        1,
    )
    .unwrap();
    yank(&mut back, span, &mut r);
    assert_eq!(back.caret, 0, "`yb` lands at the start of what it took");

    let mut fwd = Line::new("foo bar", 0);
    let span = operated_span(
        &fwd.text,
        fwd.caret,
        Operator::Yank,
        Motion::WordStartNext,
        1,
    )
    .unwrap();
    yank(&mut fwd, span, &mut r);
    assert_eq!(fwd.caret, 0, "`yw` starts where the caret already was");
}

#[test]
fn p_pastes_after_and_capital_p_pastes_at_the_caret() {
    let mut r = Register::default();
    {
        let mut l = Line::new("xy", 0);
        let span = 0..1;
        take(&mut l, span, &mut r); // register := "x", text := "y"
        assert_eq!(r.text(), "x");
    }
    let mut a = Line::new("ab", 0);
    paste(&mut a, &r, true, 1);
    assert_eq!(a.text, "axb", "`p` goes after the char under the caret");

    let mut b = Line::new("ab", 0);
    paste(&mut b, &r, false, 1);
    assert_eq!(b.text, "xab", "`P` goes at the caret");
}

#[test]
fn a_counted_paste_repeats_the_register() {
    let mut l = Line::new("a", 0);
    let mut r = Register::default();
    take(&mut l, 0..1, &mut r);
    let mut t = Line::new("", 0);
    paste(&mut t, &r, false, 3);
    assert_eq!(t.text, "aaa");
}

// ── text objects ─────────────────────────────────────────────────────────

#[test]
fn iw_takes_the_run_and_aw_takes_its_trailing_whitespace() {
    let t = "foo  bar";
    assert_eq!(
        object_span(t, 1, TextObject::Word { around: false }),
        Some(0..3)
    );
    assert_eq!(
        object_span(t, 1, TextObject::Word { around: true }),
        Some(0..5),
        "`aw` takes the trailing blanks too",
    );
}

/// **The case that actually fires in a picker**: the caret is at the end of
/// the query, so there IS no trailing whitespace and `aw` must fall back to
/// the leading run. Without the fallback `daw` on the last word leaves a
/// stranded space.
#[test]
fn aw_falls_back_to_leading_whitespace_at_the_end_of_the_line() {
    let t = "foo bar";
    assert_eq!(
        object_span(t, 5, TextObject::Word { around: true }),
        Some(3..7),
        "no trailing blanks, so `aw` takes the leading one",
    );
}

#[test]
fn delimited_objects_handle_quotes_and_nesting() {
    assert_eq!(
        object_span(
            "say \"hi there\" ok",
            7,
            TextObject::Delimited {
                open: '"',
                close: '"',
                around: false
            }
        ),
        Some(5..13),
    );
    assert_eq!(
        object_span(
            "f(a(b)c)",
            4,
            TextObject::Delimited {
                open: '(',
                close: ')',
                around: false
            }
        ),
        Some(4..5),
        "the INNER pair, because the caret is inside it",
    );
    assert_eq!(
        object_span(
            "f(a(b)c)",
            2,
            TextObject::Delimited {
                open: '(',
                close: ')',
                around: true
            }
        ),
        Some(1..8),
        "`a(` from the outer level takes the parens too",
    );
}

#[test]
fn a_line_object_has_no_single_line_reading() {
    assert_eq!(
        object_span("anything", 0, TextObject::Line),
        None,
        "`dd` on one line is not a line delete — it must be refused, not \
         silently reinterpreted as clear-the-query",
    );
}

// ── motions with no single-line reading ──────────────────────────────────

/// A typed `None`, never a silent no-op: a caller must be able to tell "this
/// key does nothing here" from "this key did nothing".
#[test]
fn motions_without_a_single_line_reading_are_refused() {
    for m in [
        Motion::Up,
        Motion::Down,
        Motion::PageUp,
        Motion::HalfPageDown,
        Motion::GotoLine(3),
        Motion::SearchNext,
        Motion::ForwardSexp,
        Motion::BeginningOfDefun,
    ] {
        assert_eq!(resolve("foo bar", 0, m, 1), None, "{m:?} must be refused");
    }
}

// ── multi-byte safety ────────────────────────────────────────────────────

/// Every offset this crate returns must land on a `char` boundary, or the
/// first slice with it panics. A cluster-context name is ASCII, but a query
/// is whatever the operator typed.
#[test]
fn every_offset_lands_on_a_char_boundary() {
    let t = "héllo wörld ünïcode";
    for from in (0..=t.len()).filter(|i| t.is_char_boundary(*i)) {
        for w in [Width::Small, Width::Big] {
            for off in [
                word_start_next(t, from, w),
                word_start_prev(t, from, w),
                word_end_next(t, from, w),
            ] {
                assert!(
                    t.is_char_boundary(off),
                    "offset {off} from {from} ({w:?}) is mid-char",
                );
            }
        }
        for s in Stance::ALL {
            assert!(t.is_char_boundary(clamp(t, from, *s)));
        }
    }
}

#[test]
fn a_multi_byte_word_deletes_whole() {
    let mut l = Line::new("héllo wörld", 0);
    let mut r = Register::default();
    let span = operated_span(&l.text, 0, Operator::Delete, Motion::WordStartNext, 1).unwrap();
    take(&mut l, span, &mut r);
    assert_eq!(l.text, "wörld");
    assert_eq!(r.text(), "héllo ");
}

// ── empty + boundary inputs ──────────────────────────────────────────────

#[test]
fn an_empty_line_survives_every_motion() {
    for m in [
        Motion::Left,
        Motion::Right,
        Motion::WordStartNext,
        Motion::WordStartPrev,
        Motion::WordEndNext,
        Motion::LineStart,
        Motion::LineEnd,
    ] {
        assert_eq!(resolve("", 0, m, 1), Some(0), "{m:?} on an empty line");
    }
    assert_eq!(object_span("", 0, TextObject::Word { around: false }), None);
}

#[test]
fn dw_on_the_last_word_takes_all_of_it() {
    let t = "foo bar";
    assert_eq!(
        operated_span(t, 4, Operator::Delete, Motion::WordStartNext, 1),
        Some(4..7),
        "saturating at text.len() is what makes this delete rather than no-op",
    );
}

#[test]
fn the_stance_badge_is_stable() {
    assert_eq!(Stance::Normal.label(), "NORMAL");
    assert_eq!(Stance::Insert.label(), "INSERT");
    assert_eq!(
        Stance::default(),
        Stance::Normal,
        "a picker opens in NORMAL"
    );
}
