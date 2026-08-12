//! The courier's vocabulary — `denrei` (伝令), the messenger.
//!
//! Work the editor wants done but must not do on its own thread: walking a
//! tree, talking to a language server, running a formatter. Each is named as a
//! [`Freight`], carried by a [`Runner`] on a thread of its own, and answered
//! with a [`Parcel`] whose payload is an ordinary [`Negai`] the interpreter
//! already knows how to apply.
//!
//! # Why this lives here and not in a crate of its own
//!
//! Half of it already did. [`ErrandId`](crate::ErrandId) has carried the
//! comment "Identifies work handed to the courier (`denrei`, plan §V Phase 5)"
//! since before any of this existed, and [`Negai::ErrandReply`] was landed to
//! be the anchored way a reply re-enters the interpreter. A separate crate
//! would have minted a second copy of a vocabulary that was already sitting
//! here waiting for its producer.
//!
//! Declaring a trait is not doing I/O, so this crate's "no runtime, no I/O"
//! charter survives: the reply channel in [`Runner::start`] is
//! `std::sync::mpsc`, deliberately, and **that signature is the containment
//! seam**. A runner that wants an async runtime hosts one privately inside its
//! own thread; nothing about that reaches this crate or the interpreter.
//!
//! # The two rules that shape everything else
//!
//! **A runner cannot name `EditorState`.** It is handed owned data and a
//! sender, so there is no editor to mutate — replies re-enter through the one
//! door, and the interpreter decides whether they still apply.
//!
//! **A runner cannot mint its own freshness.** The anchor on an [`Errand`] is
//! sealed by the dispatcher, which is the only party that knows the world. A
//! handler naming an errand names a [`Freight`] and nothing else — see
//! [`Negai::Errand`].

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use escriba_shirube::NonEmptyAnchor;

use crate::negai::{ErrandId, Negai};

/// What is to be done — the errand's CLASS and its inputs, in one closed enum.
///
/// Closed rather than a trait because the one thing the design needs is an
/// exhaustive key: [`Crew::get`] must not be able to compile without a runner
/// for every class. A trait with associated types gives extensibility nobody
/// asked for and takes away the exhaustiveness that is the whole point.
///
/// Every variant carries **owned** data. That is not a convenience — it is how
/// "a runner cannot reach the editor" is true by construction rather than by
/// discipline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freight {
    /// Walk a tree and report matches.
    ///
    /// Carries `(raw, case)` rather than an `escriba_search::SearchPattern`
    /// because `SearchPattern` holds a compiled `regex::Regex` and derives only
    /// `Debug + Clone`, while [`Negai`] derives `PartialEq + Eq` — embedding it
    /// would strip those from every slip in the editor. Compilation is
    /// deterministic in these two values, so the dispatcher compiles once to
    /// reject a bad pattern with a typed message and the runner recompiles the
    /// identical matcher from the identical inputs.
    Scan {
        raw: String,
        case: escriba_search::CaseMode,
        root: std::path::PathBuf,
    },
    /// Ask a language server about one document.
    Diagnostics {
        buffer: escriba_core::BufferId,
        path: std::path::PathBuf,
        /// Which language this document is, as the EDITOR resolved it.
        ///
        /// Sent rather than re-derived because the editor owns the real answer:
        /// `escriba_core::FiletypeTable`, populated from every `defmode` in the
        /// catalog. The runner's own `language_of` knows two extensions and is
        /// explicit in its doc that a dispatcher wanting to disagree "should
        /// send the language rather than teach this function more extensions" —
        /// this field is that. `None` means the editor had no opinion, and the
        /// runner falls back, which is what keeps `--no-defaults` (an empty
        /// filetype table) working exactly as before.
        language: Option<String>,
        text: String,
    },
    /// Run a formatter over text.
    ///
    /// The text goes **by value** and the reply is an edit. A runner that wrote
    /// the file itself would have already written it by the time the
    /// interpreter noticed the operator kept typing — the anchor fences the
    /// reply, and it can only fence a reply that has not happened yet.
    Format {
        /// The buffer the reply edits.
        ///
        /// Carried rather than re-found by path, because the reply IS a
        /// `Negai::Edit` and an edit needs a `BufferId`. The seal used to
        /// resolve this with `find_by_path`, which was the honest shape while
        /// nothing constructed this variant outside a test; with a real runner
        /// the id has to survive the round trip.
        buffer: escriba_core::BufferId,
        path: std::path::PathBuf,
        /// The language, as the editor resolved it. See
        /// [`Freight::Diagnostics`]' field of the same name.
        language: Option<String>,
        text: String,
    },
}

