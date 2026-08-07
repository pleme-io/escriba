//! `Snapshot` — the window behaviour reads the editor through.
//!
//! ## The seal
//!
//! Every method takes `&self` and returns owned or borrowed data. There is
//! no `&mut` anywhere in this trait and no interior mutability behind it, so
//! a handler holding a `&dyn Snapshot` **cannot change the editor**. Not "is
//! discouraged from" — cannot. That is the entire security property of the
//! crate, and it is enforced by the type, not by review.
//!
//! Today the ceiling this replaces is `EditContext`, which hands out
//! `&mut BufferSet` and `&mut ModalState` and *still* is not enough: the
//! runtime special-cases `:noh` because `EditContext` cannot reach
//! `SearchState`. Widening `EditContext` would have made every handler
//! omnipotent to fix a problem of it being too narrow. Reading through a
//! window and returning slips makes the surface wider AND the power smaller
//! at the same time.
//!
//! ## Capabilities are M2
//!
//! In M0 the trait is one flat surface. M2 narrows it so a handler declares
//! which views it touches and the type system hands it exactly those — at
//! which point a handler bound to `(Search,)` that calls `.buffers()` fails
//! to COMPILE. The views are already split by concern here so that narrowing
//! is a type-level change rather than a re-carve of the surface.

use std::marker::PhantomData;

use escriba_core::{BufferId, Mode, Position};

use crate::cap;

/// What a handler can learn about one buffer.
pub trait BufferView {
    fn id(&self) -> BufferId;
    /// The buffer's path, if it has one. Scratch buffers do not.
    fn path(&self) -> Option<&std::path::Path>;
    fn line_count(&self) -> u32;
    /// One line, without its terminator. `None` past the end.
    fn line(&self, n: u32) -> Option<String>;
    fn is_modified(&self) -> bool;
    /// The whole text. Deliberately explicit and deliberately unattractive:
    /// a handler that reaches for this on every keystroke is telling you it
    /// wants a narrower view that does not exist yet.
    fn text(&self) -> String;
}

/// What a handler can learn about where the operator is.
pub trait CursorView {
    fn position(&self) -> Position;
    fn mode(&self) -> Mode;
}

/// The read window itself.
///
/// `&dyn Snapshot` is what a handler receives. Adding a view here widens what
/// behaviour can KNOW; it never widens what behaviour can DO, because doing
/// is [`Negai`](crate::Negai) and nothing else.
pub trait Snapshot {
    /// The buffer the operator is in, if any.
    fn active(&self) -> Option<&dyn BufferView>;
    /// A specific buffer.
    fn buffer(&self, id: BufferId) -> Option<&dyn BufferView>;
    /// Every open buffer id, in a stable order.
    fn buffer_ids(&self) -> Vec<BufferId>;
    /// Cursor + mode.
    fn cursor(&self) -> &dyn CursorView;
    /// An editor option (`number`, `tabstop`, `mapleader`, …).
    fn option(&self, name: &str) -> Option<&str>;
    /// The search session — the view `EditContext` could not reach, and the
    /// reason `:noh` was special-cased inside the runtime for so long.
    fn search(&self) -> &dyn SearchView;
}

// ─── The capability-narrowed window ──────────────────────────────────────

/// Read the search session.
pub trait SearchView {
    /// The committed pattern, if one has been searched for.
    fn pattern(&self) -> Option<&str>;
    /// How many matches the committed pattern has, if it is known.
    fn match_count(&self) -> Option<usize>;
    /// Is a `/` or `?` prompt open right now?
    fn is_prompting(&self) -> bool;
}

/// A [`Snapshot`] narrowed to the capabilities `C`.
///
/// This is what a handler actually receives. Each accessor carries a
/// `C: Has<Cap, _>` bound, so reaching for a view the handler did not
/// declare is a missing trait impl — a compile error, not a runtime check
/// and not a convention.
///
/// Zero-cost: it is one reference plus a `PhantomData`. Narrowing is
/// entirely a type-level act; nothing is copied, hidden or wrapped at
/// runtime, and the underlying `Snapshot` is the same object the
/// interpreter built.
pub struct View<'a, C> {
    snap: &'a dyn Snapshot,
    _caps: PhantomData<C>,
}

impl<'a, C> View<'a, C> {
    /// Narrow a snapshot to `C`.
    ///
    /// Callable by anyone, and that is fine: narrowing can only ever REMOVE
    /// reach. The interesting direction — widening — is impossible, because
    /// there is no method that returns the underlying `&dyn Snapshot`.
    #[must_use]
    pub const fn new(snap: &'a dyn Snapshot) -> Self {
        Self {
            snap,
            _caps: PhantomData,
        }
    }

    /// Read open buffers. Requires [`Buffers`](cap::Buffers).
    pub fn buffers<I>(&self) -> Buffers<'_>
    where
        C: cap::Has<cap::Buffers, I>,
    {
        Buffers { snap: self.snap }
    }

    /// Read cursor position and mode. Requires [`Cursor`](cap::Cursor).
    pub fn cursor<I>(&self) -> &dyn CursorView
    where
        C: cap::Has<cap::Cursor, I>,
    {
        self.snap.cursor()
    }

    /// Read an editor option. Requires [`Options`](cap::Options).
    pub fn option<I>(&self, name: &str) -> Option<&str>
    where
        C: cap::Has<cap::Options, I>,
    {
        self.snap.option(name)
    }

    /// Read the search session. Requires [`Search`](cap::Search).
    pub fn search<I>(&self) -> &dyn SearchView
    where
        C: cap::Has<cap::Search, I>,
    {
        self.snap.search()
    }
}

