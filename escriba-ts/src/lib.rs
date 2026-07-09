//! `escriba-ts` — tree-sitter multi-grammar host.
//!
//! Phase-1.B scope: one `GrammarRegistry` keyed by language-name string,
//! shipped with tree-sitter-rust; per-buffer `BufferParser` that keeps a
//! `tree_sitter::Tree` and re-parses on edits. Highlight capture via
//! `tree_sitter_highlight::HighlightConfiguration` → `Semantic` bindings
//! from `caixa_theme`. Phase 2: caixa-ts grammar + markdown + bash + incremental
//! `Tree::edit()` per `Edit`.

extern crate self as escriba_ts;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The fleet semantic highlight vocabulary — **owned by `hikari-token`** and
/// re-exported here (dedup: the fleet grew several byte-identical copies of
/// this 16-variant enum; the canonical one now lives in one crate and every
/// consumer inherits changes on the next dep bump). `hikari_token::Semantic`
/// carries the total `From<Semantic> for HlClass` morphism, so a highlight
/// span produced here lowers into the hikari spine with no local mapping.
pub use hikari_token::Semantic;
use tree_sitter::{InputEdit, Language, Parser, Point, Tree};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

#[derive(Debug, Error)]
pub enum TsError {
    #[error("grammar not registered: {0}")]
    Unknown(String),
    #[error("tree-sitter: {0}")]
    Ts(String),
}

pub type Result<T> = std::result::Result<T, TsError>;

/// A registered grammar — name, language, highlights config.
pub struct Grammar {
    pub name: String,
    pub language: Language,
    pub config: HighlightConfiguration,
    /// File extensions (no dot) this grammar claims. Mutable at
    /// runtime so `defmode :extensions (…)` declarations can
    /// broaden the mapping without recompilation.
    pub extensions: Vec<String>,
}

/// Registry — language-name → Grammar.
pub struct GrammarRegistry {
    grammars: HashMap<String, Grammar>,
    /// The highlight name space — indices into this vector are what
    /// `HighlightEvent::HighlightStart(…)` returns.
    pub highlight_names: Vec<&'static str>,
}

impl GrammarRegistry {
    #[must_use]
    pub fn builtin() -> Result<Self> {
        let highlight_names = canonical_highlight_names();
        let mut grammars = HashMap::new();

        // Rust.
        let lang: Language = tree_sitter_rust::language();
        let mut cfg = HighlightConfiguration::new(
            lang.clone(),
            "rust",
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        )
        .map_err(|e| TsError::Ts(format!("rust: {e}")))?;
        cfg.configure(&highlight_names);
        grammars.insert(
            "rust".to_string(),
            Grammar {
                name: "rust".to_string(),
                language: lang,
                config: cfg,
                extensions: vec!["rs".to_string()],
            },
        );

        Ok(Self {
            grammars,
            highlight_names,
        })
    }

    #[must_use]
    pub fn get(&self, language: &str) -> Option<&Grammar> {
        self.grammars.get(language)
    }

    /// Look up a language by file extension (e.g. `"rs"` → `"rust"`).
    #[must_use]
    pub fn from_extension(&self, ext: &str) -> Option<&Grammar> {
        self.grammars
            .values()
            .find(|g| g.extensions.iter().any(|e| e == ext))
    }

    /// Broaden an existing grammar's extension list — used by
    /// `escriba-lisp::apply_plan_to_grammar_extensions` so a `defmode`
    /// in the rc can teach the registry that `.rs.in` is rust too.
    /// Returns `true` iff the grammar was registered; `false` means
    /// the caller referenced a language the registry doesn't know.
    pub fn add_extension(&mut self, language: &str, ext: impl Into<String>) -> bool {
        if let Some(g) = self.grammars.get_mut(language) {
            let ext = ext.into();
            if !g.extensions.iter().any(|e| *e == ext) {
                g.extensions.push(ext);
            }
            true
        } else {
            false
        }
    }

    /// Iterate every registered language name — used by diagnostics
    /// in `--list-rc` and the planned `escriba doctor` subcommand.
    pub fn languages(&self) -> impl Iterator<Item = &str> {
        self.grammars.keys().map(String::as_str)
    }
}

