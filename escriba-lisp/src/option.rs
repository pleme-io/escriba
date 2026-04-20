//! `defoption` — Lisp-authored editor option.
//!
//! ```lisp
//! (defoption :name "number"          :value "true")
//! (defoption :name "relativenumber"  :value "true")
//! (defoption :name "tabstop"         :value "4")
//! (defoption :name "wrap"            :value "false")
//! ```
//!
//! The value is parsed as a string; consumers coerce to their
//! preferred type (bool / integer / enum).

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defoption")]
pub struct OptionSpec {
    /// The option name (e.g., `"number"`, `"tabstop"`, `"wrap"`).
    pub name: String,
    /// The option's string-encoded value (`"true"`, `"4"`, `"unix"`, …).
    pub value: String,
}
