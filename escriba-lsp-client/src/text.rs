//! Byte offsets ↔ LSP positions.
//!
//! **LSP `Position.character` counts UTF-16 code units.** Not bytes, not
//! characters, not grapheme clusters — UTF-16 code units, because the protocol
//! was designed around a JavaScript editor. Every other layer here counts
//! bytes: escriba's buffer, rnix's spans, `sui-ir`'s `Span`.
//!
//! That mismatch is the LSP equivalent of the `Content-Length` bug in
//! [`crate::wire`], and it fails the same way — silently, and only for some
//! users. Nobody notices while the file is ASCII, because all three counts
//! agree. The first accented identifier shifts every column on its line; the
//! first emoji in a comment shifts them by two, since a character outside the
//! Basic Multilingual Plane is ONE Rust `char`, up to four UTF-8 bytes, and
//! **two** UTF-16 code units. A diagnostic then underlines the wrong span and a
//! goto-definition lands in the wrong place, on other people's files.
//!
//! So the conversion is one primitive with the three widths written down, and
//! the astral-plane case is a test rather than an assumption.

use serde::{Deserialize, Serialize};

/// An LSP position: zero-based line, and a character offset in **UTF-16 code
/// units** from the start of that line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    #[must_use]
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// An LSP range, half-open as the spec requires: `end` is exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// Line starts for one document, so a byte offset can be resolved without
/// rescanning from the top each time.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset at which each line begins. Always starts with 0.
    starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0usize];
        // Split on '\n' only. A CRLF document's '\r' belongs to the PRECEDING
        // line, which is what every editor and the LSP spec assume; treating
        // "\r\n" as two breaks would insert a phantom empty line and shift
        // every subsequent line number by one.
        starts.extend(text.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1));
        Self { starts, len: text.len() }
    }

    /// The line containing `offset`, and the byte offset that line starts at.
    fn line_of(&self, offset: usize) -> (u32, usize) {
        // partition_point gives the first line start STRICTLY greater than
        // offset, so the line we want is the one before it.
        let idx = self.starts.partition_point(|&s| s <= offset).saturating_sub(1);
        (u32::try_from(idx).unwrap_or(u32::MAX), self.starts[idx])
    }

    /// Convert a byte offset into an LSP position.
    ///
    /// An offset past the end clamps to the end of the document rather than
    /// panicking: offsets reach us from a *server*, and a server that is one
    /// byte optimistic must not take the editor down.
    #[must_use]
    pub fn position(&self, text: &str, offset: usize) -> Position {
        let offset = offset.min(self.len);
        let (line, start) = self.line_of(offset);
        // Not a byte count and not a char count — the number of UTF-16 code
        // units in the text between the line start and the offset.
        let character = text
            .get(start..offset)
            .map_or(0, |s| s.chars().map(|c| u32::try_from(c.len_utf16()).unwrap_or(1)).sum());
        Position { line, character }
    }

    /// Convert an LSP position back into a byte offset.
    ///
    /// The inverse of [`Self::position`], and needed because the client sends
    /// positions too. Out-of-range lines and characters clamp for the same
    /// reason.
    #[must_use]
    pub fn offset(&self, text: &str, pos: Position) -> usize {
        let Some(&start) = self.starts.get(pos.line as usize) else {
            return self.len;
        };
        let line_end = self
            .starts
            .get(pos.line as usize + 1)
            .map_or(self.len, |&next| next.saturating_sub(1));
        let mut utf16 = 0u32;
        let mut byte = start;
        for c in text.get(start..line_end).unwrap_or_default().chars() {
            if utf16 >= pos.character {
                break;
            }
            utf16 += u32::try_from(c.len_utf16()).unwrap_or(1);
            byte += c.len_utf8();
        }
        byte
    }

    /// A byte span as an LSP range.
    #[must_use]
    pub fn range(&self, text: &str, start: usize, end: usize) -> Range {
        Range { start: self.position(text, start), end: self.position(text, end) }
    }

    #[must_use]
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{LineIndex, Position};

    fn pos(text: &str, offset: usize) -> Position {
        LineIndex::new(text).position(text, offset)
    }

    #[test]
    fn ascii_positions_are_the_obvious_ones() {
        let t = "let x = 1;\nin x\n";
        assert_eq!(pos(t, 0), Position::new(0, 0));
        assert_eq!(pos(t, 4), Position::new(0, 4));
        assert_eq!(pos(t, 11), Position::new(1, 0), "just past the newline is line 1 col 0");
        assert_eq!(pos(t, 13), Position::new(1, 2));
    }

    /// A 2-byte character is ONE UTF-16 unit. Counting bytes would put the
    /// column one too far right for every position after it on the line.
    #[test]
    fn a_two_byte_character_advances_the_column_by_one() {
        let t = "café = 1";
        assert_eq!("café".len(), 5, "fixture must be multibyte");
        assert_eq!(pos(t, 5), Position::new(0, 4), "after 'café' the column is 4, not 5");
    }

    /// Three-byte characters, same rule. `日本語` is 9 bytes and 3 columns.
    #[test]
    fn three_byte_characters_advance_one_column_each() {
        let t = "# 日本語\n";
        assert_eq!("日本語".len(), 9);
        assert_eq!(pos(t, 2 + 9), Position::new(0, 2 + 3));
    }

    /// **The killer case.** A character outside the Basic Multilingual Plane is
    /// one Rust `char`, FOUR UTF-8 bytes, and TWO UTF-16 code units. Counting
    /// chars is as wrong as counting bytes here, just differently wrong — which
    /// is why the conversion is written in terms of `len_utf16` and not either.
    #[test]
    fn an_astral_plane_character_is_two_utf16_units() {
        let t = "# 🎉 done";
        assert_eq!("🎉".len(), 4, "four UTF-8 bytes");
        assert_eq!("🎉".chars().count(), 1, "one char");
        assert_eq!("🎉".chars().next().unwrap().len_utf16(), 2, "TWO UTF-16 units");
        // "# " is 2, the emoji adds 2 -> column 4, not 3 (chars) and not 6 (bytes).
        assert_eq!(pos(t, 2 + 4), Position::new(0, 4));
    }

    /// CRLF: the '\r' belongs to the line it ends. Treating "\r\n" as two
    /// breaks inserts a phantom empty line and shifts every later line number.
    #[test]
    fn crlf_does_not_create_a_phantom_line() {
        let t = "a\r\nb\r\nc";
        let ix = LineIndex::new(t);
        assert_eq!(ix.line_count(), 3, "three lines, not five");
        assert_eq!(ix.position(t, 3), Position::new(1, 0), "'b' starts line 1");
        assert_eq!(ix.position(t, 6), Position::new(2, 0), "'c' starts line 2");
    }

    /// Offsets arrive from a SERVER. One byte optimistic must clamp, not panic
    /// — an editor that dies because a language server miscounted is a worse
    /// outcome than a diagnostic in slightly the wrong place.
    #[test]
    fn an_offset_past_the_end_clamps_instead_of_panicking() {
        let t = "abc";
        assert_eq!(pos(t, 3), Position::new(0, 3), "exactly at the end is valid");
        assert_eq!(pos(t, 99), Position::new(0, 3), "past the end clamps");
        assert_eq!(pos("", 5), Position::new(0, 0), "even for an empty document");
    }

    /// The two directions must agree, including across every awkward width.
    #[test]
    fn offset_and_position_round_trip() {
        for t in [
            "let x = 1;\nin x\n",
            "café = 1\nnaïve = 2\n",
            "# 🎉 done\nx = 1\n",
            "a\r\nb\r\nc",
            "",
            "\n\n\n",
        ] {
            let ix = LineIndex::new(t);
            // Only char boundaries are legal offsets; an interior byte has no
            // position, which is itself the point of doing this in one place.
            for (b, _) in t.char_indices().chain(std::iter::once((t.len(), ' '))) {
                let p = ix.position(t, b);
                assert_eq!(ix.offset(t, p), b, "round trip failed at byte {b} of {t:?}");
            }
        }
    }

    /// A position beyond the end of its line clamps to the line end rather
    /// than spilling onto the next one — a server asking for column 999 means
    /// "end of line", not "somewhere in the following line".
    #[test]
    fn an_overlong_character_clamps_to_the_line_end_not_the_next_line() {
        let t = "ab\ncd\n";
        let ix = LineIndex::new(t);
        assert_eq!(ix.offset(t, Position::new(0, 999)), 2, "end of line 0, before the newline");
        assert_eq!(ix.offset(t, Position::new(1, 999)), 5);
        assert_eq!(ix.offset(t, Position::new(99, 0)), t.len(), "a line past the end clamps");
    }

    #[test]
    fn a_byte_span_becomes_a_range() {
        let t = "let x = 1;\nin x\n";
        let ix = LineIndex::new(t);
        let r = ix.range(t, 4, 5);
        assert_eq!(r.start, Position::new(0, 4));
        assert_eq!(r.end, Position::new(0, 5));
    }
}
