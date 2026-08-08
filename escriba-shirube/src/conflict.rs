//! Merge-conflict regions — a producer that needs no git at all.
//!
//! A conflict is text a merge tool wrote INTO the buffer:
//!
//! ```text
//! <<<<<<< HEAD
//! ours
//! =======
//! theirs
//! >>>>>>> branch
//! ```
//!
//! Resolving one is an edit over lines that are already open. The `git.*`
//! verbs need a git layer; these five do not, which is why they land first
//! among the version-control actions — the whole cluster was tiered as
//! "needs git" and only half of it does.
//!
//! Navigation rides `shirube` like every other located finding: a conflict IS
//! a finding with a range, so `]x` and `[x` are the same walk as `]t`.

use escriba_core::{BufferId, Position, Range};

use crate::finding::{Finding, Origin, Severity, Site};

/// The three markers, exactly as git writes them.
///
/// Seven characters, and the count matters: git uses exactly seven, and a
/// line of six or eight dashes is ordinary text — a diff of a diff, or a
/// comment rule. Matching loosely turns documentation into conflicts.
const OURS: &str = "<<<<<<<";
const BOTH: &str = "=======";
const THEIRS: &str = ">>>>>>>";

/// One conflict region, by LINE.
///
/// Lines rather than a char range because every resolution is line-wise:
/// there is no such thing as keeping half of "ours".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conflict {
    /// The `<<<<<<<` line.
    pub start: u32,
    /// The `=======` line.
    pub middle: u32,
    /// The `>>>>>>>` line.
    pub end: u32,
}

impl Conflict {
    /// Does this region contain `line`?
    #[must_use]
    pub const fn contains(&self, line: u32) -> bool {
        line >= self.start && line <= self.end
    }

    /// The half-open line range `[start, end + 1)` the resolution replaces.
    #[must_use]
    pub const fn lines(&self) -> (u32, u32) {
        (self.start, self.end + 1)
    }
}

/// Which side to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Everything between `<<<<<<<` and `=======`.
    Ours,
    /// Everything between `=======` and `>>>>>>>`.
    Theirs,
    /// Both halves, markers removed, ours first.
    Both,
}

/// Every conflict region in `text`, in line order.
///
/// A region is only reported when all three markers appear in order. A file
/// containing `<<<<<<<` with no closing marker is not a conflict — it is a
/// file that mentions the characters, and reporting it would put a gutter
/// mark on prose about merge conflicts. This module's own documentation is
/// the example.
#[must_use]
pub fn scan(text: &str) -> Vec<Conflict> {
    let mut out = Vec::new();
    let mut start: Option<u32> = None;
    let mut middle: Option<u32> = None;
    for (n, line) in text.lines().enumerate() {
        let Ok(n) = u32::try_from(n) else { break };
        if line.starts_with(OURS) {
            // A second `<<<<<<<` before a close abandons the first: the
            // nearest opener is the one that pairs.
            start = Some(n);
            middle = None;
        } else if line.starts_with(BOTH) && start.is_some() {
            middle = Some(n);
        } else if line.starts_with(THEIRS) {
            if let (Some(s), Some(m)) = (start, middle) {
                out.push(Conflict {
                    start: s,
                    middle: m,
                    end: n,
                });
            }
            start = None;
            middle = None;
        }
    }
    out
}

/// The conflict containing `line`, if any.
#[must_use]
pub fn at(text: &str, line: u32) -> Option<Conflict> {
    scan(text).into_iter().find(|c| c.contains(line))
}

/// The replacement text for resolving `c` in favour of `side`.
///
/// Returns the lines to substitute for `[start, end + 1)`, newline-terminated
/// so the caller replaces whole lines and the file keeps its shape.
#[must_use]
pub fn resolution(text: &str, c: Conflict, side: Side) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let take = |from: u32, to: u32| -> String {
        lines
            .iter()
            .skip(from as usize)
            .take((to.saturating_sub(from)) as usize)
            .fold(String::new(), |mut acc, l| {
                acc.push_str(l);
                acc.push('\n');
                acc
            })
    };
    match side {
        Side::Ours => take(c.start + 1, c.middle),
        Side::Theirs => take(c.middle + 1, c.end),
        // Ours first, then theirs — the order they appear in the file, which
        // is the order a reader expects to find them in afterwards.
        Side::Both => {
            let mut s = take(c.start + 1, c.middle);
            s.push_str(&take(c.middle + 1, c.end));
            s
        }
    }
}

