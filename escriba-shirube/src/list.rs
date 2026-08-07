//! Named lists of findings, and walking them.
//!
//! ## One registry, not a quickfix/loclist distinction
//!
//! vim has two list kinds (quickfix, location) and every plugin invents more.
//! The distinction is accidental: what people actually want is "the
//! diagnostics list", "the grep list", "the test list" — several live at
//! once, each navigable, each replaceable wholesale by its producer. So this
//! is a registry keyed by NAME, and `]d` and `]q` are the same verb over
//! different names.
//!
//! ## Navigation reuses the search stepper
//!
//! `step` implements no wrapping of its own: `memori::Bound::step_wrapping`
//! does it, and it is what drives `n`/`N`, so result and search navigation
//! wrap and land identically by construction rather than by two
//! implementations agreeing for now.
//!
//! That sharing is not what was there to begin with. `Bound::first_matching`
//! existed and did NOT wrap — the wrap lived in `escriba-search`, along with
//! the reporting that makes vim print "search hit BOTTOM, continuing at TOP".
//! Writing a second wrap here would have been easy and would have been the
//! mistake; the wrap was lifted into memori instead, and `escriba-search`
//! became an adapter over it.

use std::collections::BTreeMap;

use escriba_memori::Bound;

use crate::anchor::Anchor;
use crate::finding::Finding;

/// A named list of findings, sealed with what it was computed against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultList {
    findings: Vec<Finding>,
    anchor: Anchor,
}

impl ResultList {
    /// Build a list, sorted into navigation order.
    ///
    /// Sorting here rather than trusting the producer is deliberate: order is
    /// a property of the LIST, and an LSP that returns diagnostics unordered
    /// must not make `]d` jump around.
    #[must_use]
    pub fn new(mut findings: Vec<Finding>, anchor: Anchor) -> Self {
        findings.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        Self { findings, anchor }
    }

    /// The findings, IF this list is still fresh against `world`.
    ///
    /// A stale list reads as EMPTY rather than as its old contents. That is
    /// the whole seal: a caller cannot accidentally paint yesterday's
    /// diagnostics, because it cannot reach them without passing the current
    /// world and being told no.
    #[must_use]
    pub fn fresh<'a>(&'a self, world: &Anchor) -> &'a [Finding] {
        if self.anchor.is_fresh(world) {
            &self.findings
        } else {
            &[]
        }
    }

    /// The findings without the freshness check.
    ///
    /// Exists for surfaces that legitimately show stale data while it is
    /// being recomputed — a list pane that dims rather than blanks. Named
    /// `_unchecked` so reaching for it is a decision, not a slip.
    #[must_use]
    pub fn findings_unchecked(&self) -> &[Finding] {
        &self.findings
    }

    #[must_use]
    pub fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    #[must_use]
    pub fn is_stale(&self, world: &Anchor) -> bool {
        !self.anchor.is_fresh(world)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    /// The finding at-or-after (forward) / at-or-before (backward) `line`,
    /// wrapping at the ends.
    ///
    /// `bound` decides whether a finding ON `line` counts: `Inclusive` for
    /// "go to the first problem from here", `Exclusive` for `]d` advancing
    /// off the one you are standing on. The same distinction, and the same
    /// implementation, that `/foo` and `n` use.
    #[must_use]
    pub fn step(&self, world: &Anchor, line: u32, forward: bool, bound: Bound) -> Option<&Finding> {
        let live = self.fresh(world);
        if live.is_empty() {
            return None;
        }
        let starts: Vec<usize> = live.iter().map(|f| f.site.line() as usize).collect();
        // `step_wrapping`, not `first_matching`: the wrap — and announcing it
        // — lives in memori, shared with `n`/`N`. Reaching for the
        // non-wrapping form here is how the two would drift.
        let landing = bound.step_wrapping(&starts, line as usize, forward)?;
        live.get(landing.index)
    }
}

/// Every live list, by name.
#[derive(Debug, Clone, Default)]
pub struct ListRegistry {
    lists: BTreeMap<String, ResultList>,
    /// Which list the bare navigation verbs walk.
    focused: Option<String>,
}

