//! Search session state: the live prompt, the committed pattern, and history.
//!
//! # The shape, and why
//!
//! Two things get confused in editor search implementations, always with the
//! same symptom — highlights that linger after Escape, or `n` repeating a
//! pattern the user cancelled:
//!
//! - the **prompt** (`/foo` being typed, not yet accepted), and
//! - the **committed** pattern (what `n` / `N` repeat, what stays highlighted).
//!
//! They are modelled as separate fields, and the prompt is an `Option<Prompt>`
//! rather than a bool-plus-string. A session that is not open therefore has no
//! text and no direction to read: "typing into a closed prompt" is not a state
//! that can be constructed, instead of one guarded by an `if is_open` that some
//! future call site forgets.
//!
//! Cancelling drops the `Prompt` and leaves the committed pattern untouched,
//! which is exactly vim's behaviour and falls out of the shape rather than
//! being restored by hand.

use crate::engine::{Direction, SearchMatch, Step, find_all, step, step_inclusive};
use crate::pattern::{CaseMode, PatternError, SearchPattern};

/// How many past searches to keep. vim's default is 50.
pub const HISTORY_LIMIT: usize = 50;

/// An open search prompt — the user is typing `/…` or `?…`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// Which key opened it, and therefore which way `<CR>` will search.
    pub direction: Direction,
    /// What has been typed so far (without the leading `/` or `?`).
    pub text: String,
    /// Where the cursor was when the prompt opened. Incremental search
    /// previews from here, and Escape returns here — so it must be captured at
    /// open time, not read live.
    pub origin: usize,
    /// Position in history while arrowing through it; `None` = editing fresh
    /// text rather than browsing.
    history_index: Option<usize>,
    /// The in-progress text stashed when history browsing began, so arrowing
    /// back down past the newest entry restores what the user actually typed.
    stashed: Option<String>,
}

/// The result of committing a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accepted {
    /// A new pattern was compiled and committed.
    Committed,
    /// The prompt was empty, so the previous pattern was reused — vim's bare
    /// `/<CR>`.
    ReusedPrevious,
    /// Nothing to do: empty prompt and no previous pattern.
    NothingToRepeat,
    /// The pattern did not compile; the prompt stays open so the user can fix
    /// it rather than losing what they typed.
    Invalid(PatternError),
}

/// Everything search needs to remember.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    prompt: Option<Prompt>,
    pattern: Option<SearchPattern>,
    /// Direction of the committed search — what `n` repeats. Distinct from the
    /// prompt's direction, which is discarded on cancel.
    direction: Direction,
    matches: Vec<SearchMatch>,
    /// vim's `hlsearch`: whether matches stay lit after the search completes.
    /// `:noh` clears this without forgetting the pattern, so `n` still works.
    highlight: bool,
    history: Vec<String>,
    case: CaseMode,
}

impl SearchState {
    #[must_use]
    pub fn new(case: CaseMode) -> Self {
        Self {
            case,
            highlight: true,
            ..Self::default()
        }
    }

    // ── prompt lifecycle ────────────────────────────────────────────────

    /// Open a prompt. `origin` is the cursor position to preview from and to
    /// return to on cancel.
    pub fn open(&mut self, direction: Direction, origin: usize) {
        self.prompt = Some(Prompt {
            direction,
            text: String::new(),
            origin,
            history_index: None,
            stashed: None,
        });
    }

    #[must_use]
    pub const fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    #[must_use]
    pub const fn is_prompting(&self) -> bool {
        self.prompt.is_some()
    }

    /// Type a character into the prompt. No-op when no prompt is open.
    pub fn push(&mut self, ch: char) {
        if let Some(p) = self.prompt.as_mut() {
            p.text.push(ch);
            // Editing ends history browsing — the text is the user's now.
            p.history_index = None;
        }
    }

    /// Backspace. Returns `true` if the prompt closed because it was already
    /// empty (vim closes the prompt when you backspace past the `/`).
    pub fn backspace(&mut self) -> bool {
        let Some(p) = self.prompt.as_mut() else {
            return false;
        };
        p.history_index = None;
        if p.text.pop().is_none() {
            self.prompt = None;
            return true;
        }
        false
    }

