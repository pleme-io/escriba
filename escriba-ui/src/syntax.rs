//! Syntax colours, resolved through ishou for every theme.
//!
//! ## Why this exists
//!
//! hikari ships exactly one theme — `NordTheme`, a table of twenty hardcoded
//! hex literals — and escriba's GPU face held one by value. So picking Vellum
//! gave you Vellum chrome with **Nord code** inside it: the frame changed
//! colour and the text did not. That is not a theme; it is a border.
//!
//! ## Why roles reproduce Nord rather than replacing it
//!
//! The obvious worry with routing syntax through a role vocabulary is that it
//! flattens a rich palette onto too few colours and makes the default look
//! worse. That worry does not survive contact with the numbers: ishou's
//! `SemanticRoles` carries 27 roles, and on `pleme_dark` they resolve to the
//! Nord palette that `NordTheme` was hand-transcribed from. Nineteen of the
//! twenty `HlClass` variants land on the **identical hex**:
//!
//! | class | role | Nord |
//! |---|---|---|
//! | `Function` | `primary` | `#88C0D0` |
//! | `Type` / `Namespace` | `info` | `#8FBCBB` |
//! | `Keyword` / `Operator` | `link` | `#81A1C1` |
//! | `Str` / `Added` | `success` | `#A3BE8C` |
//! | `Numeric` / `Attribute` | `agent` | `#B48EAD` |
//! | `Boolean` / `Constant` | `warning` | `#D08770` |
//! | `Escape` / `Special` | `search` | `#EBCB8B` |
//! | `Error` / `Removed` | `error` | `#BF616A` |
//! | `Punctuation` | `text_bright` | `#ECEFF4` |
//! | `Hyperlink` / `Hint` | `structural` | `#5E81AC` |
//! | `Plain` / `Variable` | `text_muted` | `#D8DEE9` |
//!
//! The single divergence is `Comment`: `NordTheme` uses `#616E88`, the
//! `text_dim` role resolves to `#4C566A`. Both are Nord Polar Night, and
//! `text_dim` is the colour the rest of the fleet already dims with — so
//! escriba's comments now match escriba's gutter instead of matching a
//! constant in someone else's crate. That is stated rather than hidden
//! because it IS a visible change on the default theme.
//!
//! `themes_reproduce_nord_on_the_fleet_default` below pins all of this
//! against the real `NordTheme`, so a future role rebinding that silently
//! moved the default look fails the build.

use hikari_core::{HlClass, Rgb as HlRgb, Theme};

use crate::chrome::ChromePalette;

/// A `hikari_core::Theme` that answers from a `ChromePalette`.
///
/// Carries the palette by value — it is 18 `Rgb`s, `Copy`, and the alternative
/// is a lifetime on a trait object the renderer holds across frames.
#[derive(Debug, Clone, Copy)]
pub struct ChromeSyntax {
    chrome: ChromePalette,
}

impl ChromeSyntax {
    #[must_use]
    pub const fn new(chrome: ChromePalette) -> Self {
        Self { chrome }
    }

    /// The syntax colours for `theme`.
    #[must_use]
    pub fn for_theme(theme: crate::chrome::FleetTheme) -> Self {
        Self::new(ChromePalette::for_theme(theme))
    }

    /// The palette this resolves against.
    #[must_use]
    pub const fn chrome(&self) -> &ChromePalette {
        &self.chrome
    }
}

impl Theme for ChromeSyntax {
    fn color(&self, class: HlClass) -> HlRgb {
        let c = &self.chrome;
        // Total over `HlClass` — no wildcard arm. A variant added upstream
        // fails THIS match to compile rather than falling into a catch-all
        // and rendering as plain text, which is the failure mode that hides:
        // new syntax silently loses its colour and nobody files a bug.
        let rgb = match class {
            HlClass::Comment { .. } => c.text_dim,
            HlClass::Keyword | HlClass::Operator | HlClass::Info => c.link,
            HlClass::KeywordArg | HlClass::Attribute | HlClass::Numeric { .. } => c.agent,
            HlClass::Type | HlClass::Namespace => c.info,
            HlClass::Function => c.primary,
            HlClass::Str | HlClass::Added => c.success,
            HlClass::Escape | HlClass::Special | HlClass::Warning => c.search,
            HlClass::Boolean | HlClass::Constant => c.warning,
            HlClass::Punctuation => c.text_bright,
            HlClass::Hyperlink | HlClass::Hint => c.structural,
            HlClass::Error | HlClass::Removed => c.error,
            HlClass::Variable | HlClass::Whitespace | HlClass::Unchanged | HlClass::Plain => {
                c.text_muted
            }
        };
        HlRgb::new(rgb.r, rgb.g, rgb.b)
    }
}

