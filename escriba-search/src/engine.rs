//! Finding matches, and stepping between them.
//!
//! # Offsets
//!
//! The `regex` crate reports **byte** offsets; escriba's buffer addresses text
//! in **char** offsets (`Buffer::char_to_position`). Mixing the two is silently
//! correct for ASCII and silently wrong the moment a document contains a
//! non-ASCII character — the classic bug that survives every test written in
//! English. So the conversion happens exactly once, here, at the boundary, and
//! [`SearchMatch`] is char-offset by construction. Nothing downstream ever sees
//! a byte offset.

use crate::pattern::SearchPattern;
use escriba_memori::Bound;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Which way a search runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub enum Direction {
    /// `/` — toward the end of the buffer.
    #[default]
    Forward,
    /// `?` — toward the start of the buffer.
    Backward,
}

impl Direction {
    /// The opposite direction — what `N` does to `n`.
    #[must_use]
    pub const fn reversed(self) -> Self {
        match self {
            Self::Forward => Self::Backward,
            Self::Backward => Self::Forward,
        }
    }
}

/// One match, in **char** offsets, half-open `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SearchMatch {
    pub start: usize,
    pub end: usize,
}

impl SearchMatch {
    /// Char length of the match. Zero for a zero-width match (`/x*`).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// Whether this is a zero-width match.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Whether `offset` falls inside this match — what the renderer asks when
    /// deciding to highlight a cell. Zero-width matches contain nothing, so
    /// they never highlight.
    #[must_use]
    pub const fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }
}

/// Whether stepping to a match ran off the end and came back around. vim
/// reports this ("search hit BOTTOM, continuing at TOP") and so do we — a wrap
/// that happens silently is how you lose your place in a large file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrapped {
    No,
    /// Ran past the end, resumed at the top.
    AtBottom,
    /// Ran past the start, resumed at the bottom.
    AtTop,
}

impl Wrapped {
    /// The message vim prints, or `None` when nothing wrapped.
    #[must_use]
    pub const fn message(self) -> Option<&'static str> {
        match self {
            Self::No => None,
            Self::AtBottom => Some("search hit BOTTOM, continuing at TOP"),
            Self::AtTop => Some("search hit TOP, continuing at BOTTOM"),
        }
    }
}

/// Where a step landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub target: SearchMatch,
    /// Index of `target` within the full match list — drives vim's `[3/17]`
    /// match-count display.
    pub index: usize,
    pub wrapped: Wrapped,
}

/// Every match of `pattern` in `text`, in ascending order, non-overlapping.
///
/// Returns char offsets. An empty result means "no matches", which callers
/// should surface as vim's `E486: Pattern not found` rather than treating as a
/// no-op.
#[must_use]
pub fn find_all(text: &str, pattern: &SearchPattern) -> Vec<SearchMatch> {
    // One pass to build the byte->char map, so the whole conversion is O(n)
    // rather than O(n*m) from repeated `text[..b].chars().count()` calls. On a
    // large buffer with many matches that difference is the whole frame budget.
    let mut byte_to_char = vec![0usize; text.len() + 1];
    for (char_idx, (byte_idx, _)) in text.char_indices().enumerate() {
        byte_to_char[byte_idx] = char_idx;
    }
    byte_to_char[text.len()] = text.chars().count();
    // Interior bytes of a multi-byte char are never match boundaries (regex
    // only reports char-aligned offsets), so leaving them 0 is safe — but fill
    // them forward anyway so a future caller cannot trip over a stale zero.
    let mut last = 0;
    for slot in &mut byte_to_char {
        if *slot == 0 && last != 0 {
            *slot = last;
        } else {
            last = *slot;
        }
    }

    pattern
        .regex()
        .find_iter(text)
        .map(|m| SearchMatch {
            start: byte_to_char[m.start()],
            end: byte_to_char[m.end()],
        })
        .collect()
}

/// Step from `from` (a char offset, normally the cursor) to the next match in
/// `direction`, wrapping around the buffer.
///
/// vim semantics, deliberately:
/// - Forward finds the first match starting **strictly after** `from`, so `n`
///   on top of a match advances instead of re-finding the same one.
/// - Backward finds the last match starting **strictly before** `from`.
/// - With no match ahead, it wraps and reports [`Wrapped`].
/// - A single match always resolves to itself, reporting a wrap.
#[must_use]
pub fn step(matches: &[SearchMatch], from: usize, direction: Direction) -> Option<Step> {
    step_bounded(matches, from, direction, Bound::Exclusive)
}

