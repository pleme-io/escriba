use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::range::Range;

/// A primitive text mutation — the atom of undo/redo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Edit {
    pub range: Range,
    pub kind: EditKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum EditKind {
    /// Insert `text` at `range.start`; range.end is ignored.
    Insert { text: String },
    /// Delete `range`.
    Delete,
    /// Replace `range` with `text`.
    Replace { text: String },
}

impl Edit {
    #[must_use]
    pub fn insert(at: crate::position::Position, text: impl Into<String>) -> Self {
        Self {
            range: Range::point(at),
            kind: EditKind::Insert { text: text.into() },
        }
    }

    #[must_use]
    pub fn delete(range: Range) -> Self {
        Self {
            range,
            kind: EditKind::Delete,
        }
    }

    #[must_use]
    pub fn replace(range: Range, text: impl Into<String>) -> Self {
        Self {
            range,
            kind: EditKind::Replace { text: text.into() },
        }
    }
}
