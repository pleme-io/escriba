//! Breakpoints — where the operator asked execution to stop.
//!
//! ## Why this is not a [`Finding`](escriba_shirube::Finding)
//!
//! Everything else escriba paints in the gutter is a finding, and reusing
//! that plane was the obvious move. It is also wrong, twice over, and both
//! failures are silent:
//!
//! - **Every `escriba_shirube` list is ANCHORED**, and the gutter's only
//!   reader flat-maps `ResultList::fresh(world)`. That is exactly right for a
//!   diagnostic — a marker computed against text the operator has since
//!   edited is confidently wrong — and exactly wrong for a breakpoint, which
//!   would vanish on the next keystroke.
//! - **`ListRegistry::publish` replaces a list wholesale and FOCUSES it**, so
//!   `]d` would start walking breakpoints as though they were problems.
//!
//! A finding is something a PRODUCER found and is only as good as the world
//! it was computed in. A breakpoint is something the OPERATOR put there, and
//! it is as good as their intention, which no edit invalidates.
//!
//! ## What this does NOT do yet, stated plainly
//!
//! A breakpoint is keyed by `(buffer, line number)` and **does not shift when
//! text above it is edited**. Insert a line at the top of the file and the
//! breakpoint stays on the line NUMBER it was set on, not on the line of code
//! it was set against. Nothing in escriba shifts a mark under an edit today —
//! findings dodge the problem by dying, which is the option a breakpoint does
//! not have — so this is the honest floor rather than a bug that slipped
//! through: the operator's breakpoint survives their typing, which is the
//! property that matters most, and it can drift.
//!
//! **Shifting under an edit is the next piece**, and it is a shared primitive
//! rather than a patch here: the same machinery would fix findings, marks
//! (`m[a-z]`), and the jumplist. Do not paper over it with an ad-hoc
//! adjustment in this file.

use std::collections::BTreeSet;

use escriba_core::BufferId;

/// Every line the operator has marked for the debugger to stop on.
///
/// Keyed by `(buffer, line)` — see the module docs for what that key does and
/// does not survive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Breakpoints {
    set: BTreeSet<(BufferId, u32)>,
}

impl Breakpoints {
    /// Set a breakpoint on `line` of `buffer` if there is none, clear it if
    /// there is. Returns whether one is set AFTERWARDS.
    ///
    /// One verb rather than `set` + `clear`, because the operator's key is
    /// one verb: a pair would let a caller ask "is it set?" and then act on
    /// the answer, which is the shape a double-toggle race lives in.
    pub fn toggle(&mut self, buffer: BufferId, line: u32) -> bool {
        if self.set.remove(&(buffer, line)) {
            false
        } else {
            self.set.insert((buffer, line));
            true
        }
    }

    /// Is there a breakpoint on `line` of `buffer`?
    #[must_use]
    pub fn is_set(&self, buffer: BufferId, line: u32) -> bool {
        self.set.contains(&(buffer, line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: BufferId = BufferId(1);
    const B: BufferId = BufferId(2);

    #[test]
    fn toggling_is_its_own_inverse() {
        let mut bp = Breakpoints::default();
        assert!(!bp.is_set(A, 3));
        assert!(bp.toggle(A, 3), "the first toggle SETS");
        assert!(bp.is_set(A, 3));
        assert!(!bp.toggle(A, 3), "the second toggle CLEARS");
        assert!(!bp.is_set(A, 3));
    }

    #[test]
    fn a_breakpoint_belongs_to_one_buffer() {
        // The key is the PAIR. Keying on the line alone would put a
        // breakpoint set in one file onto the same row of every other one —
        // which looks correct in any single-buffer test.
        let mut bp = Breakpoints::default();
        bp.toggle(A, 7);
        assert!(bp.is_set(A, 7));
        assert!(!bp.is_set(B, 7), "buffer B never had one");
        assert!(!bp.is_set(A, 8), "and neither did line 8");
    }
}