/// Like [`step`], but a match starting exactly **at** `from` counts as a hit.
///
/// This is what incremental search needs and `n` must not have. Typing `/foo`
/// while the cursor already sits on a `foo` should light up *that* `foo`;
/// pressing `n` on the same `foo` must move to the next one.
///
/// `from.saturating_sub(1)` is not a substitute: at offset 0 it saturates back
/// to 0, so a match at 0 stays unreachable — the exact case that fails on the
/// first line of a file.
#[must_use]
pub fn step_inclusive(matches: &[SearchMatch], from: usize, direction: Direction) -> Option<Step> {
    step_bounded(matches, from, direction, Bound::Inclusive)
}

/// Step to the next match, with the endpoint rule stated rather than chosen by
/// picking a function name.
///
/// [`step`] and [`step_inclusive`] are now one-line delegations to this. They
/// stay because they are published API with their own tests (★★ MODULARIZE,
/// DON'T DELETE) and because `step`/`step_inclusive` read better at a call site
/// that has no other reason to name a bound — but there is exactly ONE
/// implementation, so the twins can no longer drift apart.
///
/// The difference between them used to be a single `>` versus `>=` duplicated
/// across two nearly identical function bodies, which is precisely the shape
/// that drifts. `memori::Bound` owns that comparison now, and
/// `Bound::first_matching` contains no subtraction — so the "back up one to
/// include the anchor" trick that made offset 0 unreachable has nowhere left
/// to live.
#[must_use]
pub fn step_bounded(
    matches: &[SearchMatch],
    from: usize,
    direction: Direction,
    bound: Bound,
) -> Option<Step> {
    if matches.is_empty() {
        return None;
    }
    let starts: Vec<usize> = matches.iter().map(|m| m.start).collect();
    let forward = matches!(direction, Direction::Forward);

    match bound.first_matching(&starts, from, forward) {
        Some(i) => Some(Step {
            target: matches[i],
            index: i,
            wrapped: Wrapped::No,
        }),
        // Nothing ahead: wrap to the far end and SAY so. Wrapping silently is
        // how a user loses track of where they are in a long file.
        None if forward => Some(Step {
            target: matches[0],
            index: 0,
            wrapped: Wrapped::AtBottom,
        }),
        None => {
            let i = matches.len() - 1;
            Some(Step {
                target: matches[i],
                index: i,
                wrapped: Wrapped::AtTop,
            })
        }
    }
}

#[must_use]
pub fn word_at(text: &str, cursor: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';

    // Scan forward for a word char, stopping at the line end like vim does.
    let mut i = cursor.min(chars.len().saturating_sub(1));
    while i < chars.len() && !is_word(chars[i]) {
        if chars[i] == '\n' {
            return None;
        }
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }
    let mut start = i;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = i;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    Some(chars[start..end].iter().collect())
}

/// Vim's default `maxcount`. Counting is bounded so one keystroke on a large
/// buffer cannot become a full scan the user waits on.
pub const MAX_COUNT: usize = 99;

/// A match-count display — vim's `[3/17]`.
///
/// Both halves were already computed and both were thrown away: the numerator
/// is [`Step::index`], the denominator is the length of [`find_all`]'s result.
/// This type is what finally carries them to a status line.
///
/// The denominator is the load-bearing half. `[1/1]` says a rename is safe;
/// `[1/240]` says narrow the pattern first. Without it the only way to learn
/// how many matches exist is to press `n` until the view looks familiar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchCount {
    /// No pattern armed and no prompt previewing — render nothing.
    Idle,
    /// A pattern exists but matches nothing. Renders `[0/0]`, which reports a
    /// bad pattern while it is still being typed rather than after Enter.
    None,
    /// `[current/total]`, both exact. `current` is 1-based for display.
    Exact { current: usize, total: usize },
    /// More matches than [`MAX_COUNT`]; the ordinal is still exact.
    Capped { current: usize },
}

impl MatchCount {
    /// Build a count from a 0-based match index and a total.
    ///
    /// `index` is [`Step::index`]; `total` is `matches.len()`. An out-of-range
    /// index yields [`MatchCount::None`] rather than a wrong ordinal — a count
    /// that lies is worse than one that declines to answer.
    #[must_use]
    pub const fn new(index: usize, total: usize) -> Self {
        if total == 0 || index >= total {
            return Self::None;
        }
        if total > MAX_COUNT {
            return Self::Capped { current: index + 1 };
        }
        Self::Exact {
            current: index + 1,
            total,
        }
    }

