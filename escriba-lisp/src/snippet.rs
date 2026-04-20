//! `defsnippet` — Lisp-authored insert-mode snippet.
//!
//! Two authoring modes:
//!
//! - **Inline body** — classic LSP-snippet grammar body, embedded
//!   in the rc. Maps cleanly onto LuaSnip / UltiSnips / vscode
//!   snippets json corpora. `${1:placeholder}`, `${2}`, `$0`
//!   for the final cursor.
//!
//! - **Content-addressed** — `:hash "<blake3-hex>"` references a
//!   snippet body by BLAKE3-128 hash (32-char lowercase hex). The
//!   hash is resolved at expansion time via the runtime's
//!   hash-store (local snippet history, mado's content-addressed
//!   clipboard via MCP, a shared team store, …). Same token
//!   works across all of them because the address **is** the
//!   content.
//!
//! Exactly one of `:body` / `:hash` must be set. The apply layer
//! rejects specs with neither or both so typos fail fast.
//!
//! ```lisp
//! ;; Inline body.
//! (defsnippet :trigger "fn"
//!             :body "fn ${1:name}(${2}) -> ${3} { ${0} }")
//!
//! ;; Hash-referenced. The payload was copied into mado or another
//! ;; content-store earlier; this snippet just names the hash.
//! (defsnippet :trigger "deploy-cmd"
//!             :hash "af42c0d18e9b3f4aa18b7c3ef1de93a4")
//! ```
//!
//! ## Why content-addressed
//!
//! No editor category member (vim / nvim / helix / zed / vscode /
//! emacs / cursor) has content-addressed snippets. A hash reference
//! gives escriba four properties at once:
//!
//! 1. **Shareable across files** — the same snippet hash can appear
//!    in many rc files; editing the payload in the store (once)
//!    updates every consumer.
//! 2. **Reproducible** — `:hash "af42…"` today means the same bytes
//!    as `:hash "af42…"` next year, by definition of content
//!    addressing.
//! 3. **Attestable** — the pleme-io convergence stack (tatara /
//!    tameshi / sekiban) speaks BLAKE3. A snippet's hash is a
//!    first-class attestation anchor: we can prove what got pasted
//!    without archiving the payload.
//! 4. **Interop-by-construction** — mado's clipboard store
//!    (`mado::clipboard_store`) uses the same hash format. Copy in
//!    the terminal, paste-by-hash in the editor, no payload crosses
//!    the socket.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defsnippet")]
pub struct SnippetSpec {
    /// Literal trigger text — typed in insert mode, expanded on a
    /// dedicated key (tab by default).
    pub trigger: String,
    /// LSP-snippet grammar body. Mutually exclusive with `:hash`.
    #[serde(default)]
    pub body: String,
    /// 32-char lowercase BLAKE3-128 hex. Same format as
    /// `mado::clipboard_store::ClipboardHash::to_hex()`. Mutually
    /// exclusive with `:body`.
    #[serde(default)]
    pub hash: String,
    /// Optional filetype scope (matches `defft :mode`). Empty = global.
    #[serde(default)]
    pub filetype: String,
    /// Optional description for the picker preview.
    #[serde(default)]
    pub description: String,
}

/// Classifier: which mode does this spec use?
///
/// Apply-time validation drops `Invalid` before the spec reaches
/// runtime, so the runtime only sees `Inline` / `Hashed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// `:body` is set; expand it directly.
    Inline,
    /// `:hash` is set; expand by looking up the hash in the active
    /// content-store.
    Hashed,
    /// Neither set, or both set. Ill-formed spec.
    Invalid,
}

impl SnippetSpec {
    /// Which expansion mode this spec represents.
    #[must_use]
    pub fn resolution(&self) -> Resolution {
        match (self.body.is_empty(), self.hash.is_empty()) {
            (false, true) => Resolution::Inline,
            (true, false) => Resolution::Hashed,
            _ => Resolution::Invalid,
        }
    }

    /// True when `:hash` looks like a valid BLAKE3-128 hex token
    /// (32 lowercase hex chars). Validation helper the apply layer
    /// calls so `:hash "typo"` fails at parse time, not at expand
    /// time.
    #[must_use]
    pub fn has_valid_hash_format(&self) -> bool {
        crate::hash::is_blake3_128_hex(&self.hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_classifies_body_and_hash_modes() {
        let inline = SnippetSpec {
            trigger: "fn".into(),
            body: "fn ${1}() { ${0} }".into(),
            ..Default::default()
        };
        assert_eq!(inline.resolution(), Resolution::Inline);

        let hashed = SnippetSpec {
            trigger: "deploy".into(),
            hash: "af42c0d18e9b3f4aa18b7c3ef1de93a4".into(),
            ..Default::default()
        };
        assert_eq!(hashed.resolution(), Resolution::Hashed);

        let neither = SnippetSpec {
            trigger: "oops".into(),
            ..Default::default()
        };
        assert_eq!(neither.resolution(), Resolution::Invalid);

        let both = SnippetSpec {
            trigger: "both".into(),
            body: "x".into(),
            hash: "af42c0d18e9b3f4aa18b7c3ef1de93a4".into(),
            ..Default::default()
        };
        assert_eq!(both.resolution(), Resolution::Invalid);
    }

    #[test]
    fn hash_format_validates_blake3_128_hex() {
        let ok = SnippetSpec {
            trigger: "x".into(),
            hash: "af42c0d18e9b3f4aa18b7c3ef1de93a4".into(),
            ..Default::default()
        };
        assert!(ok.has_valid_hash_format());

        let too_short = SnippetSpec {
            trigger: "x".into(),
            hash: "af42".into(),
            ..Default::default()
        };
        assert!(!too_short.has_valid_hash_format());

        let has_uppercase = SnippetSpec {
            trigger: "x".into(),
            hash: "AF42c0d18e9b3f4aa18b7c3ef1de93a4".into(),
            ..Default::default()
        };
        assert!(!has_uppercase.has_valid_hash_format());

        let has_garbage = SnippetSpec {
            trigger: "x".into(),
            hash: "af42c0d18e9b3f4aa18b7c3ef1de93_!".into(),
            ..Default::default()
        };
        assert!(!has_garbage.has_valid_hash_format());

        let empty = SnippetSpec {
            trigger: "x".into(),
            ..Default::default()
        };
        // Empty is falsy — intentional; used alongside `resolution()`
        // to distinguish "no hash given" from "hash given but malformed".
        assert!(!empty.has_valid_hash_format());
    }
}
