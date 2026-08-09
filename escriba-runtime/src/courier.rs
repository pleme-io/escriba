//! The courier — `denrei` (伝令), the editor's side of it.
//!
//! Owns one `std::sync::mpsc` channel, the hired [`Crew`], and the errand
//! counter. Runners post [`Parcel`]s from their own threads; the editor drains
//! them at a tick boundary and applies each as an ordinary slip.
//!
//! # What is deliberately absent
//!
//! No epoch, no generation counter, no in-flight map. An earlier design had all
//! three and they were a **second freshness authority** running alongside
//! `shirube::Anchor` — two mechanisms deciding the same question, which is the
//! shape drift comes from. Freshness is the anchor's job, checked by the
//! interpreter at apply time against the live world. The courier only carries
//! things.
//!
//! That deletion also closed a bug the epoch version had: a batch-level epoch
//! check reads the world once, but applying an earlier slip in that batch can
//! dispatch a new errand and supersede a later one, which then applies anyway.
//! The anchor has no such window — `is_fresh` is evaluated per slip, at the
//! moment that slip is honoured.
//!
//! # What is not solved here
//!
//! Nothing stops a runner's thread. A superseded scan keeps walking until it
//! finishes; the editor simply ignores what it says. That is a **reply filter**,
//! and calling it cancellation would be a lie — reaping threads and bounding
//! child processes are separate mechanisms that belong to the runners that
//! spawn them.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use escriba_madoguchi::errand::{Crew, Errand, Freight, Parcel};
use escriba_madoguchi::{ErrandId, Negai};
use escriba_shirube::NonEmptyAnchor;

/// The editor's dispatch and delivery point for off-thread work.
pub struct Courier {
    tx: Sender<Parcel>,
    rx: Receiver<Parcel>,
    crew: Crew,
    next: u32,
    /// Cancel flags for errands that have been dispatched, newest per class.
    ///
    /// Advisory only — see the module note. Kept so a superseded runner has
    /// something to observe if it bothers to look; a runner that never looks is
    /// bounded by nothing here.
    live: Vec<(ErrandId, Arc<AtomicBool>)>,
}

impl std::fmt::Debug for Courier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Courier {{ next: {}, live: {} }}",
            self.next,
            self.live.len()
        )
    }
}

