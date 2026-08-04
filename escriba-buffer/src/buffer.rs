use std::collections::HashMap;
use std::path::{Path, PathBuf};

use escriba_core::{BufferId, Edit, EditKind, Position, Range};
use ropey::Rope;
use serde::{Deserialize, Serialize};

use crate::encoding::Encoding;
use crate::error::BufferError;
use crate::line_ending::LineEnding;
use crate::undo::{UndoEntry, UndoTree};

/// A single open text buffer.
/// A monotonic counter of TEXT changes to one buffer.
///
/// Deliberately distinct from `escriba-core`'s `EditGen`, which is a REFRESH
/// generation: that one bumps on every action, including pure cursor moves,
/// because its job is telling the renderer when to repaint. Keying staleness
/// on it would mark every cached offset stale after any keypress — noise, not
/// staleness.
///
/// This one bumps if and only if the rope changed, which is the question an
/// offset actually needs answered: "is the text I was measured against still
/// the text?"
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TextRev(pub u64);

impl TextRev {
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

#[derive(Debug, Clone)]
pub struct Buffer {
    pub id: BufferId,
    pub path: Option<PathBuf>,
    pub rope: Rope,
    pub modified: bool,
    pub encoding: Encoding,
    pub line_ending: LineEnding,
    pub undo: UndoTree,
    /// Bumped by every successful [`Buffer::apply`]. Private so it can only
    /// advance through a real mutation — a settable revision is a revision
    /// that lies.
    text_rev: TextRev,
}

impl Buffer {
    #[must_use]
    pub fn empty(id: BufferId) -> Self {
        Self {
            id,
            path: None,
            rope: Rope::new(),
            modified: false,
            encoding: Encoding::default(),
            line_ending: LineEnding::default(),
            undo: UndoTree::new(),
            text_rev: TextRev::default(),
        }
    }

    /// This buffer's current text revision — the token an offset measured
    /// against it should carry.
    #[must_use]
    pub const fn text_rev(&self) -> TextRev {
        self.text_rev
    }

    #[must_use]
    pub fn from_str(id: BufferId, src: &str) -> Self {
        let line_ending = LineEnding::detect(src);
        Self {
            id,
            path: None,
            rope: Rope::from_str(src),
            modified: false,
            encoding: Encoding::default(),
            line_ending,
            undo: UndoTree::new(),
            text_rev: TextRev::default(),
        }
    }

    pub fn open(id: BufferId, path: impl AsRef<Path>) -> Result<Self, BufferError> {
        let path = path.as_ref().to_path_buf();
        let src = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };
        let line_ending = LineEnding::detect(&src);
        Ok(Self {
            id,
            path: Some(path),
            rope: Rope::from_str(&src),
            modified: false,
            encoding: Encoding::default(),
            line_ending,
            undo: UndoTree::new(),
            text_rev: TextRev::default(),
        })
    }

    pub fn save(&mut self) -> Result<(), BufferError> {
        let path = self.path.clone().ok_or(BufferError::NoPath)?;
        let mut src = String::new();
        for line in self.rope.lines() {
            src.push_str(&line.to_string());
        }
        std::fs::write(&path, src)?;
        self.modified = false;
        Ok(())
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), BufferError> {
        self.path = Some(path.as_ref().to_path_buf());
        self.save()
    }

    // ── Queries ────────────────────────────────────────────────────

    #[must_use]
    pub fn line_count(&self) -> u32 {
        u32::try_from(self.rope.len_lines()).unwrap_or(u32::MAX)
    }

    #[must_use]
    pub fn byte_count(&self) -> usize {
        self.rope.len_bytes()
    }

