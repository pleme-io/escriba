//! The jumplist — where you were before the editor moved you a long way.
//!
//! # Why this is a core primitive and not a search feature
//!
//! It arrived while finishing search, but it does not belong to search. A
//! *far jump* is any motion that relocates you somewhere you cannot find your
//! way back from by reversing the keys you pressed: `/pattern`, `n`, `*`, `G`,
//! `gg`, `%`, a tag jump, a quickfix hop. Every one of those needs the same
//! return ticket, so the ticket is a primitive with several consumers rather
//! than a field on `SearchState`.
//!
//! The distinction that decides what records a jump: **near motions do not.**
//! `j`, `w`, `}` move you somewhere you can get back from with `k`, `b`, `{`.
//! Recording those would fill the list with noise and make `<C-o>` useless,
//! which is the failure mode to avoid — a jumplist that remembers everything
//! remembers nothing.
//!
//! # Semantics (vim's, deliberately)
//!
//! - A jump records the position you jumped **from**, not where you land.
//! - `<C-o>` walks back through the list, `<C-i>` walks forward again.
//! - The first `<C-o>` also records where you currently are, so `<C-i>` can
//!   return to it. Without that the forward direction has no destination and
//!   `<C-o>` becomes a one-way door.
//! - Making a **new** jump while partway back truncates the forward entries —
//!   you have taken a different branch, and the abandoned future is gone. This
//!   is the same rule an undo tree applies to a new edit after undo.
//! - Consecutive jumps from the same line collapse. Vim does this because
//!   repeated `n` on one line otherwise buries the interesting history under
//!   near-duplicates.

use crate::{BufferId, Position};

/// How many jumps to keep. Vim's default is 100.
pub const JUMPLIST_LIMIT: usize = 100;

/// A located position — WHERE, in WHICH buffer.
///
/// The third place escriba dropped the buffer half of a location, after the
/// findings walker and the gutter. A jump destination is not a `Position`:
/// `<C-o>` after a cross-file jump has to come back to the FILE it left, and
/// storing a bare line meant it returned to that line number in whatever
/// buffer happened to be active.
///
/// Named rather than a tuple because it now travels through the jumplist,
/// the findings walker, and every future cross-file producer.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct Spot {
    pub buffer: BufferId,
    pub pos: Position,
}

impl Spot {
    #[must_use]
    pub const fn new(buffer: BufferId, pos: Position) -> Self {
        Self { buffer, pos }
    }

    /// Same buffer AND same line — the collapse rule. Pressing `n` five times
    /// down one long line leaves one entry, but the same line in a DIFFERENT
    /// file is a different place and must not collapse into it.
    #[must_use]
    pub const fn same_line(&self, other: &Self) -> bool {
        self.buffer.0 == other.buffer.0 && self.pos.line == other.pos.line
    }
}

/// A ring of jump origins with a walk cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JumpList {
    /// Oldest first. Bounded by [`JUMPLIST_LIMIT`].
    entries: Vec<Spot>,
    /// Where the walk currently sits. `entries.len()` means "not walking" —
    /// at the newest end, with nothing to walk forward to.
    cursor: usize,
}

