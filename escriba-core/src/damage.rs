//! [`Damage`] + [`LineDelta`] — the typed dirty-region boundary.
//!
//! The second node of escriba's sealed refresh tree (`theory/ESCRIBA.md` §X),
//! below [`EditGen`](crate::EditGen): where `EditGen` answers *did anything
//! change?*, `Damage` answers *what region changed?* so the renderer can scope
//! its work (re-shape / scissored present) to that region instead of the whole
//! document.
//!
//! The invariant it seals (S3): **`Damage ⊇ the actually-changed region`**.
//! The type only ever *widens* (the [`join`](Damage::join) is a
//! join-semilattice with `Full` as top), so the producer can be conservative
//! and a consumer that repaints `Damage` never misses a changed cell. An
//! under-approximation — a changed line the renderer skips — has no
//! constructible path: there is no operation that *narrows* `Damage`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How the line count changed under one edit — the scope signal a buffer
/// mutation reports up to the refresh tree. `start` is the first line the edit
/// touched; the counts let a consumer tell an in-place edit (`old == new`,
/// damage is local) from one that shifted every line below (`old != new`,
/// damage runs to the end of the document).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct LineDelta {
    pub start: u32,
    pub old_lines: u32,
    pub new_lines: u32,
}

impl LineDelta {
    /// Did this edit change the number of lines? If so, every line below
    /// `start` shifted and the damage runs to the end of the document.
    #[must_use]
    pub fn shifted_below(self) -> bool {
        self.old_lines != self.new_lines
    }
}

/// The dirty region of the document to refresh. A join-semilattice ordered
/// `None ⊑ Lines ⊑ Viewport ⊑ Full`, so combining two damages always yields
/// one that covers both (never less). Line ranges are inclusive `[from, to]`;
/// `to == u32::MAX` means "to the end of the document" (used when an edit
/// shifted every line below it).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub enum Damage {
    /// Nothing changed — an idle frame reuses everything.
    #[default]
    None,
    /// The inclusive line range `[from, to]` changed.
    Lines { from: u32, to: u32 },
    /// The whole visible region must repaint (scroll / resize), but the
    /// document's shaped content is unchanged.
    Viewport,
    /// Everything must be recomputed (buffer swap, theme change, full reflow).
    Full,
}

impl Damage {
    /// A single changed line.
    #[must_use]
    pub fn line(n: u32) -> Self {
        Damage::Lines { from: n, to: n }
    }

    /// An inclusive changed line span (order-normalized).
    #[must_use]
    pub fn span(a: u32, b: u32) -> Self {
        Damage::Lines {
            from: a.min(b),
            to: a.max(b),
        }
    }

    /// The changed span implied by a [`LineDelta`]: local if the line count
    /// held, otherwise to end-of-document (every line below shifted).
    #[must_use]
    pub fn from_delta(d: LineDelta) -> Self {
        if d.shifted_below() {
            Damage::Lines {
                from: d.start,
                to: u32::MAX,
            }
        } else {
            Damage::line(d.start)
        }
    }

    /// The lattice join: the smallest [`Damage`] covering both `self` and
    /// `other`. Widening only — the seal that keeps `Damage ⊇ changed`.
    #[must_use]
    pub fn join(self, other: Damage) -> Damage {
        use Damage::{Full, Lines, None, Viewport};
        match (self, other) {
            (Full, _) | (_, Full) => Full,
            (Viewport, _) | (_, Viewport) => Viewport,
            (None, x) | (x, None) => x,
            (
                Lines {
                    from: f0,
                    to: t0,
                },
                Lines {
                    from: f1,
                    to: t1,
                },
            ) => Lines {
                from: f0.min(f1),
                to: t0.max(t1),
            },
        }
    }

    /// Is there anything to repaint?
    #[must_use]
    pub fn is_none(self) -> bool {
        matches!(self, Damage::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_is_widening_and_full_is_top() {
        assert_eq!(Damage::None.join(Damage::line(3)), Damage::line(3));
        assert_eq!(Damage::Full.join(Damage::Viewport), Damage::Full);
        assert_eq!(Damage::Viewport.join(Damage::line(3)), Damage::Viewport);
        assert_eq!(
            Damage::span(2, 4).join(Damage::span(3, 9)),
            Damage::Lines { from: 2, to: 9 },
            "two line spans union",
        );
    }

    #[test]
    fn join_is_idempotent_and_commutative() {
        let d = Damage::span(1, 5);
        assert_eq!(d.join(d), d, "idempotent");
        assert_eq!(
            Damage::Viewport.join(d),
            d.join(Damage::Viewport),
            "commutative",
        );
    }

    #[test]
    fn line_count_change_runs_to_end() {
        let d = LineDelta {
            start: 10,
            old_lines: 40,
            new_lines: 41,
        };
        assert!(d.shifted_below());
        assert_eq!(
            Damage::from_delta(d),
            Damage::Lines {
                from: 10,
                to: u32::MAX,
            },
        );
    }

    #[test]
    fn in_place_edit_is_local() {
        let d = LineDelta {
            start: 10,
            old_lines: 40,
            new_lines: 40,
        };
        assert!(!d.shifted_below());
        assert_eq!(Damage::from_delta(d), Damage::line(10));
    }
}
