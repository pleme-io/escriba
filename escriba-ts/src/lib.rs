//! `escriba-ts` — tree-sitter host for escriba.
//!
//! The tree-sitter host now lives in **`hikari-ts`** (the fleet syntax spine's
//! tree-sitter backend, self-contained on crates.io). This crate re-exports it
//! so escriba's consumers keep a stable `escriba_ts::…` surface while the host
//! itself has exactly one home — the dedup + load-bearing-fix that also breaks
//! the old `hikari-ts → escriba-ts` dependency inversion (a fleet library must
//! not depend on an application crate).
//!
//! `escriba_ts::Semantic` is `hikari_token::Semantic` (the deduped fleet
//! vocabulary), carrying the total `From<Semantic> for hikari_core::HlClass`.

extern crate self as escriba_ts;

pub use hikari_ts::{
    BufferParser, Grammar, GrammarRegistry, HighlightSpan, Result, Semantic, TreeSitterHighlighter,
    TreeSitterHost, TreeSitterPlugin, TsEdit, TsError, highlight,
};
