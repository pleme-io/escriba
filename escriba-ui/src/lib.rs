//! `escriba-ui` — Layout, Window, Viewport, StatusLine. Pure state; rendering lives in escriba-render.

extern crate self as escriba_ui;

use escriba_core::{BufferId, Position, WindowId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Viewport {
    pub top_line: u32,
    pub left_column: u32,
    pub visible_lines: u32,
    pub visible_columns: u32,
}

impl Viewport {
    #[must_use]
    pub fn scroll_to_contain(mut self, p: Position, margin: u32) -> Self {
        let bot = p.line.saturating_add(margin);
        if p.line < self.top_line {
            self.top_line = p.line.saturating_sub(margin);
        }
        if bot >= self.top_line.saturating_add(self.visible_lines) {
            self.top_line = bot.saturating_sub(self.visible_lines.saturating_sub(1));
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Window {
    pub id: WindowId,
    pub buffer_id: BufferId,
    pub viewport: Viewport,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Layout {
    pub windows: Vec<Window>,
    pub active: WindowId,
    pub statusline: bool,
    pub tabbar: bool,
}

impl Layout {
    #[must_use]
    pub fn single(window: Window) -> Self {
        Self {
            active: window.id,
            windows: vec![window],
            statusline: true,
            tabbar: true,
        }
    }

    #[must_use]
    pub fn active_window(&self) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == self.active)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct StatusLine {
    pub mode: String,
    pub path: Option<String>,
    pub cursor: Position,
    pub modified: bool,
    pub line_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_scrolls_down() {
        let v = Viewport {
            top_line: 0,
            left_column: 0,
            visible_lines: 20,
            visible_columns: 80,
        };
        let v2 = v.scroll_to_contain(Position::new(30, 0), 2);
        assert!(v2.top_line > 0);
    }

    #[test]
    fn layout_active_resolves() {
        let w = Window {
            id: WindowId(1),
            buffer_id: BufferId(1),
            viewport: Viewport::default(),
            rect: Rect::default(),
        };
        let layout = Layout::single(w);
        assert_eq!(layout.active_window().unwrap().id, WindowId(1));
    }
}
