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
}
