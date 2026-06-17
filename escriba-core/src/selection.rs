use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::id::CaretId;
use crate::position::Position;
use crate::range::Range;

/// A single cursor — anchor + head. Visual selection is the range between
/// them. When equal, it's just an insertion point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub struct Cursor {
    pub id: CaretId,
    pub anchor: Position,
    pub head: Position,
}

impl Cursor {
    #[must_use]
    pub const fn new(id: CaretId, anchor: Position, head: Position) -> Self {
        Self { id, anchor, head }
    }

    #[must_use]
    pub fn at(id: CaretId, p: Position) -> Self {
        Self {
            id,
            anchor: p,
            head: p,
        }
    }

    /// The selected range — normalized so start ≤ end.
    #[must_use]
    pub fn range(self) -> Range {
        Range::new(self.anchor, self.head).normalized()
    }

    #[must_use]
    pub const fn is_caret(self) -> bool {
        // Cannot use == on Position in const fn without #![feature(const_trait_impl)],
        // so inline the equality check.
        self.anchor.line == self.head.line && self.anchor.column == self.head.column
    }

    /// Move the head to `p`, leaving the anchor fixed — grows the selection.
    #[must_use]
    pub const fn extend_to(self, p: Position) -> Self {
        Self {
            id: self.id,
            anchor: self.anchor,
            head: p,
        }
    }

    /// Collapse to the head — becomes a pure caret.
    #[must_use]
    pub const fn collapse(self) -> Self {
        Self {
            id: self.id,
            anchor: self.head,
            head: self.head,
        }
    }
}

/// The editor's cursor state — the single typed home for "where the
/// caret(s) are".
///
/// **Phase 1 (today):** holds exactly one primary [`Position`]. The
/// mutation surface (`primary` / `set_primary` / `map_primary`) is the same
/// shape a single bare `Position` had, so callers route through it without
/// behavioral change — but now there is ONE typed place that owns cursor
/// state instead of a loose `Position` field beside an unused multi-caret
/// [`Selection`]. That removes the dual-source-of-truth that would desync
/// the moment multi-cursor lands.
///
/// **Phase 2 (later):** the inner representation grows to
/// `{ primary: Position, secondaries: Vec<Position> }` (or wraps
/// [`Selection`] directly). Because every caller already goes through this
/// newtype's surface, that growth is a localized change here — not a
/// fleet-wide rewrite of `self.cursor.line` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Cursors {
    primary: Position,
}

impl Cursors {
    /// One cursor at `p`.
    #[must_use]
    pub const fn single(p: Position) -> Self {
        Self { primary: p }
    }

    /// The primary caret position — the single read accessor every renderer
    /// and motion path uses.
    #[must_use]
    pub const fn primary(&self) -> Position {
        self.primary
    }

    /// Move the primary caret to `p`. The single write path — `set_cursor`
    /// in the runtime routes here, keeping clamp + viewport-follow as the
    /// only way the cursor changes.
    pub const fn set_primary(&mut self, p: Position) {
        self.primary = p;
    }

    /// Transform the primary caret in place.
    pub fn map_primary(&mut self, f: impl FnOnce(Position) -> Position) {
        self.primary = f(self.primary);
    }

    /// Number of carets — always 1 in phase 1; the seam multi-cursor grows
    /// at.
    #[must_use]
    pub const fn count(&self) -> usize {
        1
    }
}

impl From<Position> for Cursors {
    fn from(p: Position) -> Self {
        Self::single(p)
    }
}

/// A multi-cursor selection — ordered by primary first, then secondaries
/// in insertion order.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Selection {
    carets: Vec<Cursor>,
    primary: usize,
}

impl Selection {
    #[must_use]
    pub fn single(cursor: Cursor) -> Self {
        Self {
            carets: vec![cursor],
            primary: 0,
        }
    }

    #[must_use]
    pub fn carets(&self) -> &[Cursor] {
        &self.carets
    }

    #[must_use]
    pub fn primary(&self) -> &Cursor {
        &self.carets[self.primary]
    }

    pub fn add(&mut self, c: Cursor) {
        self.carets.push(c);
    }

    pub fn map_primary(&mut self, f: impl FnOnce(Cursor) -> Cursor) {
        let idx = self.primary;
        self.carets[idx] = f(self.carets[idx]);
    }

    pub fn map_all(&mut self, mut f: impl FnMut(Cursor) -> Cursor) {
        for c in &mut self.carets {
            *c = f(*c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_is_empty_range() {
        let c = Cursor::at(CaretId(0), Position::new(1, 3));
        assert!(c.is_caret());
        assert!(c.range().is_empty());
    }

    #[test]
    fn extend_grows_but_anchor_stays() {
        let c = Cursor::at(CaretId(0), Position::new(0, 0));
        let grown = c.extend_to(Position::new(0, 5));
        assert_eq!(grown.anchor, Position::new(0, 0));
        assert_eq!(grown.head, Position::new(0, 5));
    }

    #[test]
    fn collapse_makes_caret() {
        let c = Cursor::new(CaretId(0), Position::new(0, 0), Position::new(0, 5));
        let collapsed = c.collapse();
        assert!(collapsed.is_caret());
        assert_eq!(collapsed.head, Position::new(0, 5));
    }

    #[test]
    fn primary_is_first_by_default() {
        let s = Selection::single(Cursor::at(CaretId(0), Position::new(3, 2)));
        assert_eq!(s.primary().head, Position::new(3, 2));
    }

    // ── Cursors newtype (phase-1 single-cursor home) ─────────────────────

    #[test]
    fn cursors_single_round_trips_position() {
        let c = Cursors::single(Position::new(2, 5));
        assert_eq!(c.primary(), Position::new(2, 5));
        assert_eq!(c.count(), 1);
    }

    #[test]
    fn cursors_set_primary_is_the_write_path() {
        let mut c = Cursors::default();
        assert_eq!(c.primary(), Position::ZERO);
        c.set_primary(Position::new(4, 1));
        assert_eq!(c.primary(), Position::new(4, 1));
    }

    #[test]
    fn cursors_map_primary_transforms_in_place() {
        let mut c = Cursors::single(Position::new(1, 1));
        c.map_primary(|p| Position::new(p.line + 1, p.column + 2));
        assert_eq!(c.primary(), Position::new(2, 3));
    }

    #[test]
    fn cursors_from_position() {
        let c: Cursors = Position::new(7, 7).into();
        assert_eq!(c.primary(), Position::new(7, 7));
    }
}
