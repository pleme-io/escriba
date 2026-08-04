//! Incremental buffer search for escriba — vim's `/` and `?`.
//!
//! # What this is
//!
//! The whole search feature as a **pure function of `(text, pattern, cursor)`**.
//! There is no I/O here: no buffer handle, no renderer, no clock. That is
//! deliberate and is what the ★★ default delivery method asks for — a pure core
//! split from side effects. Search happens to need *no* `Environment` seam at
//! all, because it genuinely has no effects to inject, so every behaviour below
//! is provable by a plain unit test with no mock and no fixture.
//!
//! The caller owns the buffer and the cursor; this crate answers "where should
//! the cursor go" and "which ranges should light up".
//!
//! # Feature parity with vim
//!
//! | vim | here |
//! |---|---|
//! | `/pat` `?pat` | [`SearchState::open`] + [`SearchState::accept`] |
//! | incremental highlight while typing | [`SearchState::preview`] |
//! | `n` / `N` | [`SearchState::repeat`] |
//! | `*` / `#` | [`SearchState::search_word`] |
//! | `hlsearch` / `:noh` | [`SearchState::highlights`] / [`SearchState::clear_highlight`] |
//! | `ignorecase` / `smartcase` | [`CaseMode`] |
//! | wrap + "hit BOTTOM" message | [`Wrapped`] |
//! | bare `/<CR>` repeats last | [`Accepted::ReusedPrevious`] |
//! | search history + arrows | [`SearchState::history_step`] |
//! | regex patterns | [`SearchPattern::compile`] |
//!
//! # Two invariants worth knowing
//!
//! **Offsets are chars, never bytes.** `regex` reports bytes; escriba's buffer
//! is char-addressed. The conversion happens once, inside [`find_all`], so no
//! consumer can mix them. A byte/char mix-up is invisible in ASCII tests and
//! wrong on the first accented character — [`engine`] tests it directly.
//!
//! **A prompt and a committed search are different things.** Cancelling `/xyz`
//! must not disturb the `foo` you were already searching. That falls out of the
//! type — the prompt is an `Option<Prompt>` holding its own text and origin, so
//! dropping it cannot touch the committed pattern.

pub mod engine;
pub mod pattern;
pub mod state;

pub use engine::{
    Direction, MAX_COUNT, MatchCount, SearchMatch, Step, Wrapped, find_all, step, step_inclusive,
    word_at,
};
pub use pattern::{CaseMode, PatternError, SearchPattern};
pub use state::{Accepted, HISTORY_LIMIT, Prompt, SearchState};

#[cfg(test)]
mod integration {
    //! End-to-end walks that cross module boundaries — the sequences a user
    //! actually performs, rather than one function at a time.

    use super::*;

    const DOC: &str = "let foo = 1;\nlet bar = foo + 2;\nprintln!(\"{foo}\");\n";

    #[test]
    fn a_full_search_session_start_to_finish() {
        let mut s = SearchState::new(CaseMode::Smart);

        // `/foo`
        s.open(Direction::Forward, 0);
        for c in "foo".chars() {
            s.push(c);
        }
        // Live preview lights the first hit before committing.
        assert!(s.preview(DOC).is_some());
        assert!(s.pattern().is_none(), "preview must not commit");

        // <CR>
        assert_eq!(s.accept(DOC), Accepted::Committed);
        assert_eq!(s.matches().len(), 3);
        assert_eq!(s.highlights().len(), 3);

        // Three matches, so three n's visit each in turn and the FOURTH wraps.
        let a = s.repeat(0, false).unwrap();
        let b = s.repeat(a.target.start, false).unwrap();
        let c = s.repeat(b.target.start, false).unwrap();
        assert!(a.target.start < b.target.start && b.target.start < c.target.start);
        assert_eq!(c.wrapped, Wrapped::No, "still walking forward");
        let d = s.repeat(c.target.start, false).unwrap();
        assert_eq!(d.wrapped, Wrapped::AtBottom, "past the last match it wraps");
        assert_eq!(d.target, a.target, "and lands back on the first");

        // :noh leaves the pattern usable
        s.clear_highlight();
        assert!(s.highlights().is_empty());
        assert!(s.repeat(0, false).is_some());
    }

    #[test]
    fn smartcase_end_to_end() {
        let mut s = SearchState::new(CaseMode::Smart);
        s.open(Direction::Forward, 0);
        for c in "LET".chars() {
            s.push(c);
        }
        s.accept(DOC);
        assert_eq!(s.matches().len(), 0, "uppercase pattern is case-sensitive");

        s.open(Direction::Forward, 0);
        for c in "let".chars() {
            s.push(c);
        }
        s.accept(DOC);
        assert_eq!(s.matches().len(), 2, "lowercase pattern ignores case");
    }

    #[test]
    fn regex_patterns_work_end_to_end() {
        let mut s = SearchState::new(CaseMode::Sensitive);
        s.open(Direction::Forward, 0);
        for c in r"let \w+ =".chars() {
            s.push(c);
        }
        assert_eq!(s.accept(DOC), Accepted::Committed);
        assert_eq!(s.matches().len(), 2);
    }

    #[test]
    fn star_then_n_walks_the_identifier() {
        let mut s = SearchState::new(CaseMode::Smart);
        // Cursor inside the first `foo`.
        let first = s.search_word(DOC, 4, Direction::Forward).unwrap();
        assert_eq!(s.pattern().unwrap().raw(), r"\bfoo\b");
        // Three `foo`s, but the third is inside `{foo}` — still a whole word,
        // since `{` and `}` are not word characters.
        assert_eq!(s.matches().len(), 3);
        let next = s.repeat(first.target.start, false).unwrap();
        assert!(next.target.start > first.target.start);
    }

    #[test]
    fn search_survives_an_edit_via_refresh() {
        let mut s = SearchState::new(CaseMode::Smart);
        s.open(Direction::Forward, 0);
        for c in "foo".chars() {
            s.push(c);
        }
        s.accept(DOC);
        assert_eq!(s.matches().len(), 3);

        let edited = DOC.replace("let foo = 1;", "let qux = 1;");
        s.refresh(&edited);
        assert_eq!(s.matches().len(), 2, "one occurrence renamed away");
    }

    #[test]
    fn unicode_document_reports_usable_char_offsets() {
        let doc = "función foo\nla foo café\n";
        let mut s = SearchState::new(CaseMode::Smart);
        s.open(Direction::Forward, 0);
        for c in "foo".chars() {
            s.push(c);
        }
        s.accept(doc);
        assert_eq!(s.matches().len(), 2);
        for m in s.matches() {
            let got: String = doc.chars().skip(m.start).take(m.len()).collect();
            assert_eq!(got, "foo", "char offsets must slice back to the match");
        }
    }
}
