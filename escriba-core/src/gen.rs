//! [`EditGen`] — the monotonic edit-generation stamp.
//!
//! One per `EditorState`. It bumps on every state mutation (edit, motion,
//! mode change, viewport move) and is the root of escriba's sealed refresh
//! tree (`theory/ESCRIBA.md` §Refresh-Seal): a rendered product is *fresh* iff
//! its stamp equals the current `EditGen`, so a stale frame — one that shows a
//! product older than the state it claims — has no reachable code path once
//! the renderer gates on the stamp. Same monotonic-`u64` technique as mado's
//! frame seqno, promoted to a typed primitive.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A monotonic edit-generation counter. Equality is the freshness test:
/// `product_gen == current_gen` ⇒ the product matches the live state.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Debug,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub struct EditGen(pub u64);

impl EditGen {
    /// The next generation. Wraps (a 64-bit counter at 1 bump/ns lasts ~585
    /// years; wrap is defensive, never reached).
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}
