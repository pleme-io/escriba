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

use escriba_memori::{CaretLine, CaretMove};

use crate::engine::{Direction, SearchMatch, Step, find_all, step, step_inclusive};
use crate::pattern::{CaseMode, PatternError, SearchPattern};

/// How many past searches to keep. vim's default is 50.
pub const HISTORY_LIMIT: usize = 50;

/// An open search prompt — the user is typing `/…` or `?…`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// Which key opened it, and therefore which way `<CR>` will search.
    pub direction: Direction,
    /// What has been typed so far (without the leading `/` or `?`), and the
    /// caret editing it.
    ///
    /// ONE value, not two fields. The invariant `caret <= text.chars().count()`
    /// lives BETWEEN them, and the six mutation sites below each used to
    /// maintain it by convention. `escriba-mode`'s ex-line had the identical
    /// two fields and got it wrong at the seventh — which is why `CaretLine`
    /// exists, and why this is the same type rather than a second copy of it.
    line: CaretLine,
    /// Where the cursor was when the prompt opened. Incremental search
    /// previews from here, and Escape returns here — so it must be captured at
    /// open time, not read live.
    pub origin: usize,
    /// How many matches past the first the preview has been stepped, via
    /// `<C-g>` / `<C-t>`.
    ///
    /// Reset to 0 by any edit to the pattern, for the same reason
    /// `history_index` is: the ordinal describes a match set that the edit
    /// just replaced, so carrying it forward would land the preview somewhere
    /// the user never asked for.
    preview_skip: usize,
    /// Position in history while arrowing through it; `None` = editing fresh
    /// text rather than browsing.
    history_index: Option<usize>,
    /// The in-progress text stashed when history browsing began, so arrowing
    /// back down past the newest entry restores what the user actually typed.
    stashed: Option<String>,
}

impl Prompt {
    /// How far `<C-g>` has stepped the preview past the first match.
    #[must_use]
    pub const fn preview_skip(&self) -> usize {
        self.preview_skip
    }

    /// The caret as a char index. `caret == text.chars().count()` means "at
    /// the end", which is the common case.
    #[must_use]
    pub const fn caret(&self) -> usize {
        self.line.caret()
    }

    /// What has been typed so far, without the leading `/` or `?`.
    #[must_use]
    pub fn text(&self) -> &str {
        self.line.text()
    }
}

/// What the in-progress pattern would do, computed in ONE pass.
///
/// Replaces `Option<Step>` plus a separate `preview_total`, which between them
/// scanned the whole document TWICE per status line — each call recompiling
/// the regex and re-running `find_all`. The committed path had always read a
/// cached match set; the prompting path doing two full scans was an oversight,
/// not a trade.
///
/// The variants also separate two states `Option<Step>` conflated. `None` meant
/// both "you have typed `a[`, which is not a pattern yet" and "your pattern
/// compiles and finds nothing", so the status line rendered `[0/0]` for both —
/// telling a user mid-character-class that their pattern matches nothing.
/// Those are different answers and now have different variants.
#[derive(Debug, Clone, PartialEq)]
pub enum Preview {
    /// No prompt open, or nothing typed into it. Report nothing.
    Idle,
    /// The pattern does not compile YET — mid-typing a character class.
    /// Reports nothing: an error per keystroke is unusable.
    Incomplete,
    /// Compiles, matches nothing. This is the one that earns `[0/0]`.
    NoMatch,
    /// Where the cursor would land, and how many matches there are.
    Landed { step: Step, total: usize },
}

impl Preview {
    /// The landing step, if the pattern found one.
    ///
    /// Lets a caller that only cares "did it land" keep reading naturally
    /// without matching all four arms. The arms still exist for callers that
    /// must distinguish `Incomplete` from `NoMatch` — which is the whole
    /// reason the enum replaced `Option<Step>`.
    #[must_use]
    pub fn step(&self) -> Option<&Step> {
        match self {
            Self::Landed { step, .. } => Some(step),
            Self::Idle | Self::Incomplete | Self::NoMatch => None,
        }
    }

