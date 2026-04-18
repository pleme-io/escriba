//! Minimal undo tree — linear stack for phase 1; branching in phase 2.

use escriba_core::Edit;

/// One atomic edit (the thing Undo reverses) + the reverse edit to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoEntry {
    pub applied: Edit,
    pub reverse: Edit,
}

#[derive(Debug, Default, Clone)]
pub struct UndoTree {
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
}

impl UndoTree {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, entry: UndoEntry) {
        self.undo_stack.push(entry);
        self.redo_stack.clear();
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn pop_undo(&mut self) -> Option<UndoEntry> {
        let e = self.undo_stack.pop()?;
        self.redo_stack.push(e.clone());
        Some(e)
    }

    pub fn pop_redo(&mut self) -> Option<UndoEntry> {
        let e = self.redo_stack.pop()?;
        self.undo_stack.push(e.clone());
        Some(e)
    }

    #[must_use]
    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    #[must_use]
    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }
}
