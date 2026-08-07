//! `escriba-madoguchi` (窓口, "the service counter") — the seam authored
//! behaviour reaches the editor through.
//!
//! ## The problem
//!
//! Today a command runs against `EditContext`, which is
//! `{ buffers, active: BufferId (a COPY), modal, quit_flag }`. That means a
//! command cannot switch the active buffer, open a window, touch the layout,
//! reach search state, registers, marks or the theme, or spawn work. escriba
//! has already hit this ceiling once: `run_command` special-cases `:noh`
//! *inside the runtime*, with a comment saying `EditContext` does not expose
//! `SearchState` "and should not".
//!
//! Every one of the 85 inert keybindings needs strictly more than that
//! context offers. Wiring them the same way means 85 more special cases.
//!
//! ## The shape
//!
//! Behaviour **never mutates**. It:
//!
//! 1. reads through a [`Snapshot`] — a window with no `&mut` anywhere in it,
//! 2. returns an [`Outcome`]: typed request slips ([`Negai`]) plus a
//!    [`Verdict`],
//! 3. and exactly one interpreter — the only code in escriba holding
//!    `&mut EditorState` — decides whether and how to honour them.
//!
//! ```
//! use escriba_madoguchi::{FakeSnapshot, Negai, Outcome, Snapshot};
//!
//! // A handler is a pure function of what it can see.
//! fn where_am_i(snap: &dyn Snapshot) -> Outcome {
//!     let Some(buf) = snap.active() else {
//!         return Outcome::declined("no buffer");
//!     };
//!     let mut msg = String::from("line ");
//!     msg.push_str(&(snap.cursor().position().line + 1).to_string());
//!     msg.push_str(" of ");
//!     msg.push_str(&buf.line_count().to_string());
//!     Outcome::did(vec![Negai::Message(msg)])
//! }
//!
//! let snap = FakeSnapshot::with_buffer("a\nb\nc\n");
//! let out = where_am_i(&snap);
//! assert_eq!(out.slips, vec![Negai::Message("line 1 of 3".into())]);
//! ```
//!
//! Note what the example does NOT need: no `EditorState`, no `BufferSet`, no
//! runtime, no I/O. That is the compounding payoff — behaviour becomes
//! testable at the crate that defines it.
//!
//! ## The seal, proven
//!
//! The claim "a handler cannot mutate the editor" is a compile-time property,
//! so it is proven the only way a compile-time property can be: by showing
//! the mutation does not build. This runs under `cargo test`.
//!
//! ```compile_fail
//! use escriba_madoguchi::{FakeSnapshot, Snapshot};
//!
//! let snap = FakeSnapshot::with_buffer("hello");
//! let buf = snap.active().expect("a buffer");
//! // There is no mutating method to reach for. Not "you shouldn't" —
//! // the surface does not contain one, so this does not compile.
//! buf.set_text("corrupted");
//! ```
//!
//! Contrast the ceiling this replaces: `EditContext` hands out
//! `&mut BufferSet`, so the equivalent line there compiles happily.
//!
//! ## Why this is worth a crate
//!
//! It makes a class of bad state unrepresentable rather than discouraged.
//! A handler *cannot* corrupt the editor, because it is never handed anything
//! it could corrupt it with. Widening what behaviour can KNOW (adding a view)
//! is then decoupled from widening what behaviour can DO (which is `Negai`,
//! and nothing else) — the two axes that `EditContext` welded together.
//!
//! ## Status
//!
//! **M0.** The vocabulary, the read surface, and the test double. Nothing is
//! wired yet: `escriba-runtime` still dispatches through `EditContext`. The
//! milestones are in `docs/backlog-plan.md` §V Phase 1 —
//!
//! - **M1** re-signature the registry and add the interpreter.
//! - **M2** capability typing, at which point a handler bound to `(Search,)`
//!   that calls `.buffers()` fails to compile.
//! - **M3** lower `Action` into slips; `apply_resolved` loses its mutations.
//! - **M4** dedupe `escriba-vm`'s `HostEffect` into `Negai`.
//! - **M5** first retirements, and the inert ratchet moves down.
//!
//! Two corrections from the plan (§IV) are already carried here, before any
//! consumer exists: [`Negai::Errand`] replaces a would-be `Spawn` that
//! duplicated the courier, and [`Negai::AwaitKey`] exists so `f{char}` and
//! friends are buildable at all.

