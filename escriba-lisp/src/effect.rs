//! `defeffect` — Lisp-authored GPU rendering effects.
//!
//! Ghostty and mado both ship a shader-effect layer: cursor glow,
//! bloom, scanlines, film grain, CRT warp. These compose over the
//! terminal/editor grid. `defeffect` gives escriba the same palette
//! as a typed declarative form — users toggle effects in the rc,
//! the GPU render backend (madori + garasu) pipes them through.
//!
//! Terminal-mode (`--render=tui`) escriba ignores effects — they
//! only apply to the GPU window surface.
//!
//! ```lisp
//! ;; Ghostty-parity cursor glow.
//! (defeffect :name "cursor-glow"
//!            :kind "cursor"
//!            :enable #t
//!            :intensity 0.6
//!            :radius 1.8
//!            :color "#88c0d0")
//!
//! ;; Bloom over the whole buffer.
//! (defeffect :name "bloom"
//!            :kind "screen"
//!            :enable #t
//!            :intensity 0.25
//!            :threshold 0.75)
//!
//! ;; Vintage CRT scanlines — off by default, users opt in.
//! (defeffect :name "scanlines"
//!            :kind "screen"
//!            :enable #f
//!            :intensity 0.15)
//!
//! ;; Load a user-authored WGSL shader.
//! (defeffect :name "my-shader"
//!            :kind "custom"
//!            :enable #t
//!            :shader "~/.config/escriba/shaders/my.wgsl")
//! ```
//!
//! # Effect kinds
//!
//! - `"cursor"`  — effect bound to the cursor position; `:radius`
//!   + `:color` are meaningful here.
//! - `"screen"`  — full-surface post-process; `:intensity` drives it.
//! - `"cursor-trail"` — motion-dependent trail along cursor path.
//! - `"underglow"` — buffer edge glow.
//! - `"custom"`  — user-supplied WGSL shader at `:shader`.
//!
//! Unknown kinds pass through to the runtime's shader registry
//! (plugins can add kinds).

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defeffect")]
pub struct EffectSpec {
    /// Effect id — unique within the plan. Canonical names map to
    /// built-in shaders (see [`CANONICAL_EFFECTS`]); user-authored
    /// effects pick any unique name + set `:shader` to a WGSL path.
    pub name: String,
    /// Effect kind — see [`KNOWN_KINDS`].
    pub kind: String,
    /// Whether the effect is active. Default `false` so a `defeffect`
    /// declaration without `:enable #t` records the preference but
    /// doesn't fire — users can flip it via runtime command without
    /// re-authoring the rc.
    #[serde(default)]
    pub enable: bool,
    /// Overall strength (0.0 = off, 1.0 = full). Runtime clamps.
    #[serde(default)]
    pub intensity: f64,
    /// For cursor effects: glow radius in cell widths.
    #[serde(default)]
    pub radius: f64,
    /// For bloom / related: brightness threshold above which pixels
    /// contribute to the effect.
    #[serde(default)]
    pub threshold: f64,
    /// Hex colour override — `"#rrggbb"`. Empty = use theme palette.
    #[serde(default)]
    pub color: String,
    /// Path to a custom WGSL shader file. Required when
    /// `:kind "custom"`, ignored otherwise. `~` expands.
    #[serde(default)]
    pub shader: String,
}

/// Canonical effect kinds the runtime ships with. Plugins register
/// more at runtime, so unknown kinds don't error — they pass through.
pub const KNOWN_KINDS: &[&str] = &[
    "cursor",
    "screen",
    "cursor-trail",
    "underglow",
    "custom",
];

/// Canonical effect names — each a built-in shader. Name collisions
/// with a `:kind "custom"` spec resolve in favour of the user
/// (custom wins — "be overridable").
pub const CANONICAL_EFFECTS: &[(&str, &str)] = &[
    ("cursor-glow",  "cursor"),
    ("cursor-pulse", "cursor"),
    ("cursor-trail", "cursor-trail"),
    ("bloom",        "screen"),
    ("scanlines",    "screen"),
    ("film-grain",   "screen"),
    ("crt-warp",     "screen"),
    ("chromatic-aberration", "screen"),
    ("underglow",    "underglow"),
];

#[must_use]
pub fn is_known_kind(name: &str) -> bool {
    KNOWN_KINDS.iter().any(|k| *k == name)
}

#[must_use]
pub fn is_canonical_effect(name: &str) -> bool {
    CANONICAL_EFFECTS.iter().any(|(n, _)| *n == name)
}

impl EffectSpec {
    /// True when `:kind "custom"` was set but no `:shader` path
    /// provided — the runtime can't load "custom" without a source.
    #[must_use]
    pub fn is_malformed_custom(&self) -> bool {
        self.kind == "custom" && self.shader.is_empty()
    }

    /// Intensity clamped to `[0.0, 1.0]`. Handles the runtime
    /// contract without touching the stored value.
    #[must_use]
    pub fn intensity_clamped(&self) -> f64 {
        self.intensity.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_effect_table_covers_ghostty_set() {
        for name in ["cursor-glow", "bloom", "scanlines", "film-grain"] {
            assert!(
                is_canonical_effect(name),
                "canonical effect {name} missing from table",
            );
        }
        assert!(!is_canonical_effect("laser-unicorn"));
    }

    #[test]
    fn known_kinds_accept_all_variants() {
        for k in ["cursor", "screen", "cursor-trail", "underglow", "custom"] {
            assert!(is_known_kind(k));
        }
        assert!(!is_known_kind("quantum"));
    }

    #[test]
    fn malformed_custom_classifier() {
        let bad = EffectSpec {
            name: "x".into(),
            kind: "custom".into(),
            ..Default::default()
        };
        assert!(bad.is_malformed_custom());

        let ok = EffectSpec {
            name: "x".into(),
            kind: "custom".into(),
            shader: "/path/to.wgsl".into(),
            ..Default::default()
        };
        assert!(!ok.is_malformed_custom());

        let builtin = EffectSpec {
            name: "bloom".into(),
            kind: "screen".into(),
            ..Default::default()
        };
        assert!(!builtin.is_malformed_custom());
    }

    #[test]
    fn intensity_clamps_both_ends() {
        let e = EffectSpec {
            intensity: -0.5,
            ..Default::default()
        };
        assert_eq!(e.intensity_clamped(), 0.0);
        let e = EffectSpec {
            intensity: 2.7,
            ..Default::default()
        };
        assert_eq!(e.intensity_clamped(), 1.0);
        let e = EffectSpec {
            intensity: 0.42,
            ..Default::default()
        };
        assert_eq!(e.intensity_clamped(), 0.42);
    }
}

impl Default for EffectSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: String::new(),
            enable: false,
            intensity: 0.0,
            radius: 0.0,
            threshold: 0.0,
            color: String::new(),
            shader: String::new(),
        }
    }
}
