//! `textDocument/semanticTokens/full` → [`SemanticSpan`].
//!
//! # Why a server's colour is worth having at all
//!
//! escriba already colours every buffer through hikari, from a lexer table.
//! A lexer sees SHAPE: it knows `foo` is an identifier and cannot know whether
//! it is a function, a local, or a field, because that answer needs the
//! language's own binding rules. A language server has those rules — blue's
//! `semantic_tokens` distinguishes `Function` (a call head, or the name after
//! `def`) from `Variable` from `Property` using its real lexer and its real
//! notion of a declaration. Consuming that is the difference between "blue
//! files are coloured" and "blue files are coloured by blue".
//!
//! # The legend is PER SERVER, and that is the whole reason this is a type
//!
//! LSP does not fix the meaning of a token-type index. Each server publishes
//! its own `legend.tokenTypes` array in the `initialize` reply, and index `3`
//! means whatever that array's fourth entry names — `number` for blue, `enum`
//! for rust-analyzer, something else tomorrow. **A hardcoded index table is
//! therefore not a shortcut, it is a mis-colouring that only shows up against
//! the second server you try.** [`Legend::from_capabilities`] decodes the
//! array the server actually sent, from the `initialize` reply escriba already
//! kept in [`ServerCaps::raw`](crate::conn::ServerCaps::raw) — so this costs no
//! extra round trip.
//!
//! # The delta encoding
//!
//! The wire is a flat `Vec<u32>`, five integers per token:
//! `(deltaLine, deltaStartChar, length, tokenType, tokenModifiers)`. Both
//! deltas are relative to the PREVIOUS token, and `deltaStartChar` is relative
//! to the previous token's start **only when the two are on the same line** —
//! otherwise it is absolute. Getting that rule wrong produces a paint that
//! looks plausible near the top of the file and drifts further right the
//! further down you read, which is why [`decode`] is the gate this module
//! actually cares about.
//!
//! Columns on the wire are UTF-16 code units. [`SemanticSpan`] counts `char`s.
//! The conversion is [`zahyou`]'s, the same one `findings.rs` uses, and it is
//! the one thing here that fails SILENTLY on real text: on an ASCII file the
//! two numbers are identical, so a decoder that skipped the conversion passes
//! every test that does not contain an astral-plane character and paints every
//! token after an emoji one column to the right.

use escriba_madoguchi::{HlClass, SemanticSpan};

use crate::{Lines, Position};

/// One server's `tokenTypes` legend, lowered to escriba's classes.
///
/// Positional: entry `i` is what token-type index `i` means for THIS server.
/// `None` at a position is a type escriba has no class for — kept as a hole
/// rather than dropped, because dropping it would shift every later index and
/// silently repaint the whole file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Legend {
    types: Vec<Option<HlClass>>,
}

impl Legend {
    /// Read the legend out of an `initialize` reply's `capabilities` object.
    ///
    /// `None` when the server advertises no `semanticTokensProvider`, or one
    /// with no legend — both mean "do not ask", and returning an EMPTY legend
    /// instead would mean "ask, then discard every answer", which looks
    /// identical from the outside and costs a round trip per open.
    #[must_use]
    pub fn from_capabilities(caps: &serde_json::Value) -> Option<Self> {
        let names = caps
            .get("semanticTokensProvider")?
            .get("legend")?
            .get("tokenTypes")?
            .as_array()?;
        Some(Self {
            types: names
                .iter()
                .map(|v| v.as_str().and_then(class_of))
                .collect(),
        })
    }

