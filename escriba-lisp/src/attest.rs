//! `defattest` — content-addressed rc integrity attestations.
//!
//! **Invention.** No editor in the category signs its own
//! configuration via content hash. Vim's `:mkvimrc`, emacs's
//! `custom-set-variables`, vscode's `settings.json`, zed's typed
//! config — none of them let you write down "this rc should
//! hash to X; flag me if it drifts". escriba brings pleme-io's
//! convergence-computing doctrine (tameshi / sekiban / kensa) to
//! the editor layer: every `defattest` records an expected hash of
//! the parsed [`ApplyPlan`](crate::ApplyPlan); the runtime compares
//! actual against expected and surfaces drift.
//!
//! ```lisp
//! ;; Pin the exact shape shipped with the 1.2.0 baseline.
//! (defattest :id "v1.2.0-baseline"
//!            :description "rc signed off by the ops team 2026-04-15"
//!            :counts-hash "af42c0d18e9b3f4aa18b7c3ef1de93a4"
//!            :kind "pin"
//!            :severity "error")
//!
//! ;; Softer attestation — flag reductions but tolerate additions.
//! (defattest :id "minimum-core"
//!            :description "we must have at least these 23 def-forms"
//!            :counts-hash "903b11ef41d09e4be9c2b7aea0f65e2f"
//!            :kind "min"
//!            :severity "warn")
//! ```
//!
//! ## Hash shape
//!
//! The `:counts-hash` is the **first 32 hex chars** of the BLAKE3
//! hash of [`ApplyPlan::summary()`] — a stable string like
//! `"keybinds=10 cmds=5 …"`. Two rcs with the same `summary()`
//! string produce the same hash.
//!
//! BLAKE3-128 (16 bytes → 32 hex chars) matches the wire format
//! used by [`SnippetSpec`]'s content-addressed bodies, mado's
//! clipboard store, and the tameshi attestation core. One token
//! shape across the stack.
//!
//! ## Kinds
//!
//! - `pin` — strict equality. Any drift fails.
//! - `min` — the actual plan's counts must **contain** every
//!   (label, count) the expected hash was derived from. Adding a
//!   new def-form is tolerated; removing one is flagged.
//! - `max` — the actual plan's counts must **not exceed** the
//!   expected shape. Adding def-forms is flagged; removing is fine.
//!   Useful for compliance "nothing-new-until-reviewed" baselines.
//!
//! Only `pin` is enforceable from the hash alone (a hash collapses
//! a structured value to 16 bytes). `min` / `max` semantics require
//! the runtime to know the expected counts — for now they fall back
//! to `pin` semantics and record the kind so a future tick can
//! surface the real semantics once we ship the full attestation
//! bundle. Kind is validated as enum today so the rc already
//! expresses intent.
//!
//! ## Why this is a pleme-io primitive
//!
//! The convergence stack already uses content-addressed identity
//! for every deploy gate (tameshi → sekiban → kensa). Extending the
//! discipline to editor rc closes a small loop: the same team that
//! ships a compliance baseline to production can ship a pinned
//! editor config with the same attestation vocabulary. The hash is
//! the API.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defattest")]
pub struct AttestSpec {
    /// Stable attestation id — unique within the plan.
    pub id: String,
    /// One-line description shown in the picker / `escriba doctor`.
    #[serde(default)]
    pub description: String,
    /// 32-char lowercase BLAKE3-128 hex of the expected
    /// [`ApplyPlan::summary()`](crate::ApplyPlan::summary) string.
    /// See [`compute_summary_hash`] for the canonical derivation.
    #[serde(default)]
    pub counts_hash: String,
    /// Attestation mode — `pin` / `min` / `max`. Defaults to
    /// `pin` when empty. Full semantics documented at module level.
    #[serde(default)]
    pub kind: String,
    /// What should happen when the attestation diverges at apply
    /// time — `info` / `warn` / `error`. Defaults to `error`.
    #[serde(default)]
    pub severity: String,
}

