use std::cmp::Ordering;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 0-indexed line + column pair. Column is a **UTF-8 char offset** into
/// the line (not a byte offset); buffer-side conversion to rope byte
/// positions happens in `escriba-buffer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

impl Position {
    pub const ZERO: Self = Self { line: 0, column: 0 };

    #[must_use]
    pub const fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }

    #[must_use]
    pub const fn line(line: u32) -> Self {
        Self { line, column: 0 }
    }

    /// Saturating addition on column.
    #[must_use]
    pub const fn shift_right(self, n: u32) -> Self {
        Self {
            line: self.line,
            column: self.column.saturating_add(n),
        }
    }

    /// Saturating subtraction on column.
    #[must_use]
    pub const fn shift_left(self, n: u32) -> Self {
        Self {
            line: self.line,
            column: self.column.saturating_sub(n),
        }
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> Ordering {
        self.line
            .cmp(&other.line)
            .then(self.column.cmp(&other.column))
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line + 1, self.column + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ord_by_line_then_column() {
        assert!(Position::new(0, 5) < Position::new(1, 0));
        assert!(Position::new(3, 2) < Position::new(3, 4));
        assert_eq!(
            Position::new(2, 0).cmp(&Position::new(2, 0)),
            Ordering::Equal
        );
    }

    #[test]
    fn shift_saturates() {
        assert_eq!(Position::new(0, 0).shift_left(5), Position::new(0, 0));
        assert_eq!(
            Position::new(0, u32::MAX).shift_right(5),
            Position::new(0, u32::MAX)
        );
    }

    #[test]
    fn display_is_one_indexed() {
        assert_eq!(Position::new(0, 0).to_string(), "1:1");
        assert_eq!(Position::new(9, 3).to_string(), "10:4");
    }
}
