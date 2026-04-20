//! `defpalette` — Lisp-authored base16 color palette.
//!
//! A palette is a named bundle of 16 colors a theme can reference.
//! Users who want more than the built-in presets (`nord`,
//! `gruvbox-dark`, `tokyo-night`, `catppuccin-mocha`) declare their
//! own palette here and reference it by name in `(deftheme :preset
//! "my-palette")`.
//!
//! Shape follows the base16 convention — 8 base colors (base00..07)
//! covering background / foreground / chrome, plus 8 accent colors
//! (base08..0F). Every modern colorscheme ecosystem (vim-base16,
//! nvim-base16, helix themes, alacritty themes) maps onto this,
//! so existing corpora port cleanly.
//!
//! ```lisp
//! (defpalette :name "gruvbox-dark-soft"
//!             :base00 "#32302f" :base01 "#3c3836" :base02 "#504945"
//!             :base03 "#665c54" :base04 "#bdae93" :base05 "#d5c4a1"
//!             :base06 "#ebdbb2" :base07 "#fbf1c7"
//!             :base08 "#fb4934" :base09 "#fe8019" :base0a "#fabd2f"
//!             :base0b "#b8bb26" :base0c "#8ec07c" :base0d "#83a598"
//!             :base0e "#d3869b" :base0f "#d65d0e")
//! ```
//!
//! Keywords are case-sensitive — tatara-lisp's kwarg lookup matches
//! the Rust field name verbatim (`base0a`, not `base0A`).

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defpalette")]
pub struct PaletteSpec {
    /// Palette identifier — `(deftheme :preset "<name>")` references
    /// this. Avoid collisions with the built-in presets
    /// ([`crate::KNOWN_PRESETS`]).
    pub name: String,

    // Base colours — background through foreground (darkest → lightest).
    /// Default background.
    #[serde(default)]
    pub base00: String,
    /// Lighter background (status bar, line numbers).
    #[serde(default)]
    pub base01: String,
    /// Selection background.
    #[serde(default)]
    pub base02: String,
    /// Comments, invisibles, line highlighting.
    #[serde(default)]
    pub base03: String,
    /// Dark foreground (status bar).
    #[serde(default)]
    pub base04: String,
    /// Default foreground, caret, delimiters, operators.
    #[serde(default)]
    pub base05: String,
    /// Light foreground.
    #[serde(default)]
    pub base06: String,
    /// Light background (not often used).
    #[serde(default)]
    pub base07: String,

    // Accent colours — base16 semantic roles.
    /// Variables, xml tags, markup link text, markup lists, diff deleted.
    #[serde(default)]
    pub base08: String,
    /// Integers, boolean, constants, xml attrs, markup link url.
    #[serde(default)]
    pub base09: String,
    /// Classes, markup bold, search bg.
    #[serde(default)]
    pub base0a: String,
    /// Strings, inherited class, markup code, diff inserted.
    #[serde(default)]
    pub base0b: String,
    /// Support, regex, escape chars, markup quotes.
    #[serde(default)]
    pub base0c: String,
    /// Functions, methods, headings.
    #[serde(default)]
    pub base0d: String,
    /// Keywords, storage, selector, markup italic, diff changed.
    #[serde(default)]
    pub base0e: String,
    /// Deprecated, opening/closing embedded tags (e.g. `<?php ?>`).
    #[serde(default)]
    pub base0f: String,
}
