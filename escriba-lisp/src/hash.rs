//! BLAKE3-128 hex — the shared hash-token vocabulary.
//!
//! pleme-io's convergence stack speaks BLAKE3 everywhere: tameshi,
//! sekiban, mado's clipboard store. Inside escriba, the same
//! 32-char lowercase-hex token format is used by three places so
//! far — `defsnippet :hash "…"` for content-addressed snippets,
//! `defattest :counts-hash "…"` for rc integrity pins, and
//! (transitively) any MCP bridge that resolves snippet bodies
//! from mado's store. Collecting the validate/compute helpers
//! here keeps the spelling of "a BLAKE3-128 hex token" in one
//! place — three sites previously held independent copies of the
//! exact same predicate.
//!
//! The hash is a **prefix** of BLAKE3 (128 bits = 16 bytes = 32
//! hex chars), not the full 256-bit output. Collision probability
//! for typical session / rc-shape workloads is ~2^-64; keeping
//! tokens short makes MCP payloads + Lisp rcs legible.

/// Compute the canonical BLAKE3-128 hex of `bytes` — 32 lowercase
/// hex chars. The token format shared across the stack:
/// `defsnippet :hash`, `defattest :counts-hash`, mado's
/// `ClipboardHash::to_hex`.
#[must_use]
pub fn compute_blake3_128_hex(bytes: &[u8]) -> String {
    let full = blake3::hash(bytes);
    let short = &full.as_bytes()[..16];
    let mut out = String::with_capacity(32);
    for byte in short {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// True when `s` is a well-formed BLAKE3-128 hex token — exactly
/// 32 chars, every char is lowercase hex (`0-9` / `a-f`). Uppercase
/// is rejected intentionally so the wire format stays canonical.
///
/// An empty string is **not** valid here — callers that tolerate
/// "unpinned" / "not-yet-set" states (e.g. `defattest` with no
/// `:counts-hash` declared) must check emptiness separately before
/// calling this.
#[must_use]
pub fn is_blake3_128_hex(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_is_stable_and_lowercase_hex() {
        let a = compute_blake3_128_hex(b"hello world");
        let b = compute_blake3_128_hex(b"hello world");
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
        assert!(is_blake3_128_hex(&a));
    }

    #[test]
    fn compute_distinguishes_different_inputs() {
        assert_ne!(compute_blake3_128_hex(b"a"), compute_blake3_128_hex(b"b"));
    }

    #[test]
    fn hex_validator_accepts_canonical_shape() {
        assert!(is_blake3_128_hex("af42c0d18e9b3f4aa18b7c3ef1de93a4"));
        assert!(is_blake3_128_hex("00000000000000000000000000000000"));
        assert!(is_blake3_128_hex("ffffffffffffffffffffffffffffffff"));
    }

    #[test]
    fn hex_validator_rejects_wrong_length() {
        assert!(!is_blake3_128_hex(""));
        assert!(!is_blake3_128_hex("af42"));
        // 33 chars — one too many.
        assert!(!is_blake3_128_hex("af42c0d18e9b3f4aa18b7c3ef1de93a4a"));
    }

    #[test]
    fn hex_validator_rejects_uppercase() {
        // Wire format is canonically lowercase; drift between
        // uppercase + lowercase would produce two different tokens
        // for the same bytes.
        assert!(!is_blake3_128_hex("AF42c0d18e9b3f4aa18b7c3ef1de93a4"));
    }

    #[test]
    fn hex_validator_rejects_non_hex_chars() {
        // One 'g' (not 0-9a-f) anywhere in the string fails.
        assert!(!is_blake3_128_hex("gf42c0d18e9b3f4aa18b7c3ef1de93a4"));
        assert!(!is_blake3_128_hex("af42c0d18e9b3f4aa18b7c3ef1de93a_"));
    }
}
