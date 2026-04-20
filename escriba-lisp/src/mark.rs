//! `defmark` — Lisp-authored named marks.
//!
//! Absorbs vim's global marks (`'A`..`'Z` + `'0`..`'9` from shada) and
//! emacs's bookmarks into a typed declarative form. A mark here is a
//! labelled pointer — name + file + line + column — plus an escriba
//! extension: a `kind` that picks the navigation semantics.
//!
//! ```lisp
//! (defmark :name "'C"
//!          :file "~/.config/escriba/rc.lisp"
//!          :line 1
//!          :kind "jump")
//!
//! (defmark :name "bug-notes"
//!          :file "~/notes/bugs.md"
//!          :line 42
//!          :column 0
//!          :kind "anchor"
//!          :description "Where I track intermittent test failures")
//!
//! (defmark :name "'S"
//!          :file "~/code/github/pleme-io/escriba/README.md"
//!          :kind "glance"
//!          :description "Peek — shows the file without stealing focus")
//! ```
//!
//! # Fields
//!
//! - `name` — the mark label. Vim-style single-letter forms (`'a`,
//!   `'A`, `'0`) work alongside longer human-readable names
//!   (`"bug-notes"`). Uniqueness is enforced at apply time.
//! - `file` — absolute path or `~`-prefixed. Empty = the mark is
//!   buffer-local (only valid inside the buffer it was set in).
//! - `line` / `column` — 1-indexed position. `column = 0` means
//!   "first non-blank" (matches vim's `'` vs `` ` `` distinction —
//!   ` shows the exact column, `'` goes to start-of-line).
//! - `kind` — `jump` (default, vim semantics), `anchor`, `glance`.
//! - `description` — shown in the mark picker (which-key-style popup).
//!
//! # Kind semantics
//!
//! - **`jump`** — cursor moves to the mark; the old cursor position
//!   is pushed onto the jumplist so `<C-o>` / `<C-i>` cycle works.
//!   Matches vim's `'A` behaviour exactly.
//! - **`anchor`** — cursor moves, but the mark's file is pinned in
//!   a sidebar / split and stays visible after the jump. Useful for
//!   reference docs you keep coming back to.
//! - **`glance`** — opens a peek window (zed-style) without moving
//!   the primary cursor or modifying the jumplist. `<Esc>` closes.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defmark")]
pub struct MarkSpec {
    /// Mark label. Single-letter (`'A`, `'b`, `'0`) or longer human
    /// name (`"bug-notes"`). Case-significant.
    pub name: String,
    /// Target file; `~` expands. Empty = buffer-local.
    #[serde(default)]
    pub file: String,
    /// 1-indexed line. 0 = stay on current line.
    #[serde(default)]
    pub line: u32,
    /// 1-indexed column. 0 = first non-blank (vim `'` vs `` ` ``
    /// distinction — `'` goes to start-of-line, `` ` `` exact col).
    #[serde(default)]
    pub column: u32,
    /// One of [`KNOWN_KINDS`]. Empty defaults to `"jump"`.
    #[serde(default)]
    pub kind: String,
    /// Picker-line description.
    #[serde(default)]
    pub description: String,
}

/// Canonical kind values. Unknown kinds reject at apply time so
/// users learn about the typo immediately instead of seeing a
/// silent fallback to jump semantics.
pub const KNOWN_KINDS: &[&str] = &["jump", "anchor", "glance"];

#[must_use]
pub fn is_known_kind(name: &str) -> bool {
    name.is_empty() || KNOWN_KINDS.iter().any(|k| *k == name)
}

impl MarkSpec {
    /// True when the name looks like a traditional vim single-letter
    /// mark — `'a..'z` (buffer-local), `'A..'Z` (global across
    /// shada), or `'0..'9` (file marks). Used by the mark picker
    /// to group vim-compatible entries first.
    #[must_use]
    pub fn is_vim_single_letter(&self) -> bool {
        let Some(rest) = self.name.strip_prefix('\'') else {
            return false;
        };
        let mut chars = rest.chars();
        let (first, next) = (chars.next(), chars.next());
        match (first, next) {
            (Some(c), None) if c.is_ascii_alphanumeric() => true,
            _ => false,
        }
    }

    /// Resolve the effective kind, collapsing empty into the default.
    #[must_use]
    pub fn effective_kind(&self) -> &str {
        if self.kind.is_empty() { "jump" } else { &self.kind }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_kind_accepts_empty_and_canonicals() {
        assert!(is_known_kind(""));
        for k in KNOWN_KINDS {
            assert!(is_known_kind(k));
        }
        assert!(!is_known_kind("laser"));
    }

    #[test]
    fn vim_single_letter_classifier() {
        let cases: &[(&str, bool)] = &[
            ("'A", true),
            ("'a", true),
            ("'0", true),
            ("'9", true),
            ("'AA", false), // two letters — escriba-native, not vim-native
            ("''", false),  // prev-jump mark, not a letter
            ("A", false),   // missing prefix
            ("bug-notes", false),
            ("", false),
        ];
        for (name, want) in cases {
            let m = MarkSpec {
                name: (*name).to_string(),
                ..Default::default()
            };
            assert_eq!(m.is_vim_single_letter(), *want, "name {name}");
        }
    }

    #[test]
    fn effective_kind_defaults_jump() {
        let m = MarkSpec {
            name: "x".into(),
            ..Default::default()
        };
        assert_eq!(m.effective_kind(), "jump");

        let m = MarkSpec {
            name: "x".into(),
            kind: "anchor".into(),
            ..Default::default()
        };
        assert_eq!(m.effective_kind(), "anchor");
    }
}

impl Default for MarkSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            file: String::new(),
            line: 0,
            column: 0,
            kind: String::new(),
            description: String::new(),
        }
    }
}
