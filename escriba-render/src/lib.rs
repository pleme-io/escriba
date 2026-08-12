//! `escriba-render` — rendering trait + implementations.
//!
//! Two backends, same trait surface:
//!   - [`TextRenderer`] — ANSI-in-stdout for CI / headless runs.
//!   - [`gpu::GpuRenderer`] — madori + garasu + glyphon real GPU window.
//!     Implements [`madori::RenderCallback`]; the escriba binary pairs it
//!     with an `on_event` handler that shares an `Arc<Mutex<EditorState>>`.

extern crate self as escriba_render;

pub mod gpu;
/// Escriba-local language tables moved to `escriba-ts` with the ecosystem
/// they register into. Re-exported so existing paths keep working.
pub use escriba_ts::langs;
/// The start screen, painted as ANSI. Layout comes from
/// `escriba_ui::splash`; this face only colors it.
pub mod splash;

pub use gpu::{GpuRenderer, SharedState};
pub use splash::render_splash_ansi;

use escriba_runtime::EditorState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderTarget {
    Text,
    Gpu,
}

pub trait Renderer {
    /// Paint one frame of `state`.
    ///
    /// Takes the whole editor rather than `(layout, buffers, cursor)`. Three
    /// parameters meant three chances to hand this face something that did
    /// not match the others — and it happened: the binary passed a literal
    /// `Position::ZERO`, so every `--render=text` dump drew the cursor at 1:1
    /// no matter where the cursor actually was.
    fn render_frame(&mut self, state: &EditorState) -> String;
}

pub struct TextRenderer;

impl Renderer for TextRenderer {
    fn render_frame(&mut self, state: &EditorState) -> String {
        let Some(win) = state.layout.active_window() else {
            return "<no window>\n".to_string();
        };
        let Some(buf) = state.buffers.get(win.buffer_id) else {
            return "<no buffer>\n".to_string();
        };
        // The operator's theme, through the ONE seam. This face used to reach
        // for `VellumPalette::vellum()` directly — the exact hardwiring
        // `ChromePalette` exists to remove, missed because escriba's notes
        // named only two faces and there are three.
        let chrome = state.chrome();
        let cursor = state.cursor();
        let world = state.world();
        let line_count = buf.line_count();
        // The shared gutter model, so this face's columns match the ratatui
        // and GPU faces. It had its own seven-column spelling with no mark
        // cell, which is how a third face drifts without anyone deciding to.
        let gutter_cols = escriba_ui::gutter::gutter_width(line_count);

        let mut out = String::new();
        let top = win.viewport.top_line;
        let left = win.viewport.left_column as usize;
        let vis_cols = (win.viewport.visible_columns as usize).saturating_sub(gutter_cols);
        let height = win.viewport.visible_lines.max(10);
        for row in 0..height {
            let ln = top + row;
            if ln >= line_count {
                break;
            }
            let line = buf.line(ln).unwrap_or_default();
            let line = line.trim_end_matches('\n').trim_end_matches('\r');
            // Both gutter planes, asked of the STATE — see
            // `EditorState::gutter_marks`. Reading `worst_on_line` here (what
            // this face used to do) is how a second plane lands on two faces
            // out of three.
            let marks = state.gutter_marks(&world, state.active, ln);
            for cell in escriba_ui::gutter::gutter_cells(ln, marks, line_count) {
                match cell.role {
                    escriba_ui::gutter::GutterRole::Mark(sev) => {
                        push_fg(&mut out, escriba_ui::chrome::severity_color(&chrome, sev));
                        out.push_str(&cell.text);
                        out.push_str(RESET);
                    }
                    escriba_ui::gutter::GutterRole::Breakpoint => {
                        push_fg(&mut out, escriba_ui::chrome::breakpoint_color(&chrome));
                        out.push_str(&cell.text);
                        out.push_str(RESET);
                    }
                    _ => out.push_str(&cell.text),
                }
            }
            // Slice the line to the visible horizontal window
            // `[left, left + vis_cols)` — char-based so multibyte text stays
            // aligned. The cursor's on-screen column is computed relative to
            // `left` so the cursor glyph tracks the horizontal scroll.
            let visible: Vec<char> = line.chars().skip(left).take(vis_cols).collect();
            if ln == cursor.line && cursor.column as usize >= left {
                let rel = cursor.column as usize - left;
                out.extend(visible.iter().take(rel));
                out.push_str(INVERT);
                out.push(visible.get(rel).copied().unwrap_or(' '));
                out.push_str(RESET);
                out.extend(visible.iter().skip(rel + 1));
            } else {
                out.extend(visible.iter());
            }
            out.push('\n');
        }
        out.push_str(INVERT);
        out.push_str(" escriba · ");
        out.push_str(&chrome.info.hex());
        out.push_str(" · ");
        out.push_str(&(cursor.line + 1).to_string());
        out.push(':');
        out.push_str(&(cursor.column + 1).to_string());
        out.push(' ');
        out.push_str(RESET);
        out.push('\n');
        out
    }
}

