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

/// Escriba-local language plugins — the languages the fleet spine does not
/// ship yet. Moved here with `build_ecosystem`; a language table is language
/// knowledge, not rendering.
pub mod langs;

pub use hikari_core::{Ecosystem, Language};

/// The highlight registry escriba renders and reasons through.
///
/// **Tree-sitter grammars take precedence** for the languages they cover; the
/// zero-dep table backend fills every other language; escriba-local tables
/// register LAST, so an upstream hikari backend for the same language always
/// wins and a local table retires itself with no edit here.
///
/// It lived in `escriba-render::gpu` until 2026-08-08, which put escriba's
/// language knowledge behind a GPU dependency: the runtime could not ask what
/// a symbol is without taking on wgpu, and the ratatui face — which needs no
/// GPU at all — has no syntax highlighting to this day. Registry construction
/// is not rendering.
#[must_use]
pub fn build_ecosystem() -> Ecosystem {
    let mut eco = Ecosystem::new();
    let mut covered: Vec<Language> = Vec::new();
    if let Ok(host) = hikari_ts::TreeSitterHost::builtin() {
        for p in host.plugins() {
            covered.push(p.language());
            eco.register(p);
        }
    }
    for p in hikari_core::langs::builtins() {
        if !covered.contains(&p.language()) {
            covered.push(p.language());
            eco.register(p);
        }
    }
    for p in langs::escriba_local() {
        if !covered.contains(&p.language()) {
            eco.register(p);
        }
    }
    eco
}

pub use hikari_ts::{
    BufferParser, Grammar, GrammarRegistry, HighlightSpan, Result, Semantic, TreeSitterHighlighter,
    TreeSitterHost, TreeSitterPlugin, TsEdit, TsError, highlight,
};