    #[must_use]
    pub fn char_count(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn line(&self, n: u32) -> Option<String> {
        let n = n as usize;
        if n >= self.rope.len_lines() {
            return None;
        }
        Some(self.rope.line(n).to_string())
    }

    pub fn line_len_chars(&self, n: u32) -> u32 {
        let n = n as usize;
        if n >= self.rope.len_lines() {
            return 0;
        }
        let line = self.rope.line(n);
        let mut len = line.len_chars();
        // Strip trailing newline from char count for cursor-placement purposes.
        if line.chars().last().is_some_and(|c| c == '\n' || c == '\r') {
            len = len.saturating_sub(1);
        }
        u32::try_from(len).unwrap_or(u32::MAX)
    }

    /// Clamp a position to valid coordinates inside this buffer.
    #[must_use]
    pub fn clamp(&self, pos: Position) -> Position {
        let line = pos.line.min(self.line_count().saturating_sub(1));
        let col = pos.column.min(self.line_len_chars(line));
        Position::new(line, col)
    }

    pub fn position_to_char(&self, pos: Position) -> Result<usize, BufferError> {
        let line = pos.line as usize;
        if line >= self.rope.len_lines() {
            return Err(BufferError::InvalidPosition {
                line: pos.line,
                column: pos.column,
                total_lines: self.line_count(),
            });
        }
        let line_start = self.rope.line_to_char(line);
        let line_slice = self.rope.line(line);
        let max_col = line_slice.len_chars();
        let col = (pos.column as usize).min(max_col);
        Ok(line_start + col)
    }

    #[must_use]
    pub fn char_to_position(&self, ch: usize) -> Position {
        let line = self.rope.char_to_line(ch.min(self.rope.len_chars()));
        let line_start = self.rope.line_to_char(line);
        let col = ch.saturating_sub(line_start);
        Position::new(
            u32::try_from(line).unwrap_or(u32::MAX),
            u32::try_from(col).unwrap_or(u32::MAX),
        )
    }

    pub fn slice(&self, range: Range) -> Result<String, BufferError> {
        let r = range.normalized();
        let a = self.position_to_char(r.start)?;
        let b = self.position_to_char(r.end)?;
        Ok(self.rope.slice(a..b).to_string())
    }

    // ── Mutation ───────────────────────────────────────────────────

    pub fn apply(&mut self, edit: &Edit) -> Result<UndoEntry, BufferError> {
        let range = edit.range.normalized();
        let start_char = self.position_to_char(range.start)?;
        let end_char = self.position_to_char(range.end)?;
        let previous_text = self.rope.slice(start_char..end_char).to_string();

        // The rope is about to change, so every offset measured against the
        // old text expires here. Bumping AFTER the fallible prelude means a
        // rejected edit does not invalidate anything.
        self.text_rev = self.text_rev.next();

        let (inserted_len_chars, reverse_kind) = match &edit.kind {
            EditKind::Insert { text } => {
                self.rope.insert(start_char, text);
                (text.chars().count(), EditKind::Delete)
            }
            EditKind::Delete => {
                self.rope.remove(start_char..end_char);
                (
                    0usize,
                    EditKind::Insert {
                        text: previous_text.clone(),
                    },
                )
            }
            EditKind::Replace { text } => {
                self.rope.remove(start_char..end_char);
                self.rope.insert(start_char, text);
                (
                    text.chars().count(),
                    EditKind::Replace {
                        text: previous_text.clone(),
                    },
                )
            }
        };
        self.modified = true;

        // Compute the reverse edit's range — where the insertion now sits.
        let reverse_end_char = start_char + inserted_len_chars;
        let reverse_range = Range::new(
            self.char_to_position(start_char),
            self.char_to_position(reverse_end_char),
        );
        let reverse_edit = Edit {
            range: reverse_range,
            kind: reverse_kind,
        };
        let entry = UndoEntry {
            applied: edit.clone(),
            reverse: reverse_edit,
        };
        self.undo.push(entry.clone());
        Ok(entry)
    }

    pub fn undo(&mut self) -> Result<UndoEntry, BufferError> {
        let entry = self.undo.pop_undo().ok_or(BufferError::NothingToUndo)?;
        // Apply reverse edit without pushing a new undo entry.
        let r = entry.reverse.range.normalized();
        let a = self.position_to_char(r.start)?;
        let b = self.position_to_char(r.end)?;
        match &entry.reverse.kind {
            EditKind::Insert { text } => {
                self.rope.insert(a, text);
            }
            EditKind::Delete => {
                self.rope.remove(a..b);
            }
            EditKind::Replace { text } => {
                self.rope.remove(a..b);
                self.rope.insert(a, text);
            }
        }
        self.modified = true;
        Ok(entry)
    }

    pub fn redo(&mut self) -> Result<UndoEntry, BufferError> {
        let entry = self.undo.pop_redo().ok_or(BufferError::NothingToRedo)?;
        let r = entry.applied.range.normalized();
        let a = self.position_to_char(r.start)?;
        let b = self.position_to_char(r.end)?;
        match &entry.applied.kind {
            EditKind::Insert { text } => {
                self.rope.insert(a, text);
            }
            EditKind::Delete => {
                self.rope.remove(a..b);
            }
            EditKind::Replace { text } => {
                self.rope.remove(a..b);
                self.rope.insert(a, text);
            }
        }
        self.modified = true;
        Ok(entry)
    }

    #[must_use]
    pub fn to_string(&self) -> String {
        self.rope.to_string()
    }
}

/// A registry of open buffers keyed by id — phase 1 single-threaded view.
#[derive(Debug, Default, Clone)]
pub struct BufferSet {
    buffers: HashMap<BufferId, Buffer>,
    next_id: u64,
}

impl BufferSet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_id(&mut self) -> BufferId {
        self.next_id += 1;
        BufferId(self.next_id)
    }

    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<BufferId, BufferError> {
        let id = self.next_id();
        let buf = Buffer::open(id, path)?;
        self.buffers.insert(id, buf);
        Ok(id)
    }