    /// Is there anything to draw?
    #[must_use]
    pub const fn is_idle(self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Append the display form to `out`.
    ///
    /// Written with `push_str` rather than `format!` — the fleet's ★★ TYPED
    /// EMISSION rule, and the same reason `pattern.rs::format_word_boundary`
    /// is hand-built.
    pub fn render_into(self, out: &mut String) {
        match self {
            Self::Idle => {}
            Self::None => out.push_str("[0/0]"),
            Self::Exact { current, total } => {
                out.push('[');
                push_usize(out, current);
                out.push('/');
                push_usize(out, total);
                out.push(']');
            }
            Self::Capped { current } => {
                out.push('[');
                push_usize(out, current);
                out.push_str("/>");
                push_usize(out, MAX_COUNT);
                out.push(']');
            }
        }
    }
}

/// Decimal-append a `usize` without `format!`.
fn push_usize(out: &mut String, mut n: usize) {
    if n == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + u8::try_from(n % 10).unwrap_or(0);
        n /= 10;
    }
    // Every byte written is an ASCII digit, so this slice is valid UTF-8.
    out.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::CaseMode;

    fn pat(p: &str) -> SearchPattern {
        SearchPattern::compile(p, CaseMode::Sensitive).unwrap()
    }

    #[test]
    fn finds_every_occurrence_in_order() {
        let m = find_all("foo bar foo baz foo", &pat("foo"));
        assert_eq!(m.len(), 3);
        assert_eq!(m[0], SearchMatch { start: 0, end: 3 });
        assert_eq!(m[1], SearchMatch { start: 8, end: 11 });
        assert_eq!(m[2], SearchMatch { start: 16, end: 19 });
    }

