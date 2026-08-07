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
    /// The rule between the gutter and the text.
    Separator,
}

/// One run of gutter text with its role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GutterCell {
    pub text: String,
    pub role: GutterRole,
}

/// The narrowest the gutter ever gets — `4` number + `1` space + `1` mark +
/// `2` rule-and-space.
///
/// A FLOOR, not a fixed width, and the difference is load-bearing. The first
/// version of this file declared a constant 8 and a test immediately caught
/// why that cannot hold: line 10 000 needs five digits, so an ordinary large
/// file would have shifted its own text column by one — and on the GPU face,
/// where the reserved columns are computed from this number, the text would
/// have been painted straight over the line numbers.
pub const MIN_GUTTER_WIDTH: usize = 8;

/// The narrowest line-number field.
const MIN_NUMBER_WIDTH: usize = 4;

/// Everything in the gutter that is not the number: a space, the mark cell,
/// and the two-column rule.
const FURNITURE_WIDTH: usize = 4;

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
/// `mark` is the WORST severity on that line, or `None`. Callers get the
/// worst rather than a list because the gutter has exactly one cell for it,
/// and showing the last-arrived finding instead of the most serious one is
/// how an error hides behind a hint.
#[must_use]
pub fn gutter_cells(line: u32, mark: Option<Severity>, line_count: u32) -> Vec<GutterCell> {
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
        match mark {
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

/// The gutter as plain text — what the GPU face shapes and what tests read.
#[must_use]
pub fn gutter_text(line: u32, mark: Option<Severity>, line_count: u32) -> String {
    gutter_cells(line, mark, line_count)
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
                for mark in [None, Some(Severity::Error), Some(Severity::Hint)] {
                    let w = gutter_text(line, mark, line_count).chars().count();
                    assert_eq!(
                        w, want,
                        "buffer of {line_count} lines: line {line} mark \
                         {mark:?} rendered {w} columns, not {want}",
                    );
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
                gutter_text(line_count - 1, None, line_count).chars().count(),
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
        let cells = gutter_cells(0, Some(Severity::Error), 40);
        let roles: Vec<GutterRole> = cells.iter().map(|c| c.role).collect();
        assert_eq!(roles[0], GutterRole::Number);
        assert_eq!(roles[2], GutterRole::Mark(Severity::Error));
        assert_eq!(roles[3], GutterRole::Separator);
    }
}