/// Per-buffer parser + last-parsed tree.
pub struct BufferParser {
    language: String,
    parser: Parser,
    tree: Option<Tree>,
}

impl BufferParser {
    pub fn new(language: &str, registry: &GrammarRegistry) -> Result<Self> {
        let grammar = registry
            .get(language)
            .ok_or_else(|| TsError::Unknown(language.to_string()))?;
        let mut parser = Parser::new();
        parser
            .set_language(&grammar.language)
            .map_err(|e| TsError::Ts(e.to_string()))?;
        Ok(Self {
            language: language.to_string(),
            parser,
            tree: None,
        })
    }

    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Re-parse `src` from scratch. Passes `None` as the old tree on purpose:
    /// tree-sitter's incremental path requires the old tree to have been
    /// `Tree::edit`-ed to reflect exactly what changed, and this method is the
    /// unstructured "the whole buffer is now `src`" path where the edit is
    /// unknown. Handing `parse()` an *un-edited* old tree against changed source
    /// violates tree-sitter's contract and can yield an incorrect tree — so the
    /// correct answer for an unknown delta is a full parse. Callers that know
    /// the edit use [`reparse_edit`](Self::reparse_edit) for the incremental
    /// (`Tree::edit` + reuse) path.
    pub fn reparse(&mut self, src: &str) -> Result<()> {
        self.tree = self.parser.parse(src, None);
        Ok(())
    }

    /// Incrementally re-parse after splicing `[start_byte, old_end_byte)` of
    /// `old_src` to produce `new_src` (M5, `theory/ESCRIBA.md` §X). Edits the
    /// retained tree by the splice ([`TsEdit`]) so tree-sitter reuses every
    /// unchanged subtree and reparses only the affected span — `O(edit)`, not
    /// `O(document)`. With no prior tree it falls back to a full parse. The
    /// resulting tree is identical to a full parse of `new_src` (the
    /// differential-equivalence invariant, tested).
    pub fn reparse_edit(
        &mut self,
        old_src: &str,
        new_src: &str,
        start_byte: usize,
        old_end_byte: usize,
    ) -> Result<()> {
        if self.tree.is_some() {
            let edit = TsEdit::from_splice(old_src, new_src, start_byte, old_end_byte);
            if let Some(tree) = self.tree.as_mut() {
                tree.edit(&edit.to_input_edit());
            }
            self.tree = self.parser.parse(new_src, self.tree.as_ref());
        } else {
            self.tree = self.parser.parse(new_src, None);
        }
        Ok(())
    }

    #[must_use]
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }
}

/// A typed, byte-based description of one contiguous splice, for incremental
/// tree-sitter reparse. tree-sitter's native unit is the byte offset + a
/// `(row, byte-column)` point, so this converts from a plain source splice —
/// no tree-sitter type crosses the caller boundary. Construct with
/// [`from_splice`](TsEdit::from_splice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TsEdit {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    /// `(row, byte-column)` of the splice start (identical in old + new — the
    /// prefix is unchanged).
    pub start_point: (usize, usize),
    pub old_end_point: (usize, usize),
    pub new_end_point: (usize, usize),
}

impl TsEdit {
    /// Compute the splice that turns `old` into `new` by replacing
    /// `old[start_byte..old_end_byte]`. The unchanged suffix
    /// (`old[old_end_byte..]`) has the same length in `new`, so
    /// `new_end_byte = new.len() - (old.len() - old_end_byte)`. Points are
    /// derived by counting the `\n`s in the respective text before each byte
    /// (tree-sitter columns are byte offsets within the row).
    #[must_use]
    pub fn from_splice(old: &str, new: &str, start_byte: usize, old_end_byte: usize) -> Self {
        let new_end_byte = new.len() - (old.len() - old_end_byte);
        Self {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_point: byte_to_point(old, start_byte),
            old_end_point: byte_to_point(old, old_end_byte),
            new_end_point: byte_to_point(new, new_end_byte),
        }
    }

    fn to_input_edit(self) -> InputEdit {
        let pt = |(row, column): (usize, usize)| Point { row, column };
        InputEdit {
            start_byte: self.start_byte,
            old_end_byte: self.old_end_byte,
            new_end_byte: self.new_end_byte,
            start_position: pt(self.start_point),
            old_end_position: pt(self.old_end_point),
            new_end_position: pt(self.new_end_point),
        }
    }
}

