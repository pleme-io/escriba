//! What an ABSENT key in a tatara-lisp form actually deserializes to.
//!
//! ## The trap
//!
//! Every spec in this crate is `#[derive(DeriveTataraDomain)]` over serde,
//! which makes `#[serde(default = "some_fn")]` *look* like it supplies a
//! non-zero default for a key the author omitted. It does not: an absent
//! key takes the FIELD TYPE'S ZERO VALUE — `false`, `""`, `0`, `vec![]`.
//!
//! This cost real time. `SplashSpec` was first written with an
//! `:enable: bool` carrying `#[serde(default = "enabled_by_default")]`
//! returning `true`. The shipped `(defsplash …)` — which never mentions
//! `:enable` — parsed cleanly, validated, appeared in `--list-rc` with a
//! count of 1, and the start screen never rendered, because `enable` was
//! `false`. Nothing in the type system, the parser, or the test suite said
//! a word.
//!
//! ## Why these tests exist rather than a comment
//!
//! A comment describing derive behaviour rots the moment `tatara-lisp` is
//! bumped, and rots SILENTLY — the failure lands in whatever spec last
//! trusted the comment. These assertions fail in ONE obvious place instead,
//! and they are equally useful in both directions: if a future tatara-lisp
//! starts honouring serde defaults, `an_absent_key_takes_the_types_zero_value`
//! goes red and every `default = …` in the crate can be revisited on
//! purpose.
//!
//! The rule for authors, until then: **a spec field's safe state must BE
//! its zero value.** Where it cannot be, mask it behind an accessor (see
//! `ThemeSpec::effective_preset`) rather than trusting the attribute.

use escriba_lisp::{SplashSpec, ThemeSpec};

#[test]
fn an_absent_key_takes_the_types_zero_value() {
    // The load-bearing assertion. A form that mentions none of its
    // optional keys must be readable as "everything zeroed", because that
    // is what the derive produces.
    let specs: Vec<SplashSpec> =
        tatara_lisp::compile_typed(r#"(defsplash :tagline "hi")"#).expect("parses");
    let spec = &specs[0];
    assert!(!spec.disabled, "bool → false");
    assert!(spec.art.is_empty(), "Vec → empty");
    assert!(spec.entries.is_empty(), "Vec → empty");
    assert_eq!(spec.tagline, "hi", "a key that IS present survives");
}

#[test]
fn a_serde_default_attribute_does_not_fire_for_an_absent_key() {
    // `ThemeSpec::preset` carries `#[serde(default = "default_preset")]`
    // returning "nord". If that attribute worked, this would be "nord".
    // It is "" — the attribute is inert, and this test is what says so.
    //
    // If this ever goes GREEN-as-"nord", tatara-lisp changed its
    // deserialization: revisit every `default = …` in the crate, and the
    // `:disabled` polarity on SplashSpec can go back to `:enable`.
    let specs: Vec<ThemeSpec> = tatara_lisp::compile_typed("(deftheme)").expect("parses");
    assert_eq!(
        specs[0].preset, "",
        "an absent :preset must be the zero value, NOT the serde default",
    );
}

#[test]
fn theme_survives_the_trap_because_an_accessor_masks_it() {
    // The correct mitigation where a zero value is not the safe state:
    // never read the field directly, read through an accessor that
    // resolves the empty case. `deftheme` with no preset still yields the
    // fleet default everywhere it matters.
    let specs: Vec<ThemeSpec> = tatara_lisp::compile_typed("(deftheme)").expect("parses");
    assert_eq!(
        specs[0].effective_preset(),
        escriba_lisp::DEFAULT_PRESET,
        "effective_preset must absorb the empty field",
    );
    assert_eq!(
        specs[0].resolve(),
        ishou_tokens::FleetTheme::prescribed_default(),
        "and resolve to a real theme, not a fallback-of-a-fallback",
    );
}

#[test]
fn the_start_screen_takes_the_other_mitigation_inverted_polarity() {
    // `SplashSpec` cannot use an accessor-over-empty the way ThemeSpec
    // does — a bool has no "unset" to detect. So it puts the SAFE state on
    // the zero value instead: absent flag ⇒ shown.
    let bare: Vec<SplashSpec> =
        tatara_lisp::compile_typed(r#"(defsplash :tagline "x")"#).expect("parses");
    assert!(bare[0].is_enabled(), "silence must mean SHOWN");

    let off: Vec<SplashSpec> =
        tatara_lisp::compile_typed(r"(defsplash :disabled #t)").expect("parses");
    assert!(!off[0].is_enabled(), "and retirement must be explicit");
}
