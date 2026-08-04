//! The status line, as DATA — one model, rendered by every face.
//!
//! ## Why this exists
//!
//! The two faces had drifted, and not cosmetically. The ratatui face drew the
//! search prompt; the GPU face — escriba's *default* renderer — built its
//! status line from a fixed `format!()` carrying mode, line, column and
//! version, and read neither `modal.minibuffer()` nor `search.prompt()`.
//!
//! The consequence is worth stating plainly, because it is the whole reason
//! this module exists: on the default face, typing `/foo` moved the cursor
//! with **no prompt, no pattern, and no message on screen**. Search was fully
//! implemented and completely invisible, which is indistinguishable from
//! search not existing.
//!
//! Two faces each deciding independently what a status line contains is a
//! divergence generator. A shared model makes them disagree only about
//! *styling*, which is what a face is actually for.
//!
//! `format!()` is not used here — the fleet's ★★ TYPED EMISSION rule. Every
//! rendering path is `push_str`/`push` into a caller-owned buffer.

use escriba_core::Mode;
use escriba_search::MatchCount;

/// What the command line is currently for.
///
/// A search prompt and an ex-command share `Mode::Command` (as they share
/// vim's cmdline), so the mode alone cannot tell a face which sigil to draw.
/// This is that discriminator, derived from the same typed `Option<Prompt>`
/// the runtime already routes `<CR>` on — never a second flag to keep in sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Not in the command line at all.
    None,
    /// `/` — a forward search.
    SearchForward,
    /// `?` — a backward search.
    SearchBackward,
    /// `:` — an ex-command.
    Ex,
}

impl PromptKind {
    /// The leading character a face draws for this prompt, if any.
    ///
    /// Total over the enum — a new prompt kind is a compile error here rather
    /// than a prompt that silently renders with no sigil.
    #[must_use]
    pub const fn sigil(self) -> Option<char> {
        match self {
            Self::None => None,
            Self::SearchForward => Some('/'),
            Self::SearchBackward => Some('?'),
            Self::Ex => Some(':'),
        }
    }

    /// Is this a search prompt (either direction)?
    #[must_use]
    pub const fn is_search(self) -> bool {
        matches!(self, Self::SearchForward | Self::SearchBackward)
    }
}

/// Everything a status line needs, borrowed from the editor state.
///
/// Borrowed rather than owned so building it costs nothing per frame — a face
/// may call this every redraw.
#[derive(Debug, Clone, Copy)]
pub struct StatusModel<'a> {
    pub mode: Mode,
    /// 1-based, display-ready.
    pub line: usize,
    /// 1-based, display-ready.
    pub column: usize,
    pub prompt: PromptKind,
    /// What has been typed into the prompt, without the sigil.
    pub prompt_text: &'a str,
    /// `[3/17]`. [`MatchCount::Idle`] when there is nothing to say.
    pub count: MatchCount,
    /// The newest message — vim's `E486` and friends. These were written to
    /// `EditorState::messages` from the start and had no reader outside the
    /// `eval` CLI's `println!`, so a failed search was indistinguishable from
    /// a dropped keystroke on both interactive faces.
    pub message: Option<&'a str>,
}

impl StatusModel<'_> {
    /// Render the prompt segment (`/foo`) into `out`. Empty when no prompt is
    /// open.
    pub fn render_prompt_into(self, out: &mut String) {
        if let Some(sigil) = self.prompt.sigil() {
            out.push(sigil);
            out.push_str(self.prompt_text);
        }
    }

    /// Render the whole line the way a plain-text face wants it:
    /// `-- NORMAL --  3:1  [2/7]  /foo` with the message last.
    ///
    /// A face with real layout (the GPU one) reads the fields instead; this is
    /// for the text/TUI paths and for tests, which want one string to assert
    /// against.
    #[must_use]
    pub fn render(self) -> String {
        let mut out = String::with_capacity(64);
        out.push_str(self.mode.as_str());
        out.push_str("  ");
        push_usize(&mut out, self.line);
        out.push(':');
        push_usize(&mut out, self.column);

        if !self.count.is_idle() {
            out.push_str("  ");
            self.count.render_into(&mut out);
        }

        if self.prompt.sigil().is_some() {
            out.push_str("  ");
            self.render_prompt_into(&mut out);
        }

        if let Some(msg) = self.message {
            out.push_str("  ");
            out.push_str(msg);
        }

        out
    }
}