/// Conflicts as located findings, for `]x` / `[x`.
///
/// `Severity::Error` because an unresolved conflict is not a suggestion —
/// the file does not compile, parse, or run while it is there.
#[must_use]
pub fn findings(buffer: BufferId, text: &str) -> Vec<Finding> {
    scan(text)
        .into_iter()
        .map(|c| {
            Finding::new(
                Site::in_buffer(
                    buffer,
                    Range::new(Position::new(c.start, 0), Position::new(c.start, 1)),
                ),
                Severity::Error,
                "merge conflict".to_string(),
                Origin::Text("conflict"),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
a
<<<<<<< HEAD
ours line 1
ours line 2
=======
theirs line
>>>>>>> branch
z
";

    #[test]
    fn a_complete_conflict_is_found() {
        let c = scan(SAMPLE);
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0],
            Conflict {
                start: 1,
                middle: 4,
                end: 6
            }
        );
    }

    #[test]
    fn an_unclosed_marker_is_not_a_conflict() {
        // THE false-positive guard. This module's own doc comment contains
        // the markers; a scanner that reported them would put an Error mark
        // on prose about merge conflicts.
        assert!(scan("<<<<<<< HEAD\nours\n").is_empty());
        assert!(scan("=======\n").is_empty());
        assert!(scan(">>>>>>> branch\n").is_empty());
    }

    #[test]
    fn markers_must_be_exactly_seven_characters() {
        // Six is a diff of a diff; eight is a comment rule. git writes seven.
        assert!(scan("<<<<<< H\nx\n======\ny\n>>>>>> B\n").is_empty());
    }

    #[test]
    fn choosing_ours_keeps_only_the_first_half() {
        let c = scan(SAMPLE)[0];
        assert_eq!(
            resolution(SAMPLE, c, Side::Ours),
            "ours line 1\nours line 2\n"
        );
    }

    #[test]
    fn choosing_theirs_keeps_only_the_second_half() {
        let c = scan(SAMPLE)[0];
        assert_eq!(resolution(SAMPLE, c, Side::Theirs), "theirs line\n");
    }

    #[test]
    fn choosing_both_keeps_ours_then_theirs_with_no_markers() {
        let c = scan(SAMPLE)[0];
        let got = resolution(SAMPLE, c, Side::Both);
        assert_eq!(got, "ours line 1\nours line 2\ntheirs line\n");
        assert!(!got.contains('<') && !got.contains('>') && !got.contains("======"));
    }

    #[test]
    fn an_empty_side_resolves_to_nothing_rather_than_a_marker() {
        // A conflict where one side deleted everything. Keeping that side
        // must remove the region, not leave a stray marker behind.
        let text = "<<<<<<< HEAD\n=======\ntheirs\n>>>>>>> b\n";
        let c = scan(text)[0];
        assert_eq!(resolution(text, c, Side::Ours), "");
    }

    #[test]
    fn the_conflict_at_the_cursor_is_the_one_containing_it() {
        let c = at(SAMPLE, 3).expect("line 3 is inside");
        assert_eq!(c.start, 1);
        assert!(at(SAMPLE, 0).is_none(), "line 0 is before it");
        assert!(at(SAMPLE, 7).is_none(), "line 7 is after it");
    }

    #[test]
    fn several_conflicts_are_all_found_in_order() {
        let two = format!("{SAMPLE}{SAMPLE}");
        let c = scan(&two);
        assert_eq!(c.len(), 2);
        assert!(c[0].start < c[1].start);
    }

    #[test]
    fn conflicts_become_error_findings() {
        let f = findings(BufferId(1), SAMPLE);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Error, "the file does not build");
        assert_eq!(f[0].site.line(), 1, "marked at the opening marker");
    }
}