    /// Abandon the prompt. The committed pattern and its highlights survive —
    /// cancelling a new search does not erase the old one.
    ///
    /// Returns the cursor position to restore, if a prompt was open.
    pub fn cancel(&mut self) -> Option<usize> {
        self.prompt.take().map(|p| p.origin)
    }

    /// Commit the prompt.
    pub fn accept(&mut self, text: &str) -> Accepted {
        let Some(p) = self.prompt.take() else {
            return Accepted::NothingToRepeat;
        };
        let direction = p.direction;

        if p.text.is_empty() {
            // Bare `/<CR>`: repeat the previous pattern in the NEW direction.
            return if self.pattern.is_some() {
                self.direction = direction;
                self.refresh(text);
                Accepted::ReusedPrevious
            } else {
                Accepted::NothingToRepeat
            };
        }

        match SearchPattern::compile(&p.text, self.case) {
            Ok(pattern) => {
                self.remember(&p.text);
                self.pattern = Some(pattern);
                self.direction = direction;
                self.highlight = true;
                self.refresh(text);
                Accepted::Committed
            }
            Err(e) => {
                // Put the prompt back so the typed text is not lost.
                self.prompt = Some(p);
                Accepted::Invalid(e)
            }
        }
    }

    // ── history ─────────────────────────────────────────────────────────

    fn remember(&mut self, raw: &str) {
        // Re-searching something moves it to the front rather than duplicating.
        self.history.retain(|h| h != raw);
        self.history.push(raw.to_string());
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
    }

    #[must_use]
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Arrow up/down through history while prompting. `back` = older.
    pub fn history_step(&mut self, back: bool) {
        if self.history.is_empty() {
            return;
        }
        let len = self.history.len();
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        match (p.history_index, back) {
            // Enter history: stash what is being typed so it can come back.
            (None, true) => {
                p.stashed = Some(p.text.clone());
                p.history_index = Some(len - 1);
                p.text.clone_from(&self.history[len - 1]);
            }
            (Some(i), true) if i > 0 => {
                p.history_index = Some(i - 1);
                p.text.clone_from(&self.history[i - 1]);
            }
            (Some(i), false) if i + 1 < len => {
                p.history_index = Some(i + 1);
                p.text.clone_from(&self.history[i + 1]);
            }
            // Stepping forward past the newest entry restores the stash.
            (Some(_), false) => {
                p.history_index = None;
                p.text = p.stashed.take().unwrap_or_default();
            }
            _ => {}
        }
    }

    // ── searching ───────────────────────────────────────────────────────

    /// Recompute matches against `text`. Call after an edit, or after the
    /// buffer changes under a live highlight.
    pub fn refresh(&mut self, text: &str) {
        self.matches = self
            .pattern
            .as_ref()
            .map_or_else(Vec::new, |p| find_all(text, p));
    }

    /// Incremental preview for the current prompt text, without committing.
    /// Returns where the cursor would land.
    #[must_use]
    pub fn preview(&self, text: &str) -> Option<Step> {
        let p = self.prompt.as_ref()?;
        if p.text.is_empty() {
            return None;
        }
        let pattern = SearchPattern::compile(&p.text, self.case).ok()?;
        let matches = find_all(text, &pattern);
        // Inclusive: typing `/foo` while sitting ON a `foo` must light up that
        // one. `n` deliberately uses the exclusive `step` instead.
        step_inclusive(&matches, p.origin, p.direction)
    }

    /// How many matches the CURRENT PROMPT text would find.
    ///
    /// The denominator of `[3/17]` while typing — the half that makes the
    /// count a safety measurement rather than a curiosity: `[1/1]` says a
    /// rename is safe, `[1/240]` says narrow the pattern first, and both
    /// answers arrive before Enter.
    ///
    /// `0` covers all three "nothing to count" cases — no prompt, empty
    /// prompt, uncompilable pattern — because a caller showing a count cannot
    /// act on the difference; [`Self::prompt_is_empty`] separates them when it
    /// matters.
    #[must_use]
    pub fn preview_total(&self, text: &str) -> usize {
        let Some(p) = self.prompt.as_ref() else {
            return 0;
        };
        if p.text.is_empty() {
            return 0;
        }
        SearchPattern::compile(&p.text, self.case)
            .ok()
            .map_or(0, |pattern| find_all(text, &pattern).len())
    }

