#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatRow {
    pub text: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FloatPanel {
    pub rows: Vec<FloatRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl FloatPanel {
    #[must_use]
    pub fn width(&self) -> u16 {
        let widest = self
            .rows
            .iter()
            .map(|r| r.text.chars().count())
            .max()
            .unwrap_or(0);
        u16::try_from(widest + 2).unwrap_or(u16::MAX)
    }

    #[must_use]
    pub fn rect(
        &self,
        caret_row: u16,
        caret_col: u16,
        area_w: u16,
        area_h: u16,
    ) -> Option<FloatRect> {
        if self.rows.is_empty() || area_w == 0 || area_h == 0 {
            return None;
        }
        let h = u16::try_from(self.rows.len()).unwrap_or(u16::MAX);
        let room_below = area_h.saturating_sub(caret_row.saturating_add(1));
        let (y, h) = if h <= room_below || room_below >= caret_row {
            (caret_row.saturating_add(1), h.min(room_below))
        } else {
            let shown = h.min(caret_row);
            (caret_row - shown, shown)
        };
        if h == 0 {
            return None;
        }
        let w = self.width().min(area_w);
        let x = caret_col.min(area_w - w);
        Some(FloatRect { x, y, w, h })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(rows: usize, width: usize) -> FloatPanel {
        FloatPanel {
            rows: (0..rows)
                .map(|_| FloatRow {
                    text: "x".repeat(width),
                    selected: false,
                })
                .collect(),
        }
    }

    #[test]
    fn it_opens_below_the_caret_when_it_fits() {
        let r = panel(3, 8).rect(2, 5, 80, 24).expect("a rect");
        assert_eq!(
            r,
            FloatRect {
                x: 5,
                y: 3,
                w: 10,
                h: 3
            }
        );
    }

    #[test]
    fn it_opens_above_the_caret_at_the_bottom_of_the_pane() {
        let r = panel(5, 8).rect(22, 5, 80, 24).expect("a rect");
        assert_eq!(
            r,
            FloatRect {
                x: 5,
                y: 17,
                w: 10,
                h: 5
            }
        );
    }

    #[test]
    fn it_never_leaves_the_pane_and_a_pane_of_no_size_shows_nothing() {
        let r = panel(30, 200).rect(10, 70, 80, 24).expect("a rect");
        assert!(r.x + r.w <= 80 && r.y + r.h <= 24, "{r:?}");
        assert!(panel(3, 3).rect(0, 0, 0, 0).is_none());
        assert!(FloatPanel::default().rect(0, 0, 80, 24).is_none());
    }
}
