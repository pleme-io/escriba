//! `defabbrev` — Lisp-authored insert-mode abbreviation.
//!
//! Triggers on word-boundary typing — same UX as vim `iabbrev` or
//! vscode abbreviation-snippet.
//!
//! ```lisp
//! (defabbrev :trigger "teh"     :expansion "the")
//! (defabbrev :trigger "recieve" :expansion "receive")
//! ```

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defabbrev")]
pub struct AbbrevSpec {
    /// The literal text the user types.
    pub trigger: String,
    /// What replaces `trigger` once the word boundary is reached.
    pub expansion: String,
}