/// The buffer half of a [`View`], handed out only against a `Buffers` proof.
///
/// A separate type rather than methods on `View` so that the proof is
/// discharged ONCE, at `.buffers()`, instead of being re-proved on every
/// call — which keeps handler bodies readable.
pub struct Buffers<'a> {
    snap: &'a dyn Snapshot,
}

impl Buffers<'_> {
    #[must_use]
    pub fn active(&self) -> Option<&dyn BufferView> {
        self.snap.active()
    }
    #[must_use]
    pub fn get(&self, id: escriba_core::BufferId) -> Option<&dyn BufferView> {
        self.snap.buffer(id)
    }
    #[must_use]
    pub fn ids(&self) -> Vec<escriba_core::BufferId> {
        self.snap.buffer_ids()
    }
}

// ─── Test double ─────────────────────────────────────────────────────────

/// A `Snapshot` built from plain values — the mockable Environment seam.
///
/// Every typed primitive in this fleet ships one, and here it earns its
/// keep twice over: a handler test needs no `EditorState`, no buffers, no
/// runtime and no I/O, so behaviour can be tested exhaustively at the crate
/// that defines it rather than through the editor that hosts it.
#[derive(Debug, Clone, Default)]
pub struct FakeSnapshot {
    pub buffers: Vec<FakeBuffer>,
    pub active: Option<BufferId>,
    pub position: Position,
    pub mode: Mode,
    pub options: Vec<(String, String)>,
    pub pattern: Option<String>,
    pub match_count: Option<usize>,
    pub prompting: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FakeBuffer {
    pub id: BufferId,
    pub path: Option<std::path::PathBuf>,
    pub text: String,
    pub modified: bool,
}

impl FakeBuffer {
    #[must_use]
    pub fn new(id: u64, text: impl Into<String>) -> Self {
        Self {
            id: BufferId(id),
            path: None,
            text: text.into(),
            modified: false,
        }
    }

    #[must_use]
    pub fn at(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    #[must_use]
    pub const fn dirty(mut self) -> Self {
        self.modified = true;
        self
    }
}

impl BufferView for FakeBuffer {
    fn id(&self) -> BufferId {
        self.id
    }
    fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }
    fn line_count(&self) -> u32 {
        // An empty buffer is one (empty) line, matching Buffer::line_count.
        u32::try_from(self.text.lines().count().max(1)).unwrap_or(u32::MAX)
    }
    fn line(&self, n: u32) -> Option<String> {
        self.text.lines().nth(n as usize).map(str::to_string)
    }
    fn is_modified(&self) -> bool {
        self.modified
    }
    fn text(&self) -> String {
        self.text.clone()
    }
}

impl FakeSnapshot {
    /// A snapshot holding one buffer, focused, cursor at the origin.
    #[must_use]
    pub fn with_buffer(text: impl Into<String>) -> Self {
        let b = FakeBuffer::new(1, text);
        Self {
            active: Some(b.id),
            buffers: vec![b],
            ..Self::default()
        }
    }

    #[must_use]
    pub fn at(mut self, position: Position) -> Self {
        self.position = position;
        self
    }

    #[must_use]
    pub const fn in_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    #[must_use]
    pub fn searching(mut self, pattern: impl Into<String>, matches: usize) -> Self {
        self.pattern = Some(pattern.into());
        self.match_count = Some(matches);
        self
    }

    #[must_use]
    pub fn with_option(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.options.push((k.into(), v.into()));
        self
    }
}

impl SearchView for FakeSnapshot {
    fn pattern(&self) -> Option<&str> {
        self.pattern.as_deref()
    }
    fn match_count(&self) -> Option<usize> {
        self.match_count
    }
    fn is_prompting(&self) -> bool {
        self.prompting
    }
}

impl CursorView for FakeSnapshot {
    fn position(&self) -> Position {
        self.position
    }
    fn mode(&self) -> Mode {
        self.mode
    }
}

impl Snapshot for FakeSnapshot {
    fn active(&self) -> Option<&dyn BufferView> {
        let id = self.active?;
        self.buffer(id)
    }
    fn buffer(&self, id: BufferId) -> Option<&dyn BufferView> {
        self.buffers
            .iter()
            .find(|b| b.id == id)
            .map(|b| b as &dyn BufferView)
    }
    fn buffer_ids(&self) -> Vec<BufferId> {
        let mut v: Vec<_> = self.buffers.iter().map(FakeBuffer::id).collect();
        v.sort_unstable();
        v
    }
    fn cursor(&self) -> &dyn CursorView {
        self
    }
    fn option(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn search(&self) -> &dyn SearchView {
        self
    }
}

// ─── The real editor's buffers ───────────────────────────────────────────

/// `escriba_buffer::Buffer` read through the counter.
///
/// The impl lives HERE rather than in `escriba-buffer` so the buffer crate
/// stays ignorant of the dispatch seam: the seam knows the editor's types,
/// the editor's types do not know the seam. It also means `Snapshot` can hand
/// back `&dyn BufferView` borrowed straight from the `BufferSet` — no wrapper
/// to store, no temporary to outlive.
impl BufferView for escriba_buffer::Buffer {
    fn id(&self) -> BufferId {
        self.id
    }
    fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }
    fn line_count(&self) -> u32 {
        escriba_buffer::Buffer::line_count(self)
    }
    fn line(&self, n: u32) -> Option<String> {
        escriba_buffer::Buffer::line(self, n)
            .map(|l| l.trim_end_matches('\n').trim_end_matches('\r').to_string())
    }
    fn is_modified(&self) -> bool {
        self.modified
    }
    fn text(&self) -> String {
        self.to_string()
    }
}
