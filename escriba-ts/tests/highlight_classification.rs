//! What the highlighter CLASSIFIES, measured against real source.
//!
//! escriba's `ChromeSyntax` maps `HlClass` to colour, and that mapping is
//! pinned against Nord. But a correct colour for a WRONG class still paints
//! the wrong thing, and no test covered the classification itself — the
//! failure is invisible because the file renders beautifully either way.
//!
//! Found while building `picker.symbols`: filtering spans for
//! `Function | Type | Namespace` returned NOTHING for ordinary Rust. That
//! led to hikari 0.1.10, which fixed half of it — see the second test for
//! which half, and why this file is still here.

const RUST: &str = "fn alpha() {}\nstruct Beta;\nfn gamma(x: u32) -> u32 { x }\n";

fn classes(src: &str, path: &str) -> Vec<(String, String)> {
    let eco = escriba_ts::build_ecosystem();
    let hl = eco.highlighter_for_path(path);
    hl.highlight(src)
        .into_iter()
        .filter_map(|sp| {
            let t = src.get(sp.span.range())?.trim().to_string();
            (!t.is_empty()).then_some((format!("{:?}", sp.class), t))
        })
        .collect()
}

#[test]
fn keywords_are_classified_as_keywords() {
    let got = classes(RUST, "probe.rs");
    assert!(
        got.iter().any(|(c, t)| t == "fn" && c == "Keyword"),
        "`fn` must be a Keyword: {got:?}",
    );
}

/// PARTIALLY FIXED — and this test records WHICH half.
///
/// Was: `function` captures folded to `Semantic::Symbol`, which becomes
/// `HlClass::Punctuation`, so every function name in every tree-sitter
/// language was classified — and painted — as punctuation. Fixed upstream in
/// hikari 0.1.10 (`highlight_index_to_semantic`: four arms contradicted
/// `hlclass_to_semantic`, two of them exactly swapped with each other).
///
/// FIXED: identifiers are no longer punctuation, and brackets/semicolons are
/// no longer `Plain`. The two most common token kinds in a file had each
/// other's class; they do not now.
///
/// STILL OPEN, and the reason this test survives rather than being deleted:
/// `Semantic` has 16 variants and cannot distinguish `Function` from `Type`.
/// Both fold to `Accent`, which comes back as `HlClass::Special`. So escriba
/// cannot paint Nord's distinct function (#88C0D0) and type (#8FBCBB)
/// colours through the tree-sitter path, and `picker.symbols` — which
/// filters for `HlClass::Function` — stays inert.
///
/// Closing it means hikari-ts emitting `HlClass` directly instead of
/// projecting through `Semantic`. When that lands, `alpha` becomes `Function`
/// and this test goes red saying exactly that.
#[test]
fn function_and_type_are_still_indistinguishable_through_semantic() {
    let got = classes(RUST, "probe.rs");
    let class_of = |name: &str| {
        got.iter()
            .find(|(_, t)| t == name)
            .map(|(c, _)| c.clone())
            .unwrap_or_default()
    };

    // The half that WAS fixed — assert it forward, so a regression is caught.
    assert_ne!(
        class_of("alpha"),
        "Punctuation",
        "a function NAME must not be punctuation — this regressed to the \
         pre-hikari-0.1.10 behaviour",
    );

    // The half still open. When these become Function/Type, hikari-ts stopped
    // projecting through `Semantic` — invert them and `picker.symbols` is
    // unblocked.
    assert_eq!(
        class_of("alpha"),
        "Special",
        "functions currently fold through Semantic::Accent",
    );
    assert_eq!(
        class_of("Beta"),
        "Special",
        "…and so do types, indistinguishably"
    );
}
