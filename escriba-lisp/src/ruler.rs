//! `defruler` — declarative column rulers / visual guides.
//!
//! Absorbs vim's `set colorcolumn=80,120`, nvim + neovide's
//! virtcolumn plugin, vscode's `"editor.rulers": [80, 120]`, and
//! jetbrains' "hard wrap at margin" guide. Every editor in the
//! category has a version of "draw a vertical line at column N so
//! the user doesn't blow past it", but the shape is untyped and
//! lives in settings JSON / vimrc strings.
//!
//! ```lisp
//! ;; Two guides, global scope.
//! (defruler :columns (80 120)
//!           :style "soft"
//!           :color "#4c566a")
//!
//! ;; Per-filetype override — rust projects cap at 100.
//! (defruler :columns (100)
//!           :filetype "rust"
//!           :style "hard"
//!           :color "#bf616a")
//!
//! ;; Dim guide at every 4th column — indentation hint.
//! (defruler :columns (4 8 12 16 20)
//!           :filetype "python"
//!           :style "dim"
//!           :description "PEP8 indent hints")
//! ```
//!
//! ## Shape
//!
//! - `:columns` — `Vec<u32>` of 1-based column positions. Must be
//!   non-empty; every entry must be > 0. The apply layer rejects
//!   empty / zero-containing vectors so a typo fails fast.
//! - `:filetype` — optional scope. Empty = global; non-empty =
//!   only active when the buffer's filetype matches.
//! - `:style` — optional enum: `soft` | `hard` | `dim`. Empty =
//!   `soft`. Anything else is rejected by the apply layer.
//! - `:color` — optional `#rrggbb` / `#rrggbbaa` override. Empty
//!   means "use the theme's default guide color". Structural
//!   validation only (color strings pass through to the renderer).
//! - `:description` — picker hint.
//!
//! Multiple `defruler` forms may target the same filetype; the
//! runtime composes the set (union of columns) and lets the most
//! specific style win per-column.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defruler")]
pub struct RulerSpec {
    /// 1-based column positions at which to draw the guide line.
    #[serde(default)]
    pub columns: Vec<u32>,
    /// Filetype scope. Empty = global.
    #[serde(default)]
    pub filetype: String,
    /// Visual intensity — `soft` / `hard` / `dim`. Empty = `soft`.
    #[serde(default)]
    pub style: String,
    /// `#rrggbb` / `#rrggbbaa` override. Empty = theme default.
    #[serde(default)]
    pub color: String,
    /// Picker / doctor description.
    #[serde(default)]
    pub description: String,
}

/// Canonical style vocabulary. Kept in sync with the renderer's
/// guide-line intensity table.
pub const KNOWN_STYLES: &[&str] = &["soft", "hard", "dim"];

/// True when `style` is a recognized intensity.
#[must_use]
pub fn is_known_style(style: &str) -> bool {
    KNOWN_STYLES.contains(&style)
}

impl RulerSpec {
    /// Effective style — `soft` by default when `:style` is unset.
    #[must_use]
    pub fn effective_style(&self) -> &str {
        crate::strutil::default_if_empty(&self.style, "soft")
    }

    /// True when every column in `:columns` is strictly positive.
    /// Apply-time validation rejects `:columns (0 …)` so a
    /// zero-column can't sneak in as a "column 0" tautology (and
    /// the renderer can assume all rulers are on real columns).
    #[must_use]
    pub fn all_columns_positive(&self) -> bool {
        self.columns.iter().all(|c| *c > 0)
    }

    /// Structural color check: empty OR `#` + 6 / 8 hex chars.
    /// The theme machinery handles the actual palette resolution;
    /// this just catches a hand-typed `:color "blue"`.
    #[must_use]
    pub fn has_valid_color_format(&self) -> bool {
        if self.color.is_empty() {
            return true;
        }
        let bytes = self.color.as_bytes();
        if bytes.first() != Some(&b'#') {
            return false;
        }
        let rest = &bytes[1..];
        if rest.len() != 6 && rest.len() != 8 {
            return false;
        }
        rest.iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b) || (b'A'..=b'F').contains(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_vocabulary_is_exactly_three_entries() {
        // Pin the vocabulary — adding new styles needs a renderer
        // update, so force a conscious change rather than silent
        // drift.
        assert_eq!(KNOWN_STYLES, &["soft", "hard", "dim"]);
        assert!(is_known_style("soft"));
        assert!(is_known_style("hard"));
        assert!(is_known_style("dim"));
        assert!(!is_known_style("bold"));
        assert!(!is_known_style(""));
    }

    #[test]
    fn effective_style_falls_back_to_soft() {
        let s = RulerSpec::default();
        assert_eq!(s.effective_style(), "soft");

        let s = RulerSpec { style: "hard".into(), ..Default::default() };
        assert_eq!(s.effective_style(), "hard");
    }

    #[test]
    fn all_columns_positive_catches_zero_entries() {
        let s = RulerSpec { columns: vec![80, 120], ..Default::default() };
        assert!(s.all_columns_positive());

        // Zero in the list should disqualify — "column 0" is
        // semantically meaningless (columns are 1-based).
        let s = RulerSpec { columns: vec![80, 0, 120], ..Default::default() };
        assert!(!s.all_columns_positive());

        // Empty vector passes this check (caught separately by the
        // apply layer's "columns must be non-empty" rule).
        let s = RulerSpec::default();
        assert!(s.all_columns_positive());
    }

    #[test]
    fn color_format_accepts_rgb_and_rgba_hex() {
        for ok in ["#000000", "#ffffff", "#4c566a", "#112233aa", "#AABBCC"] {
            let s = RulerSpec { color: ok.into(), ..Default::default() };
            assert!(s.has_valid_color_format(), "should accept {ok:?}");
        }
    }

    #[test]
    fn color_format_rejects_named_or_malformed() {
        for bad in ["blue", "#fff", "#123456789", "rgb(0,0,0)", "#zzzzzz", "fff"] {
            let s = RulerSpec { color: bad.into(), ..Default::default() };
            assert!(!s.has_valid_color_format(), "should reject {bad:?}");
        }
    }

    #[test]
    fn empty_color_is_valid_means_theme_default() {
        let s = RulerSpec::default();
        assert!(s.has_valid_color_format());
        assert!(s.color.is_empty());
    }
}