/// Reverse video on / all attributes off. Named rather than inlined so the
/// escape sequences appear once each.
const INVERT: &str = "\x1b[7m";
const RESET: &str = "\x1b[0m";

/// Append an SGR truecolor foreground set for `c`.
///
/// `write!` into a `String`, not `format!` — ★★ TYPED EMISSION. The
/// `Display` impls of the three `u8`s are the typed surface; the alternative
/// this replaces built ANSI by string interpolation.
fn push_fg(out: &mut String, c: ishou_tokens::Rgb) {
    use std::fmt::Write as _;
    // Writing into a String is infallible; the Result exists only because
    // `fmt::Write` is shared with fallible sinks.
    let _ = write!(out, "\x1b[38;2;{};{};{}m", c.r, c.g, c.b);
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_buffer::BufferSet;

    /// An editor holding `text`, with a viewport wide enough that nothing is
    /// clipped. Built through `EditorState` rather than a hand-assembled
    /// `Layout` — the point of the signature change is that this face reads
    /// ONE object, so a test that constructed a second one would be testing
    /// a configuration the binary can never produce.
    fn editor(text: &str) -> EditorState {
        let mut bufs = BufferSet::new();
        let id = bufs.scratch(text);
        let mut st = EditorState::new_with_buffer(bufs, id);
        st.dismiss_splash();
        if let Some(w) = st.layout.active_window_mut() {
            w.viewport.visible_lines = 20;
            w.viewport.visible_columns = 80;
        }
        st
    }

    #[test]
    fn renders_buffer_lines() {
        // The cursor sits on line 3, so lines 1 and 2 render unbroken. On
        // the line it occupies, the cursor cell is wrapped in an SGR pair,
        // which splits the word — that is correct output, not a defect, and
        // asserting on a word under the cursor tests the escape sequence
        // rather than the text.
        let mut st = editor("hello\nworld\nfoo");
        st.on_key(&escriba_keymap::Key::Char('j'));
        st.on_key(&escriba_keymap::Key::Char('j'));
        let frame = TextRenderer.render_frame(&st);
        assert!(frame.contains("hello"), "{frame:?}");
        assert!(frame.contains("world"), "{frame:?}");
    }

    #[test]
    fn cursor_is_highlighted() {
        let frame = TextRenderer.render_frame(&editor("hello world"));
        assert!(frame.contains(INVERT), "{frame:?}");
    }

    #[test]
    fn the_cursor_is_drawn_where_the_editor_actually_has_it() {
        // The bug the signature change fixed: the binary passed a literal
        // `Position::ZERO`, so this face drew the cursor at 1:1 for every
        // dump regardless of the real position — and the status line printed
        // "1:1" to match, which made the report self-consistent and wrong.
        let mut st = editor("alpha\nbravo\ncharlie\n");
        st.on_key(&escriba_keymap::Key::Char('j'));
        st.on_key(&escriba_keymap::Key::Char('j'));
        assert_eq!(st.cursor().line, 2, "precondition: the cursor moved");
        let frame = TextRenderer.render_frame(&st);
        assert!(
            frame.contains("3:1"),
            "the status line must report the REAL cursor: {frame:?}",
        );
        // And the inverted cell must sit on the third BODY line, not the
        // first. Counted over body lines only — the status line is itself
        // inverted, so including it would make any cursor position pass.
        let body: Vec<&str> = frame.lines().filter(|l| l.contains('\u{2502}')).collect();
        let cursor_row = body
            .iter()
            .position(|l| l.contains(INVERT))
            .expect("the cursor is painted on some line");
        assert_eq!(cursor_row, 2, "{frame:?}");
    }

    #[test]
    fn the_gutter_matches_the_shared_model() {
        // Three faces, one gutter. This face used to spell its own seven
        // columns with no mark cell.
        let st = editor("one\ntwo\n");
        let frame = TextRenderer.render_frame(&st);
        let first = frame.lines().next().expect("a line");
        let rule = first
            .chars()
            .position(|c| c == '\u{2502}')
            .expect("the gutter rule");
        assert_eq!(rule + 2, escriba_ui::gutter::gutter_width(2), "{first:?}");
    }

    #[test]
    fn the_status_line_reports_the_theme_the_editor_paints() {
        // Not `VellumPalette::vellum()`, which is what this face read before
        // and which is a theme escriba no longer defaults to.
        let st = editor("x");
        let frame = TextRenderer.render_frame(&st);
        assert!(
            frame.contains(&st.chrome().info.hex()),
            "{frame:?} should carry the editor's own accent",
        );
    }
}