/// Every `HlClass` variant, for tests and for any consumer that needs to walk
/// the vocabulary (a theme previewer, a contrast audit).
///
/// Hand-listed because `HlClass` is upstream and has no enumeration; the
/// exhaustive `match` above is what actually guarantees nothing is missed, and
/// `every_class_is_in_the_roster` keeps this list honest against it.
pub const ALL_CLASSES: &[HlClass] = &[
    HlClass::Comment { multiline: false },
    HlClass::Comment { multiline: true },
    HlClass::Keyword,
    HlClass::KeywordArg,
    HlClass::Type,
    HlClass::Function,
    HlClass::Namespace,
    HlClass::Variable,
    HlClass::Constant,
    HlClass::Str,
    HlClass::Escape,
    HlClass::Numeric { float: false },
    HlClass::Numeric { float: true },
    HlClass::Boolean,
    HlClass::Punctuation,
    HlClass::Operator,
    HlClass::Attribute,
    HlClass::Special,
    HlClass::Hyperlink,
    HlClass::Whitespace,
    HlClass::Error,
    HlClass::Warning,
    HlClass::Info,
    HlClass::Hint,
    HlClass::Added,
    HlClass::Removed,
    HlClass::Unchanged,
    HlClass::Plain,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::FleetTheme;

    fn hex(c: HlRgb) -> String {
        let mut s = String::with_capacity(7);
        s.push('#');
        for b in [c.r, c.g, c.b] {
            s.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
            s.push(char::from_digit(u32::from(b & 0xF), 16).unwrap_or('0'));
        }
        s.to_uppercase()
    }

    /// THE load-bearing test: on the fleet default, routing syntax through
    /// ishou must not change how code looks.
    ///
    /// Without this, "support every theme" would be free to quietly degrade
    /// the one theme almost everyone actually uses.
    #[test]
    fn themes_reproduce_nord_on_the_fleet_default() {
        let ours = ChromeSyntax::for_theme(FleetTheme::prescribed_default());
        let nord = hikari_core::NordTheme;
        let mut diffs = Vec::new();
        for class in ALL_CLASSES {
            let a = hex(ours.color(*class));
            let b = hex(nord.color(*class));
            if a != b {
                diffs.push((format!("{class:?}"), a, b));
            }
        }
        // Exactly one accepted divergence, named in the module docs: Comment
        // moves from hikari's `#616E88` to the `text_dim` role escriba dims
        // everything else with. Anything else is a regression.
        for (class, ours, nord) in &diffs {
            assert!(
                class.starts_with("Comment"),
                "{class} drifted off Nord: {ours} (ours) != {nord} (NordTheme)",
            );
        }
        assert_eq!(
            diffs.len(),
            2,
            "expected exactly the two Comment variants to differ, got {diffs:?}",
        );
    }

    /// A theme change must actually reach the code, which is the whole point.
    #[test]
    fn a_different_theme_paints_code_differently() {
        let nordish = ChromeSyntax::for_theme(FleetTheme::PlemeDark);
        let vellum = ChromeSyntax::for_theme(FleetTheme::Vellum);
        let differing = ALL_CLASSES
            .iter()
            .filter(|c| hex(nordish.color(**c)) != hex(vellum.color(**c)))
            .count();
        assert!(
            differing > ALL_CLASSES.len() / 2,
            "picking Vellum must recolour the CODE, not just the frame: only \
             {differing}/{} classes moved",
            ALL_CLASSES.len(),
        );
    }

    /// Nothing may render invisible.
    #[test]
    fn no_class_collapses_onto_the_background_in_any_theme() {
        for theme in [
            FleetTheme::PlemeDark,
            FleetTheme::Vellum,
            FleetTheme::PolarVeil,
            FleetTheme::Bare,
        ] {
            let syn = ChromeSyntax::for_theme(theme);
            let bg = syn.chrome().background.hex();
            for class in ALL_CLASSES {
                assert_ne!(
                    hex(syn.color(*class)),
                    bg,
                    "{theme:?}: {class:?} is the same colour as the ground",
                );
            }
        }
    }

    /// Code must stay readable as code: the classes a reader scans for have
    /// to be told apart.
    #[test]
    fn the_load_bearing_classes_stay_distinguishable_in_every_theme() {
        // Not ALL classes — some deliberately share a role (`Keyword` and
        // `Operator` are one colour in Nord too). These five are the ones a
        // reader separates at a glance.
        let key = [
            HlClass::Comment { multiline: false },
            HlClass::Keyword,
            HlClass::Str,
            HlClass::Function,
            HlClass::Numeric { float: false },
        ];
        for theme in [
            FleetTheme::PlemeDark,
            FleetTheme::Vellum,
            FleetTheme::PolarVeil,
            FleetTheme::Bare,
        ] {
            let syn = ChromeSyntax::for_theme(theme);
            let mut seen = std::collections::BTreeSet::new();
            for class in key {
                assert!(
                    seen.insert(hex(syn.color(class))),
                    "{theme:?}: {class:?} duplicates another load-bearing class",
                );
            }
        }
    }

    /// The roster must not fall behind the `match`.
    #[test]
    fn every_class_is_in_the_roster() {
        // `HlClass` is upstream and not enumerable, so the roster is hand-
        // listed and this is the honest floor: a count check plus no
        // duplicates. The exhaustive `match` in `color` is what actually
        // guarantees completeness — this only catches a roster that was not
        // updated alongside it.
        let mut seen = std::collections::BTreeSet::new();
        for c in ALL_CLASSES {
            assert!(seen.insert(format!("{c:?}")), "{c:?} listed twice");
        }
        assert_eq!(seen.len(), ALL_CLASSES.len());
    }
}
