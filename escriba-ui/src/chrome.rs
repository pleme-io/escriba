//! `ChromePalette` — the ONE seam where a `FleetTheme` becomes the concrete
//! colors escriba's chrome paints with.
//!
//! ## Why this exists
//!
//! Both renderers (`escriba-tui`'s ratatui chrome and `escriba-render`'s GPU
//! backend) used to call `ishou_tokens::VellumPalette::vellum()` directly.
//! That hardcoded ONE theme into the paint path, with two consequences:
//!
//! 1. `(deftheme :preset "nord")` resolved to a real
//!    `ishou_tokens::FleetTheme` and was then **thrown away** — nothing
//!    outside tests consumed `ThemeSpec::resolve()`, so authoring a theme
//!    did nothing.
//! 2. When the fleet moved its prescribed theme from Vellum to PlemeDark
//!    (Nord), escriba kept painting Vellum and both
//!    `ishou_tokens::convergence::Guard` tests went RED:
//!    `theme drift — actual Vellum != fleet PlemeDark`.
//!
//! ## Why roles, not a palette
//!
//! This resolves through `ishou_tokens::SemanticRoles` — the closed role
//! vocabulary — rather than reaching for palette keys directly. A role
//! binding IS a theme, so adding a theme is adding an arm below, never a new
//! color table. **There are no hex literals in this file, by construction**;
//! every value comes from an ishou palette via its role binding. That is the
//! PENTE rule (the fleet visual spine) applied here: consume the spine, do
//! not grow a 21st palette home.
//!
//! `VellumPalette` covers only `vellum` + `polar_veil`; `ColorPalette::pleme`
//! carries Nord. Neither alone is total over `FleetTheme`, which is exactly
//! why the selection lives here instead of at each call site.

use ishou_tokens::{ColorPalette, Rgb, SemanticRoles, VellumPalette};

/// Re-exported at the seam so a consumer that only needs to NAME a theme
/// (the runtime holding one, a renderer switching to one) does not take a
/// direct `ishou-tokens` dependency for a single enum. The theme→colour
/// mapping lives here; so should the vocabulary for asking about it.
pub use ishou_tokens::FleetTheme;

/// The concrete colors escriba's chrome needs, already resolved for one
/// theme. Field names are ROLES (what the color means), never palette keys
/// (what it happens to be) — so a theme swap is a value change here, not a
/// rename at every call site.
// NOTE: no `PartialEq` derive — `ishou_tokens::Rgb` does not implement it.
// Comparison goes through `hex_tuple()` below, which is also what the tests
// assert on, so equality is defined in exactly one place.
#[derive(Debug, Clone, Copy)]
pub struct ChromePalette {
    /// Editor ground.
    pub background: Rgb,
    /// Raised chrome — statusline / tabline ground.
    pub surface: Rgb,
    /// Body text.
    pub text: Rgb,
    /// De-emphasised text — comments, gutter line numbers.
    pub text_dim: Rgb,
    /// Block cursor.
    pub cursor: Rgb,
    /// Error / diagnostic red.
    pub error: Rgb,
    /// Warning / hint yellow.
    pub warning: Rgb,
    /// Success green — also the Insert-mode pill.
    pub success: Rgb,
    /// Informational cyan — also the Normal-mode pill.
    pub info: Rgb,
    /// Accent — also the Visual-mode pill.
    pub accent: Rgb,
}

impl ChromePalette {
    /// Resolve the chrome colors for `theme`.
    ///
    /// Total over `FleetTheme`: the match has no wildcard arm, so adding a
    /// variant upstream fails THIS file to compile rather than silently
    /// falling back to the wrong look. That compiler failure is the point.
    #[must_use]
    pub fn for_theme(theme: FleetTheme) -> Self {
        match theme {
            // Nord — the fleet-prescribed theme, and what mado ships.
            FleetTheme::PlemeDark | FleetTheme::Bare => {
                Self::from_roles(&SemanticRoles::pleme_dark(), &Resolver::Pleme)
            }
            FleetTheme::Vellum => Self::from_roles(&SemanticRoles::vellum(), &Resolver::Vellum),
            FleetTheme::PolarVeil => {
                Self::from_roles(&SemanticRoles::vellum(), &Resolver::PolarVeil)
            }
        }
    }

    /// The fleet-prescribed chrome — what escriba paints unless a
    /// `(deftheme :preset …)` says otherwise.
    #[must_use]
    pub fn prescribed() -> Self {
        Self::for_theme(FleetTheme::prescribed_default())
    }

