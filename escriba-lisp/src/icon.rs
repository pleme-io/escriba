//! `deficon` — Lisp-authored filetype / file-pattern icon binding.
//!
//! Absorbs nvim-web-devicons: map a filetype or filename pattern to a glyph
//! + colour.
//!
//! ```lisp
//! (deficon :filetype "rust"     :glyph "" :fg "#dea584")
//! (deficon :filetype "python"   :glyph "" :fg "#ffbc03")
//! (deficon :pattern "Cargo.*"   :glyph "" :fg "#dea584")
//! (deficon :pattern ".envrc"    :glyph "" :fg "#89e051")
//! ```
//!
//! ## Tier-honest: this is a DECLARATION and nothing reads it yet
//!
//! `ApplyPlan::icons` really is populated at boot from the bundled catalog,
//! but no renderer resolves an icon: repo-wide, `.glyph` has no reader
//! outside this file. This doc used to say "renderers pick the icon up
//! automatically for the tab line, file tree, and picker previews" — that was
//! never true, and it is exactly the kind of claim that makes an unbuilt
//! surface read as finished. The match order below is the INTENDED contract:
//! there is no lookup function, no fnmatch call, and no caller.
//!
//! Intended (unimplemented) match order: `:pattern` first, glob-style, then
//! `:filetype` as an exact match on the buffer's major mode. A buffer with no
//! match renders no icon — no default fallback.
//!
//! ## `:glyph` is gated by the matrix, not by this crate
//!
//! Icons compile through the bare `compile()` rather than
//! `compile_validated()`, so an empty `:glyph` parses cleanly and no
//! `LispError` names icons at all. That gap is not theoretical: the shipped
//! catalog carried 23 empty glyphs while `plan.icons.len() >= 20` stayed
//! green, because an arity assertion is blind to content. The invariant now
//! lives in `escriba/tests/plugin_matrix.rs`
//! (`every_bundled_icon_has_a_real_nerd_font_glyph`), which requires exactly
//! one codepoint in a nerd-font private-use area.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "deficon")]
pub struct IconSpec {
    /// Filetype name (matches [`crate::MajorModeSpec::name`]).
    /// Either `:filetype` or `:pattern` must be set.
    #[serde(default)]
    pub filetype: String,
    /// Glob pattern matched against the buffer filename.
    #[serde(default)]
    pub pattern: String,
    /// Glyph / string rendered as the icon. Typically a nerd-font
    /// character, but plain ASCII works (for non-nerd-font terms).
    pub glyph: String,
    /// Optional foreground colour (`"#rrggbb"` or palette ref).
    #[serde(default)]
    pub fg: String,
}

impl IconSpec {
    /// True when this spec binds via filename pattern rather than
    /// filetype. Matters for lookup order at dispatch.
    #[must_use]
    pub fn is_pattern(&self) -> bool {
        !self.pattern.is_empty()
    }
}
