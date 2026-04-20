//! `defkmacro` — declarative keyboard macro (stored key sequence).
//!
//! Absorbs vim's `q`/`Q` recording + `@`-register replay, emacs's
//! `kmacro.el` named macros, and jetbrains' "keyboard macros" action.
//! In those editors macros are recorded *live* (press q, perform
//! actions, press q again) and live in a volatile register. Escriba
//! lifts the concept into the rc: the sequence is typed, named,
//! filetype-scoped, and version-controllable alongside the rest of
//! the declarative editor spec.
//!
//! ```lisp
//! ;; Wrap the current line in a C-style header comment.
//! (defkmacro :name "header-comment"
//!            :description "wrap line in C-style block comment"
//!            :keys "I/* <Esc>A */<Esc>"
//!            :mode "normal"
//!            :filetype "c"
//!            :keybind "<leader>mh")
//!
//! ;; Insert the ISO date on a new line.
//! (defkmacro :name "insert-date"
//!            :keys ":put =strftime('%Y-%m-%d')<CR>"
//!            :mode "normal"
//!            :keybind "<leader>md")
//!
//! ;; Surround visual selection with backticks (markdown code span).
//! (defkmacro :name "mark-code"
//!            :keys "c`<C-r>\"`<Esc>"
//!            :mode "visual"
//!            :filetype "markdown"
//!            :keybind "<leader>mc")
//!
//! ;; Named with a classic vim register so `@a` replays it too —
//! ;; the apply layer pre-populates register "a" at startup.
//! (defkmacro :name "quote-paragraph"
//!            :keys "vip>"
//!            :mode "normal"
//!            :register "a")
//! ```
//!
//! ## Field contract
//!
//! - `:name` is required (tatara-lisp enforces). Unique within the plan.
//! - `:keys` is required and must be non-empty (the apply layer rejects
//!   empty-keys specs so a typo fails fast instead of silently producing
//!   a no-op macro).
//! - `:mode` is optional. Empty means "replay in whatever mode is current";
//!   a non-empty value must match the same vocabulary as `defkeybind :mode`
//!   (`normal`, `insert`, `visual`, `visual-line`, `command`).
//! - `:register` is optional. When set it must be a single `a-z` /
//!   `A-Z` / `0-9` character — the vim register name that `@<register>`
//!   looks up. Setting it makes the declarative kmacro reachable via
//!   the classic vim replay path.
//!
//! ## Why declarative macros
//!
//! No editor in the category serializes stored macros to the rc
//! out-of-the-box. vim's `:mkvimrc` dumps register contents but not in
//! a typed form; emacs's `kmacro-edit-macro` edits live bytes with
//! no schema. Escriba's `defkmacro` gives kmacros a stable identity
//! (BLAKE3-addressable via the parse plan), a filetype scope, and
//! reviewable diffs — the same benefits every other def-form gets
//! for free.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defkmacro")]
pub struct KmacroSpec {
    /// Stable id — unique within the plan. Used for replay lookups
    /// (`escriba kmacro <name>`) and the picker entry.
    pub name: String,
    /// One-line picker description.
    #[serde(default)]
    pub description: String,
    /// Key sequence in the same grammar `defkeybind :key` uses:
    /// `<Esc>`, `<C-r>`, `<CR>`, `<Space>`, `<Tab>`, plus literal
    /// characters. Multi-key strings are concatenated verbatim.
    #[serde(default)]
    pub keys: String,
    /// Replay-entry mode. Empty = "replay in the current mode". A
    /// non-empty value is validated against the [`KNOWN_MODES`]
    /// vocabulary at apply time.
    #[serde(default)]
    pub mode: String,
    /// Filetype scope — empty = global, non-empty = only visible /
    /// firable when the active buffer's filetype matches.
    #[serde(default)]
    pub filetype: String,
    /// Optional keybind that replays the kmacro without going through
    /// the picker or `@<register>`.
    #[serde(default)]
    pub keybind: String,
    /// Optional single-char vim register name (`a-z`, `A-Z`, `0-9`).
    /// When set the runtime pre-populates the register with the
    /// key sequence so `@<register>` replays classically — makes
    /// the declarative macro backward-compatible with vim reflexes.
    #[serde(default)]
    pub register: String,
}