impl JumpList {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            cursor: 0,
        }
    }

    /// Record a jump taken **from** `from`.
    ///
    /// Call this immediately before a far jump moves the cursor.
    pub fn push(&mut self, from: Spot) {
        // A new jump abandons any forward history — a different branch was
        // taken, so the old one is not reachable any more.
        self.entries.truncate(self.cursor);

        // Collapse consecutive jumps from the same line. Pressing `n` five
        // times down one long line should leave one entry, not five.
        if self
            .entries
            .last()
            .is_some_and(|last| last.same_line(&from))
        {
            self.entries.pop();
        }

        self.entries.push(from);
        self.trim();
        self.cursor = self.entries.len();
    }

    /// `<C-o>` — walk back one jump. `current` is where the cursor is now.
    ///
    /// Returns the position to move to, or `None` when there is nothing older.
    pub fn back(&mut self, current: Spot) -> Option<Spot> {
        if self.entries.is_empty() {
            return None;
        }

        // Stepping into the list for the first time: park the current position
        // at the newest end so `<C-i>` has somewhere to return to. Skipped when
        // we are already standing on that line, so a `<C-o>` immediately after
        // a jump does not record the place the jump just landed twice.
        if self.cursor == self.entries.len() {
            if self
                .entries
                .last()
                .is_some_and(|last| last.same_line(&current))
            {
                // The newest entry already describes this line.
            } else {
                self.entries.push(current);
                self.trim();
            }
            self.cursor = self.entries.len().saturating_sub(1);
        }

        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        self.entries.get(self.cursor).copied()
    }

    /// `<C-i>` — walk forward again. `None` when already at the newest end.
    pub fn forward(&mut self) -> Option<Spot> {
        if self.cursor + 1 >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        self.entries.get(self.cursor).copied()
    }

    /// The recorded jumps, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[Spot] {
        &self.entries
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop the oldest entries past the limit, keeping the walk cursor
    /// pointing at the same logical entry.
    fn trim(&mut self) {
        while self.entries.len() > JUMPLIST_LIMIT {
            self.entries.remove(0);
            self.cursor = self.cursor.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spot in ONE buffer — these tests are about the walk semantics, not
    /// about crossing files. Cross-buffer behaviour is pinned in
    /// `escriba-runtime/tests/cross_file_findings.rs`, where a real editor
    /// with two buffers exists.
    fn p(line: u32) -> Spot {
        Spot::new(BufferId(1), Position::new(line, 0))
    }

    /// The same line in a DIFFERENT buffer — a distinct place that must not
    /// collapse into `p(line)`.
    fn q(line: u32) -> Spot {
        Spot::new(BufferId(2), Position::new(line, 0))
    }

    #[test]
    fn an_empty_list_has_nowhere_to_go() {
        let mut j = JumpList::new();
        assert_eq!(j.back(p(5)), None);
        assert_eq!(j.forward(), None);
        assert!(j.is_empty());
    }

    #[test]
    fn back_returns_to_where_the_jump_was_taken_from() {
        let mut j = JumpList::new();
        // Sitting on line 3, jump to line 40.
        j.push(p(3));
        assert_eq!(j.back(p(40)), Some(p(3)), "<C-o> returns to the origin");
    }

    #[test]
    fn forward_returns_to_where_back_was_pressed_from() {
        // The property that makes <C-o> a door rather than a trapdoor.
        let mut j = JumpList::new();
        j.push(p(3));

        assert_eq!(j.back(p(40)), Some(p(3)));
        assert_eq!(j.forward(), Some(p(40)), "<C-i> comes back to where I was");
    }

    #[test]
    fn walking_back_twice_visits_both_origins_newest_first() {
        let mut j = JumpList::new();
        j.push(p(1)); // 1 → …
        j.push(p(10)); // 10 → …

        assert_eq!(j.back(p(50)), Some(p(10)));
        assert_eq!(j.back(p(10)), Some(p(1)));
        assert_eq!(j.back(p(1)), None, "nothing older than the first jump");
    }

    #[test]
    fn a_new_jump_abandons_the_forward_history() {
        let mut j = JumpList::new();
        j.push(p(1));
        j.push(p(10));
        j.back(p(50)); // now sitting mid-list

        // Branch off somewhere else.
        j.push(p(20));

        assert_eq!(j.forward(), None, "the abandoned future must be gone");
        assert_eq!(
            j.back(p(99)),
            Some(p(20)),
            "the new branch is what we return to"
        );
    }

    #[test]
    fn consecutive_jumps_from_one_line_collapse() {
        // Pressing `n` repeatedly while several matches share a line must not
        // bury the useful history.
        let mut j = JumpList::new();
        j.push(Spot::new(BufferId(1), Position::new(7, 0)));
        j.push(Spot::new(BufferId(1), Position::new(7, 20)));
        j.push(Spot::new(BufferId(1), Position::new(7, 40)));

        assert_eq!(j.len(), 1, "one entry for the line, got {:?}", j.entries());
    }

    #[test]
    fn jumps_from_different_lines_all_survive() {
        let mut j = JumpList::new();
        j.push(p(1));
        j.push(p(2));
        j.push(p(3));
        assert_eq!(j.len(), 3);
    }

    #[test]
    fn the_list_is_bounded_and_drops_the_oldest() {
        let mut j = JumpList::new();
        for line in 0..(JUMPLIST_LIMIT as u32 + 25) {
            j.push(p(line));
        }
        assert_eq!(j.len(), JUMPLIST_LIMIT, "bounded");
        assert_eq!(
            j.entries().first().copied(),
            Some(p(25)),
            "the oldest entries are the ones dropped",
        );
    }

    #[test]
    fn back_from_the_same_line_does_not_duplicate_it() {
        let mut j = JumpList::new();
        j.push(p(3));
        // `<C-o>` while still standing on the line the jump was recorded from.
        j.back(p(3));
        assert_eq!(
            j.len(),
            1,
            "no duplicate entry for one line: {:?}",
            j.entries()
        );
    }

    #[test]
    fn forward_past_the_newest_end_is_none() {
        let mut j = JumpList::new();
        j.push(p(1));
        j.back(p(9));
        assert_eq!(j.forward(), Some(p(9)));
        assert_eq!(j.forward(), None, "cannot walk past where I started");
    }

    #[test]
    fn the_same_line_in_a_different_buffer_does_not_collapse() {
        // The collapse rule exists so five `n` presses down one long line
        // leave one entry. It must key on (buffer, line): the same line in a
        // DIFFERENT file is a different place, and collapsing it would make
        // `<C-o>` unable to return across a cross-file jump.
        let mut j = JumpList::new();
        j.push(p(7));
        j.push(q(7));
        assert_eq!(
            j.entries().len(),
            2,
            "line 7 of two different buffers is two places: {:?}",
            j.entries(),
        );
    }

    #[test]
    fn walking_back_returns_the_buffer_too() {
        let mut j = JumpList::new();
        j.push(p(3));
        let back = j.back(q(9)).expect("something older");
        assert_eq!(back.buffer, BufferId(1), "must return the buffer we left");
        assert_eq!(back.pos.line, 3);
    }
}