/// `(row, byte-column)` of `byte` within `text` — row = number of `\n` before
/// `byte`, column = bytes since the last `\n`. tree-sitter's point columns are
/// byte offsets within the line, not char offsets.
#[must_use]
fn byte_to_point(text: &str, byte: usize) -> (usize, usize) {
    let byte = byte.min(text.len());
    let prefix = &text[..byte];
    let row = prefix.bytes().filter(|&b| b == b'\n').count();
    let col = prefix.len() - prefix.rfind('\n').map_or(0, |i| i + 1);
    (row, col)
}

/// A colored text span — byte range + canonical semantic bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    pub semantic: Semantic,
}

/// Compute highlight spans over `src` using the given grammar.
pub fn highlight(
    src: &str,
    grammar: &Grammar,
    registry: &GrammarRegistry,
) -> Result<Vec<HighlightSpan>> {
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(&grammar.config, src.as_bytes(), None, |_| None)
        .map_err(|e| TsError::Ts(e.to_string()))?;

    let mut stack: Vec<usize> = Vec::new();
    let mut spans: Vec<HighlightSpan> = Vec::new();
    let mut run_start: Option<(usize, usize)> = None;

    for ev in events {
        let ev = ev.map_err(|e| TsError::Ts(e.to_string()))?;
        match ev {
            HighlightEvent::HighlightStart(h) => {
                stack.push(h.0);
            }
            HighlightEvent::HighlightEnd => {
                stack.pop();
                run_start = None;
            }
            HighlightEvent::Source { start, end } => {
                if let Some(&top) = stack.last() {
                    let sem = highlight_index_to_semantic(top, &registry.highlight_names);
                    match run_start {
                        Some((rs, _)) if rs == start => {}
                        _ => {
                            spans.push(HighlightSpan {
                                start,
                                end,
                                semantic: sem,
                            });
                            run_start = Some((start, end));
                        }
                    }
                }
            }
        }
    }

    Ok(spans)
}

/// The canonical highlight-name namespace every grammar is configured against.
/// Indices into this vector map to `Semantic` buckets.
fn canonical_highlight_names() -> Vec<&'static str> {
    vec![
        "keyword",
        "function",
        "function.call",
        "function.method",
        "type",
        "type.builtin",
        "constant",
        "constant.builtin",
        "string",
        "string.special",
        "number",
        "boolean",
        "comment",
        "operator",
        "punctuation",
        "punctuation.bracket",
        "punctuation.delimiter",
        "variable",
        "variable.parameter",
        "variable.builtin",
        "attribute",
        "label",
        "tag",
    ]
}

