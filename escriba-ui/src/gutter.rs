//! The gutter — one definition, painted by every face.
//!
//! ## Why this is a model and not two `format!` calls
//!
//! The ratatui face composed its gutter inline (`format!("{:>4} │ ", ln+1)`)
//! and the GPU face had **no gutter at all** — no line numbers, no marks.
//! That is the same divergence the status line had before `StatusModel`, and
//! it appeared the same way: two faces each deciding independently what a
//! thing contains, with only one of them ever getting a new feature.
//!
//! So the gutter is cells, here, and a face's only job is to colour them.
//! Adding git signs later is one change in this file rather than two that
//! have to agree.

use escriba_shirube::Severity;

/// What a gutter cell MEANS. Roles, never colours — the chrome decides how a
/// role looks, exactly as it does for the splash and the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterRole {
    /// The line number.
    Number,
    /// A finding's severity mark.
    Mark(Severity),
    /// Empty space where a mark would go.
    NoMark,
    /// A debug breakpoint the operator set on this line.
    Breakpoint,
    /// Empty space where a breakpoint would go.
    NoBreakpoint,
    /// The rule between the gutter and the text.
    Separator,
}

/// Everything the gutter has to say about ONE line.
///
/// ## Why this is a struct and not a second `Option<Severity>` parameter
///
/// The gutter had exactly one mark cell and `gutter_cells` took exactly one
/// `Option<Severity>`. A breakpoint is not a severity — it is a thing the
/// OPERATOR put there, not a thing a producer found — so squeezing it into
/// that cell would mean a line carrying both an error and a breakpoint could
/// only show one of them, and whichever lost would be invisible with no
/// indication that anything had been hidden.
///
/// So each plane gets its own cell, and they arrive together in one value.
/// There is deliberately **no** `From<Option<Severity>>` and no `Default`:
/// every construction names both planes, which is what makes adding a third
/// (git hunks are next) a compile error at every call site rather than a
/// silently-omitted column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GutterMarks {
    /// The WORST finding severity on the line, or `None`.
    pub severity: Option<Severity>,
    /// Whether a debug breakpoint sits on the line.
    pub breakpoint: bool,
}

impl GutterMarks {
    /// Both planes, named.
    #[must_use]
    pub const fn new(severity: Option<Severity>, breakpoint: bool) -> Self {
        Self {
            severity,
            breakpoint,
        }
    }

    /// Nothing on this line. Spelled out rather than `Default` so a caller
    /// that means "I have not looked yet" cannot spell it the same way.
    #[must_use]
    pub const fn clear() -> Self {
        Self::new(None, false)
    }
}

/// One run of gutter text with its role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GutterCell {
    pub text: String,
    pub role: GutterRole,
}

/// The narrowest the gutter ever gets — `4` number + `1` space +
/// `1` breakpoint + `1` mark + `2` rule-and-space.
///
/// A FLOOR, not a fixed width, and the difference is load-bearing. The first
/// version of this file declared a constant 8 and a test immediately caught
/// why that cannot hold: line 10 000 needs five digits, so an ordinary large
/// file would have shifted its own text column by one — and on the GPU face,
/// where the reserved columns are computed from this number, the text would
/// have been painted straight over the line numbers.
pub const MIN_GUTTER_WIDTH: usize = 9;

/// The narrowest line-number field.
const MIN_NUMBER_WIDTH: usize = 4;

/// Everything in the gutter that is not the number: a space, the breakpoint
/// cell, the severity-mark cell, and the two-column rule.
///
/// The breakpoint cell is reserved on EVERY line of every buffer, including
/// buffers with no breakpoints at all. vim's `signcolumn=auto` grows the
/// column when the first sign appears, and the cost of that is the whole
/// file sliding one column sideways on the keystroke that sets a breakpoint —
/// which is the same defect a fixed-8 gutter had for 10 000-line files, just
/// triggered by a different event. One reserved column is cheaper than a
/// re-flow the operator did not ask for.
const FURNITURE_WIDTH: usize = 5;

/// How many columns the gutter needs for a buffer of `line_count` lines.
///
/// The invariant is **constant within a frame**, not constant forever. A file
/// that grows past 9 999 lines widening its gutter is what vim does and what
/// a reader expects; the text jumping sideways while they scroll is not.
/// Every face derives its geometry from this ONE function so they cannot
/// disagree about where a line starts.
#[must_use]
pub fn gutter_width(line_count: u32) -> usize {
    number_width(line_count) + FURNITURE_WIDTH
}

/// The line-number field width for a buffer of `line_count` lines.
#[must_use]
pub fn number_width(line_count: u32) -> usize {
    // `line_count` lines are numbered 1..=line_count, so the widest label is
    // `line_count` itself — not `line_count - 1`, and not `line_count + 1`.
    let digits = line_count.max(1).to_string().len();
    digits.max(MIN_NUMBER_WIDTH)
}

