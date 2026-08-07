//! `defsplash` — the Lisp-authored start screen.
//!
//! Every editor in the category ships one (vim `:intro`, alpha.nvim /
//! startify, the emacs splash buffer, the `VSCode` welcome tab) and every
//! one of them hard-codes it in the renderer. This form is the whole
//! screen as data: the wordmark, the line under it, and the menu.
//!
//! ```lisp
//! (defsplash
//!   :tagline "Rust owns the invariants · Lisp owns the authoring"
//!   :art ("  ___ " " |___|")
//!   :entries ((:key "e" :label "start editing" :action "normal")
//!             (:key "/" :label "search"        :action "search-forward")
//!             (:key "q" :label "quit"          :action "quit")))
//! ```
//!
//! `:disabled #t` turns the screen off while keeping the declaration —
//! retirement is a typed flag, never a deletion.
//!
//! ## Why `:disabled` and not `:enable`
//!
//! `defeffect` spells this `:enable #t`, and the inconsistency is
//! deliberate. A key absent from a form does NOT reach serde's
//! `#[serde(default = …)]`; it takes the field type's zero value. For a
//! `bool` that is `false`, so an `:enable` field makes a bare
//! `(defsplash :art … :entries …)` parse cleanly and then never appear
//! — declared, valid, and silently inert. Inverting the polarity puts
//! the SAFE state on the zero value: say nothing and the screen shows.
//! (Verified, not assumed: `:enable` was tried first and the shipped
//! form booted with `enable = false`.)
//!
//! An entry's `:action` goes through the SAME resolver a `defkeybind`
//! `:action` does ([`resolve_action`](crate::resolve_action)), so the
//! two authoring surfaces can never disagree about what `"quit"` means,
//! and an entry naming a plugin-registered command resolves at dispatch
//! exactly like a key bound to it would.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

/// One menu row: the key, what it does in words, the action it runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplashEntrySpec {
    /// The key an operator presses. Exactly one character — a
    /// multi-character key would be a sequence, and the splash is
    /// dismissed by the first keypress, so there is no second one to
    /// wait for.
    pub key: String,
    /// Human-readable description shown beside the key.
    #[serde(default)]
    pub label: String,
    /// Action string, resolved through the shared keybind resolver.
    #[serde(default)]
    pub action: String,
}

impl SplashEntrySpec {
    /// The key as a single `char`, or `None` when `:key` is empty or
    /// longer than one character.
    #[must_use]
    pub fn key_char(&self) -> Option<char> {
        let mut chars = self.key.chars();
        let c = chars.next()?;
        chars.next().is_none().then_some(c)
    }
}

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defsplash")]
pub struct SplashSpec {
    /// Retire the screen without deleting its declaration. Defaults to
    /// `false` (shown) — see the module docs for why the polarity is
    /// inverted relative to `defeffect`'s `:enable`.
    #[serde(default)]
    pub disabled: bool,
    /// The line under the wordmark.
    #[serde(default)]
    pub tagline: String,
    /// Wordmark rows, top to bottom.
    #[serde(default)]
    pub art: Vec<String>,
    /// The menu, in display order.
    #[serde(default)]
    pub entries: Vec<SplashEntrySpec>,
}

impl SplashSpec {
    /// Is this screen live? The one place the flag's polarity is read, so
    /// no call site has to remember which way `disabled` points.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        !self.disabled
    }

    /// Entries whose `:key` is a usable single character, paired with it.
    /// Malformed entries are rejected at parse time, so by the time this
    /// runs every entry qualifies — it exists so the apply path never
    /// re-derives the char.
    pub fn resolved_entries(&self) -> impl Iterator<Item = (char, &SplashEntrySpec)> {
        self.entries
            .iter()
            .filter_map(|e| e.key_char().map(|c| (c, e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_char_accepts_exactly_one_character() {
        let one = |k: &str| SplashEntrySpec {
            key: k.to_string(),
            ..Default::default()
        };
        assert_eq!(one("e").key_char(), Some('e'));
        assert_eq!(one("/").key_char(), Some('/'));
        // A multibyte glyph is still ONE character — the check is on
        // chars, not bytes, or an operator could not bind `é`.
        assert_eq!(one("é").key_char(), Some('é'));
        assert_eq!(one("").key_char(), None);
        assert_eq!(one("gg").key_char(), None);
    }

    /// The regression that forced the `:disabled` polarity: a form with
    /// no flag at all must be LIVE. With an `:enable` field it parsed to
    /// `false` and the shipped start screen never appeared.
    #[test]
    fn a_form_that_says_nothing_about_the_flag_is_live() {
        let specs: Vec<SplashSpec> =
            tatara_lisp::compile_typed(r#"(defsplash :tagline "hi")"#).expect("parses");
        assert_eq!(specs.len(), 1);
        assert!(
            specs[0].is_enabled(),
            "a bare defsplash must show, not silently vanish",
        );
    }

    #[test]
    fn the_disabled_flag_retires_the_screen() {
        let specs: Vec<SplashSpec> =
            tatara_lisp::compile_typed(r"(defsplash :disabled #t)").expect("parses");
        assert!(!specs[0].is_enabled());
    }
}
