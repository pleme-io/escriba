//! `escriba-mode` — modal state machine.
//!
//! Phase 1: vim-ish Normal/Insert/Visual/VisualLine/Command. Tracks pending
//! count (`5dd`) and pending operator (the `d` in `dw`) so the keymap can
//! emit a single [`escriba_core::CountedAction`].

extern crate self as escriba_mode;

use escriba_core::{Mode, Operator};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModalState {
    pub mode: Mode,
    pub pending_count: Option<u32>,
    pub pending_operator: Option<Operator>,
    /// The accumulating minibuffer / command-mode input line.
    pub minibuffer: String,
}

impl ModalState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enter(&mut self, mode: Mode) {
        self.mode = mode;
        if mode == Mode::Normal {
            self.pending_count = None;
            self.pending_operator = None;
            self.minibuffer.clear();
        }
        if mode == Mode::Command {
            self.minibuffer.clear();
        }
    }

    pub fn set_operator(&mut self, op: Operator) {
        self.pending_operator = Some(op);
    }

    pub fn append_count(&mut self, digit: u32) {
        let n = self.pending_count.unwrap_or(0);
        self.pending_count = Some(n.saturating_mul(10).saturating_add(digit));
    }

    #[must_use]
    pub fn consume_count(&mut self) -> u32 {
        self.pending_count.take().unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn consume_operator(&mut self) -> Option<Operator> {
        self.pending_operator.take()
    }

    pub fn push_minibuffer(&mut self, ch: char) {
        self.minibuffer.push(ch);
    }

    pub fn pop_minibuffer(&mut self) -> Option<char> {
        self.minibuffer.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_resets_pending_state() {
        let mut s = ModalState::new();
        s.enter(Mode::Insert);
        s.set_operator(Operator::Delete);
        s.append_count(5);
        s.enter(Mode::Normal);
        assert!(s.pending_count.is_none());
        assert!(s.pending_operator.is_none());
    }

    #[test]
    fn count_accumulates() {
        let mut s = ModalState::new();
        s.append_count(5);
        s.append_count(3);
        assert_eq!(s.consume_count(), 53);
        assert_eq!(s.consume_count(), 1); // default 1 when consumed again
    }

    #[test]
    fn operator_round_trip() {
        let mut s = ModalState::new();
        s.set_operator(Operator::Yank);
        assert_eq!(s.consume_operator(), Some(Operator::Yank));
        assert_eq!(s.consume_operator(), None);
    }

    #[test]
    fn minibuffer_append_pop() {
        let mut s = ModalState::new();
        s.enter(Mode::Command);
        s.push_minibuffer('w');
        assert_eq!(s.minibuffer, "w");
        assert_eq!(s.pop_minibuffer(), Some('w'));
    }
}