    /// Is a prompt open with nothing typed into it yet?
    ///
    /// Distinguishes "you have not typed a pattern" from "your pattern matches
    /// nothing" — the first should stay silent, the second should say `[0/0]`.
    #[must_use]
    pub fn prompt_is_empty(&self) -> bool {
        self.prompt.as_ref().is_none_or(|p| p.text.is_empty())
    }

    /// Where committing the prompt should land, given the prompt's origin.
    ///
    /// **Uses `step_inclusive`, exactly as [`Self::preview`] does** — that is
    /// the entire contract: what the preview showed is where Enter lands.
    ///
    /// `repeat(origin - 1)` is NOT a substitute, and the difference is not
    /// theoretical. `repeat` is exclusive, so it needs the caller to back up
    /// one to include a match sitting on the origin; `saturating_sub` cannot
    /// back up past 0, so a match at offset 0 — the first word of the file —
    /// became unreachable and the commit silently jumped to the *second*
    /// match. `engine.rs` documents this saturation trap on `step_inclusive`
    /// itself; this method is why that type exists.
    #[must_use]
    pub fn commit_step(&self, origin: usize) -> Option<Step> {
        step_inclusive(&self.matches, origin, self.direction)
    }

    /// `n` (`reverse = false`) / `N` (`reverse = true`).
    #[must_use]
    pub fn repeat(&self, from: usize, reverse: bool) -> Option<Step> {
        let dir = if reverse {
            self.direction.reversed()
        } else {
            self.direction
        };
        step(&self.matches, from, dir)
    }

    /// `*` / `#` — search the word under the cursor, whole-word and literal.
    /// Returns where to jump, or `None` if there is no word or no match.
    pub fn search_word(&mut self, text: &str, cursor: usize, direction: Direction) -> Option<Step> {
        let word = crate::engine::word_at(text, cursor)?;
        let pattern = SearchPattern::whole_word(&word, self.case).ok()?;
        self.remember(pattern.raw());
        self.pattern = Some(pattern);
        self.direction = direction;
        self.highlight = true;
        self.refresh(text);
        self.repeat(cursor, false)
    }

    // ── committed view ──────────────────────────────────────────────────

    #[must_use]
    pub const fn pattern(&self) -> Option<&SearchPattern> {
        self.pattern.as_ref()
    }

    #[must_use]
    pub const fn direction(&self) -> Direction {
        self.direction
    }

    #[must_use]
    pub fn matches(&self) -> &[SearchMatch] {
        &self.matches
    }

    /// Matches the renderer should light up. Empty when `hlsearch` is off, so
    /// the caller needs no separate check.
    #[must_use]
    pub fn highlights(&self) -> &[SearchMatch] {
        if self.highlight { &self.matches } else { &[] }
    }

    #[must_use]
    pub const fn highlight_enabled(&self) -> bool {
        self.highlight
    }

    /// Re-enable highlighting for the committed pattern.
    ///
    /// `n` after an auto-clear must light the matches again — vim does the
    /// same. Without this, highlighting would be a one-shot that the first
    /// motion extinguished permanently.
    pub fn relight(&mut self) {
        self.highlight = true;
    }

    /// `:noh` — stop highlighting, but keep the pattern so `n` still works.
    pub fn clear_highlight(&mut self) {
        self.highlight = false;
    }

    pub fn set_case(&mut self, case: CaseMode) {
        self.case = case;
    }

