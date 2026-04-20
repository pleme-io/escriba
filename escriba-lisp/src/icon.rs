//! `deficon` — Lisp-authored filetype / file-pattern icon binding.
//!
//! Absorbs nvim-web-devicons: map a filetype or filename pattern to
//! a glyph + colour. Renderers pick the icon up automatically for
//! the tab line, file tree, and picker previews.
//!
//! ```lisp
//! (deficon :filetype "rust"     :glyph "" :fg "#dea584")
//! (deficon :filetype "python"   :glyph "" :fg "#ffbc03")
//! (deficon :pattern "Cargo.*"   :glyph "" :fg "#dea584")
//! (deficon :pattern ".envrc"    :glyph "" :fg "#89e051")
//! ```
//!
//! Match order is `:pattern` first (glob-style fnmatch), then
//! `:filetype` (exact match on the buffer's major mode). A buffer
//! without a match renders no icon — no default fallback.

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