    /// How many matches the in-progress pattern finds. `0` unless it landed.
    #[must_use]
    pub const fn total(&self) -> usize {
        match self {
            Self::Landed { total, .. } => *total,
            Self::Idle | Self::Incomplete | Self::NoMatch => 0,
        }
    }
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
            line: CaretLine::default(),
            origin,
            preview_skip: 0,
            history_index: None,
            stashed: None,
        });
    }

    #[must_use]
    pub const fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    /// The COMMITTED pattern's text — what `n`/`N` repeat.
    ///
    /// Distinct from the live prompt (`prompt().text()`), which is discarded
    /// on cancel. A reader wanting "what is the editor searching for" wants
    /// this one.
    #[must_use]
    pub fn committed_pattern(&self) -> Option<&str> {
        self.pattern.as_ref().map(SearchPattern::source)
    }

    /// How many matches the committed pattern has.
    ///
    /// Not `highlights().len()`: that is empty after `:noh`, which clears the
    /// HIGHLIGHT without forgetting the pattern. Conflating them would make
    /// `:noh` look like "no matches".
    #[must_use]
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    #[must_use]
    pub const fn is_prompting(&self) -> bool {
        self.prompt.is_some()
    }

    /// Type a character into the prompt. No-op when no prompt is open.
    pub fn push(&mut self, ch: char) {
        if let Some(p) = self.prompt.as_mut() {
            p.line.insert(ch);
            // Editing ends history browsing — the text is the user's now.
            p.history_index = None;
            p.preview_skip = 0;
        }
    }

    /// Move the caret. Saturates at both ends rather than wrapping — a caret
    /// that wraps from the start to the end deletes the wrong character next.
    pub fn move_caret(&mut self, to: CaretMove) {
        if let Some(p) = self.prompt.as_mut() {
            p.line.move_caret(to);
        }
    }

    /// Delete the character AT the caret (`<Del>`). No-op at the end.
    ///
    /// Distinct from [`Self::backspace`], which deletes the one before it and
    /// can close the prompt. Forward-delete never closes the prompt: emptying
    /// the text by deleting rightwards is not the "backspaced past the `/`"
    /// gesture that means "I changed my mind".
    pub fn delete_at_caret(&mut self) {
        if let Some(p) = self.prompt.as_mut() {
            if p.line.caret() < p.line.len_chars() {
                p.line.delete();
                p.history_index = None;
                p.preview_skip = 0;
            }
        }
    }

    /// `<C-w>` — delete the word before the caret.
    ///
    /// Trailing whitespace goes first, then the run of non-whitespace, which
    /// is what makes a second `<C-w>` delete a whole second word rather than
    /// only the gap.
    pub fn delete_word_before_caret(&mut self) {
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        p.line.delete_word_before();
        p.history_index = None;
        p.preview_skip = 0;
    }

    /// `<C-u>` — delete from the caret back to the start.
    pub fn clear_before_caret(&mut self) {
        if let Some(p) = self.prompt.as_mut() {
            p.line.clear_before_caret();
            p.history_index = None;
            p.preview_skip = 0;
        }
    }

    /// Backspace. Returns `true` if the prompt closed because it was already
    /// empty (vim closes the prompt when you backspace past the `/`).
    pub fn backspace(&mut self) -> bool {
        let Some(p) = self.prompt.as_mut() else {
            return false;
        };
        p.history_index = None;
        p.preview_skip = 0;
        if p.line.caret() == 0 {
            // Backspacing past the `/` closes the prompt — but only when there
            // is nothing to the left. With text ahead of the caret this is a
            // no-op, not a cancel: losing a pattern because the caret happened
            // to be at the start would be the worst kind of surprise.
            if p.line.text().is_empty() {
                self.prompt = None;
                return true;
            }
            return false;
        }
        p.line.backspace();
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

        if p.line.text().is_empty() {
            // Bare `/<CR>`: repeat the previous pattern in the NEW direction.
            return if self.pattern.is_some() {
                self.arm(direction, text);
                Accepted::ReusedPrevious
            } else {
                Accepted::NothingToRepeat
            };
        }

        match SearchPattern::compile(p.line.text(), self.case) {
            Ok(pattern) => {
                self.remember(p.line.text());
                self.pattern = Some(pattern);
                self.arm(direction, text);
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
                p.stashed = Some(p.line.text().to_owned());
                p.history_index = Some(len - 1);
                p.line.set_text(self.history[len - 1].clone());
            }
            (Some(i), true) if i > 0 => {
                p.history_index = Some(i - 1);
                p.line.set_text(self.history[i - 1].clone());
            }
            (Some(i), false) if i + 1 < len => {
                p.history_index = Some(i + 1);
                p.line.set_text(self.history[i + 1].clone());
            }
            // Stepping forward past the newest entry restores the stash.
            (Some(_), false) => {
                p.history_index = None;
                p.line.set_text(p.stashed.take().unwrap_or_default());
            }
            _ => {}
        }
        // A recalled pattern arrives whole; the caret belongs at its end,
        // which is where a user expects to continue typing.
        p.line.move_caret(CaretMove::End);
        p.preview_skip = 0;
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

    /// A search just became active: point `n` at it, light it, rescan.
    ///
    /// The ONE place that knows arming implies highlighting. Before this,
    /// three sites assigned `direction` / `highlight` / `refresh` separately
    /// and one of them — the bare `/<CR>` re-search — forgot the highlight, so
    /// re-running the previous pattern after an auto-clear jumped correctly
    /// and stayed dark.
    ///
    /// Patching that one branch would have left the shape intact: a fourth
    /// arming path would still have to REMEMBER three assignments. Here a new
    /// path either calls `arm` and is correct by default, or does not arm.
    fn arm(&mut self, direction: Direction, text: &str) {
        self.direction = direction;
        self.highlight = true;
        self.refresh(text);
    }

    /// Incremental preview for the current prompt text, without committing.
    /// Returns where the cursor would land.
    #[must_use]
    pub fn preview(&self, text: &str) -> Preview {
        let Some(p) = self.prompt.as_ref() else {
            return Preview::Idle;
        };
        if p.line.text().is_empty() {
            return Preview::Idle;
        }
        let Ok(pattern) = SearchPattern::compile(p.line.text(), self.case) else {
            return Preview::Incomplete;
        };

        // ONE scan. The total and the landing come from the same match set, so
        // they cannot disagree and cannot cost twice.
        let matches = find_all(text, &pattern);
        let total = matches.len();

        // Inclusive: typing `/foo` while sitting ON a `foo` must light up that
        // one. `n` deliberately uses the exclusive `step` instead.
        let Some(mut landed) = step_inclusive(&matches, p.origin, p.direction) else {
            return Preview::NoMatch;
        };
        // Then walk forward however far `<C-g>` has taken us. Exclusive from
        // here, so each press advances by exactly one match, and wrapping
        // falls out of `step` rather than needing its own arithmetic.
        for _ in 0..p.preview_skip {
            let Some(next) = step(&matches, landed.target.start, p.direction) else {
                return Preview::NoMatch;
            };
            landed = next;
        }
        Preview::Landed {
            step: landed,
            total,
        }
    }

    /// `<C-g>` / `<C-t>` — walk the preview to the next/previous match without
    /// committing.
    ///
    /// This collapses `/pat<CR>nnn` into one gesture that is STILL
    /// CANCELLABLE: Escape from here returns to where the search started,
    /// which `n` after a commit cannot do. It is the reason to prefer stepping
    /// the preview over committing and then repeating.
    ///
    /// Backward saturates at the first match rather than wrapping: `<C-t>`
    /// walks back through what `<C-g>` advanced, and stopping at the start is
    /// the honest floor for a counter that begins there.
    pub fn preview_step(&mut self, forward: bool) {
        if let Some(p) = self.prompt.as_mut() {
            p.preview_skip = if forward {
                p.preview_skip.saturating_add(1)
            } else {
                p.preview_skip.saturating_sub(1)
            };
        }
    }

    /// Why the open prompt's pattern would fail to compile, if it would.
    ///
    /// `None` when there is no prompt, when the prompt is EMPTY (that is the
    /// legitimate bare-`/<CR>` reuse-previous path, not an error), or when the
    /// pattern compiles. Lets a caller classify a submit BEFORE acting on it.
    #[must_use]
    pub fn prompt_error(&self) -> Option<PatternError> {
        let p = self.prompt.as_ref()?;
        if p.line.text().is_empty() {
            return None;
        }
        SearchPattern::compile(p.line.text(), self.case).err()
    }

    /// Is a prompt open with nothing typed into it yet?
    ///
    /// Distinguishes "you have not typed a pattern" from "your pattern matches
    /// nothing" — the first should stay silent, the second should say `[0/0]`.
    #[must_use]
    pub fn prompt_is_empty(&self) -> bool {
        self.prompt
            .as_ref()
            .is_none_or(|p| p.line.text().is_empty())
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
        self.commit_step_skipping(origin, 0)
    }

    /// [`Self::commit_step`], honouring however far `<C-g>` stepped the
    /// preview.
    ///
    /// Without the skip, stepping the preview to the third match and pressing
    /// Enter landed on the FIRST — breaking the contract the commit anchor was
    /// fixed to establish, that what the preview showed is where you land.
    /// `accept()` consumes the prompt, so the caller reads
    /// [`Prompt::preview_skip`] before committing and passes it here.
    #[must_use]
    pub fn commit_step_skipping(&self, origin: usize, skip: usize) -> Option<Step> {
        let mut landed = step_inclusive(&self.matches, origin, self.direction)?;
        for _ in 0..skip {
            landed = step(&self.matches, landed.target.start, self.direction)?;
        }
        Some(landed)
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
        self.arm(direction, text);
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
        assert_eq!(s.prompt().unwrap().text(), "a[b", "text must survive");
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
    // `N` is a DIFFERENT vim key from `n`, and the test name says which
    // pair it covers. Renaming to snake case would make two distinct
    // bindings read as one.
    #[allow(non_snake_case)]
    fn n_and_N_move_opposite_ways() {
        let s = committed("foo");
        let fwd = s.repeat(0, false).unwrap();
        let back = s.repeat(20, true).unwrap();
        assert!(fwd.target.start > 0);
        assert!(back.target.start < 20);
    }

    #[test]
    #[allow(non_snake_case)]
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
        assert!(s.preview(TEXT).step().is_some(), "preview finds it");
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
            s.preview(TEXT).step().copied().unwrap().target.start,
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
        assert!(s.preview(TEXT).step().is_none());
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
        assert_eq!(s.prompt().unwrap().text(), "two");
        s.history_step(true);
        assert_eq!(s.prompt().unwrap().text(), "one");
        s.history_step(false);
        assert_eq!(s.prompt().unwrap().text(), "two");
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
        assert_eq!(s.prompt().unwrap().text(), "old");
        s.history_step(false);
        assert_eq!(s.prompt().unwrap().text(), "typ", "the stash comes back");
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
        assert_eq!(s.prompt().unwrap().text(), "old");
        s.push('x');
        assert_eq!(s.prompt().unwrap().text(), "oldx");
        // Arrowing down now restores nothing (we are editing, not browsing).
        s.history_step(false);
        assert_eq!(s.prompt().unwrap().text(), "oldx");
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

    // ── caret editing ───────────────────────────────────────────────────

    fn prompting(text: &str) -> SearchState {
        let mut st = SearchState::new(CaseMode::Smart);
        st.open(Direction::Forward, 0);
        for c in text.chars() {
            st.push(c);
        }
        st
    }

    fn shown(st: &SearchState) -> (String, usize) {
        let p = st.prompt().expect("prompting");
        (p.line.text().to_owned(), p.caret())
    }

    #[test]
    fn typing_appends_and_the_caret_follows() {
        let st = prompting("foo");
        assert_eq!(shown(&st), ("foo".to_string(), 3));
    }

    #[test]
    fn a_character_typed_mid_pattern_lands_at_the_caret() {
        // The complaint this exists for: fixing a typo in the middle.
        let mut st = prompting("fo");
        st.move_caret(CaretMove::Left);
        st.push('X');
        assert_eq!(shown(&st), ("fXo".to_string(), 2), "inserted AT the caret");
    }

    #[test]
    fn backspace_deletes_before_the_caret_not_at_the_end() {
        let mut st = prompting("abc");
        st.move_caret(CaretMove::Left); // between b and c
        assert!(!st.backspace());
        assert_eq!(shown(&st), ("ac".to_string(), 1), "deleted `b`, not `c`");
    }

    #[test]
    fn delete_at_caret_removes_the_character_ahead() {
        let mut st = prompting("abc");
        st.move_caret(CaretMove::Start);
        st.delete_at_caret();
        assert_eq!(shown(&st), ("bc".to_string(), 0));
    }

    #[test]
    fn forward_delete_never_closes_the_prompt() {
        // Emptying the text rightwards is not the "backspaced past the /"
        // gesture, so the prompt must survive it.
        let mut st = prompting("a");
        st.move_caret(CaretMove::Start);
        st.delete_at_caret();
        assert!(st.is_prompting(), "prompt must stay open");
        assert_eq!(shown(&st), (String::new(), 0));
    }

    #[test]
    fn backspace_at_the_start_with_text_ahead_is_a_no_op_not_a_cancel() {
        // Losing a typed pattern because the caret happened to be at column 0
        // would be the worst kind of surprise.
        let mut st = prompting("abc");
        st.move_caret(CaretMove::Start);
        assert!(!st.backspace(), "must not report a close");
        assert!(st.is_prompting(), "prompt survives");
        assert_eq!(shown(&st), ("abc".to_string(), 0), "text untouched");
    }

    #[test]
    fn backspace_on_an_empty_prompt_still_closes_it() {
        // The vim gesture must keep working.
        let mut st = prompting("");
        assert!(st.backspace(), "empty + backspace closes");
        assert!(!st.is_prompting());
    }

    #[test]
    fn caret_movement_saturates_at_both_ends() {
        let mut st = prompting("ab");
        for _ in 0..5 {
            st.move_caret(CaretMove::Left);
        }
        assert_eq!(shown(&st).1, 0, "cannot go left of the start");
        for _ in 0..5 {
            st.move_caret(CaretMove::Right);
        }
        assert_eq!(shown(&st).1, 2, "cannot go right of the end");
    }

    #[test]
    fn start_and_end_jump_the_caret() {
        let mut st = prompting("hello");
        st.move_caret(CaretMove::Start);
        assert_eq!(shown(&st).1, 0);
        st.move_caret(CaretMove::End);
        assert_eq!(shown(&st).1, 5);
    }

    #[test]
    // CHARS is shouted because the byte/char confusion is the bug.
    #[allow(non_snake_case)]
    fn the_caret_counts_CHARS_not_bytes() {
        // A byte caret lands mid-codepoint the first time anyone searches for
        // an accented word, and `String::insert` then panics.
        let mut st = prompting("héllo");
        st.move_caret(CaretMove::Start);
        st.move_caret(CaretMove::Right);
        st.move_caret(CaretMove::Right); // after `é`
        st.push('X');
        assert_eq!(shown(&st), ("héXllo".to_string(), 3));
    }

    #[test]
    fn editing_multibyte_text_backwards_does_not_panic() {
        let mut st = prompting("🔥é日");
        st.move_caret(CaretMove::End);
        assert!(!st.backspace());
        assert!(!st.backspace());
        assert_eq!(shown(&st), ("🔥".to_string(), 1));
    }

    #[test]
    fn ctrl_w_deletes_the_word_before_the_caret() {
        let mut st = prompting("foo bar");
        st.delete_word_before_caret();
        assert_eq!(shown(&st), ("foo ".to_string(), 4));
    }

    #[test]
    fn a_second_ctrl_w_eats_the_gap_and_the_next_word() {
        // Whitespace first, then the word — otherwise the second press only
        // removes the space and feels broken.
        let mut st = prompting("foo bar");
        st.delete_word_before_caret();
        st.delete_word_before_caret();
        assert_eq!(shown(&st), (String::new(), 0));
    }

    #[test]
    fn ctrl_w_keeps_what_is_ahead_of_the_caret() {
        let mut st = prompting("foo bar");
        st.move_caret(CaretMove::Start);
        st.move_caret(CaretMove::Right);
        st.move_caret(CaretMove::Right);
        st.move_caret(CaretMove::Right); // after "foo"
        st.delete_word_before_caret();
        assert_eq!(shown(&st), (" bar".to_string(), 0));
    }

    #[test]
    fn ctrl_u_clears_back_to_the_start_only() {
        let mut st = prompting("abcdef");
        st.move_caret(CaretMove::Start);
        for _ in 0..3 {
            st.move_caret(CaretMove::Right);
        }
        st.clear_before_caret();
        assert_eq!(shown(&st), ("def".to_string(), 0));
    }

    #[test]
    fn history_recall_parks_the_caret_at_the_end() {
        let mut st = SearchState::new(CaseMode::Smart);
        st.open(Direction::Forward, 0);
        for c in "alpha".chars() {
            st.push(c);
        }
        let _ = st.accept("alpha beta");

        st.open(Direction::Forward, 0);
        st.history_step(true);
        let (text, caret) = shown(&st);
        assert_eq!(caret, text.chars().count(), "continue typing at the end");
    }

    #[test]
    fn the_caret_never_exceeds_the_text_length() {
        // The standing invariant, exercised across a mixed edit sequence.
        let mut st = prompting("hello");
        let ops: &[CaretMove] = &[
            CaretMove::End,
            CaretMove::Left,
            CaretMove::Start,
            CaretMove::Right,
        ];
        for op in ops {
            st.move_caret(*op);
            st.delete_at_caret();
            let (t, c) = shown(&st);
            assert!(c <= t.chars().count(), "caret {c} past {t:?}");
        }
    }

    // ── preview stepping (<C-g> / <C-t>) ────────────────────────────────

    const HAYSTACK: &str = "aa bb aa bb aa";

    fn previewing(pat: &str) -> SearchState {
        let mut st = SearchState::new(CaseMode::Smart);
        st.open(Direction::Forward, 0);
        for c in pat.chars() {
            st.push(c);
        }
        st
    }

    #[test]
    fn preview_starts_on_the_first_match() {
        let st = previewing("aa");
        assert_eq!(
            st.preview(HAYSTACK)
                .step()
                .copied()
                .expect("a match")
                .target
                .start,
            0
        );
        assert_eq!(st.prompt().expect("open").preview_skip(), 0);
    }

    #[test]
    fn ctrl_g_walks_the_preview_forward_one_match_at_a_time() {
        let mut st = previewing("aa");
        st.preview_step(true);
        assert_eq!(
            st.preview(HAYSTACK)
                .step()
                .copied()
                .expect("a match")
                .target
                .start,
            6
        );
        st.preview_step(true);
        assert_eq!(
            st.preview(HAYSTACK)
                .step()
                .copied()
                .expect("a match")
                .target
                .start,
            12
        );
    }

    #[test]
    fn ctrl_t_walks_it_back() {
        let mut st = previewing("aa");
        st.preview_step(true);
        st.preview_step(true);
        st.preview_step(false);
        assert_eq!(
            st.preview(HAYSTACK)
                .step()
                .copied()
                .expect("a match")
                .target
                .start,
            6
        );
    }

    #[test]
    fn stepping_back_saturates_at_the_first_match() {
        // The counter starts at zero, so stopping there is the honest floor —
        // and it must not underflow.
        let mut st = previewing("aa");
        for _ in 0..5 {
            st.preview_step(false);
        }
        assert_eq!(st.prompt().expect("open").preview_skip(), 0);
        assert_eq!(
            st.preview(HAYSTACK)
                .step()
                .copied()
                .expect("a match")
                .target
                .start,
            0
        );
    }

    #[test]
    fn stepping_past_the_last_match_wraps() {
        // Wrapping falls out of `step`; this pins that it is not special-cased
        // away by the skip loop.
        let mut st = previewing("aa");
        for _ in 0..3 {
            st.preview_step(true);
        }
        assert_eq!(
            st.preview(HAYSTACK)
                .step()
                .copied()
                .expect("a match")
                .target
                .start,
            0,
            "three steps past three matches comes back to the first",
        );
    }

    #[test]
    fn editing_the_pattern_resets_the_step() {
        // The ordinal describes a match set the edit just replaced; carrying
        // it would land the preview somewhere never asked for.
        let mut st = previewing("aa");
        st.preview_step(true);
        assert_eq!(st.prompt().expect("open").preview_skip(), 1);

        st.push(' ');
        assert_eq!(
            st.prompt().expect("open").preview_skip(),
            0,
            "typing resets"
        );

        st.preview_step(true);
        st.backspace();
        assert_eq!(
            st.prompt().expect("open").preview_skip(),
            0,
            "backspace resets"
        );
    }

    #[test]
    fn every_pattern_edit_resets_the_step() {
        // One test per verb, so a new edit op that forgets the reset is caught
        // rather than inheriting the bug quietly.
        for (name, op) in [
            ("delete_at_caret", 0),
            ("delete_word_before_caret", 1),
            ("clear_before_caret", 2),
        ] {
            let mut st = previewing("aa bb");
            st.preview_step(true);
            match op {
                0 => {
                    st.move_caret(CaretMove::Start);
                    st.delete_at_caret();
                }
                1 => st.delete_word_before_caret(),
                _ => st.clear_before_caret(),
            }
            assert_eq!(
                st.prompt().expect("open").preview_skip(),
                0,
                "{name} must reset the preview step",
            );
        }
    }

    #[test]
    fn committing_lands_where_the_stepped_preview_showed() {
        // The whole promise of stepping: what you are looking at is what
        // Enter gives you.
        let mut st = previewing("aa");
        st.preview_step(true);
        let previewed = st
            .preview(HAYSTACK)
            .step()
            .copied()
            .expect("a match")
            .target
            .start;

        let prompt = st.prompt().expect("open");
        let (origin, skip) = (prompt.origin, prompt.preview_skip());
        assert_eq!(st.accept(HAYSTACK), Accepted::Committed);

        assert_eq!(previewed, 6, "the preview HAD moved");
        assert_eq!(
            st.commit_step_skipping(origin, skip)
                .expect("a match")
                .target
                .start,
            previewed,
            "Enter must land on the match the stepped preview was showing",
        );
    }

    // The byte_of_caret ↔ Ruler differential test moved to `escriba-memori`
    // (`law_the_caret_byte_offset_agrees_with_the_hand_rolled_conversion`)
    // along with the conversion itself. It was pinning `Prompt`'s private
    // copy; there is no longer a copy to pin.
}
