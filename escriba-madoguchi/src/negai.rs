//! `Negai` (願い, "a request") — what behaviour ASKS the editor to do.
//!
//! A slip is a *request*, not a mutation. Behaviour returns these; the one
//! interpreter in `escriba-runtime` decides whether and how to honour them.
//! That split is the whole point of the crate: a handler cannot reach past
//! this vocabulary, so "authored behaviour corrupted the editor" stops being
//! a thing that can happen and becomes a thing that cannot be expressed.

use escriba_core::{BufferId, Edit, Mode, Position};

/// Identifies work handed to the courier (`denrei`, plan §V Phase 5).
///
/// Opaque on purpose. Behaviour names an errand it wants run; it does not
/// describe HOW to run it, hold a handle to it, or see its reply directly —
/// the reply comes back as a fresh dispatch, anchored to the revisions it was
/// computed against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ErrandId(pub u32);

/// Where yanked text goes. `None` is vim's unnamed register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Register(pub Option<char>);

/// Resume instructions for [`Negai::AwaitKey`].
///
/// `ys{motion}{char}`, `f{char}`, `r{char}`, `m{a-z}` and `"{reg}y` all need
/// a key that has not been typed yet. Without this they are unbuildable, and
/// the original design of this crate omitted it — caught by the subsystem
/// designers before any code existed (plan §IV.3).
///
/// The interpreter routes the captured key back by re-dispatching `resume`
/// with `carried` plus the key appended. It does NOT invent a second
/// pending-key state machine: escriba already has one, `zenmai`-based, that
/// holds the operator-pending state, and a parallel mechanism would be the
/// duplication the compounding directive forbids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Continuation {
    /// The action symbol to re-enter once a key arrives.
    pub resume: String,
    /// Arguments accumulated so far, in order.
    pub carried: Vec<String>,
}

impl Continuation {
    #[must_use]
    pub fn new(resume: impl Into<String>) -> Self {
        Self {
            resume: resume.into(),
            carried: Vec::new(),
        }
    }

    #[must_use]
    pub fn carrying(mut self, arg: impl Into<String>) -> Self {
        self.carried.push(arg.into());
        self
    }
}

/// One typed request.
///
/// ## Why there is no `Spawn`
///
/// An earlier design had `Negai::Spawn(JobSpec)` alongside the courier's own
/// `Errand` — two job systems, independently derived, in one plan (§IV.1).
/// Spawning is [`Negai::Errand`]: one supervisor, one cancellation path, one
/// place staleness is decided.
///
/// ## Why there is no `RunCommand`
///
/// A slip that re-entered the command registry would make dispatch
/// recursive, and recursion here means a handler can reach anything by
/// naming it — which is exactly the ceiling this crate exists to remove.
/// The boundary is already pinned in `escriba-command`
/// (`action_naming_a_command_is_inert_not_recursive`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Negai {
    // ── text ─────────────────────────────────────────────────────────
    /// Apply an edit to a buffer. Undo is the interpreter's business.
    Edit {
        buffer: BufferId,
        edit: Edit,
    },
    /// Put the primary cursor somewhere. Clamping is the interpreter's job —
    /// behaviour is allowed to ask for an out-of-range position and get a
    /// sensible answer, rather than each handler re-implementing clamping.
    SetCursor {
        buffer: BufferId,
        to: Position,
    },

    // ── modal state ──────────────────────────────────────────────────
    EnterMode(Mode),

    // ── buffers ──────────────────────────────────────────────────────
    /// Make a buffer active.
    FocusBuffer(BufferId),
    /// Open a path, or focus it if already open — the dedup lives in
    /// `BufferSet::open`, sealed in Phase 0.
    OpenPath(std::path::PathBuf),
    /// Close a buffer. Whether a modified buffer may close is policy, and
    /// policy belongs to the interpreter, not to whoever asked.
    CloseBuffer(BufferId),

    // ── registers ────────────────────────────────────────────────────
    Yank {
        text: String,
        register: Register,
    },

    // ── operator feedback ────────────────────────────────────────────
    /// Say something on the status line. The channel Phase 0 opened.
    Message(String),

    // ── deferred / external ──────────────────────────────────────────
    /// Run an errand out of process. See [`ErrandId`].
    Errand(ErrandId),
    /// Capture the next keypress and resume. See [`Continuation`].
    AwaitKey {
        then: Continuation,
    },

    // ── lifecycle ────────────────────────────────────────────────────
    Quit,
}

impl Negai {
    /// Does honouring this slip change buffer text?
    ///
    /// Used by the interpreter to decide undo grouping and damage. Answered
    /// by the VARIANT rather than by observing a mutation, which is safe here
    /// precisely because slips are declarative — the same question asked of
    /// an already-applied `Action` has to be answered by observation, and
    /// getting that backwards is what once made `.` replay a search prompt.
    #[must_use]
    pub const fn touches_text(&self) -> bool {
        matches!(self, Self::Edit { .. })
    }

    /// Does this slip hand control somewhere else and expect to be resumed?
    ///
    /// Both variants suspend the current dispatch. An interpreter that
    /// treats them as ordinary fire-and-forget slips will drop the
    /// continuation, which is why they are asked about as one class.
    #[must_use]
    pub const fn suspends(&self) -> bool {
        matches!(self, Self::AwaitKey { .. } | Self::Errand(_))
    }
}
