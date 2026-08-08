//! What the highlighter CLASSIFIES, measured against real source.
//!
//! escriba's `ChromeSyntax` maps `HlClass` to colour, and that mapping is
//! pinned against Nord. But a correct colour for a WRONG class still paints
//! the wrong thing, and no test covered the classification itself — the
//! failure is invisible because the file renders beautifully either way.
//!
//! Found while building `picker.symbols`: filtering spans for
//! `Function | Type | Namespace` returned NOTHING for ordinary Rust.

const RUST: &str = "fn alpha() {}\nstruct Beta;\nfn gamma(x: u32) -> u32 { x }\n";

fn classes(src: &str, path: &str) -> Vec<(String, String)> {
    let eco = escriba_ts::build_ecosystem();
    let hl = eco.highlighter_for_path(path);
    let mut hl = hl;
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

/// PINNED DEFECT — this asserts the CURRENT WRONG behaviour on purpose.
///
/// `hikari-token-0.1.9/src/lib.rs:69` maps `Semantic::Symbol => HlClass::Punctuation`,
/// and `hikari-ts` maps tree-sitter's `@function` capture to `Semantic::Symbol`.
/// So **every function name in every language is classified as punctuation**,
/// and escriba paints it `text_bright` (near-white) instead of `primary`
/// (frost blue). A struct name lands on `Special`, painted as the search
/// colour.
///
/// Pinned rather than left unmentioned so that FIXING it upstream turns this
/// test red and says exactly what changed. When hikari classifies these
/// correctly, invert the assertions and `picker.symbols` becomes buildable —
/// it was written, found to rest on this, and backed out.
#[test]
fn function_and_type_names_are_currently_misclassified() {
    let got = classes(RUST, "probe.rs");
    let class_of = |name: &str| {
        got.iter()
            .find(|(_, t)| t == name)
            .map(|(c, _)| c.clone())
            .unwrap_or_default()
    };
    assert_ne!(
        class_of("alpha"),
        "Function",
        "if this is now `Function`, the upstream mapping was FIXED — invert \
         this test and `picker.symbols` is unblocked",
    );
    assert_ne!(class_of("Beta"), "Type", "same for type names");
}
