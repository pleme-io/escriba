use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::position::Position;

/// Half-open `[start, end)` text range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub const EMPTY: Self = Self {
        start: Position::ZERO,
        end: Position::ZERO,
    };

    #[must_use]
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub fn point(p: Position) -> Self {
        Self { start: p, end: p }
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    #[must_use]
    pub fn contains(self, p: Position) -> bool {
        p >= self.start && p < self.end
    }

    /// Canonicalize start ≤ end; useful when caller built the range from a
    /// selection that may have been "grown backwards".
    #[must_use]
    pub fn normalized(self) -> Self {
        if self.start <= self.end {
            self
        } else {
            Self {
                start: self.end,
                end: self.start,
            }
        }
    }

    /// Minimum range covering both.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let a = self.normalized();
        let b = other.normalized();
        Self {
            start: a.start.min(b.start),
            end: a.end.max(b.end),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_half_open() {
        let r = Range::new(Position::new(0, 0), Position::new(0, 5));
        assert!(r.contains(Position::new(0, 0)));
        assert!(r.contains(Position::new(0, 4)));
        assert!(!r.contains(Position::new(0, 5))); // exclusive end
    }

    #[test]
    fn normalize_swaps_inverted() {
        let r = Range::new(Position::new(1, 4), Position::new(0, 2));
        let n = r.normalized();
        assert_eq!(n.start, Position::new(0, 2));
        assert_eq!(n.end, Position::new(1, 4));
    }

    #[test]
    fn union_covers_both() {
        let a = Range::new(Position::new(0, 0), Position::new(0, 4));
        let b = Range::new(Position::new(1, 0), Position::new(1, 5));
        let u = a.union(b);
        assert_eq!(u.start, Position::new(0, 0));
        assert_eq!(u.end, Position::new(1, 5));
    }
}
