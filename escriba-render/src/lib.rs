//! `escriba-render` — rendering trait + implementations.
//!
//! Two backends, same trait surface:
//!   - [`TextRenderer`] — ANSI-in-stdout for CI / headless runs.
//!   - [`gpu::GpuRenderer`] — madori + garasu + glyphon real GPU window.
//!     Implements [`madori::RenderCallback`]; the escriba binary pairs it
//!     with an `on_event` handler that shares an `Arc<Mutex<EditorState>>`.

extern crate self as escriba_render;

pub mod gpu;

pub use gpu::{GpuRenderer, SharedState};

use escriba_buffer::BufferSet;
use escriba_core::Position;
use escriba_ui::Layout;
use ishou_tokens::VellumPalette;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderTarget {
    Text,
    Gpu,
}

pub trait Renderer {
    fn render_frame(&mut self, layout: &Layout, buffers: &BufferSet, cursor: Position) -> String;
}

pub struct TextRenderer;

impl Renderer for TextRenderer {
    fn render_frame(&mut self, layout: &Layout, buffers: &BufferSet, cursor: Position) -> String {
        let Some(win) = layout.active_window() else {
            return "<no window>\n".to_string();
        };
        let Some(buf) = buffers.get(win.buffer_id) else {
            return "<no buffer>\n".to_string();
        };
        let mut out = String::new();
        let top = win.viewport.top_line;
        let left = win.viewport.left_column as usize;
        let vis_cols = win.viewport.visible_columns as usize;
        let height = win.viewport.visible_lines.max(10);
        for row in 0..height {
            let ln = top + row;
            if ln >= buf.line_count() {
                break;
            }
            let line = buf.line(ln).unwrap_or_default();
            let line = line.trim_end_matches('\n').trim_end_matches('\r');
            // Slice the line to the visible horizontal window
            // `[left, left + vis_cols)` — char-based so multibyte text stays
            // aligned. The cursor's on-screen column is computed relative to
            // `left` so the cursor glyph tracks the horizontal scroll.
            let visible: Vec<char> = line.chars().skip(left).take(vis_cols).collect();
            if ln == cursor.line && cursor.column as usize >= left {
                let rel = cursor.column as usize - left;
                let before: String = visible.iter().take(rel).collect();
                let under = visible.get(rel).copied().unwrap_or(' ');
                let rest: String = visible.iter().skip(rel + 1).collect();
                out.push_str(&format!(
                    "{:4} │ {before}\x1b[7m{under}\x1b[0m{rest}\n",
                    ln + 1,
                ));
            } else {
                let visible: String = visible.iter().collect();
                out.push_str(&format!("{:4} │ {visible}\n", ln + 1));
            }
        }
        let accent = VellumPalette::vellum().ice_cyan; // Vellum matte accent
        out.push_str(&format!(
            "\x1b[7m escriba · {} · {}:{} \x1b[0m\n",
            accent.hex(),
            cursor.line + 1,
            cursor.column + 1,
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_buffer::BufferSet;
    use escriba_core::{BufferId, WindowId};
    use escriba_ui::{Layout, Rect, Viewport, Window};

    #[test]
    fn renders_buffer_lines() {
        let mut bufs = BufferSet::new();
        let id = bufs.scratch("hello\nworld\nfoo");
        let _ = BufferId::default();
        let layout = Layout::single(Window {
            id: WindowId(1),
            buffer_id: id,
            viewport: Viewport {
                top_line: 0,
                left_column: 0,
                visible_lines: 20,
                visible_columns: 80,
            },
            rect: Rect::default(),
        });
        let mut r = TextRenderer;
        // Cursor on line 2 so other lines render without ANSI splitting "hello".
        let frame = r.render_frame(&layout, &bufs, Position::new(2, 0));
        assert!(frame.contains("hello"));
        assert!(frame.contains("world"));
    }

    #[test]
    fn cursor_is_highlighted() {
        let mut bufs = BufferSet::new();
        let id = bufs.scratch("hello world");
        let layout = Layout::single(Window {
            id: WindowId(1),
            buffer_id: id,
            viewport: Viewport {
                top_line: 0,
                left_column: 0,
                visible_lines: 10,
                visible_columns: 80,
            },
            rect: Rect::default(),
        });
        let mut r = TextRenderer;
        let frame = r.render_frame(&layout, &bufs, Position::new(0, 3));
        assert!(frame.contains("\x1b[7m"));
    }
}
