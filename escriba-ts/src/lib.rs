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

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Semantic highlight buckets — the small enum every tool (fmt/lint/LSP/nvim)
/// agrees on. Mirrors `caixa_theme::Semantic` so consumers can map 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Semantic {
    Keyword,
    Symbol,
    KeywordArg,
    String,
    Number,
    Literal,
    Comment,
    Accent,
    Muted,
    Error,
    Warning,
    Info,
    Hint,
    Added,
    Removed,
    Unchanged,
}
use tree_sitter::{Language, Parser, Tree};
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
    pub extensions: Vec<&'static str>,
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
                extensions: vec!["rs"],
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
        self.grammars.values().find(|g| g.extensions.contains(&ext))
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

    /// Re-parse the full source. Phase 2: incremental `Tree::edit` + reparse.
    pub fn reparse(&mut self, src: &str) -> Result<()> {
        let new = self.parser.parse(src, self.tree.as_ref());
        self.tree = new;
        Ok(())
    }

    #[must_use]
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }
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
}
