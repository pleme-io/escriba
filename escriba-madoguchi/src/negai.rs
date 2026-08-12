//! `Negai` (願い, "a request") — what behaviour ASKS the editor to do.
//!
//! A slip is a *request*, not a mutation. Behaviour returns these; the one
//! interpreter in `escriba-runtime` decides whether and how to honour them.
//! That split is the whole point of the crate: a handler cannot reach past
//! this vocabulary, so "authored behaviour corrupted the editor" stops being
//! a thing that can happen and becomes a thing that cannot be expressed.

use crate::errand::Freight;
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
// NOT `Copy`: `FilesUnder` carries a `PathBuf`. Losing `Copy` is the cost
// of a source that can name a root, and it is the right trade — the
// alternative is a second source vocabulary for "same walk, different
// root".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerSource {
    /// Open buffers.
    Buffers,
    /// Every registered command.
    Commands,
    /// Every binding — the searchable keymap.
    Help,
    /// Files under the working directory.
    Files,
    /// Directories that look like project roots.
    Project,
    /// Files under an explicit root rather than the working directory.
    ///
    /// `files.open-parent` is the reason this exists: "browse upward from
    /// where I am" is a different root, not a different walk, so it reuses
    /// the same bounded traversal instead of adding a second one.
    FilesUnder(std::path::PathBuf),
    /// Located findings from the result registry — the `trouble.*` family.
    ///
    /// `workspace: false` narrows to the active buffer, which is the whole
    /// difference between `trouble.document` and `trouble.workspace`.
    /// Freshness is NOT decided here: the interpreter asks the registry for
    /// findings fresh against the current world, so a stale list cannot be
    /// offered as a live one.
    Findings { workspace: bool },
}

/// The highlight class vocabulary, re-exported so a producer of
/// [`SemanticSpan`] takes ONE dependency and cannot end up on a different
/// `hikari-core` than the slip it fills in.
///
/// The same reasoning `escriba-ts` records for re-exporting hikari-ts: a
/// payload type and its vocabulary belong to the same crate boundary, or two
/// consumers can compile against two `HlClass`es that are structurally equal
/// and nominally distinct.
pub use hikari_core::HlClass;

