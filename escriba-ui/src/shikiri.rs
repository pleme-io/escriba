//! 仕切り — *shikiri*, "partition". The container tree behind `:sp` / `:vsp`.
//!
//! ## Why a tree and not `egaku::SplitPane`
//!
//! The fleet rule is extend a near-miss rather than build fresh, and
//! `egaku::SplitPane` looks like one. It is not, for two reasons that are
//! about the ALGEBRA rather than about effort:
//!
//! - it holds exactly **two** panes and cannot nest, while `:sp` `:sp` `:sp`
//!   is the ordinary case;
//! - its rects are `f32` (`egaku::layout::Rect`), while a terminal frame is
//!   an **integer cell grid** whose panes and separators must sum to the
//!   frame EXACTLY. Float geometry reintroduces the off-by-one class that
//!   `Window.rect` was deleted for.
//!
//! escriba consumes egaku where egaku's machine is the right one — the
//! picker rides `egaku::FuzzyPicker` and always should. This is a different
//! algebra, and saying so is not the same as reinventing.
//!
//! ## What is derived and what is retained
//!
//! **Pane geometry is derived**; scroll position is retained. That line was
//! settled by two earlier repairs and is not relitigated here: `Window.rect`
//! was deleted (six writes, zero reads, and its writers disagreed about
//! pixels vs cells), and `sync_viewport` established that a face reports its
//! size while the runtime keeps the cursor inside it.
//!
//! So there are no stored pane sizes. `solve(tree, frame)` computes them
//! every time, which makes vim's `equalalways` re-equalisation not a step
//! anyone can forget — it is what "no sizes are stored" MEANS.

use escriba_core::{BufferId, WindowId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Viewport;

/// A rectangle in terminal CELLS.
///
/// Integers, deliberately. The panes plus their separators must tile the
/// frame with no gap and no overlap, which float arithmetic cannot promise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    #[must_use]
    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub const fn area(&self) -> u32 {
        self.w as u32 * self.h as u32
    }

    /// Does this rect share any cell with `other`?
    #[must_use]
    pub const fn overlaps(&self, other: &Self) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }
}

/// Which way a split's children are laid out.
///
/// Named by WHAT IT DOES rather than "horizontal"/"vertical", because every
/// codebase using those two words gets them backwards at least once: vim
/// calls `:sp` a *horizontal* split and it stacks its children *vertically*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Axis {
    /// `:sp` — children stacked top to bottom.
    Stacked,
    /// `:vsp` — children side by side.
    SideBySide,
}

/// One node of the container tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Shikiri {
    /// A leaf, which OWNS its window — there is no second collection that
    /// could disagree with the tree about which windows exist.
    Pane(Window),
    Split(Split),
}

/// A window: which buffer, and where the reader is inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Window {
    pub id: WindowId,
    pub buffer_id: BufferId,
    /// SCROLL POSITION. Retained, because no function of `(tree, frame)` can
    /// compute where a reader has scrolled to.
    pub viewport: Viewport,
}

/// A split of two-or-more children along one axis.
///
/// The `(first, second, rest)` encoding is load-bearing. A `Vec` would let a
/// split hold ONE child, which is exactly what closing a window produces if
/// the collapse is forgotten — and that bug renders *correctly*, because a
/// one-child split solves to its parent's whole rect. It would surface as
/// wrong geometry several operations later. Here it does not compile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Split {
    pub axis: Axis,
    first: Box<Shikiri>,
    second: Box<Shikiri>,
    /// Third and subsequent children.
    rest: Vec<Shikiri>,
}

impl Split {
    #[must_use]
    pub fn new(axis: Axis, first: Shikiri, second: Shikiri) -> Self {
        Self {
            axis,
            first: Box::new(first),
            second: Box::new(second),
            rest: Vec::new(),
        }
    }

    /// Children in layout order — the ONLY way to read them, so no call site
    /// can walk `rest` while forgetting `first` and `second`.
    pub fn children(&self) -> impl Iterator<Item = &Shikiri> {
        std::iter::once(self.first.as_ref())
            .chain(std::iter::once(self.second.as_ref()))
            .chain(self.rest.iter())
    }

    pub fn children_mut(&mut self) -> impl Iterator<Item = &mut Shikiri> {
        std::iter::once(self.first.as_mut())
            .chain(std::iter::once(self.second.as_mut()))
            .chain(self.rest.iter_mut())
    }

    /// Always at least 2.
    #[must_use]
    pub fn len(&self) -> usize {
        2 + self.rest.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        false // a Split has two children by construction
    }

    /// Add a child at the end.
    pub fn push(&mut self, child: Shikiri) {
        self.rest.push(child);
    }
}

/// A one-cell separator between two siblings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub axis: Axis,
    pub rect: Rect,
}

/// The laid-out result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Solved {
    /// Exactly one entry per leaf, in tree order.
    pub panes: Vec<(WindowId, Rect)>,
    pub rules: Vec<Rule>,
}

