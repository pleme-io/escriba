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

/// WHICH external session a [`Axis::Session`] axis tracks.
///
/// Sessions used to be one undifferentiated axis, and that was fine while
/// nothing bumped them — `bump_session_gen` shipped with zero call sites. The
/// moment two independent producers anchor on "a session", the single axis
/// aliases them: `same_subject` answered `true` for any two `Session` axes, so
/// closing a file picker would have staled every LSP diagnostic in the gutter.
///
/// Two external generations that can move independently must not share a
/// subject. Adding a third session-like producer means adding a variant here,
/// which is the visible edit that keeps that true.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SessionKind {
    /// A language-server generation — bumped when a server restarts.
    Lsp,
    /// A filesystem-scan generation — bumped when the surface a scan feeds is
    /// opened or closed, which is what makes a superseded scan's rows stale.
    Scan,
}

/// One thing a list can depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Text(BufferId, TextRev),
    Index(IndexRev),
    Session(SessionKind, SessionGen),
}

impl Axis {
    /// Two axes are COMPARABLE when they name the same thing — the same
    /// buffer, the index, **the same kind of** session. Comparability is
    /// separate from equality so freshness can ask "has this axis moved"
    /// rather than "is this the same axis value", which are different
    /// questions with the same shape.
    #[must_use]
    pub const fn same_subject(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a, _), Self::Text(b, _)) => a.0 == b.0,
            (Self::Index(_), Self::Index(_)) => true,
            // NOT a blanket `true` for any two sessions — see [`SessionKind`].
            (Self::Session(a, _), Self::Session(b, _)) => matches!(
                (a, b),
                (SessionKind::Lsp, SessionKind::Lsp) | (SessionKind::Scan, SessionKind::Scan)
            ),
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

/// An [`Anchor`] that provably depends on at least one axis.
///
/// # Why a whole type for "not empty"
///
/// An empty `Anchor` is **vacuously fresh forever** — `is_fresh` is an
/// all-over-my-axes fold, so with no axes it answers `true` against every
/// world. That is the correct reading for a list an operator typed by hand,
/// and it is a silent catastrophe for a result computed off-thread: a reply
/// carrying an empty anchor passes the freshness gate unconditionally, no
/// matter how far the world moved while it was being computed.
///
/// Worse than passing, it gets *upgraded*. The gate is only the door; the
/// consumer that stores the result re-seals it against the world it observes
/// on arrival, so an anchor that earned nothing becomes a durable claim to
/// have been computed against the current state.
///
/// `Anchor` derives `Default` and cannot stop being constructible empty
/// without breaking every honest caller. So the constraint lives in a wrapper
/// whose only constructor takes an axis, and which deliberately does **not**
/// implement `Default`. A courier reply anchored on nothing has no way to be
/// spelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonEmptyAnchor(Anchor);

impl NonEmptyAnchor {
    /// The only constructor. Takes an axis by value, so there is no empty case.
    #[must_use]
    pub fn on(axis: Axis) -> Self {
        Self(Anchor::new().on(axis))
    }

    /// Add another axis. Still non-empty, by induction from [`Self::on`].
    #[must_use]
    pub fn and(self, axis: Axis) -> Self {
        Self(self.0.on(axis))
    }

    /// Widen back to a plain `Anchor` for comparison and storage.
    ///
    /// One-way on purpose: there is no `From<Anchor>`, because that would be
    /// precisely the hole this type exists to close.
    #[must_use]
    pub fn into_anchor(self) -> Anchor {
        self.0
    }

    /// Borrow as a plain `Anchor`, for a freshness check that does not consume.
    #[must_use]
    pub const fn as_anchor(&self) -> &Anchor {
        &self.0
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
            .on(Axis::Session(SessionKind::Lsp, SessionGen(99)))
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

    /// **Why `SessionKind` exists.** Two independent external generations must
    /// not alias, or bumping one silently invalidates the other's lists. Before
    /// the split, `same_subject` answered `true` for any two `Session` axes, so
    /// closing a file picker would have staled every LSP diagnostic.
    #[test]
    fn two_session_kinds_are_different_subjects() {
        let lsp = Axis::Session(SessionKind::Lsp, SessionGen(1));
        let scan = Axis::Session(SessionKind::Scan, SessionGen(1));
        assert!(!lsp.same_subject(&scan), "Lsp and Scan must not alias");
        assert!(lsp.same_subject(&Axis::Session(SessionKind::Lsp, SessionGen(9))));

        // The consequence that matters: an Anchor pinning both keeps BOTH,
        // because `on` only replaces a same-subject axis.
        let a = Anchor::new().on(lsp).on(scan);
        assert_eq!(a.axes().len(), 2, "one kind must not evict the other");
    }

    /// The collateral this prevents, spelled out end to end: a diagnostic
    /// sealed on the LSP session survives a scan-session bump.
    #[test]
    fn bumping_the_scan_session_does_not_stale_an_lsp_list() {
        let diagnostic = Anchor::new().on(Axis::Session(SessionKind::Lsp, SessionGen(3)));
        let after_scan_bump = Anchor::new()
            .on(Axis::Session(SessionKind::Lsp, SessionGen(3)))
            .on(Axis::Session(SessionKind::Scan, SessionGen(4)));
        assert!(diagnostic.is_fresh(&after_scan_bump));

        // …and the converse still bites, which is the point of anchoring at all.
        let after_lsp_restart = Anchor::new().on(Axis::Session(SessionKind::Lsp, SessionGen(4)));
        assert!(!diagnostic.is_fresh(&after_lsp_restart));
    }

    /// **The F3 hole this type closes.** An empty `Anchor` is fresh against
    /// every world, so a reply carrying one is never rejected however far the
    /// world moved. `NonEmptyAnchor` has no constructor that produces it.
    #[test]
    fn an_empty_anchor_passes_every_world_which_is_why_nonempty_exists() {
        let empty = Anchor::new();
        for world in [
            Anchor::new(),
            Anchor::new().on(Axis::Text(B1, TextRev(99))),
            Anchor::new().on(Axis::Session(SessionKind::Scan, SessionGen(1000))),
        ] {
            assert!(empty.is_fresh(&world), "an empty anchor is vacuously fresh");
        }

        let sealed = NonEmptyAnchor::on(Axis::Session(SessionKind::Scan, SessionGen(1)));
        assert!(!sealed.as_anchor().is_empty(), "cannot be built empty");
        assert!(
            !sealed
                .as_anchor()
                .is_fresh(&Anchor::new().on(Axis::Session(SessionKind::Scan, SessionGen(2))))
        );
    }

    #[test]
    fn a_nonempty_anchor_accumulates_and_widens_back() {
        let a = NonEmptyAnchor::on(Axis::Text(B1, TextRev(1)))
            .and(Axis::Session(SessionKind::Lsp, SessionGen(2)));
        let widened = a.into_anchor();
        assert_eq!(widened.axes().len(), 2);
        assert!(
            widened.is_fresh(
                &Anchor::new()
                    .on(Axis::Text(B1, TextRev(1)))
                    .on(Axis::Session(SessionKind::Lsp, SessionGen(2)))
            )
        );
    }
}