    pub fn scratch(&mut self, src: &str) -> BufferId {
        let id = self.next_id();
        self.buffers.insert(id, Buffer::from_str(id, src));
        id
    }

    #[must_use]
    pub fn get(&self, id: BufferId) -> Option<&Buffer> {
        self.buffers.get(&id)
    }

    pub fn get_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.buffers.get_mut(&id)
    }

    #[must_use]
    pub fn ids(&self) -> Vec<BufferId> {
        let mut v: Vec<_> = self.buffers.keys().copied().collect();
        v.sort();
        v
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferSummary {
    pub id: BufferId,
    pub path: Option<PathBuf>,
    pub line_count: u32,
    pub modified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_core::{Edit, Position, Range};

    fn buf(src: &str) -> Buffer {
        Buffer::from_str(BufferId(1), src)
    }

    #[test]
    fn empty_has_one_line() {
        let b = Buffer::empty(BufferId(1));
        assert_eq!(b.line_count(), 1);
    }

    #[test]
    fn line_counts() {
        let b = buf("a\nb\nc\n");
        assert_eq!(b.line_count(), 4); // trailing \n leaves a final empty line
    }

    #[test]
    fn position_char_round_trip() {
        let b = buf("hello\nworld\n");
        let p = Position::new(1, 3);
        let c = b.position_to_char(p).unwrap();
        assert_eq!(b.char_to_position(c), p);
    }

    #[test]
    fn insert_then_undo() {
        let mut b = buf("hello");
        let e = Edit::insert(Position::new(0, 5), " world");
        b.apply(&e).unwrap();
        assert_eq!(b.to_string(), "hello world");
        b.undo().unwrap();
        assert_eq!(b.to_string(), "hello");
    }

    #[test]
    fn delete_then_redo() {
        let mut b = buf("hello world");
        let e = Edit::delete(Range::new(Position::new(0, 5), Position::new(0, 11)));
        b.apply(&e).unwrap();
        assert_eq!(b.to_string(), "hello");
        b.undo().unwrap();
        assert_eq!(b.to_string(), "hello world");
        b.redo().unwrap();
        assert_eq!(b.to_string(), "hello");
    }

    #[test]
    fn replace_is_delete_plus_insert() {
        let mut b = buf("hello world");
        let e = Edit::replace(
            Range::new(Position::new(0, 6), Position::new(0, 11)),
            "tatara",
        );
        b.apply(&e).unwrap();
        assert_eq!(b.to_string(), "hello tatara");
        b.undo().unwrap();
        assert_eq!(b.to_string(), "hello world");
    }

    #[test]
    fn clamp_constrains_position() {
        let b = buf("ab\ncd");
        assert_eq!(b.line_count(), 2);
        assert_eq!(b.clamp(Position::new(0, 99)), Position::new(0, 2));
        assert_eq!(b.clamp(Position::new(99, 0)), Position::new(1, 0));
    }

    #[test]
    fn slice_returns_text() {
        let b = buf("hello world");
        let s = b
            .slice(Range::new(Position::new(0, 6), Position::new(0, 11)))
            .unwrap();
        assert_eq!(s, "world");
    }

    #[test]
    fn save_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("demo.txt");
        let mut b = Buffer::from_str(BufferId(1), "hello\n");
        b.save_as(&path).unwrap();
        let b2 = Buffer::open(BufferId(2), &path).unwrap();
        assert_eq!(b2.to_string(), "hello\n");
    }

    #[test]
    fn buffer_set_tracks_ids() {
        let mut set = BufferSet::new();
        let a = set.scratch("one");
        let b = set.scratch("two");
        assert_ne!(a, b);
        assert_eq!(set.ids().len(), 2);
        assert_eq!(set.get(a).unwrap().to_string(), "one");
    }
}