impl Freight {
    /// A short label for a message or a log line.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Scan { .. } => "scan",
            Self::Diagnostics { .. } => "diagnostics",
            Self::Format { .. } => "format",
        }
    }
}

/// One dispatched errand: what to do, which instance it is, and what world it
/// was sealed against.
///
/// Constructed **only** by the dispatcher. A handler cannot build one into a
/// slip because [`Negai::Errand`] carries a [`Freight`], not an `Errand` — see
/// that variant's note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Errand {
    pub id: ErrandId,
    pub freight: Freight,
    /// The world this errand was dispatched against.
    ///
    /// [`NonEmptyAnchor`] rather than `Anchor`: an empty anchor is fresh
    /// against every world, so an errand carrying one could never be judged
    /// stale no matter how long it took.
    pub anchor: NonEmptyAnchor,
}

/// One reply from a runner.
///
/// `slip` is an ordinary [`Negai`] — the courier introduces no second reply
/// vocabulary. In practice it is always a [`Negai::ErrandReply`] wrapping the
/// real payload, or a [`Negai::Message`] when the errand has something to say
/// that is not a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parcel {
    pub id: ErrandId,
    pub slip: Negai,
}

/// Something that can carry one class of errand.
///
/// `Send + Sync` because the `Crew` is held by the editor and its runners are
/// invoked to spawn threads. Note what is **absent**: no `&mut` anything, no
/// editor, no return value. A runner's only output is what it posts.
pub trait Runner: Send + Sync {
    /// Begin the errand. MUST NOT block the caller — spawn and return.
    ///
    /// `cancel` is advisory and cooperative: a runner is expected to check it
    /// between units of work and stop posting. It does not kill anything, and
    /// pretending otherwise is how a "cancellation" that cancels nothing gets
    /// shipped. Bounding a *child process* is a different mechanism and belongs
    /// to the runner that spawns one.
    fn start(&self, errand: Errand, cancel: Arc<AtomicBool>, reply: Sender<Parcel>);
}

/// The registry: exactly one runner per [`Freight`] class.
///
/// **A struct, not a map.** With a `HashMap<_, Box<dyn Runner>>` a missing
/// runner is a runtime `None`; with a field per class, a new `Freight` variant
/// does not compile until someone decides who carries it. That is the only
/// reason this is not a map, and it is worth the rigidity.
pub struct Crew {
    pub scan: Box<dyn Runner>,
    pub diagnostics: Box<dyn Runner>,
    pub format: Box<dyn Runner>,
}

impl Crew {
    /// Route a freight to its runner. Exhaustive by construction.
    #[must_use]
    pub fn get(&self, freight: &Freight) -> &dyn Runner {
        match freight {
            Freight::Scan { .. } => &*self.scan,
            Freight::Diagnostics { .. } => &*self.diagnostics,
            Freight::Format { .. } => &*self.format,
        }
    }
}

impl std::fmt::Debug for Crew {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Crew { scan, diagnostics, format }")
    }
}

