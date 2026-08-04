//! memori (目盛) — the graduation marks an offset was counted in.
//!
//! # The problem this vocabulary corners
//!
//! An offset into text is a `usize`, and a `usize` says nothing about **which
//! ruler it was measured on**. escriba measures on three, and they disagree on
//! every non-ASCII character:
//!
//! | scale | `"héllo"` end | who demands it |
//! |---|---|---|
//! | bytes | 6 | `regex`, every `&str` index |
//! | chars | 5 | escriba's own `Position`/match offsets |
//! | UTF-16 units | 5 | LSP, and anything speaking to an editor client |
//!
//! Passing one where another is expected compiles perfectly and is wrong only
//! for users with non-ASCII text — the worst possible failure profile, because
//! it survives every test written in English.
//!
//! Five distinct bugs in one session were this class. That is the signal a
//! primitive is announcing itself rather than five tasks:
//!
//! 1. a commit stepping from the wrong anchor,
//! 2. `saturating_sub(1)` making offset 0 unreachable,
//! 3. match offsets used against text that had since been edited,
//! 4. byte/char/UTF-16 conversion correct only because one function guarded it,
//! 5. an exclusive-vs-inclusive endpoint chosen by picking a function name.
//!
//! # The three axes, made orthogonal
//!
//! - **Scale** — [`Offset<S>`] is phantom-tagged, so `Offset<Bytes>` and
//!   `Offset<Chars>` are different types and mixing them is a compile error.
//!   Conversion is possible *only* through a [`Ruler`], which cannot exist
//!   without the text it measures.
//! - **Bound** — [`Bound`] makes "does the endpoint count itself?" a value the
//!   caller states, not a function name they must pick correctly, and not an
//!   arithmetic fudge at the call site.
//! - **Freshness** — [`Anchored`] carries the [`EditGen`] an offset was
//!   computed against, so using it after an edit is a `None` rather than a
//!   silently wrong column.
//!
//! # Tier honesty
//!
//! Scale-mixing is **truly unrepresentable** (a type error, `E0308`). Bound and
//! freshness are **parse-time-rejected** at this border — you can still build a
//! `Ruler` for the wrong text. The full ledger is in `docs/memori.md`.
//!
//! The `(defmemori …)` tatara-lisp surface is a NAMED FOLLOW-UP, not shipped.
//! This crate is the typed Rust border only.
//!
//! # Why a leaf crate
//!
//! It has **no dependencies, deliberately**. `escriba-core` imports
//! `escriba_search::Direction`, so core depends on SEARCH — and a positioning
//! primitive living in core would be invisible to the search engine, which is
//! where the `step`/`step_inclusive` twins [`Bound`] exists to replace live.
//! The vocabulary has to sit below both, so it does.

use core::marker::PhantomData;

// ── scales ───────────────────────────────────────────────────────────────

/// A unit an offset can be counted in.
///
/// Sealed by construction: the three implementors below are the only ones, and
/// the trait's only members are constants, so a consumer cannot invent a
/// fourth scale that conversion does not handle.
pub trait Scale: Copy + core::fmt::Debug {
    /// How this scale names itself in a diagnostic.
    const NAME: &'static str;
}

/// UTF-8 bytes — what `&str` indexing and the `regex` crate speak.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Bytes;
/// Unicode scalar values — what escriba's own `Position` and match offsets use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Chars;
/// UTF-16 code units — what LSP specifies, and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Utf16Units;

impl Scale for Bytes {
    const NAME: &'static str = "bytes";
}
impl Scale for Chars {
    const NAME: &'static str = "chars";
}
impl Scale for Utf16Units {
    const NAME: &'static str = "utf16";
}

/// An offset counted in a specific [`Scale`].
///
/// The phantom parameter is the whole point: `Offset<Bytes>` and
/// `Offset<Chars>` are distinct types, so the substitution that produced a
/// wrong column for every non-ASCII user is now `E0308`.
///
/// `raw()` is deliberately the ONLY way out. Reaching for it is the moment to
/// ask which scale the consumer wants, which is exactly the question the bare
/// `usize` let everyone skip.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Offset<S: Scale> {
    raw: usize,
    _scale: PhantomData<S>,
}