    /// What token-type index `i` means, or `None` when the index is out of
    /// range or names a type escriba has no class for.
    ///
    /// An out-of-range index is a server bug (or a legend/data mismatch after
    /// a restart), and the honest answer is "skip this token" — not a panic,
    /// and not a fallback class, which would paint a word a confident wrong
    /// colour.
    #[must_use]
    pub fn class(&self, i: u32) -> Option<HlClass> {
        self.types.get(i as usize).copied().flatten()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.types.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

/// The token types escriba declares it can render, in `initialize`.
///
/// **The domain of [`class_of`], and the same list `initialize_params` sends** —
/// one array, read by both, because the alternative is two spellings of one
/// claim that disagree silently. Declaring a type we drop on the floor is a
/// lie to the server; omitting one we can paint costs colour for no reason,
/// and a server is entitled to filter its answer to what the client asked for.
///
/// `every_declared_type_resolves_to_a_class` is what keeps it honest from the
/// other direction — a name here that `class_of` does not know would be a
/// declaration with no renderer behind it.
pub const RENDERABLE_TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "event",
    "function",
    "method",
    "macro",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
    "decorator",
];

/// One LSP standard token type name → escriba's highlight class.
///
/// The names are the spec's `SemanticTokenTypes` set. Several collapse onto one
/// class on purpose — `class` / `enum` / `interface` / `struct` /
/// `typeParameter` are all "a type" as far as a colour is concerned, and
/// hikari's `HlClass` has one `Type`. That collapse is a rendering decision,
/// not a loss: `ChromeSyntax` would resolve them to the same `info` role
/// anyway.
///
/// A name this does not know returns `None` and its tokens go unpainted, which
/// leaves them the buffer's foreground — the same thing every tree-sitter theme
/// does with a capture it has no rule for. Inventing a class for an unknown
/// name would be a guess rendered as a fact.
fn class_of(name: &str) -> Option<HlClass> {
    Some(match name {
        "namespace" => HlClass::Namespace,
        "type" | "class" | "enum" | "interface" | "struct" | "typeParameter" => HlClass::Type,
        "parameter" => HlClass::KeywordArg,
        "variable" | "property" => HlClass::Variable,
        // An enum member is a named constant, which is what `Constant` paints.
        "enumMember" => HlClass::Constant,
        "event" | "function" | "method" => HlClass::Function,
        "macro" => HlClass::Special,
        "keyword" => HlClass::Keyword,
        // A modifier (`pub`, `async`) and a decorator (`@foo`, `#[derive]`)
        // are both attached-to-a-declaration marks, which is what `Attribute`
        // paints. One arm, because two arms with one body is a claim that
        // they might diverge and they will not.
        "modifier" | "decorator" => HlClass::Attribute,
        "comment" => HlClass::Comment { multiline: false },
        "string" => HlClass::Str,
        "number" => HlClass::Numeric { float: false },
        "regexp" => HlClass::Escape,
        "operator" => HlClass::Operator,
        _ => return None,
    })
}

/// Undo the delta encoding, resolve each type through `legend`, and convert
/// UTF-16 columns to `char` columns against `text`.
///
/// `text` MUST be the document the server answered about — the same
/// requirement, for the same reason, as
/// [`to_findings`](crate::findings::to_findings): a UTF-16 column converted
/// against a different revision is confidently wrong rather than absent.
///
/// A trailing partial quintuple is ignored rather than treated as an error:
/// the tokens before it are still correct, and refusing the whole paint over a
/// truncated tail would lose a screen of correct colour to one bad integer.
#[must_use]
pub fn decode(data: &[u32], legend: &Legend, text: &str) -> Vec<SemanticSpan> {
    let lines = Lines::new(text);
    let mut out = Vec::with_capacity(data.len() / 5);
    // The running cursor the deltas are relative to. Both start at zero: the
    // first token's `deltaLine` is its absolute line, and its `deltaStartChar`
    // is absolute because `deltaLine` cannot be anything but "same line as the
    // imaginary token at 0:0" when it is zero — which is exactly what the
    // same-line rule below computes.
    let mut line: u32 = 0;
    let mut start: u32 = 0;

    for q in data.chunks_exact(5) {
        let (d_line, d_start, length, ty) = (q[0], q[1], q[2], q[3]);
        // `q[4]` is the modifier bitset. Deliberately unread: escriba has no
        // modifier-aware class (there is no "declaration" colour in
        // `HlClass`), and decoding it into a value nothing consumes would be a
        // field to keep in sync with no reader.
        line = line.saturating_add(d_line);
        start = if d_line == 0 {
            start.saturating_add(d_start)
        } else {
            d_start
        };
        // Resolve AFTER advancing the cursor. An unknown type must skip the
        // TOKEN, never the delta — the deltas are a running sum, so continuing
        // before updating `line`/`start` would shift every token after it.
        let Some(class) = legend.class(ty) else {
            continue;
        };
        // UTF-16 → chars. Done through the two absolute endpoints rather than
        // by converting the length on its own: a length is not convertible
        // without knowing where it starts.
        let a = lines.to_char(text, Position::new(line, start));
        let b = lines.to_char(text, Position::new(line, start.saturating_add(length)));
        let len_chars = b.character.saturating_sub(a.character);
        if len_chars == 0 {
            continue;
        }
        out.push(SemanticSpan {
            line: a.line,
            start_char: a.character,
            len_chars,
            class,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Legend, class_of, decode};
    use escriba_madoguchi::HlClass;

    /// blue's real legend, in blue's real wire order
    /// (`blue-lang-lsp::tokens::SemanticTokenType::ALL`).
    fn blue_caps() -> serde_json::Value {
        serde_json::json!({
            "textDocumentSync": 1,
            "semanticTokensProvider": {
                "legend": {
                    "tokenTypes": [
                        "keyword", "comment", "string", "number", "operator",
                        "variable", "function", "property", "enumMember"
                    ],
                    "tokenModifiers": ["declaration"],
                },
                "full": true,
            },
        })
    }

    /// The legend is READ, never assumed — index 6 is `function` for blue and
    /// would be something else for any other server.
    #[test]
    fn a_legend_is_decoded_from_the_servers_own_wire_order() {
        let l = Legend::from_capabilities(&blue_caps()).expect("blue advertises a legend");
        assert_eq!(l.len(), 9);
        assert_eq!(l.class(0), Some(HlClass::Keyword));
        assert_eq!(l.class(6), Some(HlClass::Function));
        assert_eq!(l.class(5), Some(HlClass::Variable));
    }

    /// A server with no provider must produce no legend, so the caller does
    /// not spend a round trip asking a question that has no answer.
    #[test]
    fn a_server_without_the_provider_has_no_legend() {
        let caps = serde_json::json!({ "hoverProvider": true });
        assert_eq!(Legend::from_capabilities(&caps), None);
        // A provider with no legend is equally unusable.
        let bare = serde_json::json!({ "semanticTokensProvider": { "full": true } });
        assert_eq!(Legend::from_capabilities(&bare), None);
    }

    /// **The gate this module exists for.**
    ///
    /// Three tokens, hand-decoded on paper from the quintuples, on a document
    /// whose SECOND LINE STARTS WITH AN EMOJI — so the UTF-16 columns the
    /// server sends and the `char` columns escriba paints in genuinely differ.
    /// The expected values below are computed by hand, not read off the
    /// decoder, because a gate derived from the thing it checks is a tautology
    /// (this repo's testing note, and it applies exactly here).
    ///
    /// The document:
    ///
    /// ```text
    /// line 0:  def add          →  "def" at chars 0..3, "add" at chars 4..7
    /// line 1:  🎉 x             →  the emoji is 1 char and TWO utf-16 units,
    ///                              so "x" is utf-16 column 3 and char column 2
    /// ```
    ///
    /// Wire (five per token, deltas):
    ///
    /// | quintuple | meaning | absolute |
    /// |---|---|---|
    /// | `0,0,3,0,0` | line += 0, new line ⇒ start = 0 | line 0, utf16 0, len 3, keyword |
    /// | `0,4,3,6,1` | same line ⇒ start = 0 + 4 | line 0, utf16 4, len 3, function |
    /// | `1,3,1,5,0` | line += 1, NEW LINE ⇒ start is ABSOLUTE 3 | line 1, utf16 3, len 1, variable |
    ///
    /// Hand-computed expectation: `(0,0,3,Keyword)`, `(0,4,3,Function)`,
    /// `(1,2,1,Variable)` — note the third's column 2, not the 3 the wire
    /// carries.
    ///
    /// RED RUN 2026-08-12, three separate mutations, each failing this test
    /// alone:
    ///   1. `start = start.saturating_add(d_start)` unconditionally (drop the
    ///      same-line rule) → third token lands at char 5, not 2.
    ///   2. `start_char: start` (skip the UTF-16 → char conversion) → third
    ///      token lands at char 3, not 2 — and NOTHING ELSE in the suite
    ///      moves, which is the point of putting an emoji in the fixture.
    ///   3. `line = d_line` (treat the line delta as absolute) → the third
    ///      token still lands on line 1 here, so this mutation is caught by
    ///      `a_run_of_same_line_tokens_accumulates` instead. Recorded because
    ///      a break attempt that does not go red means the mutation or the
    ///      gate is wrong, and finding out which is the discipline.
    #[test]
    fn the_delta_decode_matches_a_hand_computed_absolute_position() {
        let text = "def add\n🎉 x\n";
        let legend = Legend::from_capabilities(&blue_caps()).unwrap();
        let data = [0, 0, 3, 0, 0, 0, 4, 3, 6, 1, 1, 3, 1, 5, 0];

        let got = decode(&data, &legend, text);
        let as_tuple: Vec<(u32, u32, u32, HlClass)> = got
            .iter()
            .map(|s| (s.line, s.start_char, s.len_chars, s.class))
            .collect();
        assert_eq!(
            as_tuple,
            vec![
                (0, 0, 3, HlClass::Keyword),
                (0, 4, 3, HlClass::Function),
                (1, 2, 1, HlClass::Variable),
            ],
        );

        // And the columns really do select the intended text in escriba's own
        // coordinates — the check that would have caught a conversion that was
        // self-consistently wrong.
        let line1: Vec<char> = "🎉 x".chars().collect();
        assert_eq!(line1[2..3].iter().collect::<String>(), "x");
        let line0: Vec<char> = "def add".chars().collect();
        assert_eq!(line0[4..7].iter().collect::<String>(), "add");
    }

    /// The same-line accumulation, isolated: three tokens on one line whose
    /// starts are a running sum. A decoder treating `deltaStartChar` as
    /// absolute puts all three at small columns near the left margin.
    #[test]
    fn a_run_of_same_line_tokens_accumulates() {
        let legend = Legend::from_capabilities(&blue_caps()).unwrap();
        // "a + b" on line 2 — `a` at 0, `+` at 2, `b` at 4.
        let data = [2, 0, 1, 5, 0, 0, 2, 1, 4, 0, 0, 2, 1, 5, 0];
        let got = decode(&data, &legend, "\n\na + b\n");
        assert_eq!(
            got.iter()
                .map(|s| (s.line, s.start_char))
                .collect::<Vec<_>>(),
            vec![(2, 0), (2, 2), (2, 4)],
        );
    }

    /// **The second gate.** An index past the end of the legend must be
    /// SKIPPED — not panic (a `Vec` index would), and not fall back to a class
    /// (which paints a word a confidently wrong colour).
    ///
    /// The token after it must still land correctly, which is the half that
    /// actually matters: the deltas are a running sum, so a `continue` placed
    /// before the cursor update would shift everything downstream.
    ///
    /// **The skipped token deliberately carries NON-ZERO deltas on BOTH axes**
    /// (`deltaLine: 1`, `deltaStartChar: 2`), and that is not decoration. The
    /// first version of this fixture put the unknown token at `0,0` — where
    /// skipping the cursor update and performing it are the same operation, so
    /// the mutation below could not violate the property and the "proof"
    /// stayed green. This repo's testing note names that trap exactly; it was
    /// hit here and is recorded rather than quietly fixed.
    ///
    /// RED RUN 2026-08-12, both mutations:
    ///   1. `legend.class(ty)` → `self.types[ty as usize]` panics with
    ///      `index out of bounds: the len is 9 but the index is 99`.
    ///   2. moving the `continue` above the `line`/`start` update yields
    ///      **an empty list**. The surviving token is computed against line 0
    ///      (`"x"`, one char) instead of line 1, its UTF-16 column 4 clamps to
    ///      that line's end, `len_chars` collapses to zero and the token is
    ///      dropped — so the whole file loses its colour from one unknown
    ///      type. A skipped token must lose its TOKEN and never its DELTA.
    #[test]
    fn an_out_of_range_token_type_is_skipped_without_disturbing_the_next() {
        let legend = Legend::from_capabilities(&blue_caps()).unwrap();
        // token A: line 0+1, col 2, len 2, type 99 (out of range)
        // token B: same line, +4 ⇒ col 6, len 3, type 6 (function)
        let data = [1, 2, 2, 99, 0, 0, 4, 3, 6, 0];
        let got = decode(&data, &legend, "x\n  ab  fun\n");
        assert_eq!(got.len(), 1, "the unknown type is skipped, not painted");
        assert_eq!(
            (
                got[0].line,
                got[0].start_char,
                got[0].len_chars,
                got[0].class
            ),
            (1, 6, 3, HlClass::Function),
            "the surviving token keeps the cursor the skipped one advanced",
        );
        // And it really does select `fun`, not the `ab` two columns left.
        let line1: Vec<char> = "  ab  fun".chars().collect();
        assert_eq!(line1[6..9].iter().collect::<String>(), "fun");
    }

    /// A legend naming a type escriba has no class for keeps its SLOT. A
    /// decoder that dropped unknown names while building the legend would
    /// shift every later index, repainting the whole file from one unfamiliar
    /// word in the server's array.
    #[test]
    fn an_unknown_legend_name_holds_its_position() {
        let caps = serde_json::json!({
            "semanticTokensProvider": {
                "legend": { "tokenTypes": ["lifetime", "keyword"], "tokenModifiers": [] }
            }
        });
        let l = Legend::from_capabilities(&caps).unwrap();
        assert_eq!(l.len(), 2, "the hole is kept");
        assert_eq!(l.class(0), None, "escriba has no class for a lifetime");
        assert_eq!(
            l.class(1),
            Some(HlClass::Keyword),
            "and keyword is still index 1, not index 0",
        );
    }

    /// A truncated tail loses only itself.
    #[test]
    fn a_partial_quintuple_is_ignored_rather_than_discarding_the_paint() {
        let legend = Legend::from_capabilities(&blue_caps()).unwrap();
        let data = [0, 0, 3, 0, 0, 0, 4]; // one whole token, then two strays
        assert_eq!(decode(&data, &legend, "def add\n").len(), 1);
    }

    /// A zero-length token paints nothing, so it must not become a run —
    /// `set_rich_text` is fed a partition and an empty piece in it is a shaping
    /// hazard, not a no-op.
    #[test]
    fn a_zero_length_token_is_dropped() {
        let legend = Legend::from_capabilities(&blue_caps()).unwrap();
        assert!(decode(&[0, 0, 0, 0, 0], &legend, "def\n").is_empty());
    }

    /// Every type escriba DECLARES it renders must actually resolve to a
    /// class.
    ///
    /// The list is sent to the server in `initialize`, and a server is
    /// entitled to filter its answer down to what the client asked for. A name
    /// here with no `class_of` arm behind it is therefore a request for tokens
    /// that are then silently discarded — the server does the work, the wire
    /// carries the bytes, and nothing is painted.
    ///
    /// RED RUN 2026-08-12: adding `"lifetime"` to `RENDERABLE_TOKEN_TYPES`
    /// fails with `declared but unrenderable: "lifetime"`.
    #[test]
    fn every_declared_type_resolves_to_a_class() {
        for name in super::RENDERABLE_TOKEN_TYPES {
            assert!(
                class_of(name).is_some(),
                "declared but unrenderable: {name:?}",
            );
        }
    }

    /// The name table collapses the type family and refuses the unknown.
    #[test]
    fn the_standard_names_lower_onto_classes() {
        assert_eq!(class_of("class"), Some(HlClass::Type));
        assert_eq!(class_of("struct"), Some(HlClass::Type));
        assert_eq!(class_of("method"), Some(HlClass::Function));
        assert_eq!(
            class_of("comment"),
            Some(HlClass::Comment { multiline: false })
        );
        assert_eq!(class_of("lifetime"), None, "not a guess");
        assert_eq!(class_of(""), None);
    }
}