impl Courier {
    /// A courier with nobody hired. Every `EditorState` starts here.
    #[must_use]
    pub fn inert() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            crew: Crew::inert(),
            next: 0,
            live: Vec::new(),
        }
    }

    /// Install the real runners. Called once, by the composition root.
    ///
    /// The root is the only place that can see both the editor and the crates
    /// runners live in — that is why hiring is a separate step rather than
    /// something `EditorState::new` does.
    pub fn hire(&mut self, crew: Crew) {
        self.crew = crew;
    }

    /// Hand work to the crew.
    ///
    /// `anchor` is minted by the caller's `seal`, never by the runner: the
    /// dispatcher is the only party that knows the world the errand was
    /// launched against.
    pub fn send(&mut self, freight: Freight, anchor: NonEmptyAnchor) -> ErrandId {
        self.next = self.next.wrapping_add(1);
        let id = ErrandId(self.next);
        let cancel = Arc::new(AtomicBool::new(false));
        self.live.push((id, Arc::clone(&cancel)));
        self.crew.get(&freight).start(
            Errand {
                id,
                freight,
                anchor,
            },
            cancel,
            self.tx.clone(),
        );
        id
    }

    /// Collect whatever has arrived, up to `budget` parcels.
    ///
    /// **Bounded on purpose.** A chatty runner must not be able to hold the
    /// editor inside one drain — a frame that never ends is indistinguishable
    /// from a hang. Anything left over is collected on the next tick.
    ///
    /// Never blocks: `try_recv` only. `Disconnected` cannot happen while `self`
    /// holds a `Sender`, and is treated as "nothing more" rather than an error
    /// for the same reason.
    pub fn drain(&mut self, budget: usize) -> Vec<Negai> {
        let mut out = Vec::new();
        for _ in 0..budget {
            match self.rx.try_recv() {
                Ok(p) => out.push(p.slip),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    /// Ask every dispatched errand to stop posting.
    ///
    /// Cooperative and advisory. Returns how many flags were set, which is a
    /// count of errands ASKED, not of errands stopped.
    pub fn cancel_all(&mut self) -> usize {
        let n = self.live.len();
        for (_, flag) in self.live.drain(..) {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::Courier;
    use escriba_madoguchi::Negai;
    use escriba_madoguchi::errand::{Crew, Errand, Freight, Parcel, Runner};
    use escriba_shirube::{Axis, NonEmptyAnchor, SessionGen, SessionKind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Sender;

    fn seal() -> NonEmptyAnchor {
        NonEmptyAnchor::on(Axis::Session(SessionKind::Scan, SessionGen(1)))
    }

    fn a_scan() -> Freight {
        Freight::Scan {
            raw: "x".into(),
            case: escriba_search::CaseMode::Smart,
            root: ".".into(),
        }
    }

    /// Posts `n` messages immediately, on the calling thread, so the test is
    /// deterministic. Threading is the runner's business, not the courier's.
    struct Chatty(usize);
    impl Runner for Chatty {
        fn start(&self, e: Errand, _c: Arc<AtomicBool>, reply: Sender<Parcel>) {
            for i in 0..self.0 {
                let _ = reply.send(Parcel {
                    id: e.id,
                    slip: Negai::Message(i.to_string()),
                });
            }
        }
    }

    /// Records whether it was asked to stop, so cancellation can be observed
    /// rather than assumed.
    struct Watcher(Arc<AtomicBool>);
    impl Runner for Watcher {
        fn start(&self, _e: Errand, c: Arc<AtomicBool>, _r: Sender<Parcel>) {
            // Keep the flag the courier handed us, so the test can read it.
            self.0.store(c.load(Ordering::Relaxed), Ordering::Relaxed);
            let mine = Arc::clone(&c);
            std::mem::forget(mine);
        }
    }

    fn crew_of(r: impl Runner + 'static) -> Crew {
        Crew {
            scan: Box::new(r),
            diagnostics: Box::new(escriba_madoguchi::errand::Idle("t")),
            format: Box::new(escriba_madoguchi::errand::Idle("t")),
        }
    }

    #[test]
    fn an_inert_courier_still_answers_rather_than_swallowing() {
        let mut c = Courier::inert();
        c.send(a_scan(), seal());
        let got = c.drain(16);
        assert_eq!(got.len(), 1, "an unhired crew must still say something");
        assert!(matches!(got[0], Negai::Message(_)));
    }

    /// A chatty runner must not be able to hold the editor inside one drain.
    /// The remainder is not lost — it arrives on the next tick.
    #[test]
    fn the_drain_is_bounded_and_the_remainder_survives() {
        let mut c = Courier::inert();
        c.hire(crew_of(Chatty(10)));
        c.send(a_scan(), seal());

        assert_eq!(c.drain(4).len(), 4, "honours the budget");
        assert_eq!(c.drain(4).len(), 4);
        assert_eq!(c.drain(4).len(), 2, "the rest arrives later, not never");
        assert!(c.drain(4).is_empty());
    }

    /// Draining an empty courier is the common case — every quiet tick — and
    /// must be a cheap non-blocking nothing.
    #[test]
    fn draining_with_nothing_pending_returns_empty_without_blocking() {
        let mut c = Courier::inert();
        assert!(c.drain(64).is_empty());
    }

    #[test]
    fn ids_are_distinct_per_dispatch() {
        let mut c = Courier::inert();
        c.hire(crew_of(Chatty(0)));
        let a = c.send(a_scan(), seal());
        let b = c.send(a_scan(), seal());
        assert_ne!(a, b);
    }

    /// Cancellation is ADVISORY: it sets a flag a runner may or may not read.
    /// The test asserts what actually happens — flags are set and the count is
    /// of errands asked — and deliberately does not claim anything stopped.
    #[test]
    fn cancelling_sets_the_flag_and_reports_how_many_were_asked() {
        let seen = Arc::new(AtomicBool::new(false));
        let mut c = Courier::inert();
        c.hire(crew_of(Watcher(Arc::clone(&seen))));
        c.send(a_scan(), seal());
        assert!(!seen.load(Ordering::Relaxed), "not cancelled at dispatch");
        assert_eq!(c.cancel_all(), 1, "one errand asked to stop");
        assert_eq!(c.cancel_all(), 0, "nothing left to ask");
    }
}
