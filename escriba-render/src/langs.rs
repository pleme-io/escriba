//! Escriba-local language plugins — the languages the fleet syntax spine does
//! not ship yet.
//!
//! hikari owns the highlighting machinery: [`LangTable`] is the per-language
//! data, [`TableLexer`](hikari_core::langs::TableLexer) is the ONE scanner that
//! reads it, and [`TablePlugin`] adapts the pair to
//! [`LanguagePlugin`]. Nothing here re-implements any of that — a language is a
//! table, which is the whole point of the table backend. If a second escriba
//! language ever needs the same treatment, it is another `static` and another
//! row in [`escriba_local`], not another module.
//!
//! These plugins register **last** in
//! [`build_ecosystem`](crate::gpu::build_ecosystem), behind both hikari
//! backends, so the day hikari ships a `blue` grammar or table upstream this
//! one is skipped and the local copy retires itself without an edit.
//!
//! ## Why a table and not tree-sitter
//!
//! There is no `tree-sitter-blue` grammar — not in hikari-ts, not upstream. The
//! table backend is what every non-tree-sitter language in escriba already gets
//! (nix, yaml, lua, toml, …), so `.b` is served at exactly the tier its
//! neighbours are, and no `defmode :tree-sitter` claims a grammar that does not
//! exist.
//!
//! ## Why blue's keyword set is not read from `blue-lang-syntax`
//!
//! It would be the drift-proof spelling, and it was rejected on the numbers:
//!
//! - `blue-lang-syntax` is at **0.0.12** on crates.io. Under cargo's semver
//!   rules a `0.0.x` version is its own compatibility range — `"0.0.12"` means
//!   *exactly* 0.0.12 — so the dependency would not follow blue at all. It went
//!   through eleven releases in the three hours after first publish; escriba
//!   would be pinned to a dead one immediately and would need a manual bump per
//!   blue release. The "cannot drift" property is illusory at `0.0.x`.
//! - It would drag `tatara-lisp` 0.3.21 into a workspace pinned at 0.3.3, for
//!   all twenty-one crates, to import fifteen strings.
//! - The rest of blue is not on crates.io, and escriba **is** published
//!   (`escriba` 0.1.20), so a `git =` dependency is categorically out: it would
//!   freeze escriba's own publishing.
//!
//! So the table is transcribed, and [`BLUE_RESERVED_WORDS`] documents the exact
//! upstream definition it is transcribed from. The blue *toolchain* (`blue
//! lsp`, `blue fmt`) is wired the other way — as a binary on `$PATH` — which
//! needs no registry at all and follows blue automatically.

use hikari_core::{
    Language, LanguagePlugin, Selector,
    langs::{LangTable, TablePlugin},
};

/// The language id escriba resolves `.b` files to.
pub const BLUE: Language = Language("blue");

/// blue's reserved words, transcribed from `blue_lang_syntax::parse`:
/// `SURFACE_KEYWORDS` (the eleven form heads) plus the four block-structure
/// words `is_reserved_word` adds on top of it.
///
/// `true` / `false` / `nil` are the other three words upstream reserves and are
/// **deliberately absent**: hikari's table lexer already classifies them as
/// [`HlClass::Boolean`](hikari_core::HlClass::Boolean), and listing them here
/// would demote them to plain `Keyword` — a worse paint, not a better one.
/// `and` / `or` / `not` are also absent, and are not an oversight either: blue
/// spells those `&&` / `||` / `not(…)`, and `and`/`or` exist only as the
/// *lowered callee names* in `blue_lang_syntax::INFIX`, never as surface
/// keywords.
pub static BLUE_RESERVED_WORDS: &[&str] = &[
    // ── SURFACE_KEYWORDS ──────────────────────────────────────────────
    "assert",
    "case",
    "def",
    "defmacro",
    "fn",
    "if",
    "quote",
    "test",
    "unless",
    "unquote",
    "unquote_splice",
    // ── the block-structure words `is_reserved_word` adds ─────────────
    "do",
    "else",
    "elsif",
    "end",
];

/// blue's lexical shape.
///
/// `colon_keywords` is on because a blue symbol is spelled `:name` and lowers
/// to a tatara-lisp keyword — the same token hikari's lisp table paints as
/// [`HlClass::KeywordArg`](hikari_core::HlClass::KeywordArg), and the same
/// meaning. The hash-literal label `name:` (colon *after* the identifier) is a
/// different token and is NOT covered; it paints as an identifier plus
/// punctuation.
///
/// One string delimiter, because blue's lexer has one: `lex_string` is reached
/// from `b'"'` alone — no single-quoted strings, no heredocs. Interpolation
/// (`"n=#{x}"`) paints as one string span; the interpolated expression is not
/// separately highlighted.
///
/// No block comment, because blue has none — comments are `#` to end of line,
/// full stop.
pub static BLUE_TABLE: LangTable = LangTable {
    keywords: BLUE_RESERVED_WORDS,
    line_comments: &["#"],
    block_comment: None,
    string_delims: &['"'],
    colon_keywords: true,
};

/// How a document claims to be blue.
///
/// `Bluefile` is here because a Bluefile **is a blue program** — blue has no
/// separate manifest grammar — and it carries no extension, which is exactly
/// what [`Selector::Filename`] is for.
pub static BLUE_SELECTORS: &[Selector] =
    &[Selector::Extension("b"), Selector::Filename("Bluefile")];