extern crate self as escriba_madoguchi;

pub mod cap;
pub mod negai;
pub mod outcome;
pub mod snapshot;

pub use negai::{Continuation, ErrandId, Negai, Register};
pub use outcome::{Outcome, Verdict};
pub use snapshot::{
    BufferView, CursorView, FakeBuffer, FakeSnapshot, SearchView, Snapshot, SyntaxView, View,
};

/// A capability-typed command body.
///
/// Implementors declare what they READ as an associated type, so the
/// capability set is part of the command's definition rather than something
/// a caller remembers to pass. [`erase`] turns one into the plain function
/// pointer the registry stores — no boxing, no allocation, no dynamic
/// dispatch beyond the `&dyn Snapshot` that was already there.
///
/// ```
/// use escriba_madoguchi::{caps, cap::Buffers, erase, Native, Outcome, View};
///
/// struct Info;
/// impl Native for Info {
///     type Reads = caps!(Buffers);
///     fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
///         match v.buffers().active() {
///             Some(_) => Outcome::nothing(),
///             None => Outcome::declined("no active buffer"),
///         }
///     }
/// }
/// let f = erase::<Info>();
/// assert!(f(&escriba_madoguchi::FakeSnapshot::default(), &[]).verdict.message().is_some());
/// ```
pub trait Native {
    /// The capabilities this command reads. `caps!()` for none.
    type Reads;
    fn run(view: &View<'_, Self::Reads>, args: &[String]) -> Outcome;
}

/// A handler that declares NO capabilities can read nothing at all.
///
/// This is not a rounding of "reads very little" — `caps!()` is `Nil`, which
/// has no `Has` impl, so every accessor on its view is unbuildable. Both
/// `quit` and `:noh` are genuinely in this class, having been handed
/// `&mut BufferSet` under the old `EditContext` to set one bool and clear
/// one flag respectively.
///
/// ```compile_fail
/// use escriba_madoguchi::{caps, Native, Outcome, View};
///
/// struct Quit;
/// impl Native for Quit {
///     type Reads = caps!();               // reads nothing
///     fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
///         let _ = v.buffers();            // does not compile
///         Outcome::nothing()
///     }
/// }
/// ```
///
/// Erase a [`Native`] into the registry's function pointer.
///
/// The capability set is discharged HERE, at the boundary: inside `T::run`
/// the narrowing is enforced by the type system, and outside it the registry
/// stores one uniform `fn`. Nothing about the erasure widens what the
/// handler could reach — it only forgets what the registry never needed.
#[must_use]
pub fn erase<T: Native>() -> fn(&dyn Snapshot, &[String]) -> Outcome {
    fn shim<T: Native>(snap: &dyn Snapshot, args: &[String]) -> Outcome {
        T::run(&View::new(snap), args)
    }
    shim::<T>
}

/// A handler: a pure function from what it can see to what it wants done.
///
/// The signature IS the seal. `&dyn Snapshot` has no `&mut` behind it, so
/// there is no path from here to editor state — a handler that wants to
/// change something must say so in slips and let the interpreter decide.
///
/// Boxed rather than generic because the registry stores heterogeneous
/// handlers by name; the cost is one indirection per command dispatch, which
/// is nothing at keyboard cadence.
pub type Handler = Box<dyn Fn(&dyn Snapshot, &[String]) -> Outcome + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_core::{BufferId, Mode, Position};

