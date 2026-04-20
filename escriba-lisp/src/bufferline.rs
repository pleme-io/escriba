//! `defbufferline` — Lisp-authored tab/buffer bar composition.
//!
//! Sibling to [`StatusLineSpec`](crate::StatusLineSpec) but for the
//! tab row. Mirrors akinsho/bufferline.nvim and vim's native
//! tabline.
//!
//! ```lisp
//! (defbufferline
//!   :show-close-icons #t
//!   :separator "│"
//!   :modified-indicator "●"
//!   :show-diagnostics #t
//!   :max-name-length 18)
//! ```

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defbufferline")]
pub struct BufferLineSpec {
    /// Whether to render a close button per tab.
    #[serde(default)]
    pub show_close_icons: bool,
    /// Separator character / string between tabs.
    #[serde(default)]
    pub separator: String,
    /// Glyph shown next to a modified buffer's name.
    #[serde(default)]
    pub modified_indicator: String,
    /// Whether to surface diagnostic counts per tab.
    #[serde(default)]
    pub show_diagnostics: bool,
    /// Truncate file names to this many chars (0 = unlimited).
    #[serde(default)]
    pub max_name_length: u32,
    /// Hide the filetype icon beside tab names. The default (`false`)
    /// shows icons when the terminal supports them.
    #[serde(default)]
    pub no_icons: bool,
    /// Disable tab click-to-focus. The default (`false`) treats
    /// mouse clicks on a tab as focus requests.
    #[serde(default)]
    pub no_click: bool,
}