/// Every language escriba registers on top of the two hikari backends.
#[must_use]
pub fn escriba_local() -> Vec<Box<dyn LanguagePlugin>> {
    vec![Box::new(TablePlugin {
        language: BLUE,
        selectors: BLUE_SELECTORS,
        table: &BLUE_TABLE,
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use hikari_core::{Ecosystem, HlClass};

    /// A registry holding only the escriba-local plugins — the unit under
    /// test, isolated from whatever hikari happens to ship.
    fn local_only() -> Ecosystem {
        let mut eco = Ecosystem::new();
        for p in escriba_local() {
            eco.register(p);
        }
        eco
    }

    fn classes(path: &str, src: &str) -> Vec<(String, HlClass)> {
        let hl = local_only().highlighter_for_path(path);
        hl.highlight(src)
            .into_iter()
            .map(|s| {
                (
                    src[s.span.start as usize..s.span.end as usize].to_string(),
                    s.class,
                )
            })
            .filter(|(t, _)| !t.trim().is_empty())
            .collect()
    }

    fn class_of(path: &str, src: &str, token: &str) -> Option<HlClass> {
        classes(path, src)
            .into_iter()
            .find(|(t, _)| t == token)
            .map(|(_, c)| c)
    }

    #[test]
    fn dot_b_resolves_to_blue() {
        assert_eq!(local_only().resolve("scratch.b"), BLUE);
        assert_eq!(local_only().resolve("/a/b/c/spec/strings.b"), BLUE);
    }

    #[test]
    fn bluefile_resolves_to_blue_by_name() {
        // No extension — the Filename selector is the only thing that can
        // claim it, and a Bluefile is a blue program.
        assert_eq!(local_only().resolve("Bluefile"), BLUE);
        assert_eq!(local_only().resolve("bidamas/retsu/Bluefile"), BLUE);
    }

    #[test]
    fn unrelated_paths_stay_plain() {
        assert_eq!(local_only().resolve("main.rs"), hikari_core::PLAIN_TEXT);
        assert_eq!(local_only().resolve("notes.txt"), hikari_core::PLAIN_TEXT);
        // `.blue` is NOT blue's extension — `.b` is. Claiming it would be a
        // guess, so the registry declines.
        assert_eq!(local_only().resolve("x.blue"), hikari_core::PLAIN_TEXT);
    }

    #[test]
    fn form_heads_and_block_words_paint_as_keywords() {
        let src = "def f(x)\n  if x\n    x\n  else\n    0\n  end\nend\n";
        for word in ["def", "if", "else", "end"] {
            assert_eq!(
                class_of("f.b", src, word),
                Some(HlClass::Keyword),
                "expected `{word}` to paint as a keyword",
            );
        }
    }

    #[test]
    fn hash_starts_a_line_comment() {
        let src = "# blue's own configuration\ndef f\nend\n";
        let spans = classes("blue.b", src);
        assert_eq!(
            spans.first().map(|(_, c)| *c),
            Some(HlClass::Comment { multiline: false }),
        );
        // The comment ends at the newline — `def` on the next line is live.
        assert_eq!(class_of("blue.b", src, "def"), Some(HlClass::Keyword));
    }

    #[test]
    fn symbols_paint_as_keyword_args() {
        // `:name` is a blue symbol, which lowers to a tatara-lisp keyword.
        assert_eq!(
            class_of("f.b", "x = :ok\n", ":ok"),
            Some(HlClass::KeywordArg),
        );
    }

    #[test]
    fn strings_and_numbers_paint() {
        let src = "x = \"hello\"\ny = 42\nz = 1.5\n";
        assert_eq!(class_of("f.b", src, "\"hello\""), Some(HlClass::Str));
        assert_eq!(
            class_of("f.b", src, "42"),
            Some(HlClass::Numeric { float: false }),
        );
        assert_eq!(
            class_of("f.b", src, "1.5"),
            Some(HlClass::Numeric { float: true }),
        );
    }

    #[test]
    fn literals_stay_boolean_not_keyword() {
        // Pins the deliberate omission documented on BLUE_RESERVED_WORDS: blue
        // reserves these three, but the table lexer paints them better than
        // `Keyword` would, so they must NOT be in the keyword list.
        for word in ["true", "false", "nil"] {
            assert_eq!(
                class_of("f.b", "x = true\ny = false\nz = nil\n", word),
                Some(HlClass::Boolean),
                "`{word}` must stay Boolean — see BLUE_RESERVED_WORDS",
            );
        }
    }

    #[test]
    fn lowered_infix_callees_are_not_surface_keywords() {
        // `and` / `or` / `not` are callee names in blue's INFIX table, not
        // surface keywords. Painting them as keywords would be a lie about the
        // language, so this pins them as ordinary identifiers.
        let src = "a = not(b)\n";
        assert_eq!(class_of("f.b", src, "not"), Some(HlClass::Variable));
        assert!(!BLUE_RESERVED_WORDS.contains(&"and"));
        assert!(!BLUE_RESERVED_WORDS.contains(&"or"));
    }

    #[test]
    fn reserved_word_set_is_the_transcribed_upstream_set() {
        // The set is closed and small, so pin it exactly. If blue adds a
        // surface keyword, this is the line that has to be visited — the
        // module docs explain why it is a transcription and not a dependency.
        assert_eq!(BLUE_RESERVED_WORDS.len(), 15);
        let mut sorted = BLUE_RESERVED_WORDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 15, "no duplicate reserved words");
    }
}