impl Solved {
    /// The rect for `id`, if it is in the tree.
    #[must_use]
    pub fn rect_of(&self, id: WindowId) -> Option<Rect> {
        self.panes.iter().find(|(w, _)| *w == id).map(|(_, r)| *r)
    }
}

/// Lay `tree` out inside `frame`. Total and pure.
///
/// A split of `n` children spends `n - 1` cells of its axis on separators
/// and divides the remainder equally, giving the remainder cells to the
/// LEADING children — vim's shape, where the first pane is the wider one.
///
/// **Degradation, stated rather than hidden:** a frame too small to hold
/// every pane yields zero-area rects for the trailing children. Callers skip
/// a zero-area pane rather than painting into it. This is a real limit, not
/// an invariant — rounding it up to "cannot happen" would be false.
#[must_use]
pub fn solve(tree: &Shikiri, frame: Rect) -> Solved {
    let mut out = Solved::default();
    lay(tree, frame, &mut out);
    out
}

fn lay(node: &Shikiri, frame: Rect, out: &mut Solved) {
    match node {
        Shikiri::Pane(w) => out.panes.push((w.id, frame)),
        Shikiri::Split(s) => {
            let n = u16::try_from(s.len()).unwrap_or(u16::MAX);
            let rules = n.saturating_sub(1);
            match s.axis {
                Axis::Stacked => {
                    let usable = frame.h.saturating_sub(rules);
                    let each = usable / n;
                    let extra = usable % n;
                    let mut y = frame.y;
                    for (i, child) in s.children().enumerate() {
                        let i = u16::try_from(i).unwrap_or(u16::MAX);
                        let h = each + u16::from(i < extra);
                        // Clamp to the frame. In a frame too small to hold
                        // the panes AND their separators, the separators
                        // would otherwise keep advancing past the edge and
                        // emit rects outside it — a rect nobody can paint
                        // and every tiling check rejects.
                        let end = frame.y.saturating_add(frame.h);
                        let h = h.min(end.saturating_sub(y));
                        lay(child, Rect::new(frame.x, y, frame.w, h), out);
                        y = y.saturating_add(h);
                        if i + 1 < n && y < end {
                            out.rules.push(Rule {
                                axis: s.axis,
                                rect: Rect::new(frame.x, y, frame.w, 1),
                            });
                            y = y.saturating_add(1);
                        }
                    }
                }
                Axis::SideBySide => {
                    let usable = frame.w.saturating_sub(rules);
                    let each = usable / n;
                    let extra = usable % n;
                    let mut x = frame.x;
                    for (i, child) in s.children().enumerate() {
                        let i = u16::try_from(i).unwrap_or(u16::MAX);
                        let w = each + u16::from(i < extra);
                        // See the Stacked arm — same clamp, same reason.
                        let end = frame.x.saturating_add(frame.w);
                        let w = w.min(end.saturating_sub(x));
                        lay(child, Rect::new(x, frame.y, w, frame.h), out);
                        x = x.saturating_add(w);
                        if i + 1 < n && x < end {
                            out.rules.push(Rule {
                                axis: s.axis,
                                rect: Rect::new(x, frame.y, 1, frame.h),
                            });
                            x = x.saturating_add(1);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: u64) -> Shikiri {
        Shikiri::Pane(Window {
            id: WindowId(id),
            buffer_id: BufferId(1),
            viewport: Viewport::default(),
        })
    }

    /// Every cell of the frame is covered exactly once by a pane or a rule.
    ///
    /// THE invariant. A gap is a column of garbage on screen; an overlap is
    /// two panes writing the same cell, where whichever paints last wins and
    /// the bug looks like a rendering glitch rather than a layout error.
    fn assert_tiles(solved: &Solved, frame: Rect) {
        let mut cover = vec![0u8; frame.area() as usize];
        let mut mark = |r: Rect| {
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    let i = (y - frame.y) as usize * frame.w as usize + (x - frame.x) as usize;
                    cover[i] += 1;
                }
            }
        };
        for (_, r) in &solved.panes {
            mark(*r);
        }
        for rule in &solved.rules {
            mark(rule.rect);
        }
        let gaps = cover.iter().filter(|c| **c == 0).count();
        let overlaps = cover.iter().filter(|c| **c > 1).count();
        assert_eq!(gaps, 0, "uncovered cells: {gaps}");
        assert_eq!(overlaps, 0, "doubly-covered cells: {overlaps}");
    }

    #[test]
    fn one_pane_fills_the_frame() {
        let f = Rect::new(0, 0, 80, 24);
        let s = solve(&pane(1), f);
        assert_eq!(s.panes, vec![(WindowId(1), f)]);
        assert!(s.rules.is_empty());
        assert_tiles(&s, f);
    }

    #[test]
    fn a_vertical_split_tiles_and_favours_the_first_pane() {
        // 80 columns, one rule, 79 usable -> 40 / 39. vim gives the extra
        // column to the leading pane.
        let f = Rect::new(0, 0, 80, 24);
        let t = Shikiri::Split(Split::new(Axis::SideBySide, pane(1), pane(2)));
        let s = solve(&t, f);
        assert_eq!(s.panes[0].1.w, 40);
        assert_eq!(s.panes[1].1.w, 39);
        assert_eq!(s.rules.len(), 1);
        assert_tiles(&s, f);
    }

    #[test]
    fn a_horizontal_split_tiles() {
        let f = Rect::new(0, 0, 80, 24);
        let t = Shikiri::Split(Split::new(Axis::Stacked, pane(1), pane(2)));
        let s = solve(&t, f);
        assert_eq!(s.panes[0].1.h + s.panes[1].1.h + 1, 24);
        assert_tiles(&s, f);
    }

    #[test]
    fn three_ways_split_equally_not_by_repeated_halving() {
        // vim's `equalalways`: a third split gives thirds, NOT 1/2 + 1/4 + 1/4.
        // Repeated halving is what you get if sizes are stored and only the
        // split pane is subdivided.
        let f = Rect::new(0, 0, 80, 24);
        let mut sp = Split::new(Axis::SideBySide, pane(1), pane(2));
        sp.push(pane(3));
        let s = solve(&Shikiri::Split(sp), f);
        let widths: Vec<u16> = s.panes.iter().map(|(_, r)| r.w).collect();
        assert_eq!(widths, vec![26, 26, 26], "78 usable / 3 = 26 each");
        assert_tiles(&s, f);
    }

    #[test]
    fn nested_splits_tile() {
        // :vsp then :sp in the right pane — the shape a flat two-pane model
        // cannot express at all.
        let f = Rect::new(0, 0, 81, 25);
        let inner = Shikiri::Split(Split::new(Axis::Stacked, pane(2), pane(3)));
        let t = Shikiri::Split(Split::new(Axis::SideBySide, pane(1), inner));
        let s = solve(&t, f);
        assert_eq!(s.panes.len(), 3);
        assert_tiles(&s, f);
    }

    #[test]
    fn every_leaf_appears_exactly_once() {
        let f = Rect::new(0, 0, 100, 40);
        let inner = Shikiri::Split(Split::new(Axis::Stacked, pane(2), pane(3)));
        let mut outer = Split::new(Axis::SideBySide, pane(1), inner);
        outer.push(pane(4));
        let s = solve(&Shikiri::Split(outer), f);
        let mut ids: Vec<u64> = s.panes.iter().map(|(w, _)| w.0).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2, 3, 4]);
    }

    #[test]
    fn panes_never_overlap_each_other() {
        let f = Rect::new(0, 0, 100, 40);
        let inner = Shikiri::Split(Split::new(Axis::Stacked, pane(2), pane(3)));
        let t = Shikiri::Split(Split::new(Axis::SideBySide, pane(1), inner));
        let s = solve(&t, f);
        for (i, (_, a)) in s.panes.iter().enumerate() {
            for (_, b) in s.panes.iter().skip(i + 1) {
                assert!(!a.overlaps(b), "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn a_frame_too_small_degrades_instead_of_panicking() {
        // The stated limit. Trailing panes get zero area; nothing panics and
        // nothing wraps around.
        for (w, h) in [(1u16, 1u16), (2, 1), (1, 2), (3, 3)] {
            let f = Rect::new(0, 0, w, h);
            let mut sp = Split::new(Axis::SideBySide, pane(1), pane(2));
            sp.push(pane(3));
            let s = solve(&Shikiri::Split(sp), f);
            assert_eq!(s.panes.len(), 3, "{w}x{h}: every leaf still reported");
            for (_, r) in &s.panes {
                assert!(
                    r.x.saturating_add(r.w) <= w && r.y.saturating_add(r.h) <= h,
                    "{w}x{h}: {r:?} escaped the frame",
                );
            }
            for rule in &s.rules {
                let r = rule.rect;
                assert!(
                    r.x.saturating_add(r.w) <= w && r.y.saturating_add(r.h) <= h,
                    "{w}x{h}: rule {r:?} escaped the frame",
                );
            }
            // Still a perfect tiling, even when degraded.
            assert_tiles(&s, f);
        }
    }

    #[test]
    fn solving_twice_gives_the_same_answer() {
        // Purity, asserted. If it were not pure, re-equalisation on split and
        // close would be a step someone has to remember.
        let f = Rect::new(0, 0, 77, 23);
        let t = Shikiri::Split(Split::new(Axis::SideBySide, pane(1), pane(2)));
        assert_eq!(solve(&t, f), solve(&t, f));
    }

    #[test]
    fn a_split_cannot_hold_fewer_than_two_children() {
        // Not a runtime check — there is no constructor that produces one.
        // `Split::new` takes two, and `push` only adds. This test documents
        // the absence rather than exercising a guard.
        let sp = Split::new(Axis::Stacked, pane(1), pane(2));
        assert_eq!(sp.len(), 2);
        assert_eq!(sp.children().count(), 2);
    }
}
