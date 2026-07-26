//! `escriba-ui` — Layout, Window, Viewport, StatusLine. Pure state; rendering lives in escriba-render.

extern crate self as escriba_ui;

/// Theme → concrete chrome colors. The ONE place a `FleetTheme` becomes
/// paintable values, shared by the TUI and GPU renderers so they cannot
/// drift apart (or off the fleet baseline) again.
pub mod chrome;

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
    /// Scroll both axes so `p` is visible within this viewport, keeping at
    /// least `margin` lines/columns of context on each edge where room
    /// allows. The same `margin` applies to both axes. All arithmetic is
    /// saturating, so the viewport never scrolls past `0` and a position
    /// at the very top/left is clamped flush to the origin.
    ///
    /// Pairing this with the editor's single cursor-mutation path makes
    /// "cursor outside its viewport" an unrepresentable state: every move
    /// re-derives the viewport from the (clamped) cursor.
    #[must_use]
    pub fn scroll_to_contain(mut self, p: Position, margin: u32) -> Self {
        // ── Vertical axis (top_line / visible_lines). ──
        let bot = p.line.saturating_add(margin);
        if p.line < self.top_line {
            self.top_line = p.line.saturating_sub(margin);
        }
        if bot >= self.top_line.saturating_add(self.visible_lines) {
            self.top_line = bot.saturating_sub(self.visible_lines.saturating_sub(1));
        }

        // ── Horizontal axis (left_column / visible_columns). ──
        let right = p.column.saturating_add(margin);
        if p.column < self.left_column {
            self.left_column = p.column.saturating_sub(margin);
        }
        if right >= self.left_column.saturating_add(self.visible_columns) {
            self.left_column = right.saturating_sub(self.visible_columns.saturating_sub(1));
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

    fn small() -> Viewport {
        // 5 visible lines × 10 visible columns — a tight window so the
        // scroll-to-contain logic is exercised on small inputs.
        Viewport {
            top_line: 0,
            left_column: 0,
            visible_lines: 5,
            visible_columns: 10,
        }
    }

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
    fn scroll_noop_when_already_visible() {
        let v = small();
        // (line 2, col 4) is well inside a 0..5 × 0..10 window.
        let v2 = v.scroll_to_contain(Position::new(2, 4), 2);
        assert_eq!(v2, v, "an in-window position must not move the viewport");
    }

    #[test]
    fn scroll_down_keeps_cursor_visible() {
        let v = small();
        let v2 = v.scroll_to_contain(Position::new(30, 0), 2);
        assert!(
            v2.top_line <= 30 && 30 < v2.top_line + v2.visible_lines,
            "cursor line must be within [top_line, top_line+visible_lines): {v2:?}"
        );
    }

    #[test]
    fn scroll_right_keeps_cursor_visible() {
        let v = small();
        // Cursor past the right edge of a 10-wide window.
        let v2 = v.scroll_to_contain(Position::new(0, 50), 2);
        assert!(
            v2.left_column <= 50 && 50 < v2.left_column + v2.visible_columns,
            "cursor column must be within [left_column, left_column+visible_columns): {v2:?}"
        );
    }

    #[test]
    fn scroll_up_left_returns_toward_origin() {
        // Start scrolled away from the origin, then ask for a position
        // above and to the left of the current window.
        let v = Viewport {
            top_line: 20,
            left_column: 30,
            visible_lines: 5,
            visible_columns: 10,
        };
        let v2 = v.scroll_to_contain(Position::new(2, 3), 2);
        assert!(v2.top_line <= 2, "must scroll up to reveal line 2: {v2:?}");
        assert!(v2.left_column <= 3, "must scroll left to reveal col 3: {v2:?}");
    }

    #[test]
    fn scroll_saturates_at_origin() {
        // Position (0,0) with a margin can't push the viewport negative.
        let v = small();
        let v2 = v.scroll_to_contain(Position::ZERO, 2);
        assert_eq!(v2.top_line, 0);
        assert_eq!(v2.left_column, 0);
    }

    #[test]
    fn scroll_respects_margin_on_both_axes() {
        // From the origin, jump just past the bottom-right corner; both
        // axes should leave `margin` of context past the cursor where the
        // window size allows.
        let v = small(); // 5 lines, 10 cols
        let v2 = v.scroll_to_contain(Position::new(4, 9), 2);
        // bot = line+margin = 6 >= top+visible(5) → top = 6 - 4 = 2
        assert_eq!(v2.top_line, 2, "{v2:?}");
        // right = col+margin = 11 >= left+visible(10) → left = 11 - 9 = 2
        assert_eq!(v2.left_column, 2, "{v2:?}");
        // and the cursor is still inside the window
        assert!(v2.top_line <= 4 && 4 < v2.top_line + v2.visible_lines);
        assert!(v2.left_column <= 9 && 9 < v2.left_column + v2.visible_columns);
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