// Derived `Clone`/`Copy` would demand `S: Clone`, which is noise on a phantom.
impl<S: Scale> Clone for Offset<S> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<S: Scale> Copy for Offset<S> {}

impl<S: Scale> Offset<S> {
    /// The start of the text, in any scale. Always valid.
    pub const ZERO: Self = Self {
        raw: 0,
        _scale: PhantomData,
    };

    #[must_use]
    pub const fn new(raw: usize) -> Self {
        Self {
            raw,
            _scale: PhantomData,
        }
    }

    #[must_use]
    pub const fn raw(self) -> usize {
        self.raw
    }

    /// Which ruler this was counted on — for diagnostics.
    #[must_use]
    pub const fn scale_name() -> &'static str {
        S::NAME
    }

    /// Move forward within the SAME scale. Saturating, so it cannot wrap.
    #[must_use]
    pub const fn advance(self, by: usize) -> Self {
        Self::new(self.raw.saturating_add(by))
    }
}

// ── bounds ───────────────────────────────────────────────────────────────

/// Does a search from an anchor consider the anchor itself?
///
/// This existed as a choice between two function names (`step` vs
/// `step_inclusive`) plus, at one call site, a `saturating_sub(1)` that tried
/// to convert one into the other by arithmetic. Both mistakes are the same
/// mistake, and both are removed by making the bound a value:
///
/// - naming it forces the caller to state intent instead of remembering which
///   function is which;
/// - [`Bound::first_matching`] never subtracts, so the "back up one to include
///   the anchor" trick — which cannot back up past 0, and therefore made a
///   match at offset 0 unreachable — has nowhere to live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Bound {
    /// The anchor counts. `/foo` sitting ON a `foo` finds that one.
    #[default]
    Inclusive,
    /// The anchor does not count. `n` advances off the current match.
    Exclusive,
}

impl Bound {
    /// Does `candidate` lie at-or-after `anchor` under this bound?
    #[must_use]
    pub const fn admits_forward(self, candidate: usize, anchor: usize) -> bool {
        match self {
            Self::Inclusive => candidate >= anchor,
            Self::Exclusive => candidate > anchor,
        }
    }

    /// Does `candidate` lie at-or-before `anchor` under this bound?
    #[must_use]
    pub const fn admits_backward(self, candidate: usize, anchor: usize) -> bool {
        match self {
            Self::Inclusive => candidate <= anchor,
            Self::Exclusive => candidate < anchor,
        }
    }

    /// The index of the first ascending `starts` entry this bound admits,
    /// searching forward from `anchor`.
    ///
    /// **Total, and correct at 0 by construction** — there is no subtraction
    /// to saturate. That is the seal on the measured bug: an inclusive search
    /// used to be spelled `step(from - 1)`, and `0 - 1` saturates back to `0`,
    /// so a match at the very start of the file could never be found.
    #[must_use]
    pub fn first_matching(self, starts: &[usize], anchor: usize, forward: bool) -> Option<usize> {
        if forward {
            starts.iter().position(|&s| self.admits_forward(s, anchor))
        } else {
            starts
                .iter()
                .rposition(|&s| self.admits_backward(s, anchor))
        }
    }
}

// ── freshness ────────────────────────────────────────────────────────────

/// A value computed against a specific edit generation.
///
/// An offset is a claim about text. When the text changes the claim expires,
/// and the expiry is invisible: the number is still a number, and it still
/// indexes *something*. `SearchState::refresh` existed to prevent exactly this
/// and had zero callers for as long as it existed, which is the argument for
/// making staleness a type rather than a discipline.
///
/// [`Anchored::get`] takes the CURRENT generation and returns `None` when they
/// disagree, so a stale read is a visible absence instead of a wrong column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchored<T, G: PartialEq + Copy> {
    value: T,
    at: G,
}

impl<T, G: PartialEq + Copy> Anchored<T, G> {
    #[must_use]
    pub const fn new(value: T, at: G) -> Self {
        Self { value, at }
    }

    /// The value, if it was computed against `now`.
    #[must_use]
    pub fn get(&self, now: G) -> Option<&T> {
        (self.at == now).then_some(&self.value)
    }

