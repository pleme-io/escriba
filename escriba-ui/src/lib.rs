//! `escriba-ui` — Layout, Window, Viewport, StatusLine. Pure state; rendering lives in escriba-render.

extern crate self as escriba_ui;

/// Theme → concrete chrome colors. The ONE place a `FleetTheme` becomes
/// paintable values, shared by the TUI and GPU renderers so they cannot
/// drift apart (or off the fleet baseline) again.
pub mod chrome;

/// The start screen — one laid-out model every face paints, rather than
/// three copies of the same centering arithmetic.
pub mod splash;

/// Syntax colours resolved through ishou, so a theme change recolours the
/// CODE and not just the frame. hikari ships one hardcoded Nord table; this
/// reproduces it on the fleet default and extends it to every theme.
pub mod syntax;

/// 仕切り — the container tree behind `:sp` / `:vsp`. Pane geometry is
/// DERIVED by `solve(tree, frame)`; scroll position stays on the window.
pub mod shikiri;

/// The picker — a filtered list of candidates that holds keys while open.
/// The narrowing machine is `egaku::FuzzyPicker`; escriba owns the SOURCE
/// (what accepting means) and the key translation.
pub mod picker;

/// The gutter — line numbers and finding marks, composed once. The ratatui
/// face built its own inline and the GPU face had none at all; this is the
/// same "one model, N faces" repair the status line and splash already had.
pub mod gutter;

use escriba_core::{BufferId, Position, WindowId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

/// A window lives in the tree that owns it. Re-exported so the paths every
/// face already uses keep working.
pub use shikiri::Window;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Layout {
    /// The container tree. Owns every window; there is no second collection
    /// that could disagree with it about which windows exist.
    tree: shikiri::Shikiri,
    /// The focused window.
    active: WindowId,
    /// The last frame a FACE reported, in cells. The only retained size in
    /// the model — every pane rect is derived from it by `solve`.
    frame: shikiri::Rect,
    next_id: u64,
    pub statusline: bool,
    pub tabbar: bool,
}

impl Layout {
    #[must_use]
    pub fn single(window: Window) -> Self {
        Self {
            active: window.id,
            next_id: window.id.0 + 1,
            tree: shikiri::Shikiri::Pane(window),
            frame: shikiri::Rect::default(),
            statusline: true,
            tabbar: true,
        }
    }

    #[must_use]
    pub const fn active(&self) -> WindowId {
        self.active
    }

    /// Focus `id` if it is in the tree. Returns whether it moved.
    pub fn focus(&mut self, id: WindowId) -> bool {
        if self.windows().any(|w| w.id == id) {
            self.active = id;
            true
        } else {
            false
        }
    }

    /// Tell the layout how big its frame is. A face calls this; nothing else
    /// stores a size.
    pub fn set_frame(&mut self, frame: shikiri::Rect) {
        self.frame = frame;
    }

    #[must_use]
    pub const fn frame(&self) -> shikiri::Rect {
        self.frame
    }

    /// Pane geometry, DERIVED. Never stored, so it cannot go stale.
    #[must_use]
    pub fn solved(&self) -> shikiri::Solved {
        shikiri::solve(&self.tree, self.frame)
    }

    /// Every window, in layout order.
    pub fn windows(&self) -> impl Iterator<Item = &Window> {
        fn walk<'a>(n: &'a shikiri::Shikiri, out: &mut Vec<&'a Window>) {
            match n {
                shikiri::Shikiri::Pane(w) => out.push(w),
                shikiri::Shikiri::Split(s) => s.children().for_each(|c| walk(c, out)),
            }
        }
        let mut v = Vec::new();
        walk(&self.tree, &mut v);
        v.into_iter()
    }

    /// Every window, mutably. Used to push per-pane viewport sizes down.
    pub fn windows_mut(&mut self) -> Vec<&mut Window> {
        fn walk<'a>(n: &'a mut shikiri::Shikiri, out: &mut Vec<&'a mut Window>) {
            match n {
                shikiri::Shikiri::Pane(w) => out.push(w),
                shikiri::Shikiri::Split(s) => s.children_mut().for_each(|c| walk(c, out)),
            }
        }
        let mut v = Vec::new();
        walk(&mut self.tree, &mut v);
        v
    }

    #[must_use]
    pub fn active_window(&self) -> Option<&Window> {
        let id = self.active;
        self.windows().find(|w| w.id == id)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut Window> {
        let id = self.active;
        self.windows_mut().into_iter().find(|w| w.id == id)
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.windows().count()
    }

    /// Split the ACTIVE window along `axis`, showing the same buffer.
    ///
    /// The new window is focused and returned. vim's default is
    /// `splitbelow`/`splitright` OFF, so the NEW window goes above (`:sp`) or
    /// left (`:vsp`) — it becomes the FIRST child. Scroll position is copied,
    /// so both panes start showing the same thing, which is what makes `:sp`
    /// feel like "look at this file in two places" rather than a jump.
    pub fn split_active(&mut self, axis: shikiri::Axis) -> WindowId {
        let id = WindowId(self.next_id);
        self.next_id += 1;
        let active = self.active;
        let fresh = self
            .active_window()
            .map(|w| Window {
                id,
                buffer_id: w.buffer_id,
                viewport: w.viewport,
            })
            .unwrap_or(Window {
                id,
                buffer_id: BufferId(0),
                viewport: Viewport::default(),
            });
        split_at(&mut self.tree, active, axis, fresh);
        self.active = id;
        id
    }

    /// Close `id`, collapsing its parent. The LAST window never closes —
    /// vim refuses too ("E444: Cannot close last window").
    pub fn close(&mut self, id: WindowId) -> bool {
        if self.count() <= 1 {
            return false;
        }
        // Focus a survivor BEFORE removing, so `active` is never dangling.
        if self.active == id {
            let next = self
                .windows()
                .map(|w| w.id)
                .find(|w| *w != id)
                .unwrap_or(id);
            self.active = next;
        }
        remove(&mut self.tree, id)
    }

    /// The window nearest the active one in `dir` — `<C-w>hjkl`.
    ///
    /// Geometric, not tree-structural: it compares SOLVED rects, so the
    /// answer is what the operator sees rather than an artefact of which
    /// split happened first.
    #[must_use]
    pub fn neighbour(&self, dir: Dir) -> Option<WindowId> {
        let solved = self.solved();
        let here = solved.rect_of(self.active)?;
        solved
            .panes
            .iter()
            .filter(|(id, _)| *id != self.active)
            .filter(|(_, r)| match dir {
                Dir::Left => r.x + r.w <= here.x,
                Dir::Right => r.x >= here.x + here.w,
                Dir::Up => r.y + r.h <= here.y,
                Dir::Down => r.y >= here.y + here.h,
            })
            // Nearest along the axis of travel, then nearest across it, so a
            // column of stacked panes picks the one beside the cursor rather
            // than whichever the tree happens to list first.
            .min_by_key(|(_, r)| match dir {
                Dir::Left => (here.x.saturating_sub(r.x + r.w), here.y.abs_diff(r.y)),
                Dir::Right => (r.x.saturating_sub(here.x + here.w), here.y.abs_diff(r.y)),
                Dir::Up => (here.y.saturating_sub(r.y + r.h), here.x.abs_diff(r.x)),
                Dir::Down => (r.y.saturating_sub(here.y + here.h), here.x.abs_diff(r.x)),
            })
            .map(|(id, _)| *id)
    }
}

