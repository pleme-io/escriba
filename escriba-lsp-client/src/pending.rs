//! Request↔response correlation, with no I/O in it.
//!
//! JSON-RPC replies arrive out of order on one stream, so every in-flight
//! request needs a slot keyed by id. That bookkeeping is where an LSP client
//! hangs, and none of the ways it hangs need a process to reproduce:
//!
//! * a reply arrives for an id nobody is waiting on (a late response after a
//!   timeout, or a server echoing an id it invented) — must be *dropped*, never
//!   matched to the wrong waiter;
//! * two requests are issued with the same id — the second must not silently
//!   evict the first, leaving that caller waiting forever;
//! * **the server dies with requests in flight** — every waiter must be woken
//!   with an error. This is the one that matters most, because the failure is
//!   an editor that stops responding to hover with no message anywhere.
//!
//! So the map is a plain typed structure here, tested against all three, and
//! the transport just calls it.

use std::collections::HashMap;

use crate::wire::RequestId;

/// Why a pending request will never be answered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Abandoned {
    /// The server process exited or its stdout closed.
    #[error("the language server exited before replying")]
    ServerGone,
    /// Its slot was taken by a later request reusing the id.
    #[error("request id was reused by a later request")]
    IdReused,
}

/// What to do with an incoming reply.
#[derive(Debug, Clone, PartialEq)]
pub enum Routed<T> {
    /// It matched a waiter; here is what that waiter was carrying.
    Delivered(T),
    /// Nobody was waiting. Dropped deliberately — see the type docs.
    Unmatched,
}

/// The in-flight table. `T` is whatever the transport parks per request (a
/// oneshot sender in the live path, a marker in tests) — keeping it generic is
/// what lets the whole thing be tested with no runtime.
#[derive(Debug)]
pub struct Pending<T> {
    slots: HashMap<String, T>,
    next: i64,
}

impl<T> Default for Pending<T> {
    fn default() -> Self {
        Self {
            slots: HashMap::new(),
            next: 1,
        }
    }
}

/// Ids are normalised to a string key so a numeric `7` and a string `"7"`
/// cannot collide into one slot — servers echo ids back and are not obliged to
/// preserve the JSON type.
fn key(id: &RequestId) -> String {
    match id {
        RequestId::Number(n) => format!("n:{n}"),
        RequestId::Str(s) => format!("s:{s}"),
    }
}

impl<T> Pending<T> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate the next request id. Monotone, so an id is never reused within
    /// a session and a late reply can always be recognised as late.
    pub fn next_id(&mut self) -> i64 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// Park a waiter. Returns the previous occupant if the id was reused, so
    /// the caller can fail it explicitly rather than let it hang.
    #[must_use]
    pub fn insert(&mut self, id: &RequestId, waiter: T) -> Option<T> {
        self.slots.insert(key(id), waiter)
    }

    /// Route a reply. An id nobody awaits yields [`Routed::Unmatched`].
    #[must_use]
    pub fn route(&mut self, id: &RequestId) -> Routed<T> {
        self.slots
            .remove(&key(id))
            .map_or(Routed::Unmatched, Routed::Delivered)
    }

    /// Take every waiter, for when the server is gone. **The transport must
    /// call this on child exit** — anything still parked afterwards is a
    /// caller that never returns.
    #[must_use]
    pub fn drain_all(&mut self) -> Vec<T> {
        self.slots.drain().map(|(_, v)| v).collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Pending, Routed};
    use crate::wire::RequestId;

    fn n(i: i64) -> RequestId {
        RequestId::Number(i)
    }

    #[test]
    fn a_reply_reaches_the_waiter_that_asked() {
        let mut p: Pending<&str> = Pending::new();
        assert!(p.insert(&n(1), "hover").is_none());
        assert!(p.insert(&n(2), "definition").is_none());
        assert_eq!(p.route(&n(2)), Routed::Delivered("definition"));
        assert_eq!(p.route(&n(1)), Routed::Delivered("hover"));
        assert!(p.is_empty());
    }

    /// Replies arrive out of order on one stream; that is normal, not an error.
    #[test]
    fn out_of_order_replies_are_routed_by_id_not_arrival() {
        let mut p: Pending<i64> = Pending::new();
        for i in 1..=5 {
            let _ = p.insert(&n(i), i);
        }
        for i in [3, 1, 5, 2, 4] {
            assert_eq!(p.route(&n(i)), Routed::Delivered(i));
        }
    }

    /// A late reply after a timeout, or a server echoing an id it invented,
    /// must be dropped — never matched to whoever happens to be waiting.
    #[test]
    fn a_reply_nobody_awaits_is_dropped_not_misrouted() {
        let mut p: Pending<&str> = Pending::new();
        let _ = p.insert(&n(1), "hover");
        assert_eq!(p.route(&n(99)), Routed::Unmatched);
        assert_eq!(
            p.route(&n(1)),
            Routed::Delivered("hover"),
            "the real waiter is untouched"
        );
    }

    /// **The hang that matters most.** A server that dies with requests in
    /// flight leaves every caller waiting forever, and the symptom is an editor
    /// that silently stops answering hover.
    #[test]
    fn every_waiter_is_recoverable_when_the_server_dies() {
        let mut p: Pending<&str> = Pending::new();
        for (i, what) in [(1, "hover"), (2, "definition"), (3, "completion")] {
            let _ = p.insert(&n(i), what);
        }
        let mut stranded = p.drain_all();
        stranded.sort_unstable();
        assert_eq!(stranded, vec!["completion", "definition", "hover"]);
        assert!(
            p.is_empty(),
            "nothing may remain parked after the server is gone"
        );
    }

    /// A reused id must hand back the evicted waiter so the caller can be
    /// failed explicitly. Silently overwriting is a permanent hang.
    #[test]
    fn reusing_an_id_surfaces_the_evicted_waiter() {
        let mut p: Pending<&str> = Pending::new();
        assert!(p.insert(&n(1), "first").is_none());
        assert_eq!(
            p.insert(&n(1), "second"),
            Some("first"),
            "the evicted waiter must surface"
        );
        assert_eq!(p.route(&n(1)), Routed::Delivered("second"));
    }

    /// Ids allocated by this table are monotone, so within a session an id is
    /// never reused and a late reply is always recognisable as late.
    #[test]
    fn allocated_ids_are_monotone() {
        let mut p: Pending<()> = Pending::new();
        let ids: Vec<i64> = (0..5).map(|_| p.next_id()).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    }

    /// A numeric `7` and a string `"7"` are different ids. Servers echo ids
    /// back and are not obliged to preserve the JSON type, so collapsing them
    /// into one slot would deliver one caller's reply to another.
    #[test]
    fn a_numeric_id_and_a_string_id_never_collide() {
        let mut p: Pending<&str> = Pending::new();
        let _ = p.insert(&RequestId::Number(7), "numeric");
        let _ = p.insert(&RequestId::Str("7".into()), "string");
        assert_eq!(p.len(), 2, "these are two distinct slots");
        assert_eq!(
            p.route(&RequestId::Str("7".into())),
            Routed::Delivered("string")
        );
        assert_eq!(p.route(&RequestId::Number(7)), Routed::Delivered("numeric"));
    }
}