/// A runner that carries nothing and says so.
///
/// The editor is fully usable with no crew hired — every `EditorState` starts
/// this way, and only the composition root replaces it. An errand dispatched to
/// an inert crew reports that it went nowhere rather than vanishing: a request
/// that silently does nothing is the failure mode this whole seam exists to
/// avoid, and it must not be reintroduced by the default.
pub struct Idle(pub &'static str);

impl Runner for Idle {
    fn start(&self, errand: Errand, _cancel: Arc<AtomicBool>, reply: Sender<Parcel>) {
        // Send failure is ignored deliberately: a closed channel means the
        // editor is gone, and there is nobody left to tell.
        let _ = reply.send(Parcel {
            id: errand.id,
            slip: Negai::Message(alloc_msg(self.0, errand.freight.label())),
        });
    }
}

/// The one place this crate builds a string. Kept as a named function so the
/// message has a single spelling and the `Idle` impl stays about routing.
fn alloc_msg(who: &str, what: &str) -> String {
    let mut s = String::with_capacity(who.len() + what.len() + 24);
    s.push_str("no ");
    s.push_str(what);
    s.push_str(" runner hired (");
    s.push_str(who);
    s.push(')');
    s
}

impl Crew {
    /// A crew that carries nothing — the default before the composition root
    /// hires anyone.
    #[must_use]
    pub fn inert() -> Self {
        Self {
            scan: Box::new(Idle("inert")),
            diagnostics: Box::new(Idle("inert")),
            format: Box::new(Idle("inert")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Crew, Errand, Freight, Parcel, Runner};
    use crate::negai::{ErrandId, Negai};
    use escriba_shirube::{Axis, NonEmptyAnchor, SessionGen, SessionKind};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::{Sender, channel};

    fn an_errand(freight: Freight) -> Errand {
        Errand {
            id: ErrandId(1),
            freight,
            anchor: NonEmptyAnchor::on(Axis::Session(SessionKind::Scan, SessionGen(1))),
        }
    }

    fn a_scan() -> Freight {
        Freight::Scan {
            raw: "needle".into(),
            case: escriba_search::CaseMode::Smart,
            root: ".".into(),
        }
    }

    struct Tagged(&'static str);
    impl Runner for Tagged {
        fn start(&self, errand: Errand, _c: Arc<AtomicBool>, reply: Sender<Parcel>) {
            let _ = reply.send(Parcel {
                id: errand.id,
                slip: Negai::Message(self.0.to_string()),
            });
        }
    }

    /// The routing table is the reason `Crew` is a struct and not a map: each
    /// class reaches its own runner, and a missing one is a compile error
    /// rather than a `None` at dispatch time.
    #[test]
    fn each_freight_class_reaches_its_own_runner() {
        let crew = Crew {
            scan: Box::new(Tagged("scan")),
            diagnostics: Box::new(Tagged("diag")),
            format: Box::new(Tagged("fmt")),
        };
        let cases = [
            (a_scan(), "scan"),
            (
                Freight::Diagnostics {
                    buffer: escriba_core::BufferId(1),
                    path: "a.nix".into(),
                    language: None,
                    text: String::new(),
                },
                "diag",
            ),
            (
                Freight::Format {
                    buffer: escriba_core::BufferId(1),
                    path: "a.rs".into(),
                    language: None,
                    text: String::new(),
                },
                "fmt",
            ),
        ];
        for (freight, expect) in cases {
            let (tx, rx) = channel();
            crew.get(&freight).start(
                an_errand(freight.clone()),
                Arc::new(AtomicBool::new(false)),
                tx,
            );
            match rx.recv().unwrap().slip {
                Negai::Message(m) => assert_eq!(m, expect, "wrong runner for {freight:?}"),
                other => panic!("expected a message, got {other:?}"),
            }
        }
    }

    /// An errand sent to a crew nobody hired must SAY so. A request that
    /// silently does nothing reads to the operator as a broken editor, and it
    /// is exactly what the pre-courier `Negai::Errand` arm did.
    #[test]
    fn an_inert_crew_announces_rather_than_swallowing() {
        let (tx, rx) = channel();
        let freight = a_scan();
        Crew::inert()
            .get(&freight)
            .start(an_errand(freight), Arc::new(AtomicBool::new(false)), tx);
        match rx.recv().unwrap().slip {
            Negai::Message(m) => {
                assert!(m.contains("scan"), "names the class: {m}");
                assert!(m.contains("no"), "says it did not run: {m}");
            }
            other => panic!("expected a message, got {other:?}"),
        }
    }

    /// A dropped receiver is the editor going away. A runner must not panic on
    /// it — the process is already on its way out.
    #[test]
    fn posting_to_a_closed_channel_does_not_panic() {
        let (tx, rx) = channel::<Parcel>();
        drop(rx);
        let freight = a_scan();
        Crew::inert()
            .get(&freight)
            .start(an_errand(freight), Arc::new(AtomicBool::new(false)), tx);
    }

    /// Freight is owned data end to end — that is what makes "a runner cannot
    /// reach the editor" structural rather than a rule people remember.
    #[test]
    fn freight_carries_owned_data_and_labels_itself() {
        assert_eq!(a_scan().label(), "scan");
        let f = a_scan();
        let moved = f.clone();
        drop(f);
        assert_eq!(moved.label(), "scan");
    }
}