    #[test]
    fn offsets_are_chars_not_bytes() {
        // "héllo foo" — é is 2 bytes, so a byte-offset bug puts `foo` at 7.
        let text = "héllo foo";
        let m = find_all(text, &pat("foo"));
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].start, 6, "char offset; byte offset would be 7");
        // Prove it by slicing the way the buffer would.
        let got: String = text.chars().skip(m[0].start).take(m[0].len()).collect();
        assert_eq!(got, "foo");
    }

    #[test]
    fn multibyte_heavy_text_stays_aligned() {
        let text = "日本語 foo 日本語 foo";
        let m = find_all(text, &pat("foo"));
        assert_eq!(m.len(), 2);
        for mm in &m {
            let got: String = text.chars().skip(mm.start).take(mm.len()).collect();
            assert_eq!(got, "foo");
        }
    }

    #[test]
    fn no_matches_is_empty_not_a_panic() {
        assert!(find_all("abc", &pat("zzz")).is_empty());
        assert!(step(&[], 0, Direction::Forward).is_none());
    }

    #[test]
    fn forward_advances_past_a_match_the_cursor_sits_on() {
        let m = find_all("foo foo foo", &pat("foo"));
        // Cursor at 0 is ON the first match; `n` must go to the second.
        let s = step(&m, 0, Direction::Forward).unwrap();
        assert_eq!(s.target.start, 4);
        assert_eq!(s.index, 1);
        assert_eq!(s.wrapped, Wrapped::No);
    }

    #[test]
    fn forward_wraps_at_the_bottom_and_says_so() {
        let m = find_all("foo foo", &pat("foo"));
        let s = step(&m, 100, Direction::Forward).unwrap();
        assert_eq!(s.target.start, 0);
        assert_eq!(s.wrapped, Wrapped::AtBottom);
        assert!(s.wrapped.message().unwrap().contains("BOTTOM"));
    }

    #[test]
    fn backward_finds_the_previous_match() {
        let m = find_all("foo foo foo", &pat("foo"));
        let s = step(&m, 8, Direction::Backward).unwrap();
        assert_eq!(s.target.start, 4);
        assert_eq!(s.wrapped, Wrapped::No);
    }

    #[test]
    fn backward_wraps_at_the_top_and_says_so() {
        let m = find_all("foo foo", &pat("foo"));
        let s = step(&m, 0, Direction::Backward).unwrap();
        assert_eq!(s.target.start, 4);
        assert_eq!(s.wrapped, Wrapped::AtTop);
        assert!(s.wrapped.message().unwrap().contains("TOP"));
    }

    #[test]
    fn a_lone_match_resolves_to_itself_by_wrapping() {
        let m = find_all("hello foo world", &pat("foo"));
        assert_eq!(m.len(), 1);
        for dir in [Direction::Forward, Direction::Backward] {
            let s = step(&m, m[0].start, dir).unwrap();
            assert_eq!(
                s.target, m[0],
                "single match must resolve to itself ({dir:?})"
            );
            assert_ne!(s.wrapped, Wrapped::No, "and must report the wrap");
        }
    }

    #[test]
    fn step_inclusive_finds_a_match_starting_at_the_cursor() {
        let m = find_all("foo foo foo", &pat("foo"));
        // The distinction that matters: exclusive `step` skips the match under
        // the cursor (correct for `n`), inclusive keeps it (correct for
        // incremental search).
        assert_eq!(step(&m, 0, Direction::Forward).unwrap().target.start, 4);
        assert_eq!(
            step_inclusive(&m, 0, Direction::Forward)
                .unwrap()
                .target
                .start,
            0
        );
    }

    #[test]
    fn step_inclusive_at_offset_zero_is_reachable() {
        // Regression: the original preview used `from.saturating_sub(1)`, which
        // saturates to 0 and therefore could never reach a match AT 0 — broken
        // precisely on the first line of a file.
        let m = find_all("foo bar", &pat("foo"));
        let s = step_inclusive(&m, 0, Direction::Forward).unwrap();
        assert_eq!(s.target.start, 0);
        assert_eq!(
            s.wrapped,
            Wrapped::No,
            "reaching it must not count as a wrap"
        );
    }

    #[test]
    fn step_inclusive_backward_also_accepts_the_cursor_position() {
        let m = find_all("foo foo foo", &pat("foo"));
        assert_eq!(step(&m, 8, Direction::Backward).unwrap().target.start, 4);
        assert_eq!(
            step_inclusive(&m, 8, Direction::Backward)
                .unwrap()
                .target
                .start,
            8
        );
    }

    #[test]
    fn step_inclusive_on_no_matches_is_none() {
        assert!(step_inclusive(&[], 0, Direction::Forward).is_none());
    }

    #[test]
    fn direction_reverses() {
        assert_eq!(Direction::Forward.reversed(), Direction::Backward);
        assert_eq!(Direction::Backward.reversed(), Direction::Forward);
    }

    #[test]
    fn zero_width_matches_terminate_and_never_highlight() {
        // `x*` matches empty at every position — a naive scanner loops forever.
        let m = find_all("abc", &pat("x*"));
        assert!(!m.is_empty());
        assert!(m.iter().all(SearchMatch::is_empty));
        assert!(!m[0].contains(0), "a zero-width match highlights nothing");
    }

    #[test]
    fn contains_is_half_open() {
        let m = SearchMatch { start: 2, end: 5 };
        assert!(!m.contains(1));
        assert!(m.contains(2));
        assert!(m.contains(4));
        assert!(!m.contains(5), "end is exclusive");
    }

    #[test]
    fn word_at_reads_the_whole_word_from_inside_it() {
        assert_eq!(word_at("hello world", 2).as_deref(), Some("hello"));
        assert_eq!(word_at("hello world", 0).as_deref(), Some("hello"));
        assert_eq!(word_at("hello world", 4).as_deref(), Some("hello"));
        assert_eq!(word_at("hello world", 8).as_deref(), Some("world"));
    }

    #[test]
    fn word_at_scans_forward_from_whitespace_like_vim() {
        assert_eq!(word_at("  hello", 0).as_deref(), Some("hello"));
    }

    #[test]
    fn word_at_stops_at_the_line_end() {
        // vim does not jump to the next line looking for a word.
        assert_eq!(word_at("   \nhello", 0), None);
    }

    #[test]
    fn word_at_includes_underscores_and_digits() {
        assert_eq!(word_at("foo_bar99 x", 0).as_deref(), Some("foo_bar99"));
    }

    #[test]
    fn word_at_on_empty_text_is_none() {
        assert_eq!(word_at("", 0), None);
    }

    #[test]
    fn case_insensitive_search_finds_mixed_case() {
        let p = SearchPattern::compile("foo", CaseMode::Ignore).unwrap();
        assert_eq!(find_all("Foo FOO foo", &p).len(), 3);
    }

    #[test]
    fn smartcase_capital_narrows_the_result_set() {
        let loose = SearchPattern::compile("foo", CaseMode::Smart).unwrap();
        let tight = SearchPattern::compile("Foo", CaseMode::Smart).unwrap();
        assert_eq!(find_all("Foo FOO foo", &loose).len(), 3);
        assert_eq!(find_all("Foo FOO foo", &tight).len(), 1);
    }

    #[test]
    fn stepping_forward_through_every_match_returns_to_the_start() {
        let text = "a foo b foo c foo d";
        let m = find_all(text, &pat("foo"));
        let mut at = 0;
        let mut seen = vec![];
        for _ in 0..m.len() {
            let s = step(&m, at, Direction::Forward).unwrap();
            seen.push(s.target.start);
            at = s.target.start;
        }
        // From 0 (before the first match at 2): 2, 8, 14 — a full cycle.
        assert_eq!(seen, vec![2, 8, 14]);
        // One more wraps back to the first.
        assert_eq!(step(&m, at, Direction::Forward).unwrap().target.start, 2);
    }
}