/// Compose the gutter for one line of a buffer with `line_count` lines.
///
/// `marks.severity` is the WORST severity on that line, or `None`. Callers
/// get the worst rather than a list because the gutter has exactly one cell
/// for it, and showing the last-arrived finding instead of the most serious
/// one is how an error hides behind a hint. `marks.breakpoint` gets its OWN
/// cell — see [`GutterMarks`] for why sharing one would hide half of it.
#[must_use]
pub fn gutter_cells(line: u32, marks: GutterMarks, line_count: u32) -> Vec<GutterCell> {
    let field = number_width(line_count);
    let mut number = (line + 1).to_string();
    if number.len() < field {
        number = " ".repeat(field - number.len()) + &number;
    }
    vec![
        GutterCell {
            text: number,
            role: GutterRole::Number,
        },
        GutterCell {
            text: " ".to_string(),
            role: GutterRole::Number,
        },
        // The breakpoint sits LEFT of the severity mark, so the mark stays
        // adjacent to the rule where every existing face and test already
        // reads it from.
        if marks.breakpoint {
            GutterCell {
                text: BREAKPOINT_GLYPH.to_string(),
                role: GutterRole::Breakpoint,
            }
        } else {
            GutterCell {
                text: " ".to_string(),
                role: GutterRole::NoBreakpoint,
            }
        },
        match marks.severity {
            Some(s) => GutterCell {
                text: mark_glyph(s).to_string(),
                role: GutterRole::Mark(s),
            },
            None => GutterCell {
                text: " ".to_string(),
                role: GutterRole::NoMark,
            },
        },
        GutterCell {
            text: "│ ".to_string(),
            role: GutterRole::Separator,
        },
    ]
}

/// The single-cell glyph for a severity.
///
/// Deliberately avoids `◆ ▸ ▮ ●`, which `ishou_tokens::EscribaSignals` already
/// uses for the modal pills and the modified indicator. One glyph meaning two
/// things is a reader's problem whichever meaning they learn first — and this
/// was found the hard way, by a gutter test failing on a `●` that belonged to
/// the status line.
#[must_use]
pub const fn mark_glyph(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "\u{2716}",   // ✖
        Severity::Warning => "\u{25b2}", // ▲
        Severity::Info => "\u{2022}",    // •
        Severity::Hint => "\u{203a}",    // ›
    }
}

/// The single-cell glyph for a breakpoint.
///
/// Chosen under the same two constraints [`mark_glyph`] documents — it must
/// collide with neither a severity mark (`✖ ▲ • ›`) nor an
/// `ishou_tokens::EscribaSignals` glyph (`◆ ▸ ▮ ●`), and both are gated by
/// tests below. `●` is the obvious debugger dot and is exactly the one that
/// is taken: it is the modified-buffer indicator on the status line, and a
/// reader who learns it there would read a breakpoint as "unsaved".
const BREAKPOINT_GLYPH: &str = "\u{25c9}"; // ◉

/// The glyph a breakpoint paints, for a face or a test that needs to NAME it.
#[must_use]
pub const fn breakpoint_glyph() -> &'static str {
    BREAKPOINT_GLYPH
}

