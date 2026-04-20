//! `deftheme` — Lisp-authored color scheme selection.
//!
//! ```lisp
//! (deftheme :preset "nord")
//! (deftheme :preset "gruvbox-dark")
//! (deftheme :preset "tokyo-night")
//! (deftheme :preset "catppuccin-mocha")
//! ```
//!
//! Preset names mirror `irodzuki` / `irodori`. Unknown presets leave
//! the theme unchanged (forward-compat with user-added presets).

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "deftheme")]
pub struct ThemeSpec {
    /// Named preset (`"nord"`, `"gruvbox-dark"`, `"tokyo-night"`, …).
    #[serde(default)]
    pub preset: String,
}

/// Known preset names, aligned with `irodzuki`. Extend freely — unknown
/// presets in a spec are ignored rather than erroring, so adding a
/// preset here is purely additive.
pub const KNOWN_PRESETS: &[&str] =
    &["nord", "gruvbox-dark", "tokyo-night", "catppuccin-mocha"];

#[must_use]
pub fn is_known_preset(name: &str) -> bool {
    KNOWN_PRESETS.iter().any(|p| *p == name)
}
