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
//! `vellum` is the fleet theme — warm aged-paper Nord-matte — and the
//! default an empty/unset `:preset` resolves to (see
//! [`ThemeSpec::default`] + [`DEFAULT_PRESET`]). It mirrors
//! `ishou_tokens::FleetTheme::Vellum`.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

/// The fleet-default preset — warm aged-paper Nord-matte. Mirrors
/// `ishou_tokens::FleetTheme::Vellum` / `prescribed_default()`. An empty
/// `:preset` (or a missing `(deftheme …)`) resolves to this.
pub const DEFAULT_PRESET: &str = "vellum";

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "deftheme")]
pub struct ThemeSpec {
    /// Named preset (`"vellum"`, `"nord"`, `"gruvbox-dark"`, …). Empty
    /// resolves to [`DEFAULT_PRESET`] (`"vellum"`).
    #[serde(default = "default_preset")]
    pub preset: String,
}

fn default_preset() -> String {
    DEFAULT_PRESET.to_string()
}

impl Default for ThemeSpec {
    /// The default theme is the fleet default — `vellum`.
    fn default() -> Self {
        Self { preset: DEFAULT_PRESET.to_string() }
    }
}

impl ThemeSpec {
    /// The effective preset — the named one, or [`DEFAULT_PRESET`]
    /// (`"vellum"`) when unset/empty.
    #[must_use]
    pub fn effective_preset(&self) -> &str {
        if self.preset.is_empty() {
            DEFAULT_PRESET
        } else {
            &self.preset
        }
    }
}

/// Known preset names, aligned with `irodzuki`. `vellum` (the fleet
/// default) leads; extend freely — unknown presets in a spec are
/// ignored rather than erroring, so adding a preset here is purely
/// additive.
pub const KNOWN_PRESETS: &[&str] =
    &["vellum", "nord", "gruvbox-dark", "tokyo-night", "catppuccin-mocha"];

#[must_use]
pub fn is_known_preset(name: &str) -> bool {
    KNOWN_PRESETS.iter().any(|p| *p == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vellum_is_a_known_preset() {
        assert!(is_known_preset("vellum"));
    }

    #[test]
    fn default_theme_is_vellum() {
        assert_eq!(ThemeSpec::default().preset, "vellum");
        assert_eq!(ThemeSpec::default().effective_preset(), "vellum");
    }

    #[test]
    fn empty_preset_resolves_to_vellum() {
        let t = ThemeSpec { preset: String::new() };
        assert_eq!(t.effective_preset(), "vellum");
    }
}