/// The gutter as plain text — what the GPU face shapes and what tests read.
#[must_use]
pub fn gutter_text(line: u32, marks: GutterMarks, line_count: u32) -> String {
    gutter_cells(line, marks, line_count)
        .into_iter()
        .map(|c| c.text)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_of_one_buffer_gets_the_same_width() {
        // THE invariant. Not "the gutter is always 8" — that was the first
        // draft and it was false for any file past 9 999 lines. What must
        // hold is that within ONE buffer every line agrees, so neither a
        // diagnostic arriving nor scrolling into five-digit territory moves
        // the text column under the reader.
        for line_count in [1u32, 9, 10, 999, 1_000, 9_999, 10_000, 123_456] {
            let want = gutter_width(line_count);
            assert!(want >= MIN_GUTTER_WIDTH, "{line_count}: {want} below floor");
            for line in [0, line_count / 2, line_count.saturating_sub(1)] {
                for severity in [None, Some(Severity::Error), Some(Severity::Hint)] {
                    // Both breakpoint states, because a toggle that changed
                    // the width would slide the whole file sideways on one
                    // keypress — the same defect a fixed-8 gutter had, on a
                    // different trigger.
                    for breakpoint in [false, true] {
                        let marks = GutterMarks::new(severity, breakpoint);
                        let w = gutter_text(line, marks, line_count).chars().count();
                        assert_eq!(
                            w, want,
                            "buffer of {line_count} lines: line {line} marks \
                             {marks:?} rendered {w} columns, not {want}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_last_line_of_a_buffer_always_fits_its_field() {
        // The off-by-one that would break the invariant on exactly one line
        // of exactly the files where it is hardest to notice: a 10 000-line
        // buffer's last label is "10000", five digits. Sizing from
        // `line_count - 1` would reserve four and overflow on the final row.
        for line_count in [9u32, 10, 99, 100, 9_999, 10_000, 100_000] {
            let label = line_count.to_string();
            assert!(
                label.len() <= number_width(line_count),
                "a {line_count}-line buffer must fit the label {label:?}",
            );
            assert_eq!(
                gutter_text(line_count - 1, GutterMarks::clear(), line_count)
                    .chars()
                    .count(),
                gutter_width(line_count),
                "the LAST line must not be wider than every other one",
            );
        }
    }

    #[test]
    fn a_small_buffer_still_gets_the_floor() {
        // A 3-line file with a 1-column number field would look broken and
        // would re-flow the instant it grew. The floor is the same one vim
        // uses for the same reason.
        assert_eq!(gutter_width(3), MIN_GUTTER_WIDTH);
        assert_eq!(gutter_width(9_999), MIN_GUTTER_WIDTH);
        assert_eq!(gutter_width(10_000), MIN_GUTTER_WIDTH + 1);
    }

    #[test]
    fn every_severity_has_a_distinct_glyph() {
        let all = [
            Severity::Error,
            Severity::Warning,
            Severity::Info,
            Severity::Hint,
        ];
        let mut seen = std::collections::BTreeSet::new();
        for s in all {
            assert!(seen.insert(mark_glyph(s)), "{s:?} duplicates another mark");
        }
    }

    #[test]
    fn no_mark_collides_with_a_fleet_signal() {
        // `◆ ▸ ▮ ●` belong to the modal pills and the modified indicator.
        // Reusing one would make the gutter say something the status line
        // already says differently.
        let fleet = ['\u{25c6}', '\u{25b8}', '\u{25ae}', '\u{25cf}'];
        for s in [
            Severity::Error,
            Severity::Warning,
            Severity::Info,
            Severity::Hint,
        ] {
            let g = mark_glyph(s).chars().next().expect("one glyph");
            assert!(!fleet.contains(&g), "{s:?} reuses a fleet signal glyph");
        }
    }

    #[test]
    fn the_mark_sits_between_the_number_and_the_rule() {
        // Position matters: a mark after the separator would be inside the
        // text column and would look like buffer content.
        let cells = gutter_cells(0, GutterMarks::new(Some(Severity::Error), false), 40);
        let roles: Vec<GutterRole> = cells.iter().map(|c| c.role).collect();
        assert_eq!(roles[0], GutterRole::Number);
        assert_eq!(roles[2], GutterRole::NoBreakpoint);
        assert_eq!(roles[3], GutterRole::Mark(Severity::Error));
        assert_eq!(roles[4], GutterRole::Separator);
    }

    #[test]
    fn a_breakpoint_and_an_error_on_one_line_are_both_visible() {
        // THE reason the signature widened. Before `GutterMarks` there was
        // one mark cell, so a breakpoint published through it would have
        // replaced the error glyph — or been replaced by it — and the
        // operator would have seen exactly one of two true facts with no
        // indication that the other had been dropped.
        //
        // RED RUN (2026-08-12): reverting `gutter_cells` to emit one shared
        // cell (`if marks.breakpoint { breakpoint } else { severity }`) fails
        // this test on the `✖` assertion — the error is gone from the frame —
        // while `every_line_of_one_buffer_gets_the_same_width` stays green,
        // which is exactly why the width test could not have caught this.
        let text = gutter_text(0, GutterMarks::new(Some(Severity::Error), true), 40);
        assert!(
            text.contains(breakpoint_glyph()),
            "the breakpoint must be painted: {text:?}",
        );
        assert!(
            text.contains(mark_glyph(Severity::Error)),
            "and the error must STILL be painted: {text:?}",
        );
    }

    #[test]
    fn the_breakpoint_glyph_collides_with_nothing_the_reader_already_knows() {
        // Two vocabularies, both already spoken in this gutter and the status
        // line beside it. `●` (U+25CF) is the tempting debugger dot and is
        // taken by the modified-buffer indicator.
        let g = breakpoint_glyph().chars().next().expect("one glyph");
        for s in [
            Severity::Error,
            Severity::Warning,
            Severity::Info,
            Severity::Hint,
        ] {
            assert_ne!(
                breakpoint_glyph(),
                mark_glyph(s),
                "the breakpoint reuses {s:?}'s mark",
            );
        }
        let fleet = ['\u{25c6}', '\u{25b8}', '\u{25ae}', '\u{25cf}'];
        assert!(
            !fleet.contains(&g),
            "the breakpoint reuses a fleet signal glyph",
        );
        assert_eq!(
            breakpoint_glyph().chars().count(),
            1,
            "the breakpoint cell is ONE column",
        );
    }

    #[test]
    fn a_line_with_no_breakpoint_still_reserves_its_column() {
        // The always-reserved cell, stated as a property rather than left to
        // the width test: a blank breakpoint cell is a SPACE with its own
        // role, not an absent cell. A face that skipped emitting it would
        // paint a gutter one column narrower than `gutter_width` declares,
        // and the text would start inside the rule.
        let cells = gutter_cells(0, GutterMarks::clear(), 40);
        assert_eq!(cells[2].role, GutterRole::NoBreakpoint);
        assert_eq!(cells[2].text, " ");
        assert_eq!(
            cells.iter().map(|c| c.text.chars().count()).sum::<usize>(),
            gutter_width(40),
        );
    }
}