/// A direction to move focus in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Replace the pane holding `target` with a split of `(fresh, that pane)`.
fn split_at(
    node: &mut shikiri::Shikiri,
    target: WindowId,
    axis: shikiri::Axis,
    fresh: Window,
) -> bool {
    match node {
        shikiri::Shikiri::Pane(w) if w.id == target => {
            let existing = std::mem::replace(node, shikiri::Shikiri::Pane(fresh.clone()));
            *node = shikiri::Shikiri::Split(shikiri::Split::new(
                axis,
                shikiri::Shikiri::Pane(fresh),
                existing,
            ));
            true
        }
        shikiri::Shikiri::Pane(_) => false,
        shikiri::Shikiri::Split(s) => s
            .children_mut()
            .any(|c| split_at(c, target, axis, fresh.clone())),
    }
}

/// Remove the pane holding `id`, collapsing a split left with one child.
fn remove(node: &mut shikiri::Shikiri, id: WindowId) -> bool {
    let shikiri::Shikiri::Split(s) = node else {
        return false;
    };
    if let Some(rest) = s.without(id) {
        *node = rest;
        return true;
    }
    let shikiri::Shikiri::Split(s) = node else {
        return false;
    };
    s.children_mut().any(|c| remove(c, id))
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
        assert!(
            v2.left_column <= 3,
            "must scroll left to reveal col 3: {v2:?}"
        );
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
        };
        let layout = Layout::single(w);
        assert_eq!(layout.active_window().unwrap().id, WindowId(1));
    }
}
