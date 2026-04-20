//! Editor-mode vocabulary — the single source of truth.
//!
//! Every mode-scoped def-form (`defkeybind :mode`, `defkmacro :mode`,
//! and eventually `defmode` dispatch) validates against
//! [`KNOWN_MODES`]. Previously the vocabulary lived in three places
//! (a match arm in `apply_source`, a parallel `const` in
//! `kmacro.rs`, and a hardcoded string in `LispError::UnknownMode`);
//! adding a new mode meant three synchronized edits with a silent
//! drift risk. This module collapses it to one.
//!
//! Modes are a closed vocabulary in escriba — new ones appear only
//! when the modal state machine in `escriba-mode` grows a new
//! variant, so a data-driven `const &[&str]` is fine (no escape
//! hatch for user-defined modes).

/// The canonical mode vocabulary. Order is the same order the
/// picker + error messages render them, so keep it user-facing-
/// stable: most-common-first.
pub const KNOWN_MODES: &[&str] = &[
    "normal",
    "insert",
    "visual",
    "visual-line",
    "command",
];

/// True when `mode` is a recognized modal-state name.
#[must_use]
pub fn is_known_mode(mode: &str) -> bool {
    KNOWN_MODES.contains(&mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_modes_all_resolve() {
        for m in KNOWN_MODES {
            assert!(is_known_mode(m));
        }
    }

    #[test]
    fn unknown_mode_falls_through() {
        assert!(!is_known_mode("superman"));
        assert!(!is_known_mode(""));
        assert!(!is_known_mode("NORMAL")); // case-sensitive
    }

    #[test]
    fn canonical_mode_list_is_stable() {
        // Pin the set. Adding / removing modes is a breaking change
        // for every rc in the wild — this test forces a conscious
        // update (rather than silent drift) when the list moves.
        assert_eq!(
            KNOWN_MODES,
            &["normal", "insert", "visual", "visual-line", "command"],
        );
    }
}
