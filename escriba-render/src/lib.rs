//! `escriba-render` — rendering trait + phase-1 text implementation.
//! GPU-backed implementation (garasu) shares the [`Renderer`] trait.

extern crate self as escriba_render;

use escriba_buffer::BufferSet;
use escriba_core::Position;
use escriba_ui::Layout;
use irodori::NORD;
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
        let height = win.viewport.visible_lines.max(10);
        for row in 0..height {
            let ln = top + row;
            if ln >= buf.line_count() {
                break;
            }
            let line = buf.line(ln).unwrap_or_default();
            let line = line.trim_end_matches('\n').trim_end_matches('\r');
            if ln == cursor.line {
                let col = cursor.column as usize;
                let (before, after) = split_at_char(line, col);
                out.push_str(&format!(
                    "{:4} │ {before}\x1b[7m{}\x1b[0m{rest}\n",
                    ln + 1,
                    after.chars().next().unwrap_or(' '),
                    rest = after.chars().skip(1).collect::<String>(),
                ));
            } else {
                out.push_str(&format!("{:4} │ {line}\n", ln + 1));
            }
        }
        let n = NORD.frost[1]; // Nord8
        out.push_str(&format!(
            "\x1b[7m escriba · {} · {}:{} \x1b[0m\n",
            n.to_hex(),
            cursor.line + 1,
            cursor.column + 1,
        ));
        out
    }
}

fn split_at_char(line: &str, col: usize) -> (&str, &str) {
    let idx = line.char_indices().nth(col).map_or(line.len(), |(b, _)| b);
    line.split_at(idx)
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