/// Decimal-append without `format!`.
fn push_usize(out: &mut String, mut n: usize) {
    if n == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + u8::try_from(n % 10).unwrap_or(0);
        n /= 10;
    }
    out.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model<'a>(
        prompt: PromptKind,
        text: &'a str,
        count: MatchCount,
        msg: Option<&'a str>,
    ) -> StatusModel<'a> {
        StatusModel {
            mode: Mode::Normal,
            line: 3,
            column: 1,
            prompt,
            prompt_text: text,
            count,
            message: msg,
        }
    }

    #[test]
    fn every_prompt_kind_has_the_sigil_a_user_expects() {
        assert_eq!(PromptKind::None.sigil(), None);
        assert_eq!(PromptKind::SearchForward.sigil(), Some('/'));
        assert_eq!(PromptKind::SearchBackward.sigil(), Some('?'));
        assert_eq!(PromptKind::Ex.sigil(), Some(':'));
        assert!(PromptKind::SearchForward.is_search());
        assert!(PromptKind::SearchBackward.is_search());
        assert!(!PromptKind::Ex.is_search());
    }

    #[test]
    fn a_search_prompt_renders_its_pattern() {
        // The regression this module exists for: the prompt must appear.
        let s = model(PromptKind::SearchForward, "foo", MatchCount::Idle, None).render();
        assert!(s.contains("/foo"), "prompt missing from {s:?}");

        let mut only = String::new();
        model(PromptKind::SearchBackward, "bar", MatchCount::Idle, None)
            .render_prompt_into(&mut only);
        assert_eq!(only, "?bar");
    }

    #[test]
    fn no_prompt_renders_no_sigil() {
        let mut out = String::new();
        model(PromptKind::None, "", MatchCount::Idle, None).render_prompt_into(&mut out);
        assert!(out.is_empty(), "got {out:?}");
    }

    #[test]
    fn the_count_is_shown_and_idle_is_silent() {
        let s = model(
            PromptKind::None,
            "",
            MatchCount::Exact {
                current: 2,
                total: 7,
            },
            None,
        )
        .render();
        assert!(s.contains("[2/7]"), "{s}");

        let idle = model(PromptKind::None, "", MatchCount::Idle, None).render();
        assert!(!idle.contains('['), "idle must draw nothing: {idle}");
    }

    #[test]
    fn zero_matches_says_so_rather_than_going_quiet() {
        let s = model(PromptKind::SearchForward, "zzz", MatchCount::None, None).render();
        assert!(s.contains("[0/0]"), "{s}");
    }

    #[test]
    fn a_capped_count_is_marked_as_capped() {
        let s = model(
            PromptKind::None,
            "",
            MatchCount::Capped { current: 5 },
            None,
        )
        .render();
        assert!(s.contains("[5/>99]"), "{s}");
    }

    #[test]
    fn the_newest_message_reaches_the_line() {
        let s = model(
            PromptKind::None,
            "",
            MatchCount::Idle,
            Some("E486: Pattern not found: zzz"),
        )
        .render();
        assert!(s.contains("E486"), "{s}");
    }

    #[test]
    fn mode_and_position_are_one_based() {
        let s = model(PromptKind::None, "", MatchCount::Idle, None).render();
        assert!(s.starts_with("NORMAL"), "{s}");
        assert!(s.contains("3:1"), "{s}");
    }

    #[test]
    fn large_numbers_render_correctly_without_format() {
        let m = StatusModel {
            mode: Mode::Insert,
            line: 12_345,
            column: 678,
            prompt: PromptKind::None,
            prompt_text: "",
            count: MatchCount::Exact {
                current: 10,
                total: 99,
            },
            message: None,
        };
        let s = m.render();
        assert!(s.contains("12345:678"), "{s}");
        assert!(s.contains("[10/99]"), "{s}");
    }
}
