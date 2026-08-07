//! The first producer: markers scanned out of buffer text.
//!
//! `TODO`, `FIXME`, `HACK`, `XXX`, `NOTE`. Chosen to go first deliberately —
//! it needs no language server, no subprocess and no async host, so it proves
//! the whole path (produce → seal → navigate → paint) end to end while the
//! courier is still four phases away.

use escriba_core::{BufferId, Position, Range};

use crate::finding::{Finding, Origin, Severity, Site};

/// A marker worth navigating to, and how much it matters.
///
/// A table rather than a regex: the set is small, closed, and each entry
/// carries a severity that a pattern could not. `FIXME` is not a `NOTE`.
const MARKERS: &[(&str, Severity)] = &[
    ("FIXME", Severity::Warning),
    ("XXX", Severity::Warning),
    ("HACK", Severity::Warning),
    ("TODO", Severity::Info),
    ("NOTE", Severity::Hint),
    ("SAFETY", Severity::Hint),
];

/// Scan `text` for markers, one finding per occurrence.
///
/// Matches are case-SENSITIVE and word-ish: `TODO` counts, `todos` does not.
/// A case-insensitive scan turns every "important note" in prose into a
/// finding, and a list that cries wolf is one people stop reading.
///
/// The line is what matters for navigation, so the range covers the marker
/// itself and the message is the rest of the line — which is what a reader
/// wants to see in a list without opening the file.
#[must_use]
pub fn scan_markers(buffer: BufferId, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let Ok(line_no) = u32::try_from(line_no) else {
            break;
        };
        for (marker, severity) in MARKERS {
            let Some(col) = find_word(line, marker) else {
                continue;
            };
            let start = Position::new(line_no, u32::try_from(col).unwrap_or(u32::MAX));
            let end = Position::new(
                line_no,
                u32::try_from(col + marker.len()).unwrap_or(u32::MAX),
            );
            let message = message_after(line, col + marker.len());
            out.push(Finding::new(
                Site::in_buffer(buffer, Range::new(start, end)),
                *severity,
                if message.is_empty() {
                    (*marker).to_string()
                } else {
                    let mut m = String::with_capacity(marker.len() + message.len() + 2);
                    m.push_str(marker);
                    m.push_str(": ");
                    m.push_str(&message);
                    m
                },
                Origin::Text("marker"),
            ));
            // One finding per line: a line saying "TODO: fix the HACK" is one
            // thing to do, and two gutter marks on it would be noise.
            break;
        }
    }
    out
}

/// The column of `word` in `line`, if it appears with non-word characters on
/// both sides.
///
/// Char-indexed, not byte-indexed: a `Position` column is a CHARACTER offset
/// everywhere else in escriba, and returning a byte offset here would put the
/// gutter mark in the wrong place on any line containing a multibyte glyph —
/// which, for a comment marker, is most of them.
fn find_word(line: &str, word: &str) -> Option<usize> {
    let chars: Vec<char> = line.chars().collect();
    let target: Vec<char> = word.chars().collect();
    if target.is_empty() || chars.len() < target.len() {
        return None;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    for start in 0..=(chars.len() - target.len()) {
        if chars[start..start + target.len()] != target[..] {
            continue;
        }
        let before_ok = start == 0 || !is_word(chars[start - 1]);
        let after = start + target.len();
        let after_ok = after >= chars.len() || !is_word(chars[after]);
        if before_ok && after_ok {
            return Some(start);
        }
    }
    None
}

/// The human part of the line after the marker, with punctuation and comment
/// noise trimmed.
fn message_after(line: &str, from_char: usize) -> String {
    line.chars()
        .skip(from_char)
        .collect::<String>()
        .trim_start_matches([':', '(', ')', '-', ' ', '\t'])
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: BufferId = BufferId(1);

    #[test]
    fn markers_are_found_with_their_message() {
        let f = scan_markers(B, "// TODO: wire the picker\nlet x = 1;\n");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].site.line(), 0);
        assert_eq!(f[0].message, "TODO: wire the picker");
        assert_eq!(f[0].severity, Severity::Info);
    }

    #[test]
    fn severity_distinguishes_a_fixme_from_a_note() {
        // A table rather than a regex, precisely so this is expressible.
        let f = scan_markers(B, "// FIXME broken\n// NOTE background\n");
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[1].severity, Severity::Hint);
    }

    #[test]
    fn matching_is_case_sensitive_and_word_bounded() {
        // A list that cries wolf is one people stop reading. Prose about
        // "todos" and identifiers like `TODOS` must not become findings.
        assert!(scan_markers(B, "// todo lowercase\n").is_empty());
        assert!(scan_markers(B, "let TODOS = 1;\n").is_empty());
        assert!(scan_markers(B, "let x_TODO_y = 1;\n").is_empty());
        assert_eq!(scan_markers(B, "// TODO\n").len(), 1, "bare marker counts");
        assert_eq!(scan_markers(B, "(TODO)\n").len(), 1, "punctuation is fine");
    }

    #[test]
    fn the_column_counts_characters_not_bytes() {
        // A Position column is a CHARACTER offset everywhere else in escriba.
        // A byte offset would put the gutter mark in the wrong place on any
        // line with a multibyte glyph — which for comments is most of them.
        let f = scan_markers(B, "// ünïcödé TODO: here\n");
        assert_eq!(f.len(), 1);
        let text = "// ünïcödé TODO: here";
        let want = text
            .chars()
            .collect::<Vec<_>>()
            .windows(4)
            .position(|w| w.iter().collect::<String>() == "TODO")
            .expect("TODO is there");
        assert_eq!(
            f[0].site.range.start.column as usize, want,
            "column must be a char offset",
        );
    }

    #[test]
    fn one_finding_per_line() {
        // "TODO: fix the HACK" is one thing to do; two gutter marks on one
        // line would be noise.
        let f = scan_markers(B, "// TODO: fix the HACK here\n");
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn an_empty_buffer_produces_nothing_rather_than_panicking() {
        assert!(scan_markers(B, "").is_empty());
        assert!(scan_markers(B, "\n\n\n").is_empty());
    }

    #[test]
    fn findings_arrive_in_line_order() {
        let f = scan_markers(B, "a\n// TODO one\nb\n// FIXME two\n");
        assert_eq!(
            f.iter().map(|x| x.site.line()).collect::<Vec<_>>(),
            vec![1, 3],
        );
    }
}
