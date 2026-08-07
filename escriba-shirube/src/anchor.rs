//! Freshness — what a list of findings was computed against.
//!
//! ## Why an axis SET, and not a revision
//!
//! The obvious model is one revision: a list is stale when the buffer moves.
//! That is wrong for most producers, and the plan (§IV.2) caught it before
//! any of them existed:
//!
//! | list | stale when |
//! |---|---|
//! | TODO scan | the buffer moves |
//! | git hunks | the buffer moves **or** the git index moves |
//! | test results | the source moves **or** the binary is rebuilt |
//! | diagnostics | the buffer moves **or** the server restarts |
//!
//! Single-axis-then-retrofit means auditing every producer later and finding
//! the ones that silently kept reporting stale results — a category of bug
//! that shows up as "the gutter is lying" long after the cause.
//!
//! ## Why this is not `memori::Anchored`
//!
//! `Anchored<T, G>` is the fleet's freshness primitive and escriba already
//! uses it for search ordinals. It is deliberately NOT reused here, and the
//! reason is a real difference rather than taste: `Anchored` compares one
//! generation for EQUALITY. This compares a SUBSET — a list anchored on
//! `{buffer, index}` is fresh iff the current world agrees on those two, and
//! says nothing about axes the list never depended on. Forcing that into an
//! equality check would mean every list carried every axis, so rebuilding a
//! project would invalidate a TODO scan.

use escriba_buffer::TextRev;
use escriba_core::BufferId;

/// A version counter for the git index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct IndexRev(pub u64);

/// A version counter for an external session — an LSP server generation, a
/// debug session, a test-runner invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct SessionGen(pub u64);

/// One thing a list can depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Text(BufferId, TextRev),
    Index(IndexRev),
    Session(SessionGen),
}

impl Axis {
    /// Two axes are COMPARABLE when they name the same thing — the same
    /// buffer, the index, a session. Comparability is separate from equality
    /// so freshness can ask "has this axis moved" rather than "is this the
    /// same axis value", which are different questions with the same shape.
    #[must_use]
    pub const fn same_subject(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a, _), Self::Text(b, _)) => a.0 == b.0,
            (Self::Index(_), Self::Index(_)) | (Self::Session(_), Self::Session(_)) => true,
            _ => false,
        }
    }
}

/// What a list was computed against.
///
/// Empty means "depends on nothing", which is always fresh — correct for a
/// static list an operator typed themselves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Anchor {
    axes: Vec<Axis>,
}

impl Anchor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an axis. A second axis with the same subject REPLACES the first —
    /// a list cannot meaningfully depend on two versions of one buffer, and
    /// letting both sit there would make freshness unsatisfiable.
    #[must_use]
    pub fn on(mut self, axis: Axis) -> Self {
        self.axes.retain(|a| !a.same_subject(&axis));
        self.axes.push(axis);
        self
    }

    #[must_use]
    pub fn axes(&self) -> &[Axis] {
        &self.axes
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.axes.is_empty()
    }

    /// Is a list anchored here still fresh, given the world's current axes?
    ///
    /// Fresh iff EVERY axis this list depends on is present in `world` with
    /// the same value. Two failure modes are deliberately both stale:
    ///
    /// - the value moved (the buffer was edited);
    /// - the axis is ABSENT from `world` (the buffer was closed, the session
    ///   ended). An absent axis is not "unchanged" — it is unknowable, and
    ///   treating unknowable as fresh is exactly how a gutter starts lying.
    #[must_use]
    pub fn is_fresh(&self, world: &Self) -> bool {
        self.axes
            .iter()
            .all(|mine| world.axes.iter().any(|theirs| mine == theirs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const B1: BufferId = BufferId(1);
    const B2: BufferId = BufferId(2);

    #[test]
    fn an_empty_anchor_is_always_fresh() {
        // A list an operator typed depends on nothing and must never be
        // invalidated by an edit somewhere else.
        assert!(Anchor::new().is_fresh(&Anchor::new()));
        assert!(Anchor::new().is_fresh(&Anchor::new().on(Axis::Index(IndexRev(7)))));
    }

    #[test]
    fn a_moved_axis_is_stale() {
        let list = Anchor::new().on(Axis::Text(B1, TextRev(1)));
        assert!(list.is_fresh(&Anchor::new().on(Axis::Text(B1, TextRev(1)))));
        assert!(!list.is_fresh(&Anchor::new().on(Axis::Text(B1, TextRev(2)))));
    }

    #[test]
    fn an_absent_axis_is_stale_not_fresh() {
        // The buffer was closed, or the session ended. Unknowable is not
        // unchanged, and treating it as fresh is how a gutter starts lying.
        let list = Anchor::new().on(Axis::Text(B1, TextRev(1)));
        assert!(!list.is_fresh(&Anchor::new()));
        assert!(
            !list.is_fresh(&Anchor::new().on(Axis::Text(B2, TextRev(1)))),
            "a DIFFERENT buffer at the same revision proves nothing",
        );
    }

    #[test]
    fn two_axes_both_have_to_hold() {
        // The case that motivated the whole design: a git hunk set is stale
        // when the buffer moves OR the index moves.
        let hunks = Anchor::new()
            .on(Axis::Text(B1, TextRev(1)))
            .on(Axis::Index(IndexRev(1)));

        let unchanged = Anchor::new()
            .on(Axis::Text(B1, TextRev(1)))
            .on(Axis::Index(IndexRev(1)));
        assert!(hunks.is_fresh(&unchanged));

        let buffer_moved = Anchor::new()
            .on(Axis::Text(B1, TextRev(2)))
            .on(Axis::Index(IndexRev(1)));
        assert!(!hunks.is_fresh(&buffer_moved), "buffer edit invalidates");

        let index_moved = Anchor::new()
            .on(Axis::Text(B1, TextRev(1)))
            .on(Axis::Index(IndexRev(2)));
        assert!(
            !hunks.is_fresh(&index_moved),
            "a `git add` invalidates too — the single-axis model would have \
             kept showing the old hunks",
        );
    }

    #[test]
    fn a_list_ignores_axes_it_never_depended_on() {
        // A rebuild must not invalidate a TODO scan. This is why freshness
        // is a subset check rather than equality of the whole world.
        let todos = Anchor::new().on(Axis::Text(B1, TextRev(1)));
        let world = Anchor::new()
            .on(Axis::Text(B1, TextRev(1)))
            .on(Axis::Session(SessionGen(99)))
            .on(Axis::Index(IndexRev(42)));
        assert!(todos.is_fresh(&world));
    }

    #[test]
    fn one_subject_cannot_be_pinned_twice() {
        // Two revisions of one buffer would make freshness unsatisfiable —
        // the list could never be fresh again.
        let a = Anchor::new()
            .on(Axis::Text(B1, TextRev(1)))
            .on(Axis::Text(B1, TextRev(2)));
        assert_eq!(a.axes().len(), 1, "the later pin replaces the earlier");
        assert!(a.is_fresh(&Anchor::new().on(Axis::Text(B1, TextRev(2)))));
    }
}