/// One span a language server claims to know the meaning of.
///
/// # Why this is not a `Finding`
///
/// A [`Finding`](escriba_shirube::Finding) keys on SEVERITY, and severity is
/// what `worst_on_line` (the gutter's only reader), `on_line` and `]d` all
/// sort and filter by. A token is not a problem — there is no severity that
/// honestly describes "this word is a function name" — so publishing tokens as
/// findings would put thousands of severity-less rows into the list `]d` steps
/// through and the gutter paints. They are a different KIND of located thing
/// and get their own slip.
///
/// # Coordinates
///
/// `line` is zero-based and absolute. **`start_char` and `len_chars` count
/// `char`s, not UTF-16 code units** — the conversion happens once, at the LSP
/// boundary in `escriba-lsp-client`, for exactly the reason
/// [`escriba_shirube::Finding`] positions do: the two agree on every ASCII file
/// and differ by one per astral-plane character, so a span that carried the
/// wire's number would be right in every test and wrong for any reader with an
/// emoji in a comment.
///
/// A span never crosses a line break — LSP requires that of the wire format,
/// and it is what lets the renderer resolve a span against one rendered row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticSpan {
    /// Zero-based, absolute — the delta encoding is undone at the boundary.
    pub line: u32,
    /// Zero-based column within `line`, in `char`s.
    pub start_char: u32,
    /// Length in `char`s. Never spans a line break.
    pub len_chars: u32,
    /// What the server says this is, lowered onto the fleet's one highlight
    /// class vocabulary. `escriba_ui::syntax::ChromeSyntax::color` is total
    /// over it with no wildcard arm, so every token is themed on every face
    /// without a second colour table to keep in sync.
    pub class: HlClass,
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

    // ── server-authored colour ───────────────────────────────────────
    /// Replace what a language server said one buffer's tokens MEAN.
    ///
    /// Deliberately not a [`PublishFindings`](Self::PublishFindings) — see
    /// [`SemanticSpan`] for why a token is not a finding.
    ///
    /// Like findings, this is only trustworthy against the revision it was
    /// computed for: colours derived from text the operator has since edited
    /// are confidently wrong rather than merely absent. The interpreter
    /// therefore stores it sealed with the anchor the reply carried, exactly
    /// as it does a `PublishFindings` arriving inside an
    /// [`ErrandReply`](Self::ErrandReply), and a stale read is an empty read.
    PublishSemanticTokens {
        buffer: BufferId,
        tokens: Vec<SemanticSpan>,
    },

    // ── operator feedback ────────────────────────────────────────────
    /// Say something on the status line. The channel Phase 0 opened.
    Message(String),

    // ── deferred / external ──────────────────────────────────────────
    /// A reply computed OFF the tick, carrying the world it was computed
    /// against.
    ///
    /// The interpreter DROPS it when that world has moved on. This exists
    /// because sealing a slip with `self.world()` at apply time — which is
    /// what every synchronous slip does, correctly — is exactly wrong for
    /// a reply that crossed a thread boundary: findings computed at
    /// `TextRev(N)` would be resealed at `TextRev(N+1)` and reported FRESH,
    /// silently, at columns that have since moved.
    ///
    /// It WRAPS a slip rather than adding an `anchor` field to
    /// `PublishFindings`, because the worse failure is not a stale
    /// diagnostic — it is `Negai::Edit`. A formatter reply MUTATES TEXT and
    /// is anchored by nothing at all; applying one against a buffer the
    /// operator has kept typing into corrupts the file rather than
    /// mis-decorating it.
    ///
    /// Landed BEFORE the courier that will produce these, deliberately: the
    /// cost of this variant is one enum arm today and an audit of every
    /// producer once threads exist.
    ErrandReply {
        /// The world the payload was computed against.
        anchor: escriba_shirube::Anchor,
        /// What to do if that world still holds.
        then: Box<Negai>,
    },
    /// Hand work to the courier. See [`crate::errand`].
    ///
    /// Carries a [`Freight`] — the class and its inputs — and **nothing else**.
    /// In particular it does not carry an anchor, and that omission is the
    /// point.
    ///
    /// The anchor decides whether a reply still applies, so whoever mints it
    /// decides freshness. A handler sees only a read-only `Snapshot`; it does
    /// not know the world and must not be able to claim it does. If this
    /// variant carried a sealed errand, any handler could attach an anchor of
    /// its choosing — including one depending on nothing, which is fresh
    /// forever — and the freshness gate would become decorative. So the
    /// dispatcher seals: it is the only party holding the state the anchor
    /// describes.
    ///
    /// Boxed because [`Freight`] carries owned buffer text and `Negai` is
    /// moved around by value on every dispatch.
    Errand(Box<Freight>),
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
    /// `:sp` / `:vsp` — split the active window along `axis`.
    ///
    /// Carries the AXIS, not "horizontal"/"vertical": vim calls `:sp` a
    /// horizontal split and it stacks its children vertically, which is the
    /// naming trap every window manager falls into once.
    SplitWindow {
        stacked: bool,
    },
    /// `<C-w>c` / `:close` — close the active window. The LAST window never
    /// closes; vim refuses too (E444).
    CloseWindow,
    /// `<C-w>hjkl` — move focus to the nearest window in a direction.
    FocusDir {
        dx: i8,
        dy: i8,
    },
    /// Open a picker over matches for `pattern` across the project.
    ///
    /// Separate from `OpenPicker` because it carries an argument AND because
    /// it is the first slip that reads the FILESYSTEM rather than editor
    /// state. The interpreter already does synchronous I/O for `OpenPath`
    /// and `Save`, so the posture is not new — but a project-wide scan is the
    /// first one big enough to be worth bounding, and the bound is stated at
    /// the interpreter rather than hidden.
    GrepProject {
        pattern: String,
    },
    /// Format the ACTIVE buffer through its language server.
    ///
    /// Carries no payload for the same reason [`Self::GrepProject`] carries
    /// only a pattern: a command reads a `View` and cannot see the buffer set,
    /// its paths or its text, and the runtime owns all three. So the command
    /// says what it wants and the runtime builds the [`Freight::Format`]
    /// errand — which also keeps the courier's anchor sealed against the
    /// revision the runtime actually read, rather than one a command captured
    /// at some earlier moment.
    FormatBuffer,

    // ── debugging ────────────────────────────────────────────────────
    /// Set a breakpoint on the cursor's line of the ACTIVE buffer, or clear
    /// the one already there.
    ///
    /// Carries no payload, for the same reason [`Self::FormatBuffer`] does
    /// not: a command reads a `View`, the runtime owns the buffer set and the
    /// cursor, and a command that captured a line number would be naming a
    /// row it read at some earlier moment.
    ///
    /// **This is a MARK, not a debugger.** There is no adapter, no session
    /// and no protocol behind it — honouring it flips one bit of editor state
    /// that the gutter paints. A future DAP client reads the set; it does not
    /// get a second slip.
    ToggleBreakpoint,
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
