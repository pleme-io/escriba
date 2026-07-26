//! `deftheme` — Lisp-authored color scheme selection.
//!
//! ```lisp
//! (deftheme :preset "vellum")
//! (deftheme :preset "nord")
//! (deftheme :preset "gruvbox-dark")
//! (deftheme :preset "tokyo-night")
//! (deftheme :preset "catppuccin-mocha")
//! ```
//!
//! Preset names mirror `irodzuki` / `irodori`. Unknown presets leave
//! the theme unchanged (forward-compat with user-added presets).
//!
//! `nord` is the fleet theme — the same look mado and frostmourne render —
//! and the default an empty/unset `:preset` resolves to (see
//! [`ThemeSpec::default`] + [`DEFAULT_PRESET`]). It mirrors
//! `ishou_tokens::FleetTheme::PlemeDark`. (`vellum` was the fleet theme
//! before 2026-07; it stays selectable — retired, not removed.)

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

/// The fleet-default preset — Nord dark. NOT a value of escriba's own: it
/// is `ishou_tokens::FleetTheme::prescribed_default().preset_name()`, i.e.
/// whatever the fleet currently prescribes, which is the same look mado and
/// frostmourne render.
///
/// It is a `const` (so it can name a preset in const contexts and in
/// `KNOWN_PRESETS`) with a test below asserting it equals the fleet value —
/// the honest tier here is CI-caught, not compile-enforced, because
/// `preset_name()` is not const-evaluable into a `&'static str` comparison.
/// If the fleet re-points its default, that test fails rather than escriba
/// silently keeping a stale one.
pub const DEFAULT_PRESET: &str = "nord";

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "deftheme")]
pub struct ThemeSpec {
    /// Named preset (`"nord"`, `"vellum"`, `"gruvbox-dark"`, …). Empty
    /// resolves to [`DEFAULT_PRESET`] (`"nord"`).
    #[serde(default = "default_preset")]
    pub preset: String,
}

fn default_preset() -> String {
    DEFAULT_PRESET.to_string()
}

impl Default for ThemeSpec {
    /// The default theme is the fleet default — `nord`.
    fn default() -> Self {
        Self { preset: DEFAULT_PRESET.to_string() }
    }
}

impl ThemeSpec {
    /// The effective preset — the named one, or [`DEFAULT_PRESET`]
    /// (`"nord"`) when unset/empty.
    #[must_use]
    pub fn effective_preset(&self) -> &str {
        if self.preset.is_empty() {
            DEFAULT_PRESET
        } else {
            &self.preset
        }
    }
}

/// Known preset names, aligned with `irodzuki`. `nord` (the fleet default)
/// leads; extend freely — unknown presets in a spec are ignored rather than
/// erroring, so adding a preset here is purely additive.
pub const KNOWN_PRESETS: &[&str] =
    &["nord", "vellum", "polar-veil", "gruvbox-dark", "tokyo-night", "catppuccin-mocha"];

impl ThemeSpec {
    /// Resolve this spec to a typed [`ishou_tokens::FleetTheme`] — the ONE
    /// place a `(deftheme :preset …)` string becomes a real theme value.
    ///
    /// This is the link that made `deftheme` a system instead of decoration.
    /// Before it, nothing consumed `ThemeSpec` at all: `escriba-lisp`'s
    /// `apply.rs` applies grammar-extensions, keymap, commands and options —
    /// and nothing else — while `escriba-tui/src/render.rs` constructed
    /// `VellumPalette::vellum()` directly. So an operator could author
    /// `(deftheme :preset "nord")`, see it parse and validate, and watch the
    /// editor render Vellum regardless. The spec was honoured on paper only.
    ///
    /// Resolution lives HERE, in the spec crate, rather than in a renderer,
    /// so the terminal face and the GPU face share one mapping and cannot
    /// drift into two different readings of the same declaration.
    ///
    /// An unrecognised preset falls back to the FLEET prescribed default
    /// rather than to a hardcoded name — so this function never pins a
    /// theme choice of its own, and the fleet decision (currently Nord dark,
    /// `ishou_tokens::FleetTheme::prescribed_default()`) stays the single
    /// source of that truth.
    #[must_use]
    pub fn resolve(&self) -> ishou_tokens::FleetTheme {
        use ishou_tokens::FleetTheme;
        match self.effective_preset() {
            // `nord` is the operator-facing spelling of the palette whose
            // serde wire name is `pleme_dark` — see
            // `FleetTheme::preset_name`, which exists precisely so this
            // string is derivable rather than hand-repeated per consumer.
            "nord" | "pleme_dark" | "pleme-dark" => FleetTheme::PlemeDark,
            "vellum" => FleetTheme::Vellum,
            "polar-veil" | "polar_veil" => FleetTheme::PolarVeil,
            "bare" => FleetTheme::Bare,
            // Presets ishou has no palette for yet (gruvbox-dark,
            // tokyo-night, catppuccin-mocha) resolve to the fleet default
            // instead of silently rendering something arbitrary. They stay
            // in KNOWN_PRESETS so authoring one is not an error today and
            // becomes exact the moment ishou grows that palette.
            _ => FleetTheme::prescribed_default(),
        }
    }
}

