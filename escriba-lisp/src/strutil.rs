//! String-field utility helpers shared across spec modules.
//!
//! Several `def*` specs have optional string fields that fall back
//! to a documented default when the user didn't set them. The
//! pattern repeated across five modules (`ruler.rs`, `mark.rs`,
//! `fold.rs`, `attest.rs` — two fields — for a total of five sites)
//! as inline `if self.field.is_empty() { "<default>" } else {
//! self.field.as_str() }` one-liners. Collapse the pattern into a
//! single helper so the fallback semantics has one spelling that's
//! trivially greppable across the crate.

/// Return `value` when non-empty; otherwise `default`. Used by
/// `effective_*` methods on specs that want to report a
/// documented fallback for unset optional fields.
///
/// Lifetimes: both arguments borrow for the same `'a`. Static
/// literal defaults satisfy any lifetime, so call sites can mix
/// `&self.field` with `"literal"` without friction.
#[must_use]
pub fn default_if_empty<'a>(value: &'a str, default: &'a str) -> &'a str {
    if value.is_empty() { default } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_picks_default() {
        assert_eq!(default_if_empty("", "fallback"), "fallback");
    }

    #[test]
    fn non_empty_picks_value() {
        assert_eq!(default_if_empty("real", "fallback"), "real");
    }

    #[test]
    fn single_whitespace_is_not_empty() {
        // Whitespace-only is still a user-set value — we don't
        // trim. Callers that want stricter semantics can trim
        // themselves before calling.
        assert_eq!(default_if_empty(" ", "fallback"), " ");
    }

    #[test]
    fn lifetimes_mix_borrowed_and_static() {
        let owned = String::from("borrowed");
        let result = default_if_empty(&owned, "static-default");
        assert_eq!(result, "borrowed");

        let empty = String::new();
        let result = default_if_empty(&empty, "static-default");
        assert_eq!(result, "static-default");
    }
}
