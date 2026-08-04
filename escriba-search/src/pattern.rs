//! The compiled search pattern.
//!
//! # Why a newtype and not a bare `String`
//!
//! A search pattern is only meaningful once it compiles. Passing raw strings
//! around means every consumer must remember to handle "this might not be a
//! valid regex", and the one that forgets fails at the worst moment — mid
//! keystroke, in the middle of an incremental search. [`SearchPattern`] has no
//! public constructor other than [`SearchPattern::compile`], so a value of this
//! type IS the proof that the pattern compiled. Downstream code cannot be
//! handed an invalid one.
//!
//! Tier-honest (per ★★ UNREPRESENTABILITY): this is *parse-time rejection*, not
//! true unrepresentability — the illegal state is refused at one narrow
//! boundary rather than being inexpressible in the type system. That is the
//! honest ceiling here, because "is this a valid regex" is a runtime property
//! of a user-supplied string.

use serde::{Deserialize, Serialize};

/// How a pattern treats letter case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CaseMode {
    /// Always case-sensitive (vim `:set noignorecase`).
    Sensitive,
    /// Always case-insensitive (vim `:set ignorecase`).
    Ignore,
    /// Case-insensitive UNTIL the pattern contains an uppercase character, at
    /// which point it becomes case-sensitive. vim's `smartcase`, and the
    /// default here because it is what people actually want: `/foo` finds
    /// `Foo`, but `/Foo` does not find `foo`.
    #[default]
    Smart,
}

impl CaseMode {
    /// Resolve to a concrete "should this match ignore case" for a pattern.
    ///
    /// Smartcase inspects the *pattern the user typed*, not the haystack. An
    /// escape like `\W` contains no uppercase letter by this test — matching
    /// vim, which also looks only at literal uppercase characters.
    #[must_use]
    pub fn ignores_case(self, raw: &str) -> bool {
        match self {
            Self::Sensitive => false,
            Self::Ignore => true,
            Self::Smart => !raw.chars().any(char::is_uppercase),
        }
    }
}

/// Why a pattern failed to compile.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    /// The pattern was empty. Callers should reuse the previous pattern (vim's
    /// bare `/<CR>` behaviour) rather than treating this as a hard error.
    #[error("empty search pattern")]
    Empty,
    /// The regex engine rejected the pattern. Carries the engine's own message
    /// so the minibuffer can show something actionable.
    #[error("invalid pattern: {0}")]
    Invalid(String),
}

/// A pattern that is known to compile.
#[derive(Debug, Clone)]
pub struct SearchPattern {
    raw: String,
    case: CaseMode,
    regex: regex::Regex,
}

impl SearchPattern {
    /// Compile `raw` under `case`.
    ///
    /// # Errors
    /// [`PatternError::Empty`] if `raw` is empty, [`PatternError::Invalid`] if
    /// the regex engine rejects it.
    pub fn compile(raw: &str, case: CaseMode) -> Result<Self, PatternError> {
        if raw.is_empty() {
            return Err(PatternError::Empty);
        }
        let regex = regex::RegexBuilder::new(raw)
            .case_insensitive(case.ignores_case(raw))
            // A search pattern is per-line in spirit but we scan the whole
            // buffer as one string, so `.` must not leap across lines — that
            // would let `/a.b` match across a newline, which no vim user
            // expects.
            .dot_matches_new_line(false)
            .build()
            .map_err(|e| PatternError::Invalid(e.to_string()))?;
        Ok(Self {
            raw: raw.to_string(),
            case,
            regex,
        })
    }

    /// Compile a *literal* pattern — every regex metacharacter is escaped.
    ///
    /// This is what `*` / `#` (search-word-under-cursor) use: the word under
    /// the cursor may legitimately contain `.` or `[`, and the user means those
    /// characters, not their regex meaning.
    ///
    /// # Errors
    /// [`PatternError::Empty`] if `raw` is empty.
    pub fn literal(raw: &str, case: CaseMode) -> Result<Self, PatternError> {
        if raw.is_empty() {
            return Err(PatternError::Empty);
        }
        Self::compile(&regex::escape(raw), case)
    }