/// Canonical attestation kinds. See [module docs](self) for semantics.
pub const KNOWN_KINDS: &[&str] = &["pin", "min", "max"];

/// Canonical severity vocabulary. Mirrors the shared vocabulary that
/// gate + lsp-diagnostic specs use so a drift report can be rendered
/// with the same style everywhere.
pub const KNOWN_SEVERITIES: &[&str] = &["info", "warn", "error"];

/// True when `kind` is a recognized attestation mode.
#[must_use]
pub fn is_known_kind(kind: &str) -> bool {
    KNOWN_KINDS.contains(&kind)
}

/// True when `severity` is a recognized attestation severity.
#[must_use]
pub fn is_known_severity(severity: &str) -> bool {
    KNOWN_SEVERITIES.contains(&severity)
}

/// Compute the canonical BLAKE3-128 hex of a plan-summary string.
/// This is the single authority for what a `:counts-hash` value
/// should be — any tool that pins an expected hash derives it by
/// hashing the same `summary()` through this function.
///
/// Returns 32 lowercase hex chars. Same format as
/// [`SnippetSpec`](crate::SnippetSpec)'s `:hash` + mado's clipboard
/// store — the hash is the shared API across the stack.
#[must_use]
pub fn compute_summary_hash(summary: &str) -> String {
    let full = blake3::hash(summary.as_bytes());
    let short = &full.as_bytes()[..16];
    let mut out = String::with_capacity(32);
    for byte in short {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

impl AttestSpec {
    /// Effective kind — `pin` by default when `:kind` is unset.
    #[must_use]
    pub fn effective_kind(&self) -> &str {
        if self.kind.is_empty() { "pin" } else { self.kind.as_str() }
    }

    /// Effective severity — `error` by default when `:severity` is unset.
    /// Matches the "signed-off rc, drift is a production incident" default.
    #[must_use]
    pub fn effective_severity(&self) -> &str {
        if self.severity.is_empty() { "error" } else { self.severity.as_str() }
    }

    /// Structural check on `:counts-hash` — 32 lowercase hex chars.
    /// Empty is valid (means "no hash pinned", just a stub attestation
    /// the user hasn't filled in yet) and always returns false here;
    /// callers distinguish "empty" from "malformed" via
    /// [`Self::is_empty_hash`].
    #[must_use]
    pub fn has_valid_hash_format(&self) -> bool {
        self.counts_hash.len() == 32
            && self.counts_hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    /// True when `:counts-hash` is empty — the user declared an
    /// attestation but hasn't pinned a value yet. Apply-time
    /// validation tolerates this; runtime attestation evaluation
    /// skips the compare.
    #[must_use]
    pub fn is_empty_hash(&self) -> bool {
        self.counts_hash.is_empty()
    }

    /// Evaluate this attestation against a plan's computed summary
    /// hash. Returns the structured result the runtime reports back
    /// to the user. Unpinned (empty-hash) attestations resolve to
    /// [`AttestResult::Skipped`]; a pin-kind with matching hash is
    /// [`AttestResult::Ok`]; anything else is
    /// [`AttestResult::Drift`].
    #[must_use]
    pub fn evaluate(&self, actual_hash: &str) -> AttestResult {
        if self.is_empty_hash() {
            return AttestResult::Skipped;
        }
        if self.counts_hash == actual_hash {
            AttestResult::Ok
        } else {
            AttestResult::Drift {
                expected: self.counts_hash.clone(),
                actual: actual_hash.to_string(),
            }
        }
    }
}

/// Result of evaluating a [`AttestSpec`] against an actual plan hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttestResult {
    /// Expected and actual hashes match.
    Ok,
    /// Expected and actual hashes diverge. Callers key off
    /// [`AttestSpec::effective_severity`] to decide what to do.
    Drift { expected: String, actual: String },
    /// `:counts-hash` is empty — the spec is a stub / work-in-progress.
    Skipped,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_summary_hash_is_stable_and_lowercase_hex() {
        let a = compute_summary_hash("keybinds=10 cmds=5 options=20");
        let b = compute_summary_hash("keybinds=10 cmds=5 options=20");
        assert_eq!(a, b, "hash must be deterministic");
        assert_eq!(a.len(), 32, "BLAKE3-128 hex is 32 chars");
        assert!(
            a.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "hash must be lowercase hex only",
        );
    }

    #[test]
    fn compute_summary_hash_distinguishes_different_summaries() {
        let a = compute_summary_hash("keybinds=10");
        let b = compute_summary_hash("keybinds=11");
        assert_ne!(a, b);
    }

    #[test]
    fn effective_kind_defaults_to_pin() {
        let s = AttestSpec { id: "x".into(), ..Default::default() };
        assert_eq!(s.effective_kind(), "pin");
        let s = AttestSpec { id: "x".into(), kind: "min".into(), ..Default::default() };
        assert_eq!(s.effective_kind(), "min");
    }

    #[test]
    fn effective_severity_defaults_to_error() {
        let s = AttestSpec { id: "x".into(), ..Default::default() };
        assert_eq!(s.effective_severity(), "error");
        let s = AttestSpec {
            id: "x".into(),
            severity: "warn".into(),
            ..Default::default()
        };
        assert_eq!(s.effective_severity(), "warn");
    }

    #[test]
    fn kind_and_severity_classifiers_match_known_vocab() {
        for k in KNOWN_KINDS {
            assert!(is_known_kind(k));
        }
        assert!(!is_known_kind("strict"));
        for s in KNOWN_SEVERITIES {
            assert!(is_known_severity(s));
        }
        assert!(!is_known_severity("critical"));
    }

    #[test]
    fn hash_format_requires_32_lowercase_hex() {
        // Happy path.
        let s = AttestSpec {
            id: "x".into(),
            counts_hash: "af42c0d18e9b3f4aa18b7c3ef1de93a4".into(),
            ..Default::default()
        };
        assert!(s.has_valid_hash_format());

        // Uppercase rejected — matches the defsnippet :hash rule so
        // the token shape is identical across stack.
        let s = AttestSpec {
            id: "x".into(),
            counts_hash: "AF42c0d18e9b3f4aa18b7c3ef1de93a4".into(),
            ..Default::default()
        };
        assert!(!s.has_valid_hash_format());

        // Wrong length.
        let s = AttestSpec {
            id: "x".into(),
            counts_hash: "af42".into(),
            ..Default::default()
        };
        assert!(!s.has_valid_hash_format());

        // Non-hex chars.
        let s = AttestSpec {
            id: "x".into(),
            counts_hash: "zz42c0d18e9b3f4aa18b7c3ef1de93a4".into(),
            ..Default::default()
        };
        assert!(!s.has_valid_hash_format());

        // Empty — not "valid" but distinguishable via is_empty_hash.
        let s = AttestSpec { id: "x".into(), ..Default::default() };
        assert!(!s.has_valid_hash_format());
        assert!(s.is_empty_hash());
    }

    #[test]
    fn evaluate_resolves_ok_drift_and_skipped() {
        // Ok: hashes agree.
        let s = AttestSpec {
            id: "x".into(),
            counts_hash: compute_summary_hash("keybinds=1"),
            ..Default::default()
        };
        assert_eq!(
            s.evaluate(&compute_summary_hash("keybinds=1")),
            AttestResult::Ok,
        );

        // Drift: hashes disagree.
        match s.evaluate(&compute_summary_hash("keybinds=2")) {
            AttestResult::Drift { expected, actual } => {
                assert_eq!(expected, s.counts_hash);
                assert_ne!(expected, actual);
            }
            other => panic!("expected Drift, got {other:?}"),
        }

        // Skipped: no hash pinned.
        let s = AttestSpec { id: "x".into(), ..Default::default() };
        assert_eq!(s.evaluate("whatever"), AttestResult::Skipped);
    }
}