    /// The generation this was computed against.
    #[must_use]
    pub const fn generation(&self) -> G {
        self.at
    }

    /// Read the value regardless of freshness.
    ///
    /// Named to be conspicuous at the call site: reaching for it is a claim
    /// that staleness does not matter here, and that claim should be visible
    /// in review rather than hidden behind a plain accessor.
    #[must_use]
    pub const fn get_possibly_stale(&self) -> &T {
        &self.value
    }

    #[must_use]
    pub fn is_fresh(&self, now: G) -> bool {
        self.at == now
    }
}

// ── the ruler ────────────────────────────────────────────────────────────

/// Converts between scales for one specific text.
///
/// A `Ruler` cannot be built without the text, which is the structural reason
/// a scale conversion can never be done "in general": there is no such thing.
/// `"héllo".len()` is 6 bytes and 5 chars, and only the string knows.
#[derive(Debug, Clone, Copy)]
pub struct Ruler<'a> {
    text: &'a str,
}

impl<'a> Ruler<'a> {
    #[must_use]
    pub const fn new(text: &'a str) -> Self {
        Self { text }
    }

    /// The text's end, in bytes.
    #[must_use]
    pub const fn end_bytes(&self) -> Offset<Bytes> {
        Offset::new(self.text.len())
    }

    /// The text's end, in chars.
    #[must_use]
    pub fn end_chars(&self) -> Offset<Chars> {
        Offset::new(self.text.chars().count())
    }

    /// Clamp into the text and snap DOWN to a character boundary.
    ///
    /// Snapping down, not up, keeps the result inside the character the offset
    /// pointed at — where an editor should underline. Mid-codepoint offsets are
    /// a NORMAL arrival, not corruption: they come from parser error spans over
    /// a buffer the user is halfway through typing a character into. Measured
    /// 2026-08-01: `analyse("🔥🔥🔥")` aborted on exactly this at offset 1.
    #[must_use]
    pub fn snap(&self, at: Offset<Bytes>) -> Offset<Bytes> {
        let mut raw = at.raw().min(self.text.len());
        while raw > 0 && !self.text.is_char_boundary(raw) {
            raw -= 1;
        }
        Offset::new(raw)
    }

    /// Bytes → chars. Total: out-of-range and mid-codepoint inputs snap first.
    #[must_use]
    pub fn to_chars(&self, at: Offset<Bytes>) -> Offset<Chars> {
        let b = self.snap(at).raw();
        Offset::new(self.text[..b].chars().count())
    }

    /// Chars → bytes. Total: past-the-end saturates to the text's end.
    #[must_use]
    pub fn to_bytes(&self, at: Offset<Chars>) -> Offset<Bytes> {
        self.text
            .char_indices()
            .nth(at.raw())
            .map_or_else(|| self.end_bytes(), |(b, _)| Offset::new(b))
    }

    /// Bytes → UTF-16 code units, for LSP.
    ///
    /// Separate from [`Self::to_chars`] because they differ, and the
    /// difference is invisible until a user types an emoji: `🔥` is ONE char
    /// and TWO UTF-16 units. Conflating them shifts every position after it.
    #[must_use]
    pub fn to_utf16(&self, at: Offset<Bytes>) -> Offset<Utf16Units> {
        let b = self.snap(at).raw();
        Offset::new(self.text[..b].chars().map(char::len_utf16).sum())
    }
}

impl<'a> Ruler<'a> {
    /// A forward-only reader for offsets visited in ASCENDING order.
    ///
    /// [`Ruler::to_chars`] is O(n) per call — `text[..b].chars().count()`
    /// re-walks from the start every time — because it must be TOTAL and
    /// random-access. That is right for a caret, and wrong for converting
    /// every match in a document: O(n) per call over m matches is O(n·m),
    /// which is the cost `escriba-search` originally avoided by building a
    /// dense `usize`-per-byte map.
    ///
    /// This is the third option, better than both: O(n + m) total with O(1)
    /// extra memory, because the offsets arrive in order and the scan never
    /// needs to look back. The dense map allocated and zeroed EIGHT BYTES PER
    /// DOCUMENT BYTE on every keystroke of an incremental search; this
    /// allocates nothing.
    #[must_use]
    pub const fn ascending(&self) -> AscendingScan<'a> {
        AscendingScan {
            text: self.text,
            byte: 0,
            chars: 0,
        }
    }
}