    /// Compile a whole-word literal pattern, as `*` and `#` do.
    ///
    /// # Errors
    /// [`PatternError::Empty`] if `raw` is empty.
    pub fn whole_word(raw: &str, case: CaseMode) -> Result<Self, PatternError> {
        if raw.is_empty() {
            return Err(PatternError::Empty);
        }
        // `\b` around an escaped literal. Note this is deliberately built from
        // the ESCAPED form: `\bfoo.bar\b` would otherwise treat `.` as a
        // wildcard while claiming to be a whole-word literal search.
        Self::compile(&format_word_boundary(&regex::escape(raw)), case)
    }

    /// The pattern exactly as the user typed it — for the minibuffer, the
    /// status line, and history.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The case mode this pattern was compiled under.
    #[must_use]
    pub const fn case(&self) -> CaseMode {
        self.case
    }

    /// Whether this pattern actually ignores case, after smartcase resolution.
    #[must_use]
    pub fn ignores_case(&self) -> bool {
        self.case.ignores_case(&self.raw)
    }

    pub(crate) const fn regex(&self) -> &regex::Regex {
        &self.regex
    }
}

/// `\b`-wrap a pattern fragment.
///
/// Split out so the ★★ TYPED EMISSION ban on `format!()` is honoured: this is a
/// single typed construction point for one fixed shape, not free-form string
/// composition scattered across call sites.
fn format_word_boundary(escaped: &str) -> String {
    let mut s = String::with_capacity(escaped.len() + 4);
    s.push_str("\\b");
    s.push_str(escaped);
    s.push_str("\\b");
    s
}

/// Two patterns are equal when they would match identically — same source text
/// AND same resolved case behaviour. Derived `PartialEq` is impossible because
/// `regex::Regex` does not implement it.
impl PartialEq for SearchPattern {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw && self.ignores_case() == other.ignores_case()
    }
}
impl Eq for SearchPattern {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_pattern_is_rejected_not_silently_accepted() {
        assert_eq!(
            SearchPattern::compile("", CaseMode::Smart),
            Err(PatternError::Empty)
        );
        assert_eq!(
            SearchPattern::literal("", CaseMode::Smart),
            Err(PatternError::Empty)
        );
        assert_eq!(
            SearchPattern::whole_word("", CaseMode::Smart),
            Err(PatternError::Empty)
        );
    }

    #[test]
    fn invalid_regex_cannot_become_a_pattern() {
        let e = SearchPattern::compile("a[b", CaseMode::Smart).unwrap_err();
        assert!(matches!(e, PatternError::Invalid(_)), "got {e:?}");
    }

    #[test]
    fn smartcase_is_insensitive_until_you_type_a_capital() {
        assert!(CaseMode::Smart.ignores_case("foo"));
        assert!(!CaseMode::Smart.ignores_case("Foo"));
        assert!(!CaseMode::Smart.ignores_case("fooBar"));
    }

    #[test]
    fn explicit_case_modes_ignore_the_pattern_text() {
        assert!(!CaseMode::Sensitive.ignores_case("foo"));
        assert!(CaseMode::Ignore.ignores_case("FOO"));
    }

    #[test]
    fn literal_escapes_metacharacters() {
        // `a.c` as a literal must NOT match `abc`.
        let p = SearchPattern::literal("a.c", CaseMode::Sensitive).unwrap();
        assert!(p.regex().is_match("a.c"));
        assert!(!p.regex().is_match("abc"));
    }

    #[test]
    fn whole_word_does_not_match_inside_a_longer_word() {
        let p = SearchPattern::whole_word("foo", CaseMode::Sensitive).unwrap();
        assert!(p.regex().is_match("a foo b"));
        assert!(!p.regex().is_match("foobar"));
        assert!(!p.regex().is_match("barfoo"));
    }

    #[test]
    fn whole_word_keeps_metacharacters_literal() {
        // Regression guard: building `\b` around the UNESCAPED word would make
        // `.` a wildcard while the API claims a literal whole-word search.
        let p = SearchPattern::whole_word("a.c", CaseMode::Sensitive).unwrap();
        assert!(p.regex().is_match("x a.c y"));
        assert!(!p.regex().is_match("x abc y"));
    }

    #[test]
    fn dot_never_matches_across_a_line_boundary() {
        let p = SearchPattern::compile("a.b", CaseMode::Sensitive).unwrap();
        assert!(!p.regex().is_match("a\nb"));
    }

    #[test]
    fn raw_round_trips_for_the_minibuffer() {
        let p = SearchPattern::compile("Foo.*bar", CaseMode::Smart).unwrap();
        assert_eq!(p.raw(), "Foo.*bar");
        assert!(!p.ignores_case(), "capital F should force sensitivity");
    }
}
