//! Capabilities — what a handler is allowed to *look at*.
//!
//! ## The problem M1 left open
//!
//! After M1 a handler cannot MUTATE the editor. It can still READ all of it.
//! `:noh` gets handed the buffer set; a formatter gets handed the search
//! state. That is not a corruption risk, but it is an ambient-authority one:
//! nothing declares what a command depends on, so nothing can tell you what
//! breaks when a view changes, and every handler silently becomes a consumer
//! of everything.
//!
//! ## The shape
//!
//! A handler names the views it needs. The type system hands it exactly
//! those, and reaching for one it did not name **does not compile**:
//!
//! ```compile_fail
//! use escriba_madoguchi::{caps, cap::*, Outcome, View};
//!
//! // This handler asked for Search and nothing else.
//! fn peek(v: &View<'_, caps!(Search)>, _: &[String]) -> Outcome {
//!     let _ = v.search();   // fine — declared
//!     let _ = v.buffers();  // NOT declared: this line does not compile
//!     Outcome::nothing()
//! }
//! ```
//!
//! ## How it works
//!
//! A type-level list (`Cons`/`Nil`) plus a membership proof (`Has`). The
//! proof carries an INDEX (`Here`/`There`) purely so the two impls below do
//! not overlap: without it, `Cons<C, T>` would match both "C is the head" and
//! "C is somewhere in the tail", and coherence would reject the pair. The
//! index is inferred at every call site and never written by hand.
//!
//! This is the standard HList membership encoding. It is worth the ceremony
//! here because the alternative — a runtime `HashSet<Cap>` checked on each
//! access — is *only-mitigated*: it turns an undeclared read into a runtime
//! error some test may or may not reach, whereas this makes it unbuildable.

use std::marker::PhantomData;

// ─── The capabilities themselves ─────────────────────────────────────────

/// Read open buffers: text, paths, line counts, modified flags.
#[derive(Debug, Clone, Copy)]
pub struct Buffers;

/// Read where the operator is: primary cursor position and modal mode.
#[derive(Debug, Clone, Copy)]
pub struct Cursor;

/// Read editor options (`number`, `tabstop`, `mapleader`, …).
#[derive(Debug, Clone, Copy)]
pub struct Options;

/// Read the search session: committed pattern, match count, live prompt.
#[derive(Debug, Clone, Copy)]
pub struct Search;

// ─── Type-level list ─────────────────────────────────────────────────────

/// The empty capability list — a handler that reads nothing.
#[derive(Debug, Clone, Copy)]
pub struct Nil;

/// `H` prepended to `T`.
#[derive(Debug, Clone, Copy)]
pub struct Cons<H, T>(PhantomData<(H, T)>);

/// Proof that `C` appears in this list, at position `I`.
///
/// `I` is an implementation detail of the proof and is always inferred.
pub trait Has<C, I> {}

/// `C` is the head.
#[derive(Debug, Clone, Copy)]
pub struct Here;

/// `C` is further along, at `I` in the tail.
#[derive(Debug, Clone, Copy)]
pub struct There<I>(PhantomData<I>);

impl<C, T> Has<C, Here> for Cons<C, T> {}
impl<C, H, T, I> Has<C, There<I>> for Cons<H, T> where T: Has<C, I> {}

/// Build a capability list: `caps!(Buffers, Cursor)`.
///
/// A handler's capability set is part of its signature, so it is written
/// often and should be cheap to write. `caps!()` is the empty set.
#[macro_export]
macro_rules! caps {
    () => { $crate::cap::Nil };
    ($head:ty $(,)?) => { $crate::cap::Cons<$head, $crate::cap::Nil> };
    ($head:ty, $($tail:ty),+ $(,)?) => {
        $crate::cap::Cons<$head, $crate::caps!($($tail),+)>
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time only: if these bounds do not hold, this file does not
    /// build. There is nothing to assert at runtime — the proof IS the test.
    fn requires<L, C, I>()
    where
        L: Has<C, I>,
    {
    }

    #[test]
    fn membership_is_provable_at_every_position() {
        requires::<caps!(Buffers), Buffers, _>();
        requires::<caps!(Buffers, Cursor), Buffers, _>();
        requires::<caps!(Buffers, Cursor), Cursor, _>();
        requires::<caps!(Buffers, Cursor, Search), Search, _>();
        // Order does not matter to the proof, only membership.
        requires::<caps!(Search, Buffers), Buffers, _>();
    }

    /// A DUPLICATE capability makes the list unusable, and that is the
    /// correct outcome — `caps!(Buffers, Buffers)` is a typo, not an intent.
    ///
    /// It is rejected because both `Here` and `There<Here>` prove membership
    /// and inference cannot choose. Worth documenting because the DIAGNOSTIC
    /// is poor: rustc says "type annotations needed ... multiple impls
    /// satisfying", which points at the use site rather than at the
    /// duplicated declaration. Honest tier: rejected at compile time, with a
    /// message that will cost someone ten minutes if this note is not here.
    ///
    /// ```compile_fail
    /// use escriba_madoguchi::{caps, cap::*};
    /// fn requires<L, C, I>() where L: Has<C, I> {}
    /// requires::<caps!(Buffers, Buffers), Buffers, _>();
    /// ```
    #[test]
    fn a_duplicate_capability_is_rejected() {
        // The proof of rejection is the doc test above; this exists so the
        // rejection is discoverable from the test list, not only from prose.
        requires::<caps!(Buffers, Cursor), Buffers, _>();
    }
}