fn highlight_index_to_semantic(index: usize, names: &[&'static str]) -> Semantic {
    let name = names.get(index).copied().unwrap_or("");
    match name {
        n if n.starts_with("keyword") => Semantic::Keyword,
        n if n.starts_with("function") => Semantic::Symbol,
        n if n.starts_with("type") => Semantic::Accent,
        n if n.starts_with("constant.builtin") || n == "boolean" => Semantic::Literal,
        n if n.starts_with("constant") => Semantic::Literal,
        n if n.starts_with("string") => Semantic::String,
        n if n == "number" => Semantic::Number,
        n if n.starts_with("comment") => Semantic::Comment,
        n if n.starts_with("operator") => Semantic::Accent,
        n if n.starts_with("punctuation") => Semantic::Muted,
        n if n.starts_with("variable") => Semantic::Symbol,
        n if n == "attribute" => Semantic::Hint,
        n if n == "label" => Semantic::Hint,
        n if n == "tag" => Semantic::Keyword,
        _ => Semantic::Symbol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registers_rust() {
        let r = GrammarRegistry::builtin().unwrap();
        assert!(r.get("rust").is_some());
        assert_eq!(
            r.from_extension("rs").map(|g| g.name.as_str()),
            Some("rust")
        );
    }

    #[test]
    fn parser_parses_rust_source() {
        let r = GrammarRegistry::builtin().unwrap();
        let mut p = BufferParser::new("rust", &r).unwrap();
        p.reparse("fn main() { let x = 42; }").unwrap();
        assert!(p.tree().is_some());
    }

    #[test]
    fn highlight_produces_spans() {
        let r = GrammarRegistry::builtin().unwrap();
        let g = r.get("rust").unwrap();
        let spans = highlight("fn main() { let x = 42; }", g, &r).unwrap();
        assert!(!spans.is_empty(), "expected some spans");
        assert!(spans.iter().any(|s| s.semantic == Semantic::Keyword));
    }

    #[test]
    fn unknown_grammar_errors() {
        let r = GrammarRegistry::builtin().unwrap();
        assert!(BufferParser::new("klingon", &r).is_err());
    }

    // ── M5: incremental Tree::edit reparse ──

    #[test]
    fn byte_to_point_counts_rows_and_byte_columns() {
        assert_eq!(byte_to_point("abc", 2), (0, 2));
        assert_eq!(byte_to_point("ab\ncd", 0), (0, 0));
        assert_eq!(byte_to_point("ab\ncd", 3), (1, 0)); // just after the \n
        assert_eq!(byte_to_point("ab\ncd", 5), (1, 2)); // end of "cd"
        assert_eq!(byte_to_point("x\ny\nz", 4), (2, 0));
    }

    /// The M5 seal: an incremental `Tree::edit` reparse yields a tree identical
    /// to a full parse of the new source — incrementality is an optimization,
    /// never a semantic change. Single-line value edit (`1` → `42`).
    #[test]
    fn incremental_reparse_equals_full_parse_single_line() {
        let r = GrammarRegistry::builtin().unwrap();
        let old = "fn main() { let x = 1; }";
        let new = "fn main() { let x = 42; }";
        let start = old.find('1').unwrap();
        let old_end = start + 1; // "1" is one byte

        let mut inc = BufferParser::new("rust", &r).unwrap();
        inc.reparse(old).unwrap();
        inc.reparse_edit(old, new, start, old_end).unwrap();

        let mut full = BufferParser::new("rust", &r).unwrap();
        full.reparse(new).unwrap();

        assert_eq!(
            inc.tree().unwrap().root_node().to_sexp(),
            full.tree().unwrap().root_node().to_sexp(),
            "incremental reparse must equal a full parse of the new source",
        );
    }

    /// Same seal across a newline-inserting edit (rows shift below the splice).
    #[test]
    fn incremental_reparse_equals_full_parse_multiline() {
        let r = GrammarRegistry::builtin().unwrap();
        let old = "fn a() {}\nfn b() {}\n";
        let new = "fn a() {}\nfn NEW() {}\nfn b() {}\n";
        // Insert "fn NEW() {}\n" at the start of line 1 (byte just after first '\n').
        let start = old.find('\n').unwrap() + 1;
        let old_end = start; // pure insertion

        let mut inc = BufferParser::new("rust", &r).unwrap();
        inc.reparse(old).unwrap();
        inc.reparse_edit(old, new, start, old_end).unwrap();

        let mut full = BufferParser::new("rust", &r).unwrap();
        full.reparse(new).unwrap();

        assert_eq!(
            inc.tree().unwrap().root_node().to_sexp(),
            full.tree().unwrap().root_node().to_sexp(),
        );
    }

    /// A no-prior-tree `reparse_edit` falls back to a correct full parse.
    #[test]
    fn reparse_edit_without_prior_tree_full_parses() {
        let r = GrammarRegistry::builtin().unwrap();
        let mut p = BufferParser::new("rust", &r).unwrap();
        let new = "fn z() {}";
        p.reparse_edit("", new, 0, 0).unwrap();
        assert!(p.tree().is_some());
        assert!(!p.tree().unwrap().root_node().has_error());
    }

    #[test]
    fn ts_edit_new_end_byte_accounts_for_length_delta() {
        // "1" -> "42": +1 byte, so new_end_byte = start + 2.
        let old = "x = 1;";
        let new = "x = 42;";
        let start = old.find('1').unwrap();
        let e = TsEdit::from_splice(old, new, start, start + 1);
        assert_eq!(e.start_byte, start);
        assert_eq!(e.old_end_byte, start + 1);
        assert_eq!(e.new_end_byte, start + 2);
    }
}