#[cfg(test)]
mod text_rev_tests {
    use super::*;
    use escriba_core::{Edit, Position, Range};

    fn buf(src: &str) -> Buffer {
        Buffer::from_str(BufferId(0), src)
    }

    #[test]
    fn a_fresh_buffer_starts_at_revision_zero() {
        assert_eq!(buf("hello").text_rev(), TextRev(0));
    }

    #[test]
    fn an_applied_edit_advances_the_revision() {
        let mut b = buf("hello");
        let before = b.text_rev();
        b.apply(&Edit::insert(Position::new(0, 0), "X".to_string()))
            .expect("insert applies");
        assert_ne!(
            b.text_rev(),
            before,
            "a text change must expire old offsets"
        );
    }

    #[test]
    fn each_edit_advances_it_again() {
        let mut b = buf("hello");
        let mut seen = vec![b.text_rev()];
        for _ in 0..3 {
            b.apply(&Edit::insert(Position::new(0, 0), "X".to_string()))
                .expect("insert applies");
            let now = b.text_rev();
            assert!(!seen.contains(&now), "revisions must not repeat: {now:?}");
            seen.push(now);
        }
    }

    #[test]
    fn a_rejected_edit_does_not_advance_the_revision() {
        // The property that makes this usable for staleness: an edit that never
        // touched the rope must not invalidate offsets that are still correct.
        let mut b = buf("hello");
        let before = b.text_rev();
        let out_of_range = Range {
            start: Position::new(99, 0),
            end: Position::new(99, 1),
        };
        assert!(
            b.apply(&Edit::delete(out_of_range)).is_err(),
            "the edit must fail"
        );
        assert_eq!(b.text_rev(), before, "a failed edit changed no text");
    }

    #[test]
    fn reading_the_buffer_does_not_advance_the_revision() {
        // Contrast with `EditGen`, which bumps on every action. If merely
        // looking moved this counter it would be a refresh generation again.
        let b = buf("hello");
        let before = b.text_rev();
        let _ = b.to_string();
        let _ = b.text_rev();
        assert_eq!(b.text_rev(), before);
    }
}