#[must_use]
pub fn is_known_preset(name: &str) -> bool {
    KNOWN_PRESETS.iter().any(|p| *p == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    use ishou_tokens::FleetTheme;

    #[test]
    fn nord_and_vellum_are_both_known_presets() {
        assert!(is_known_preset("nord"), "the fleet default must be authorable");
        assert!(is_known_preset("vellum"), "vellum stays selectable (retired, not removed)");
    }

    #[test]
    fn default_theme_is_nord() {
        assert_eq!(ThemeSpec::default().preset, "nord");
        assert_eq!(ThemeSpec::default().effective_preset(), "nord");
    }

    #[test]
    fn empty_preset_resolves_to_nord() {
        let t = ThemeSpec { preset: String::new() };
        assert_eq!(t.effective_preset(), "nord");
    }

    /// DEFAULT_PRESET is not escriba's own opinion — it must equal whatever
    /// the FLEET prescribes. If ishou re-points its default, this fails here
    /// instead of escriba silently keeping a stale value (which is exactly
    /// how escriba came to render Vellum while mado rendered Nord).
    #[test]
    fn default_preset_tracks_the_fleet_prescribed_theme() {
        assert_eq!(
            DEFAULT_PRESET,
            FleetTheme::prescribed_default().preset_name(),
            "escriba's default preset must BE the fleet's prescribed theme"
        );
    }

    /// THE CIRCUIT: a `(deftheme :preset …)` string becomes a real typed
    /// theme. Before this existed, nothing consumed ThemeSpec at all and
    /// `render.rs` hardcoded `VellumPalette::vellum()`, so authoring
    /// `:preset "nord"` parsed, validated, and changed nothing on screen.
    #[test]
    fn deftheme_resolves_to_a_real_fleet_theme() {
        let of = |p: &str| ThemeSpec { preset: p.to_string() }.resolve();
        assert_eq!(of("nord"), FleetTheme::PlemeDark);
        assert_eq!(of("vellum"), FleetTheme::Vellum);
        assert_eq!(of("polar-veil"), FleetTheme::PolarVeil);
        assert_eq!(of("bare"), FleetTheme::Bare);
        // `nord` is the preset spelling of the palette whose serde wire name
        // is `pleme_dark`; both spellings must land on one theme.
        assert_eq!(of("pleme_dark"), of("nord"), "wire and preset spellings agree");
        assert_eq!(of("polar_veil"), of("polar-veil"), "underscore and dash agree");
    }

    /// The default path resolves to Nord, and it resolves to the SAME theme
    /// the fleet prescribes — the property that makes escriba match mado and
    /// frostmourne without anyone hand-matching a hex.
    #[test]
    fn the_default_resolves_to_the_same_theme_the_fleet_prescribes() {
        assert_eq!(ThemeSpec::default().resolve(), FleetTheme::prescribed_default());
        assert_eq!(ThemeSpec::default().resolve(), FleetTheme::PlemeDark);
        // And it really is Nord Polar Night on screen, by hex.
        assert_eq!(ThemeSpec::default().resolve().resolve().background, "#2E3440");
    }

    /// An unknown preset must fall back to the FLEET default, never to a
    /// value hardcoded in this function — otherwise this file becomes a
    /// second place the fleet look is decided.
    #[test]
    fn an_unknown_preset_falls_back_to_the_fleet_default() {
        let t = ThemeSpec { preset: "no-such-theme-42".to_string() };
        assert_eq!(t.resolve(), FleetTheme::prescribed_default());
        // Presets we accept for authoring but ishou has no palette for yet
        // behave the same way — deliberately, not accidentally.
        for p in ["gruvbox-dark", "tokyo-night", "catppuccin-mocha"] {
            assert!(is_known_preset(p), "{p} stays authorable");
            assert_eq!(
                ThemeSpec { preset: p.to_string() }.resolve(),
                FleetTheme::prescribed_default(),
                "{p} has no ishou palette yet — falls back, never renders arbitrary colour"
            );
        }
    }
}