impl ListRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace a named list wholesale.
    ///
    /// Producers publish complete lists, never deltas. A delta protocol would
    /// need every producer to track what it had already said, and the first
    /// one to get it wrong leaves a finding on screen that nothing owns.
    /// Publishing also FOCUSES the list, so `]x` after a grep walks the grep.
    pub fn publish(&mut self, name: impl Into<String>, list: ResultList) {
        let name = name.into();
        self.focused = Some(name.clone());
        self.lists.insert(name, list);
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ResultList> {
        self.lists.get(name)
    }

    /// The list the bare navigation verbs walk.
    #[must_use]
    pub fn focused(&self) -> Option<(&str, &ResultList)> {
        let name = self.focused.as_deref()?;
        self.lists.get(name).map(|l| (name, l))
    }

    /// Point the bare verbs at an existing list. `false` when there is no
    /// such list — reported rather than silently focusing nothing.
    pub fn focus(&mut self, name: &str) -> bool {
        if self.lists.contains_key(name) {
            self.focused = Some(name.to_string());
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self, name: &str) -> bool {
        if self.focused.as_deref() == Some(name) {
            self.focused = None;
        }
        self.lists.remove(name).is_some()
    }

    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.lists.keys().map(String::as_str).collect()
    }

    /// Every finding on `line` of `buffer`, worst severity first — what a
    /// gutter paints and what a hover shows.
    #[must_use]
    pub fn on_line<'a>(
        &'a self,
        world: &Anchor,
        buffer: escriba_core::BufferId,
        line: u32,
    ) -> Vec<&'a Finding> {
        let mut hits: Vec<&Finding> = self
            .lists
            .values()
            .flat_map(|l| l.fresh(world))
            .filter(|f| f.site.buffer == Some(buffer) && f.site.line() == line)
            .collect();
        hits.sort_by_key(|f| f.severity);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::Axis;
    use crate::finding::{Origin, Severity, Site};
    use escriba_buffer::TextRev;
    use escriba_core::{BufferId, Position, Range};

    const B: BufferId = BufferId(1);

    fn at(line: u32, sev: Severity) -> Finding {
        Finding::new(
            Site::in_buffer(B, Range::point(Position::new(line, 0))),
            sev,
            "x",
            Origin::Text("todo"),
        )
    }

    fn world(rev: u64) -> Anchor {
        Anchor::new().on(Axis::Text(B, TextRev(rev)))
    }

    fn list_at(rev: u64, lines: &[u32]) -> ResultList {
        ResultList::new(
            lines.iter().map(|l| at(*l, Severity::Warning)).collect(),
            world(rev),
        )
    }

    #[test]
    fn a_stale_list_reads_as_empty_not_as_yesterdays_findings() {
        // The seal. A caller cannot paint stale diagnostics because it
        // cannot reach them without passing the current world.
        let l = list_at(1, &[2, 5]);
        assert_eq!(l.fresh(&world(1)).len(), 2);
        assert!(l.fresh(&world(2)).is_empty(), "the buffer moved");
        assert!(l.is_stale(&world(2)));
        // …and the escape hatch is named so reaching for it is a decision.
        assert_eq!(l.findings_unchecked().len(), 2);
    }

    #[test]
    fn stepping_wraps_like_search_does() {
        let l = list_at(1, &[2, 5, 9]);
        let w = world(1);
        let line = |f: Option<&Finding>| f.map(|f| f.site.line());

        assert_eq!(line(l.step(&w, 0, true, Bound::Exclusive)), Some(2));
        assert_eq!(line(l.step(&w, 2, true, Bound::Exclusive)), Some(5));
        assert_eq!(
            line(l.step(&w, 9, true, Bound::Exclusive)),
            Some(2),
            "forward from the last wraps to the first",
        );
        assert_eq!(
            line(l.step(&w, 0, false, Bound::Exclusive)),
            Some(9),
            "backward from the first wraps to the last",
        );
    }

    #[test]
    fn the_bound_decides_whether_where_you_stand_counts() {
        // `]d` must advance off the diagnostic you are on; "go to the first
        // problem from here" must not. Same distinction as `/foo` vs `n`.
        let l = list_at(1, &[5]);
        let w = world(1);
        assert_eq!(
            l.step(&w, 5, true, Bound::Inclusive).map(|f| f.site.line()),
            Some(5),
            "inclusive lands on where you are",
        );
        assert_eq!(
            l.step(&w, 5, true, Bound::Exclusive).map(|f| f.site.line()),
            Some(5),
            "exclusive with one finding wraps back around to it",
        );
    }

    #[test]
    fn stepping_a_stale_list_finds_nothing() {
        let l = list_at(1, &[2, 5]);
        assert!(l.step(&world(2), 0, true, Bound::Exclusive).is_none());
    }

    #[test]
    fn publishing_replaces_wholesale_and_focuses() {
        // Producers publish complete lists, never deltas: a delta protocol
        // needs every producer to track what it already said, and the first
        // one to get it wrong strands a finding nothing owns.
        let mut r = ListRegistry::new();
        r.publish("diagnostics", list_at(1, &[2, 5, 9]));
        assert_eq!(r.get("diagnostics").map(ResultList::len), Some(3));
        r.publish("diagnostics", list_at(1, &[7]));
        assert_eq!(r.get("diagnostics").map(ResultList::len), Some(1));
        assert_eq!(r.focused().map(|(n, _)| n), Some("diagnostics"));
    }

    #[test]
    fn several_lists_live_at_once() {
        // The reason this is a registry and not vim's quickfix/loclist pair.
        let mut r = ListRegistry::new();
        r.publish("diagnostics", list_at(1, &[2]));
        r.publish("grep", list_at(1, &[8]));
        assert_eq!(r.names(), vec!["diagnostics", "grep"]);
        assert_eq!(r.focused().map(|(n, _)| n), Some("grep"), "newest focused");
        assert!(r.focus("diagnostics"));
        assert_eq!(r.focused().map(|(n, _)| n), Some("diagnostics"));
        assert!(!r.focus("nope"), "focusing a missing list is reported");
    }

    #[test]
    fn a_gutter_sees_the_worst_severity_on_a_line() {
        let mut r = ListRegistry::new();
        r.publish(
            "diagnostics",
            ResultList::new(
                vec![at(3, Severity::Hint), at(3, Severity::Error)],
                world(1),
            ),
        );
        let hits = r.on_line(&world(1), B, 3);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].severity, Severity::Error, "worst first");
        assert!(r.on_line(&world(2), B, 3).is_empty(), "stale shows nothing");
    }

    #[test]
    fn clearing_a_focused_list_leaves_nothing_focused() {
        let mut r = ListRegistry::new();
        r.publish("grep", list_at(1, &[1]));
        assert!(r.clear("grep"));
        assert!(r.focused().is_none(), "no dangling focus on a gone list");
    }
}