    #[must_use]
    pub const fn case(&self) -> CaseMode {
        self.case
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "foo bar\nbaz foo\nqux foo end";

    fn committed(pat: &str) -> SearchState {
        let mut s = SearchState::new(CaseMode::Sensitive);
        s.open(Direction::Forward, 0);
        for c in pat.chars() {
            s.push(c);
        }
        assert_eq!(s.accept(TEXT), Accepted::Committed);
        s
    }

    #[test]
    fn typing_then_accepting_commits_and_finds() {
        let s = committed("foo");
        assert_eq!(s.pattern().unwrap().raw(), "foo");
        assert_eq!(s.matches().len(), 3);
        assert!(!s.is_prompting(), "prompt closes on accept");
    }

    #[test]
    fn cancel_keeps_the_previous_search_intact() {
        let mut s = committed("foo");
        s.open(Direction::Forward, 5);
        s.push('z');
        let origin = s.cancel();
        assert_eq!(origin, Some(5), "cancel returns the cursor home");
        assert_eq!(s.pattern().unwrap().raw(), "foo", "old pattern survives");
        assert_eq!(s.matches().len(), 3, "old highlights survive");
        assert!(!s.is_prompting());
    }

    #[test]
    fn backspacing_past_the_slash_closes_the_prompt() {
        let mut s = SearchState::new(CaseMode::Smart);
        s.open(Direction::Forward, 0);
        s.push('a');
        assert!(!s.backspace(), "still has text");
        assert!(s.backspace(), "empty -> closes");
        assert!(!s.is_prompting());
    }

    #[test]
    fn an_invalid_pattern_keeps_the_prompt_open_so_typing_is_not_lost() {
        let mut s = SearchState::new(CaseMode::Smart);
        s.open(Direction::Forward, 0);
        for c in "a[b".chars() {
            s.push(c);
        }
        assert!(matches!(s.accept(TEXT), Accepted::Invalid(_)));
        assert!(s.is_prompting(), "prompt must stay open");
        assert_eq!(s.prompt().unwrap().text, "a[b", "text must survive");
    }

    #[test]
    fn bare_enter_reuses_the_previous_pattern() {
        let mut s = committed("foo");
        s.open(Direction::Backward, 0);
        assert_eq!(s.accept(TEXT), Accepted::ReusedPrevious);
        assert_eq!(s.pattern().unwrap().raw(), "foo");
        assert_eq!(s.direction(), Direction::Backward, "direction updates");
    }

    #[test]
    fn bare_enter_with_no_history_does_nothing() {
        let mut s = SearchState::new(CaseMode::Smart);
        s.open(Direction::Forward, 0);
        assert_eq!(s.accept(TEXT), Accepted::NothingToRepeat);
    }

    #[test]
    fn n_and_N_move_opposite_ways() {
        let s = committed("foo");
        let fwd = s.repeat(0, false).unwrap();
        let back = s.repeat(20, true).unwrap();
        assert!(fwd.target.start > 0);
        assert!(back.target.start < 20);
    }

    #[test]
    fn N_after_a_backward_search_goes_forward() {
        let mut s = SearchState::new(CaseMode::Sensitive);
        s.open(Direction::Backward, 0);
        for c in "foo".chars() {
            s.push(c);
        }
        s.accept(TEXT);
        assert_eq!(s.direction(), Direction::Backward);
        // N reverses a backward search into a forward one. TEXT has `foo` at
        // 0, 12 and 20; forward-from-0 is exclusive, so it lands on 12.
        let n = s.repeat(0, true).unwrap();
        assert_eq!(n.target.start, 12, "first match strictly after 0");
    }

    #[test]
    fn noh_stops_highlighting_but_n_still_works() {
        let mut s = committed("foo");
        assert_eq!(s.highlights().len(), 3);
        s.clear_highlight();
        assert!(s.highlights().is_empty(), "nothing lit");
        assert_eq!(s.matches().len(), 3, "but matches are remembered");
        assert!(s.repeat(0, false).is_some(), "and n still moves");
    }

    #[test]
    fn incremental_preview_does_not_commit() {
        let mut s = SearchState::new(CaseMode::Sensitive);
        s.open(Direction::Forward, 0);
        for c in "baz".chars() {
            s.push(c);
        }
        assert!(s.preview(TEXT).is_some(), "preview finds it");
        assert!(s.pattern().is_none(), "but nothing is committed yet");
        assert!(s.matches().is_empty());
    }

    #[test]
    fn preview_finds_a_match_starting_at_the_cursor() {
        let mut s = SearchState::new(CaseMode::Sensitive);
        s.open(Direction::Forward, 0); // cursor sits on "foo" at 0
        for c in "foo".chars() {
            s.push(c);
        }
        assert_eq!(
            s.preview(TEXT).unwrap().target.start,
            0,
            "must light the one under the cursor"
        );
    }

    #[test]
    fn preview_of_an_invalid_pattern_is_none_not_a_panic() {
        let mut s = SearchState::new(CaseMode::Smart);
        s.open(Direction::Forward, 0);
        for c in "a[b".chars() {
            s.push(c);
        }
        assert!(s.preview(TEXT).is_none());
    }

    #[test]
    fn history_records_accepted_searches_newest_last() {
        let mut s = SearchState::new(CaseMode::Sensitive);
        for p in ["foo", "bar", "baz"] {
            s.open(Direction::Forward, 0);
            for c in p.chars() {
                s.push(c);
            }
            s.accept(TEXT);
        }
        assert_eq!(s.history(), ["foo", "bar", "baz"]);
    }

    #[test]
    fn repeating_a_search_moves_it_to_the_front_without_duplicating() {
        let mut s = SearchState::new(CaseMode::Sensitive);
        for p in ["foo", "bar", "foo"] {
            s.open(Direction::Forward, 0);
            for c in p.chars() {
                s.push(c);
            }
            s.accept(TEXT);
        }
        assert_eq!(s.history(), ["bar", "foo"], "no duplicate 'foo'");
    }

    #[test]
    fn arrowing_up_walks_back_through_history() {
        let mut s = SearchState::new(CaseMode::Sensitive);
        for p in ["one", "two"] {
            s.open(Direction::Forward, 0);
            for c in p.chars() {
                s.push(c);
            }
            s.accept(TEXT);
        }
        s.open(Direction::Forward, 0);
        s.history_step(true);
        assert_eq!(s.prompt().unwrap().text, "two");
        s.history_step(true);
        assert_eq!(s.prompt().unwrap().text, "one");
        s.history_step(false);
        assert_eq!(s.prompt().unwrap().text, "two");
    }

    #[test]
    fn arrowing_back_down_restores_what_you_were_typing() {
        let mut s = SearchState::new(CaseMode::Sensitive);
        s.open(Direction::Forward, 0);
        for c in "old".chars() {
            s.push(c);
        }
        s.accept(TEXT);

        s.open(Direction::Forward, 0);
        for c in "typ".chars() {
            s.push(c);
        }
        s.history_step(true);
        assert_eq!(s.prompt().unwrap().text, "old");
        s.history_step(false);
        assert_eq!(s.prompt().unwrap().text, "typ", "the stash comes back");
    }

    #[test]
    fn history_is_bounded() {
        let mut s = SearchState::new(CaseMode::Sensitive);
        for i in 0..(HISTORY_LIMIT + 10) {
            s.open(Direction::Forward, 0);
            for c in i.to_string().chars() {
                s.push(c);
            }
            s.accept(TEXT);
        }
        assert_eq!(s.history().len(), HISTORY_LIMIT);
    }

    #[test]
    fn star_searches_the_whole_word_under_the_cursor() {
        let text = "foo foobar foo";
        let mut s = SearchState::new(CaseMode::Sensitive);
        let hit = s.search_word(text, 0, Direction::Forward).unwrap();
        // Whole-word: "foobar" must NOT match, so from 0 the next is at 11.
        assert_eq!(hit.target.start, 11);
        assert_eq!(s.matches().len(), 2, "two whole-word 'foo', not three");
    }

    #[test]
    fn star_on_a_wordless_line_is_none_and_changes_nothing() {
        let mut s = committed("foo");
        let before = s.matches().len();
        assert!(s.search_word("   \n", 0, Direction::Forward).is_none());
        assert_eq!(s.matches().len(), before, "state untouched");
    }

    #[test]
    fn typing_after_browsing_history_stops_browsing() {
        let mut s = SearchState::new(CaseMode::Sensitive);
        s.open(Direction::Forward, 0);
        for c in "old".chars() {
            s.push(c);
        }
        s.accept(TEXT);
        s.open(Direction::Forward, 0);
        s.history_step(true);
        assert_eq!(s.prompt().unwrap().text, "old");
        s.push('x');
        assert_eq!(s.prompt().unwrap().text, "oldx");
        // Arrowing down now restores nothing (we are editing, not browsing).
        s.history_step(false);
        assert_eq!(s.prompt().unwrap().text, "oldx");
    }

    #[test]
    fn refresh_tracks_an_edited_buffer() {
        let mut s = committed("foo");
        assert_eq!(s.matches().len(), 3);
        s.refresh("foo");
        assert_eq!(s.matches().len(), 1, "matches follow the new text");
    }

    #[test]
    fn pushing_into_a_closed_prompt_is_a_no_op_not_a_panic() {
        let mut s = SearchState::new(CaseMode::Smart);
        s.push('x');
        assert!(!s.is_prompting());
        assert!(!s.backspace());
        assert_eq!(s.cancel(), None);
    }
}