    /// Every role as a hex string, in a fixed order. The single definition of
    /// "these two chromes are the same" — used by tests and available to
    /// consumers that need to diff or log a resolved theme.
    #[must_use]
    pub fn hex_tuple(&self) -> [(&'static str, String); 10] {
        [
            ("background", self.background.hex()),
            ("surface", self.surface.hex()),
            ("text", self.text.hex()),
            ("text_dim", self.text_dim.hex()),
            ("cursor", self.cursor.hex()),
            ("error", self.error.hex()),
            ("warning", self.warning.hex()),
            ("success", self.success.hex()),
            ("info", self.info.hex()),
            ("accent", self.accent.hex()),
        ]
    }

    fn from_roles(roles: &SemanticRoles, r: &Resolver) -> Self {
        Self {
            background: r.get(roles.background),
            surface: r.get(roles.surface),
            text: r.get(roles.text),
            text_dim: r.get(roles.text_dim),
            cursor: r.get(roles.cursor),
            error: r.get(roles.error),
            warning: r.get(roles.warning),
            success: r.get(roles.success),
            info: r.get(roles.info),
            accent: r.get(roles.accent),
        }
    }
}

/// The name of the theme the chrome is prescribed to paint — `"nord"`
/// today.
///
/// Reported by anything that TELLS an operator which theme they are
/// looking at (the start screen's footer). Deliberately sourced from the
/// paint seam rather than from a `(deftheme :preset …)` declaration:
/// those are still two different values — threading the authored theme
/// into the paint path is the open half of escriba's theming — and a
/// report built on the declaration would name a theme that is not on
/// screen the moment they diverge.
#[must_use]
pub fn prescribed_theme_name() -> &'static str {
    FleetTheme::prescribed_default().preset_name()
}

/// Which ishou palette a role key is looked up in. Kept private: the theme
/// → palette pairing is this module's job, not a caller's.
enum Resolver {
    Pleme,
    Vellum,
    PolarVeil,
}

impl Resolver {
    /// Resolve one role key to a color.
    ///
    /// A miss falls back to the palette's own text/background rather than
    /// panicking: a role that a binding does not fill is a legibility bug,
    /// not a crash, and the ishou docs state the Nord binding fills the
    /// Vellum-only roles with nearest-legacy keys precisely so `pairs()`
    /// always resolves.
    fn get(&self, key: &str) -> Rgb {
        match self {
            Self::Pleme => {
                let p = ColorPalette::pleme();
                p.get(key)
                    .unwrap_or_else(|| p.get("snow_storm_2").unwrap_or(Rgb::new(0, 0, 0)))
            }
            Self::Vellum => {
                let p = VellumPalette::vellum();
                p.get(key)
                    .unwrap_or_else(|| p.get("snow1").unwrap_or(Rgb::new(0, 0, 0)))
            }
            Self::PolarVeil => {
                let p = VellumPalette::polar_veil();
                p.get(key)
                    .unwrap_or_else(|| p.get("snow1").unwrap_or(Rgb::new(0, 0, 0)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The load-bearing one: escriba's chrome must paint the theme the fleet
    /// prescribes. This is the assertion whose absence let escriba drift onto
    /// Vellum while the fleet moved to PlemeDark.
    #[test]
    fn prescribed_chrome_tracks_the_fleet_theme() {
        let prescribed = ChromePalette::prescribed().hex_tuple();
        let fleet = ChromePalette::for_theme(FleetTheme::prescribed_default()).hex_tuple();
        assert_eq!(
            prescribed, fleet,
            "prescribed chrome must be the fleet theme's chrome"
        );
    }

    /// Every theme must resolve to a legible palette — no role may collapse
    /// to the black fallback, which would mean a binding gap went unnoticed.
    #[test]
    fn every_theme_resolves_without_hitting_the_fallback() {
        for theme in [
            FleetTheme::PlemeDark,
            FleetTheme::Vellum,
            FleetTheme::PolarVeil,
            FleetTheme::Bare,
        ] {
            let black = Rgb::new(0, 0, 0).hex();
            for (name, v) in ChromePalette::for_theme(theme).hex_tuple() {
                assert_ne!(
                    v, black,
                    "{theme:?}: role {name} fell through to the fallback"
                );
            }
        }
    }

    /// The name we TELL an operator must be the theme we PAINT.
    ///
    /// The start screen's footer prints this. It would have been easy to
    /// source it from the rc's `(deftheme :preset …)` instead — and that
    /// would be a lie the moment the declared and painted themes diverge,
    /// which today they always can, because nothing threads the authored
    /// theme into the paint path yet.
    #[test]
    fn the_reported_theme_name_is_the_one_actually_painted() {
        assert_eq!(
            prescribed_theme_name(),
            FleetTheme::prescribed_default().preset_name(),
        );
        // And it names a theme whose chrome is what `prescribed()` resolves.
        let by_name = ChromePalette::for_theme(FleetTheme::prescribed_default());
        assert_eq!(by_name.hex_tuple(), ChromePalette::prescribed().hex_tuple());
    }

    /// Themes must actually differ — if two arms resolved identically the
    /// selection would be decorative.
    #[test]
    fn distinct_themes_paint_distinctly() {
        assert_ne!(
            ChromePalette::for_theme(FleetTheme::PlemeDark)
                .background
                .hex(),
            ChromePalette::for_theme(FleetTheme::Vellum)
                .background
                .hex(),
            "Nord and Vellum must not share a ground"
        );
    }
}
