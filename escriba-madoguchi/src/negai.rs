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
/// ## `RunCommand`, and a correction
///
/// M0 said there would be no `RunCommand`, on the grounds that re-entering
/// the registry "means a handler can reach anything by naming it — which is
/// exactly the ceiling this crate exists to remove". That reasoning was
/// **wrong**, and M4 corrects it: madoguchi's capabilities govern what a
/// handler can READ. They have never governed what it can ASK FOR — any
/// handler may already emit `Quit`, `Save` or `Edit`. `RunCommand` therefore
/// adds no write authority that did not exist; it is a convenience over
/// emitting the same slips directly.
///
/// The real hazard is the one the original note buried: unbounded RECURSION,
/// command → slip → command → … . That is bounded explicitly by the
/// interpreter's dispatch-depth budget, and a refusal is REPORTED rather than
/// silent, per Phase 0.
///
/// The separate boundary that `:action` takes action SYMBOLS and never
/// command names still holds, and is still pinned by
/// `action_naming_a_command_is_inert_not_recursive`.
/// Deliberately NOT `#[non_exhaustive]`.
///
/// It was, briefly. `#[non_exhaustive]` forces every out-of-crate consumer to
/// carry a wildcard arm, which means a slip added here reaches the
/// interpreter and lands in a fallback — reported, but silently unhandled in
/// the sense that matters: nobody was made to think about it. Exhaustive
/// makes adding a variant a COMPILE ERROR at every interpreter, which is the
/// stronger seal and the one this repo asks for. escriba-madoguchi is
/// workspace-internal; the API-stability that `#[non_exhaustive]` buys is not
/// a trade worth making here.
/// Which source a `Negai::OpenPicker` opens over.
///
/// Named here rather than in `escriba-ui` because the VOCABULARY is
/// madoguchi's; the widget that renders it is the face's problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerSource {
    /// Open buffers.
    Buffers,
    /// Every registered command.
    Commands,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Move to the next/previous buffer, wrapping. A slip rather than
    /// `FocusBuffer(computed_id)` because "which buffer is next" depends on
    /// the live set, and a handler computing it from a snapshot would race
    /// any close that happened in between.
    CycleBuffer {
        forward: bool,
    },
    /// Open a path, or focus it if already open — the dedup lives in
    /// `BufferSet::open`, sealed in Phase 0.
    OpenPath(std::path::PathBuf),
    /// Close a buffer. Whether a modified buffer may close is policy, and
    /// policy belongs to the interpreter, not to whoever asked.
    CloseBuffer(BufferId),

    /// Write a buffer to its path. The I/O is the interpreter's — a handler
    /// asking to save must not be the thing that touches the filesystem, or
    /// "behaviour cannot reach the outside world" stops being true.
    Save {
        buffer: BufferId,
    },
    Undo {
        buffer: BufferId,
    },
    Redo {
        buffer: BufferId,
    },

    // ── registers ────────────────────────────────────────────────────
    Yank {
        text: String,
        register: Register,
    },

    /// Stop highlighting search matches while KEEPING the pattern, so `n`
    /// still works. This slip is why the crate exists: `:noh` was
    /// special-cased inside the runtime because `EditContext` could not
    /// reach `SearchState`, and that workaround was the visible proof of
    /// the ceiling. It is now an ordinary request.
    ClearSearchHighlight,

    /// Set an editor option. The declarative `defoption` apply path and the
    /// imperative `(set-option …)` effect converge on this one slip, so the
    /// two config tiers cannot write the option store differently.
    SetOption {
        name: String,
        value: String,
    },
    /// Insert text at the cursor, advancing it. Distinct from
    /// [`Edit`](Self::Edit), which is positional: this is where the operator
    /// IS, which is what a script means by "insert".
    InsertText(String),
    /// Run a registered command. See the correction above.
    RunCommand {
        name: String,
        args: Vec<String>,
    },

    // ── located findings ─────────────────────────────────────────────
    /// Replace a named result list wholesale.
    ///
    /// The interpreter seals it with the CURRENT world anchor. A handler
    /// cannot supply the anchor itself: it would be sealing against a world
    /// it read a moment ago, and the gap between read and apply is exactly
    /// where a stale-but-claimed-fresh list comes from.
    PublishFindings {
        list: String,
        findings: Vec<escriba_shirube::Finding>,
    },
    /// Move the cursor to the next/previous finding in a list, wrapping.
    WalkList {
        list: String,
        forward: bool,
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
    /// Open a picker over a named source.
    ///
    /// The picker's ITEMS are built by the interpreter, not the handler: a
    /// handler reads a read-only `Snapshot` and cannot hold the `&mut` the
    /// widget needs across many keypresses. So the slip names the source and
    /// the interpreter populates it — the seam holds, and the picker still
    /// lowers its accept back through `interpret` like everything else.
    OpenPicker(PickerSource),
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
        matches!(
            self,
            Self::Edit { .. } | Self::Undo { .. } | Self::Redo { .. } | Self::InsertText(_)
        )
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