/// Modes that can appear as `:mode` on a [`KmacroSpec`]. Kept in
/// sync with the `validate_mode` table the keybind apply layer uses.
pub const KNOWN_MODES: &[&str] = &["normal", "insert", "visual", "visual-line", "command"];

/// True when `mode` is a recognized kmacro replay mode.
#[must_use]
pub fn is_known_mode(mode: &str) -> bool {
    KNOWN_MODES.contains(&mode)
}

impl KmacroSpec {
    /// True when `:register` is a single `a-z` / `A-Z` / `0-9` char —
    /// the vim register-name vocabulary. Empty is valid (means
    /// "no register binding"); anything else is malformed.
    #[must_use]
    pub fn has_valid_register(&self) -> bool {
        if self.register.is_empty() {
            return true;
        }
        let mut chars = self.register.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return false;
        };
        c.is_ascii_alphanumeric()
    }

    /// Count how many named-key tokens (`<Esc>`, `<CR>`, …) appear
    /// in the sequence. A quick structural heuristic used by the
    /// picker to show e.g. "12 keys, 3 specials" next to the macro.
    #[must_use]
    pub fn named_key_count(&self) -> usize {
        let bytes = self.keys.as_bytes();
        let mut count = 0usize;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'<' {
                if let Some(j) = bytes[i + 1..].iter().position(|&b| b == b'>') {
                    // Only count if the content between < and > is
                    // non-empty — bare "<>" isn't a named key.
                    if j > 0 {
                        count += 1;
                        i += j + 2;
                        continue;
                    }
                }
            }
            i += 1;
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare(name: &str, keys: &str) -> KmacroSpec {
        KmacroSpec {
            name: name.into(),
            keys: keys.into(),
            ..Default::default()
        }
    }

    #[test]
    fn known_mode_vocabulary_matches_defkeybind_vocabulary() {
        // The kmacro replay-entry mode list must match the keybind
        // mode list — they're the same concept at the runtime
        // layer. Diverging them silently would mean a kmacro can
        // claim a mode the keymap doesn't know.
        assert!(is_known_mode("normal"));
        assert!(is_known_mode("insert"));
        assert!(is_known_mode("visual"));
        assert!(is_known_mode("visual-line"));
        assert!(is_known_mode("command"));
        assert!(!is_known_mode("superman"));
        assert!(!is_known_mode(""));
    }

    #[test]
    fn register_empty_is_valid() {
        // Empty register = "no register binding" — the default.
        let s = bare("m", "iX<Esc>");
        assert!(s.has_valid_register());
    }

    #[test]
    fn register_accepts_single_alphanumeric_char() {
        for r in ["a", "z", "A", "Z", "0", "9"] {
            let s = KmacroSpec {
                name: "m".into(),
                keys: "x".into(),
                register: r.into(),
                ..Default::default()
            };
            assert!(s.has_valid_register(), "register {r:?} should be valid");
        }
    }

    #[test]
    fn register_rejects_multichar_or_symbols() {
        // Empty string is intentionally valid (no register binding) —
        // covered by `register_empty_is_valid` above. Everything else
        // non-conforming must reject.
        for bad in ["ab", "!", "@@", "aa", " ", "-"] {
            let s = KmacroSpec {
                name: "m".into(),
                keys: "x".into(),
                register: bad.into(),
                ..Default::default()
            };
            assert!(
                !s.has_valid_register(),
                "register {bad:?} should be invalid",
            );
        }
    }

    #[test]
    fn named_key_count_handles_canonical_sequences() {
        // Bare chars: zero specials.
        assert_eq!(bare("m", "ihello").named_key_count(), 0);
        // Single `<Esc>` at the end.
        assert_eq!(bare("m", "ihello<Esc>").named_key_count(), 1);
        // Three specials.
        assert_eq!(bare("m", "<Esc>:wq<CR>").named_key_count(), 2);
        // `<C-r>"` — the `<C-r>` counts; the `"` is bare.
        assert_eq!(bare("m", "c`<C-r>\"`<Esc>").named_key_count(), 2);
    }

    #[test]
    fn named_key_count_ignores_unclosed_or_empty_brackets() {
        // Unclosed `<` — no count.
        assert_eq!(bare("m", "iless <than").named_key_count(), 0);
        // Empty `<>` — skipped.
        assert_eq!(bare("m", "i<>x").named_key_count(), 0);
        // Stray `>` — no count.
        assert_eq!(bare("m", "i>x").named_key_count(), 0);
    }
}
