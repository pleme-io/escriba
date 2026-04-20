//! `deffold` — declarative folding rules per filetype.
//!
//! Absorbs vim's `set foldmethod=…`, nvim-treesitter-fold, helix's
//! built-in folding, vscode's `FoldingRangeProvider`, emacs's
//! `hs-minor-mode`, jetbrains' language-specific fold registration.
//! Every editor exposes folding, but the shape varies: vim ships a
//! scalar option, nvim puts the table in a Lua plugin, vscode
//! registers programmatic providers, helix hard-codes per-language
//! TS queries. Escriba lifts the whole axis into one typed form so
//! the fold grammar is reviewable alongside the rest of the rc.
//!
//! ```lisp
//! ;; Tree-sitter folding — queries fire against the grammar.
//! (deffold :filetype "rust"
//!          :method "treesitter"
//!          :queries ("(function_item) @fold"
//!                    "(impl_item) @fold"
//!                    "(struct_item) @fold"
//!                    "(mod_item) @fold")
//!          :default-level 2)
//!
//! ;; Indent folding — trigger keywords open folds at each level.
//! (deffold :filetype "python"
//!          :method "indent"
//!          :trigger-chars "def class if for while"
//!          :default-level 1)
//!
//! ;; Marker folding — classic vim `{{{` / `}}}` blocks.
//! (deffold :filetype "vim"
//!          :method "marker"
//!          :marker-start "{{{"
//!          :marker-end "}}}")
//!
//! ;; Heading folding — markdown / org-mode style.
//! (deffold :filetype "markdown"
//!          :method "heading"
//!          :default-level 2)
//! ```
//!
//! ## Methods
//!
//! | Method      | Semantics                                     |
//! |-------------|-----------------------------------------------|
//! | `treesitter`| Run `:queries` against the TS grammar.        |
//! | `indent`    | Indent-based folding (nvim-like).             |
//! | `marker`    | Paired `:marker-start` / `:marker-end` tags.  |
//! | `heading`   | Markdown / org-mode heading hierarchy.        |
//! | `syntax`    | Syntax-highlight group boundaries.            |
//!
//! Vocabulary mirrors vim's `foldmethod` set so users coming from
//! vim recognize the values. Unknown methods are rejected at apply
//! time.
//!
//! ## Validation
//!
//! - `:filetype` required — folds are always filetype-scoped.
//! - `:method` required. Empty defaults to `treesitter` at runtime
//!   (the most useful when the buffer has a grammar) but the apply
//!   layer leaves empty alone so `deffold :filetype "txt"` can
//!   represent "folding disabled" by setting `:method ""`.
//! - `method=treesitter` → `:queries` must be non-empty.
//! - `method=marker` → both `:marker-start` and `:marker-end` must
//!   be non-empty.
//! - `method=indent` / `heading` / `syntax` → no extra invariants;
//!   trigger-chars is informational.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "deffold")]
pub struct FoldSpec {
    /// Filetype scope — folds are always per-filetype.
    #[serde(default)]
    pub filetype: String,
    /// Folding method. See [module docs](self) for the vocabulary.
    #[serde(default)]
    pub method: String,
    /// Tree-sitter queries when `:method = "treesitter"`. Each query
    /// is a TS S-expression; the runtime compiles the set and folds
    /// every `@fold`-captured node.
    #[serde(default)]
    pub queries: Vec<String>,
    /// Space-separated trigger keywords for `:method = "indent"`
    /// / `"heading"`. Informational — the runtime still drives off
    /// the buffer's indent or heading structure, but advertising the
    /// keywords lets escriba doctor show a recognizable summary.
    #[serde(default)]
    pub trigger_chars: String,
    /// Initial fold depth. `0` = "all folds open" (show everything);
    /// `N` = "fold everything deeper than level N on open".
    #[serde(default)]
    pub default_level: u32,
    /// Opening marker for `:method = "marker"`. Classic vim values:
    /// `{{{`, `<<<`, etc.
    #[serde(default)]
    pub marker_start: String,
    /// Closing marker for `:method = "marker"`. Classic vim values:
    /// `}}}`, `>>>`, etc.
    #[serde(default)]
    pub marker_end: String,
}

/// Canonical folding methods. Matches vim's `foldmethod` vocabulary
/// so the rc reads naturally for vim users.
pub const KNOWN_METHODS: &[&str] = &["treesitter", "indent", "marker", "heading", "syntax"];

/// True when `method` is a recognized folding strategy.
#[must_use]
pub fn is_known_method(method: &str) -> bool {
    KNOWN_METHODS.contains(&method)
}

impl FoldSpec {
    /// Effective method — `treesitter` when `:method` is empty.
    /// Chose treesitter as the default because every major editor's
    /// fold story has converged on it; users who want something
    /// else just set `:method` explicitly.
    #[must_use]
    pub fn effective_method(&self) -> &str {
        if self.method.is_empty() { "treesitter" } else { self.method.as_str() }
    }

    /// Structural check — for `:method = "marker"`, both markers
    /// must be non-empty. Pairs with the apply-time validator that
    /// rejects half-configured marker specs.
    #[must_use]
    pub fn marker_pair_complete(&self) -> bool {
        !self.marker_start.is_empty() && !self.marker_end.is_empty()
    }

    /// Count of tree-sitter queries attached — zero for non-TS
    /// methods, otherwise the literal length. Used by the apply
    /// layer to enforce "treesitter method requires at least one
    /// query" and by the picker for the summary display.
    #[must_use]
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_methods_match_vim_foldmethod_vocabulary() {
        // Pin the vocabulary — adding a method here needs a matching
        // runtime arm, so force a conscious update rather than
        // silent drift.
        assert_eq!(
            KNOWN_METHODS,
            &["treesitter", "indent", "marker", "heading", "syntax"],
        );
        for m in KNOWN_METHODS {
            assert!(is_known_method(m));
        }
        assert!(!is_known_method("origami"));
        assert!(!is_known_method(""));
    }

    #[test]
    fn effective_method_falls_back_to_treesitter() {
        let s = FoldSpec::default();
        assert_eq!(s.effective_method(), "treesitter");

        let s = FoldSpec {
            method: "indent".into(),
            ..Default::default()
        };
        assert_eq!(s.effective_method(), "indent");
    }

    #[test]
    fn marker_pair_complete_requires_both_sides() {
        let mut s = FoldSpec {
            method: "marker".into(),
            ..Default::default()
        };
        assert!(!s.marker_pair_complete(), "neither set");
        s.marker_start = "{{{".into();
        assert!(!s.marker_pair_complete(), "only start set");
        s.marker_end = "}}}".into();
        assert!(s.marker_pair_complete(), "both set");
    }

    #[test]
    fn query_count_reports_query_length() {
        let s = FoldSpec::default();
        assert_eq!(s.query_count(), 0);

        let s = FoldSpec {
            queries: vec!["(a)".into(), "(b)".into(), "(c)".into()],
            ..Default::default()
        };
        assert_eq!(s.query_count(), 3);
    }
}