    /// The load-bearing test of the whole crate: a handler is handed a
    /// read-only window, so it cannot mutate. If `Snapshot` ever grows a
    /// `&mut` method this stops compiling, which is the point — the seal is
    /// enforced by the type, and this is where that gets noticed.
    #[test]
    fn a_handler_cannot_reach_editor_state() {
        let h: Handler = Box::new(|snap, _| {
            // Everything available is a read. There is no method here that
            // takes &mut self, and no interior mutability behind the trait.
            let n = snap.buffer_ids().len();
            Outcome::did(vec![Negai::Message(n.to_string())])
        });
        let snap = FakeSnapshot::with_buffer("x");
        assert_eq!(h(&snap, &[]).slips, vec![Negai::Message("1".into())]);
    }

    #[test]
    fn the_three_verdicts_are_distinguishable() {
        // Phase 0 established that "did nothing" and "worked" must differ.
        // There are three states, not two, and only success is silent.
        assert_eq!(Outcome::nothing().verdict.message(), None);
        assert_eq!(
            Outcome::declined("no more hunks").verdict.message(),
            Some("no more hunks"),
        );
        assert_eq!(
            Outcome::failed("cannot read file").verdict.message(),
            Some("cannot read file"),
        );
        assert!(!Outcome::declined("none").verdict.is_failure());
        assert!(Outcome::failed("boom").verdict.is_failure());
    }

    #[test]
    fn a_failure_carries_no_slips() {
        // Half-applying the requests of a handler that then reported failure
        // is how an editor reaches a state nobody designed.
        assert!(Outcome::failed("boom").is_coherent());
        assert!(Outcome::did(vec![Negai::Quit]).is_coherent());
        // Only-mitigated, and this proves the hole is real rather than
        // pretending the constructors are the only way in.
        let hand_built = Outcome {
            slips: vec![Negai::Quit],
            verdict: Verdict::Failed("boom".into()),
        };
        assert!(
            !hand_built.is_coherent(),
            "the incoherent value is still constructible — see Outcome::is_coherent",
        );
    }

    #[test]
    fn slips_classify_themselves_for_the_interpreter() {
        let edit = Negai::Edit {
            buffer: BufferId(1),
            edit: escriba_core::Edit::insert(Position::ZERO, "x"),
        };
        assert!(edit.touches_text());
        assert!(!Negai::Message("hi".into()).touches_text());

        // Both suspending variants must answer as one class, or an
        // interpreter treating them as fire-and-forget drops the resume.
        assert!(Negai::Errand(ErrandId(1)).suspends());
        assert!(
            Negai::AwaitKey {
                then: Continuation::new("surround.add")
            }
            .suspends()
        );
        assert!(!Negai::Quit.suspends());
    }

    #[test]
    fn a_continuation_carries_what_has_been_gathered() {
        // `ys{motion}{char}`: by the time the char arrives, the motion is
        // already known and must survive the round trip.
        let c = Continuation::new("surround.add").carrying("iw");
        assert_eq!(c.resume, "surround.add");
        assert_eq!(c.carried, vec!["iw".to_string()]);
    }

    #[test]
    fn the_fake_reads_like_a_real_buffer() {
        let snap = FakeSnapshot::with_buffer("alpha\nbeta\n")
            .at(Position::new(1, 2))
            .in_mode(Mode::Insert)
            .with_option("tabstop", "2");
        let buf = snap.active().expect("active buffer");
        assert_eq!(buf.line_count(), 2);
        assert_eq!(buf.line(1).as_deref(), Some("beta"));
        assert_eq!(buf.line(9), None, "past the end is None, not a panic");
        assert_eq!(snap.cursor().position(), Position::new(1, 2));
        assert_eq!(snap.cursor().mode(), Mode::Insert);
        assert_eq!(snap.option("tabstop"), Some("2"));
        assert_eq!(snap.option("nonexistent"), None);
    }

    #[test]
    fn an_empty_snapshot_declines_rather_than_panicking() {
        // A handler running before any buffer exists is a real state (boot,
        // and every `--no-defaults` run). It must be answerable, not fatal.
        let snap = FakeSnapshot::default();
        assert!(snap.active().is_none());
        assert!(snap.buffer_ids().is_empty());
        assert_eq!(snap.buffer(BufferId(1)).map(BufferView::id), None);
    }
}