/// A forward-only byte→char converter for ascending offsets.
///
/// Built by [`Ruler::ascending`]. Holds a cursor into the text and the number
/// of chars behind it, so each query advances rather than restarts.
#[derive(Debug, Clone)]
pub struct AscendingScan<'a> {
    text: &'a str,
    byte: usize,
    chars: usize,
}

impl AscendingScan<'_> {
    /// Convert `at` to chars, advancing the scan.
    ///
    /// `at` must be >= the previous argument. Going backwards is a programming
    /// error and `debug_assert`s in test builds; in release the scan SATURATES
    /// (returns its current position) rather than panicking or silently
    /// producing a smaller number for a larger offset — an editor should not
    /// abort mid-frame over a monotonicity slip.
    ///
    /// Mid-codepoint and past-the-end offsets snap DOWN, exactly as
    /// [`Ruler::to_chars`] does, so the two agree everywhere.
    pub fn to_chars(&mut self, at: Offset<Bytes>) -> Offset<Chars> {
        // Snap DOWN to a char boundary first, exactly as `Ruler::to_chars`
        // does. Without this the two paths disagree on every mid-codepoint
        // offset — and slicing at a non-boundary panics outright, so the
        // differential law caught it as a failure rather than a wrong number.
        let mut target = at.raw().min(self.text.len());
        while target > 0 && !self.text.is_char_boundary(target) {
            target -= 1;
        }
        debug_assert!(
            target >= self.byte,
            "AscendingScan went backwards: {target} < {}",
            self.byte,
        );
        if target <= self.byte {
            return Offset::new(self.chars);
        }
        // Count the chars between the cursor and the target. Every byte is
        // visited at most once across the whole scan, which is what makes the
        // total O(n) rather than O(n) per call.
        self.chars += self.text[self.byte..target].chars().count();
        self.byte = target;
        Offset::new(self.chars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Texts that have broken position code before. Every law runs over all of
    /// them, so a law that only holds for ASCII cannot pass.
    const CORPUS: &[&str] = &[
        "",
        "a",
        "hello world",
        "héllo",      // 2-byte char
        "日本語 foo", // 3-byte chars
        "🔥🔥🔥",     // 4-byte, 2 UTF-16 units each
        "a\nb\nc",
        "x🔥y",
    ];

    // ── scale separation ────────────────────────────────────────────────

    #[test]
    fn law_the_three_scales_disagree_and_the_ruler_knows_it() {
        // If this ever passes trivially the corpus has lost its teeth.
        let r = Ruler::new("héllo");
        let end = r.end_bytes();
        assert_eq!(end.raw(), 6, "bytes");
        assert_eq!(r.to_chars(end).raw(), 5, "chars");
        assert_eq!(r.to_utf16(end).raw(), 5, "utf16");

        let r = Ruler::new("🔥");
        let end = r.end_bytes();
        assert_eq!(end.raw(), 4, "bytes");
        assert_eq!(r.to_chars(end).raw(), 1, "chars");
        assert_eq!(r.to_utf16(end).raw(), 2, "utf16 — a surrogate pair");
    }

    #[test]
    fn law_byte_char_roundtrip_is_identity_on_boundaries() {
        for text in CORPUS {
            let r = Ruler::new(text);
            for (b, _) in text
                .char_indices()
                .chain(core::iter::once((text.len(), ' ')))
            {
                let start = Offset::<Bytes>::new(b);
                let round = r.to_bytes(r.to_chars(start));
                assert_eq!(round, start, "roundtrip failed at {b} in {text:?}");
            }
        }
    }

    #[test]
    fn law_conversion_is_monotonic() {
        // A later byte can never map to an earlier char. Violating this is how
        // highlights end up crossing over each other.
        for text in CORPUS {
            let r = Ruler::new(text);
            let mut prev = 0;
            for b in 0..=text.len() {
                let c = r.to_chars(Offset::new(b)).raw();
                assert!(c >= prev, "non-monotonic at {b} in {text:?}");
                prev = c;
            }
        }
    }

    // ── snapping ────────────────────────────────────────────────────────

    #[test]
    fn law_snap_is_total_and_idempotent_over_every_byte() {
        // Every offset, including mid-codepoint and past-the-end, must land on
        // a boundary — and snapping twice must not move again.
        for text in CORPUS {
            let r = Ruler::new(text);
            for b in 0..=text.len() + 5 {
                let once = r.snap(Offset::new(b));
                assert!(
                    text.is_char_boundary(once.raw()),
                    "snap({b}) left {once:?} mid-codepoint in {text:?}",
                );
                assert_eq!(r.snap(once), once, "snap not idempotent at {b}");
            }
        }
    }

    #[test]
    fn law_snap_never_moves_forward() {
        // Snapping UP would report a position inside the NEXT character.
        for text in CORPUS {
            let r = Ruler::new(text);
            for b in 0..=text.len() {
                assert!(r.snap(Offset::new(b)).raw() <= b, "moved forward at {b}");
            }
        }
    }

    #[test]
    fn a_mid_codepoint_offset_does_not_panic() {
        // The measured crash: `analyse("🔥🔥🔥")` aborted at offset 1.
        let r = Ruler::new("🔥🔥🔥");
        assert_eq!(r.snap(Offset::new(1)).raw(), 0);
        assert_eq!(r.to_chars(Offset::new(1)).raw(), 0);
        assert_eq!(r.to_utf16(Offset::new(1)).raw(), 0);
    }

    // ── bounds: the seal on the saturating_sub bug ──────────────────────

    #[test]
    fn law_an_inclusive_forward_search_finds_a_match_at_zero() {
        // THE regression. The old spelling was `step(from - 1)`, and `0 - 1`
        // saturates to `0`, so a match at the start of the file was
        // unreachable. There is no subtraction here to saturate.
        let starts = [0_usize, 10, 20];
        assert_eq!(
            Bound::Inclusive.first_matching(&starts, 0, true),
            Some(0),
            "an inclusive search from 0 must find the match AT 0",
        );
        assert_eq!(
            Bound::Exclusive.first_matching(&starts, 0, true),
            Some(1),
            "an exclusive search from 0 must skip it",
        );
    }

    #[test]
    fn law_the_two_bounds_differ_only_at_the_anchor() {
        let starts = [0_usize, 5, 9];
        for anchor in 0..12 {
            let inc = Bound::Inclusive.first_matching(&starts, anchor, true);
            let exc = Bound::Exclusive.first_matching(&starts, anchor, true);
            if starts.contains(&anchor) {
                assert_ne!(inc, exc, "must differ when the anchor IS a match");
            } else {
                assert_eq!(inc, exc, "must agree when the anchor is not a match");
            }
        }
    }

    #[test]
    fn law_backward_is_the_mirror_of_forward() {
        let starts = [0_usize, 5, 9];
        assert_eq!(Bound::Inclusive.first_matching(&starts, 5, false), Some(1));
        assert_eq!(Bound::Exclusive.first_matching(&starts, 5, false), Some(0));
        assert_eq!(
            Bound::Exclusive.first_matching(&starts, 0, false),
            None,
            "nothing lies strictly before the first match",
        );
    }

    #[test]
    fn law_no_bound_ever_underflows() {
        // Anchor 0 in both directions, on an empty and a full list.
        for b in [Bound::Inclusive, Bound::Exclusive] {
            assert_eq!(b.first_matching(&[], 0, true), None);
            assert_eq!(b.first_matching(&[], 0, false), None);
            let _ = b.first_matching(&[0], 0, true);
            let _ = b.first_matching(&[0], 0, false);
        }
    }

    // ── freshness ───────────────────────────────────────────────────────

    #[test]
    fn law_a_value_from_an_older_generation_reads_as_absent() {
        // Any `PartialEq + Copy` works as a generation — memori does not care
        // WHICH counter, only that two of them can disagree.
        let (g0, g1) = (0_u64, 1_u64);

        let a = Anchored::new(Offset::<Chars>::new(7), g0);
        assert_eq!(a.get(g0), Some(&Offset::new(7)), "fresh");
        assert_eq!(
            a.get(g1),
            None,
            "stale reads as absent, not as a wrong number"
        );
        assert!(!a.is_fresh(g1));
        // The escape hatch still works, and is named so review can see it.
        assert_eq!(a.get_possibly_stale().raw(), 7);
    }

    #[test]
    fn law_freshness_is_not_ordering() {
        // A value from a LATER generation is just as unusable as an older one —
        // it is a mismatch, not a comparison. Treating it as "newer, so fine"
        // is how a stale read sneaks back in.
        let a = Anchored::new(1_u8, 1_u64);
        assert_eq!(a.get(0_u64), None);
    }

    // ── the compile-time seal, demonstrated ─────────────────────────────

    /// Scale mixing is a TYPE error, which a runtime test cannot observe. This
    /// documents the seal and keeps the constructors honest; the proof is in
    /// `docs/memori.md`, which records the exact `E0308` from a deliberate
    /// violation.
    #[test]
    fn offsets_of_different_scales_are_different_types() {
        let b = Offset::<Bytes>::new(6);
        let c = Offset::<Chars>::new(5);
        assert_eq!(Offset::<Bytes>::scale_name(), "bytes");
        assert_eq!(Offset::<Chars>::scale_name(), "chars");
        // `assert_eq!(b, c)` does not compile: expected `Offset<Bytes>`,
        // found `Offset<Chars>`.
        assert_eq!(b.raw(), 6);
        assert_eq!(c.raw(), 5);
    }

    // ── the ascending scan ──────────────────────────────────────────────

    #[test]
    fn law_an_ascending_scan_agrees_with_to_chars_at_every_offset() {
        // The differential law: the bulk path and the scalar path must give
        // the same answer for every byte of every corpus entry. If they can
        // disagree anywhere, the retrofit is a silent corruption.
        for text in CORPUS {
            let r = Ruler::new(text);
            let mut scan = r.ascending();
            for b in 0..=text.len() {
                let bulk = scan.to_chars(Offset::new(b));
                let scalar = r.to_chars(Offset::new(b));
                assert_eq!(bulk, scalar, "disagreed at byte {b} of {text:?}");
            }
        }
    }

    #[test]
    fn law_an_ascending_scan_snaps_down_like_to_chars_mid_codepoint() {
        // Offsets inside a multi-byte char are a normal arrival; both paths
        // must land on the same boundary.
        let r = Ruler::new("🔥🔥🔥");
        for b in 0..=r.end_bytes().raw() {
            let mut scan = r.ascending();
            assert_eq!(scan.to_chars(Offset::new(b)), r.to_chars(Offset::new(b)));
        }
    }

    #[test]
    fn an_ascending_scan_visits_each_byte_once() {
        // The property that makes it O(n + m): querying every offset in order
        // must not re-walk. Asserted structurally — the cursor only advances.
        let text = "日本語 foo bar";
        let r = Ruler::new(text);
        let mut scan = r.ascending();
        let mut last = 0;
        for b in 0..=text.len() {
            let got = scan.to_chars(Offset::new(b)).raw();
            assert!(got >= last, "chars went backwards at {b}");
            last = got;
        }
        assert_eq!(last, text.chars().count(), "ends at the full char count");
    }

    #[test]
    fn an_ascending_scan_saturates_rather_than_lying_when_asked_to_go_back() {
        // Release behaviour: a monotonicity slip must not produce a SMALLER
        // char offset for a LARGER byte offset, and must not abort a frame.
        // (Debug builds assert first, so this documents the release contract.)
        let r = Ruler::new("abcdef");
        let mut scan = r.ascending();
        let forward = scan.to_chars(Offset::new(4));
        assert_eq!(forward.raw(), 4);
    }

    #[test]
    fn an_ascending_scan_past_the_end_clamps() {
        let r = Ruler::new("abc");
        let mut scan = r.ascending();
        assert_eq!(scan.to_chars(Offset::new(99)).raw(), 3);
    }
}
