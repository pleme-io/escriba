//! `escriba-runtime` — editor state machine.
//!
//! Wraps everything: `BufferSet`, `ModalState`, `Keymap`, `CommandRegistry`,
//! `Layout`. Exposes `tick(input)` which advances one frame's worth of
//! state given one input event. Pure — no rendering, no I/O beyond file
//! save/load through `BufferSet`.

extern crate self as escriba_runtime;

pub mod breakpoint;
mod courier;
mod plugin_host;
pub mod scan;
pub use breakpoint::Breakpoints;
pub use plugin_host::{LazyTrigger, PluginHost};

pub mod status;

/// Re-exported from [`escriba_mode`], which now OWNS the operator-pending FSM.
///
/// It moved down in 0.1.68 because it only ever needed `escriba-core` +
/// `zenmai`; keeping it here meant a consumer had to depend on the entire
/// editor to compose `d` with `w`. This re-export is deliberate rather than a
/// deprecation shim: `escriba_runtime::OpState` is the spelling every existing
/// consumer and test uses, and a path change would be a breaking one for no
/// gain.
pub use escriba_mode::{OpState, OperatorPending};

/// What one key meant to the operator-pending object layer.
enum ObjectKey {
    /// Swallowed — the key began an object and nothing runs yet.
    Consumed,
    /// The object is complete; run this.
    Compose(Action),
}
pub use status::{PromptKind, StatusModel};

use std::collections::HashMap;

use awase::KeyRepeatGate;
use escriba_buffer::BufferSet;
use escriba_buffer::TextRev;
use escriba_command::CommandRegistry;
use escriba_core::{
    Action, Anchored, Bound, BufferId, Cursors, Damage, Edit, EditGen, HighlightEffect, InsertAt,
    JumpList, Mode, Motion, Operator, Position, Range, Register, RegisterKind, TextEffect,
    WindowId,
};
use escriba_input::{InputOutcome, translate_app_event};
use escriba_keymap::{Key, Keymap};
use escriba_madoguchi::{Negai, Outcome};
use escriba_mode::ModalState;
use escriba_search::{Direction as SearchDirection, MatchCount, SearchState};
use escriba_ui::chrome::{ChromePalette, FleetTheme};
use escriba_ui::splash::Splash;
use escriba_ui::{Layout, Viewport, Window};
use escriba_vm::{EditorSnapshot, EscribaHost, EscribaVm, VmError};
use madori::AppEvent;
use std::time::Instant;

/// Full editor state — the single Rust value the binary hands to the
/// renderer each frame.
pub struct EditorState {
    pub buffers: BufferSet,
    pub modal: ModalState,
    /// Search session — the committed pattern, its matches, the live `/`
    /// prompt and history. Owns no buffer or cursor; it answers questions
    /// about text and this runtime applies the answers.
    pub search: SearchState,
    pub keymap: Keymap,
    pub commands: CommandRegistry,
    pub layout: Layout,
    pub active: BufferId,
    /// The single typed home for cursor state. Phase-1 holds one primary
    /// [`Position`]; reads go through [`Self::cursor`], writes through
    /// [`Self::set_cursor`] → [`Cursors::set_primary`]. There is no loose
    /// `Position` field beside an unused multi-caret type to desync.
    cursors: Cursors,
    pub quit_requested: bool,
    /// Messages surfaced to the user (status line / `:messages`) — the
    /// sink for the tatara-lisp `(message …)` effect and other feedback.
    pub messages: Vec<String>,
    /// Which match the cursor last landed on (0-based) — the `[3/17]`
    /// numerator, ANCHORED to the text revision it was computed against.
    ///
    /// The anchor is what removes the manual invalidation this field used to
    /// need. An ordinal indexes a match set; when the text changes the set
    /// changes underneath it and the number silently means something else.
    /// Reading through `Anchored::get(current_rev)` makes that a `None`, so
    /// forgetting to clear is no longer a thing that can be forgotten.
    search_at: Option<Anchored<usize, TextRev>>,
    /// The last text change, for `.`.
    ///
    /// An action plus whatever was typed while it held Insert open. Both
    /// halves are needed: `cw` alone is not a change, it is the FIRST HALF of
    /// one — the text that followed is the rest, and replaying without it
    /// would delete a word and leave the buffer in Insert.
    last_change: Option<LastChange>,
    /// True while an insert session belonging to `last_change` is open, so
    /// typed characters are appended to it. Cleared on leaving Insert.
    recording_insert: bool,

    /// Where the cursor was before each far jump — `<C-o>` / `<C-i>`.
    /// Search commits, `n`/`N` and `*`/`#` all record into it, which is what
    /// makes a search a place you can come back from.
    pub jumps: JumpList,
    /// Generic editor option store (name → value). Written by the
    /// tatara-lisp `(set-option …)` effect and the declarative
    /// `defoption` apply path; typed accessors layer on top later.
    pub options: HashMap<String, String>,
    /// Cached embedded tatara-lisp runtime, built lazily on first
    /// `run_lisp`. Caching avoids re-installing the ~175-definition full
    /// stdlib on every call; the interpreter's top-level env also
    /// persists across calls, giving REPL-like session semantics (an
    /// earlier `(define …)` is visible to a later `run_lisp`).
    lisp_vm: Option<EscribaVm>,
    /// Keys accumulated for an in-progress multi-key sequence — e.g.
    /// holding `[,, f]` while waiting for the final key of
    /// `<leader>ff`. Empty when not mid-sequence. Lives on
    /// `EditorState` (not `ModalState`) so `escriba-mode` needn't
    /// depend on `escriba-keymap`'s `Key`.
    pub pending_keys: Vec<Key>,
    /// Per-key debouncer for OS key-repeat storms. Holding `j`/`l` makes
    /// the windowing system deliver one `KeyDown` per repeat tick
    /// (~30-50ms); without a gate those flood the motion path and thrash
    /// the viewport. The gate lets ONE event per `min_interval` (80ms
    /// default — ~12 intentional taps/sec still pass) reach the editor in
    /// the navigation modes. The fleet primitive (`awase::KeyRepeatGate`,
    /// the same one mado uses) is reused — not reinvented.
    repeat_gate: KeyRepeatGate<Key>,
    /// Runtime lazy-activation host for USER plugin caixas (the bundled
    /// default catalog is applied eagerly at boot, not through here).
    /// A command / filetype-open / event fires the matching plugins'
    /// entries through the escriba-lisp apply paths. See [`PluginHost`].
    pub plugin_host: PluginHost,
    /// The unnamed register — the home for text an operator yanks or
    /// deletes (`Operator::leaves_register`). `None` until the first
    /// register-leaving operator runs. Phase-1 holds the single unnamed
    /// register; named registers (`"ay`) layer on later.
    ///
    /// Typed as a [`Register`], not a `String`: the put has to know whether
    /// the text was captured CHARWISE (`dw`) or LINEWISE (`dd`), and the
    /// capture is the only place that knows. A `String` register makes `p`
    /// after `dd` guess, and the only guess available is the wrong one —
    /// splicing a whole line into the middle of another.
    register: Option<Register>,
    /// The operator-pending FSM (`d`/`c`/`y` then a motion → `dw`/`c$`/`y0`),
    /// standing on the fleet `zenmai` Mealy-machine primitive. Every dispatched
    /// action passes through it; only an operator-then-motion pair is rewritten
    /// into an [`Action::ApplyOperator`].
    op_pending: zenmai::Stateful<OperatorPending>,
    /// Operator-pending OBJECT selection, held at the KEY layer.
    ///
    /// `Some(around)` means `d` + `i`/`a` have been pressed and the NEXT key
    /// names the object. It lives here rather than in the operator FSM
    /// because the FSM sees `Action`s and this decision needs the KEY: `a`
    /// and every bracket are unbound in Normal, so they all arrive as
    /// `Action::Pending` with the character already discarded. vim has a
    /// whole operator-pending keymap for the same reason.
    pending_object: Option<bool>,
    /// `f`/`F`/`t`/`T` was pressed and the editor is waiting for the character
    /// to search for. Like [`Self::pending_object`] this is a KEY-layer
    /// concern: the character never reaches the keymap, so it cannot be a
    /// binding, and it must be claimed before the sequence stepper or `f` then
    /// `f` would resolve as the bound `ff` sequence.
    pending_find: Option<FindSpec>,
    /// `r` was pressed and the editor is waiting for the replacement
    /// character. Same key-layer shape as [`Self::pending_find`], and needed
    /// for the same reason: `rw` must not read as `r` then *move a word*, and
    /// `rr` must reach the operand branch rather than resolve as a sequence.
    pending_replace: bool,
    /// The last resolved character search — what `;` and `,` repeat.
    last_find: Option<FindSpec>,
    /// `m`, `` ` `` or `'` was pressed and the editor is waiting for the mark
    /// letter. Same key-layer shape as [`Self::pending_find`].
    pending_mark: Option<MarkKey>,
    /// `m{a-z}` → position. Buffer-agnostic today, which is honest and
    /// limited: vim's `a-z` marks are per-buffer and `A-Z` are global, and a
    /// single map is `a-z`-shaped. Jumping to a mark set in another buffer
    /// would land at that position in THIS one, so only `a-z` are accepted.
    marks: HashMap<char, Position>,
    /// Monotonic refresh-generation stamp — the root of the sealed refresh
    /// tree (`theory/ESCRIBA.md` §Refresh-Seal). Bumped on every applied
    /// action + resize; the renderer gates on it so an idle frame does zero
    /// re-highlight / re-shape, and a stale frame is unreachable.
    edit_gen: EditGen,
    /// The accumulated dirty region since the renderer last drained it (M1).
    /// Only ever widened via [`Damage::join`] at the mutation funnel, so it
    /// always covers the changed region (`Damage ⊇ changed`); the renderer
    /// drains it with [`take_damage`](Self::take_damage) to scope its work.
    damage: Damage,
    /// The theme every face paints with.
    ///
    /// ONE owner. Before this, `(deftheme :preset …)` parsed, validated,
    /// resolved to a real `FleetTheme` — and then nothing consumed it,
    /// because each renderer called `ChromePalette::prescribed()` at every
    /// paint site. The declaration was honoured on paper only. Holding it
    /// here means a face reads the operator's theme the same way it reads
    /// the cursor: from the state, per frame.
    theme: FleetTheme,
    /// `theme` resolved to concrete colours — cached because it is a plain
    /// `Copy` struct read many times per frame, and re-derived only in
    /// [`set_theme`](Self::set_theme), so the two cannot disagree.
    chrome: ChromePalette,
    /// How deep the current command dispatch is nested.
    ///
    /// `Negai::RunCommand` lets a command invoke a command, which is useful
    /// and which can also recurse forever. The budget makes the runaway
    /// bounded and REPORTED rather than a stack overflow — the difference
    /// between a typed refusal and the editor dying under the operator.
    dispatch_depth: u8,
    /// Every live result list — diagnostics, hunks, grep hits, TODOs.
    ///
    /// Public so a producer outside the runtime can publish into it once the
    /// courier lands; today the only producer is the marker scan.
    pub results: escriba_shirube::ListRegistry,
    /// The open picker, if any.
    ///
    /// `Option<Picker>` on the state, exactly like `splash` — deliberately
    /// NOT a `Mode` variant. A mode is a state keys are interpreted IN; this
    /// is a surface that OWNS keys while it is up, which is a different
    /// thing and composes differently with the keymap.
    picker: Option<escriba_ui::picker::Picker>,
    /// The git-index generation. See [`world`](Self::world) — every axis the
    /// world can move is emitted unconditionally, so a producer anchoring on
    /// one is not born permanently stale.
    index_rev: escriba_shirube::IndexRev,
    /// The language-server generation — bumped when a server restarts.
    lsp_gen: escriba_shirube::SessionGen,
    /// The filesystem-scan generation — bumped when a surface a scan feeds
    /// opens or closes, which is what supersedes an in-flight scan.
    scan_gen: escriba_shirube::SessionGen,
    /// Work handed off the editor thread. Inert until the composition root
    /// hires a crew, so every existing `EditorState::new*` call site is
    /// unchanged and the editor is fully usable with no runners at all.
    courier: courier::Courier,
    /// Which findings list the open picker is a VIEW of, if any —
    /// `(workspace, list)`. Set when a picker opens over a live producer,
    /// cleared whenever the picker closes.
    picker_projects: Option<(bool, Option<String>)>,
    /// Extension → language facts, populated from `(defmode …)`.
    ///
    /// The consumer `:commentstring` never had. Public so the binary's apply
    /// pass can fill it the way it fills the keymap and the option store.
    pub filetypes: escriba_core::FiletypeTable,
    /// The start screen, while it is up.
    ///
    /// `Some` only between boot and the first keypress, and only when the
    /// editor opened with no file. It is deliberately NOT a `Mode`: a mode
    /// is a state keys are interpreted *in*, and the splash interprets
    /// exactly one key before it is gone. Modelling it as `Option<Splash>`
    /// keeps the modal state machine's variant set — and every exhaustive
    /// match over it — untouched.
    splash: Option<Splash>,
    /// What a language server last said one buffer's tokens MEAN.
    ///
    /// One slot, not a per-buffer map, and that is the honest shape of what
    /// produces it: the diagnostics errand runs per OPEN, so at any moment
    /// there is one buffer whose tokens are both present and fresh. A map
    /// would model a fan-out that has no producer.
    ///
    /// Sealed, and read through [`semantic_spans`](Self::semantic_spans) —
    /// never directly — for the same reason a `ResultList` is: a colour
    /// derived from text the operator has since edited is *confidently wrong*,
    /// which is worse than absent. A stale read is an empty read and the face
    /// falls back to its own lexer.
    semantic: Option<SemanticPaint>,
    /// Where the operator asked the debugger to stop.
    ///
    /// Its own field rather than a [`escriba_shirube::ResultList`], and the
    /// difference is the whole reason [`Breakpoints`] exists as a type: every
    /// result list is ANCHORED and a stale read is an empty read, so a
    /// breakpoint published as a finding would vanish on the next keystroke —
    /// and `publish` also FOCUSES the list it replaces, so `]d` would start
    /// walking breakpoints. See [`crate::breakpoint`] for the full argument
    /// and for what the line-number key does not yet survive.
    breakpoints: Breakpoints,
}

/// Semantic tokens for one buffer, sealed with the world they describe.
///
/// A twin of [`escriba_shirube::ResultList`] rather than a use of it: that
/// type's payload is `Vec<Finding>`, and a token is deliberately not a finding
/// (see [`escriba_madoguchi::SemanticSpan`]). What IS shared is the part that
/// matters — the [`Anchor`](escriba_shirube::Anchor) and the rule that a stale
/// read yields nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticPaint {
    buffer: BufferId,
    spans: Vec<escriba_madoguchi::SemanticSpan>,
    anchor: escriba_shirube::Anchor,
}

impl SemanticPaint {
    #[must_use]
    pub const fn new(
        buffer: BufferId,
        spans: Vec<escriba_madoguchi::SemanticSpan>,
        anchor: escriba_shirube::Anchor,
    ) -> Self {
        Self {
            buffer,
            spans,
            anchor,
        }
    }

    /// The spans, IF they describe `buffer` AND are still fresh against
    /// `world`. Anything else is empty.
    #[must_use]
    pub fn fresh(
        &self,
        world: &escriba_shirube::Anchor,
        buffer: BufferId,
    ) -> &[escriba_madoguchi::SemanticSpan] {
        if self.buffer == buffer && self.anchor.is_fresh(world) {
            &self.spans
        } else {
            &[]
        }
    }
}

/// What the start screen did with a keypress.
///
/// Total, and matched exhaustively at its one call site, so a future
/// outcome (a menu that opens a submenu, say) is a compile error rather
/// than a key that silently falls through to the buffer.
enum SplashKey {
    /// No start screen is up — the key is the buffer's.
    NotShowing,
    /// The key selected a menu entry; run this.
    Ran(Action),
    /// The screen is gone and the key was not a menu key, so it still
    /// means whatever it normally means. Anything else would make the
    /// first keystroke after boot vanish.
    Dismissed,
}

// ─── The counter, and the one place slips become mutations ───────────────

/// `EditorState` read through the counter.
///
/// Borrowed, never copied: building it is free, so a command dispatch does
/// not pay for a snapshot of the buffers.
pub struct EditorWindow<'a> {
    state: &'a EditorState,
}

impl escriba_madoguchi::CursorView for EditorWindow<'_> {
    fn position(&self) -> Position {
        self.state.cursor()
    }
    fn mode(&self) -> Mode {
        self.state.modal.mode()
    }
}

impl escriba_madoguchi::SyntaxView for EditorWindow<'_> {
    fn filetype(&self) -> Option<&escriba_core::Filetype> {
        let path = self.state.buffers.get(self.state.active)?.path.as_deref()?;
        self.state.filetypes.resolve(path)
    }
}

impl escriba_madoguchi::SearchView for EditorWindow<'_> {
    fn pattern(&self) -> Option<&str> {
        self.state.search.committed_pattern()
    }
    fn match_count(&self) -> Option<usize> {
        // `None` means "nothing committed", which is not the same as zero
        // matches — a distinction the status line already makes and that a
        // handler must not have to re-derive.
        self.state
            .search
            .committed_pattern()
            .map(|_| self.state.search.match_count())
    }
    fn is_prompting(&self) -> bool {
        self.state.search.is_prompting()
    }
}

impl escriba_madoguchi::Snapshot for EditorWindow<'_> {
    fn active(&self) -> Option<&dyn escriba_madoguchi::BufferView> {
        self.buffer(self.state.active)
    }
    fn buffer(&self, id: BufferId) -> Option<&dyn escriba_madoguchi::BufferView> {
        self.state
            .buffers
            .get(id)
            .map(|b| b as &dyn escriba_madoguchi::BufferView)
    }
    fn buffer_ids(&self) -> Vec<BufferId> {
        self.state.buffers.ids()
    }
    fn cursor(&self) -> &dyn escriba_madoguchi::CursorView {
        self
    }
    fn option(&self, name: &str) -> Option<&str> {
        self.state.options.get(name).map(String::as_str)
    }
    fn search(&self) -> &dyn escriba_madoguchi::SearchView {
        self
    }
    fn syntax(&self) -> &dyn escriba_madoguchi::SyntaxView {
        self
    }
}

impl EditorState {
    /// A read-only window onto this editor.
    #[must_use]
    pub fn window(&self) -> EditorWindow<'_> {
        EditorWindow { state: self }
    }

    /// Honour an [`Outcome`] — the ONLY place slips become mutations.
    ///
    /// Every `&mut self` in the dispatch path lives here. A command cannot
    /// reach editor state, so if the editor ends up in a state nobody
    /// designed, this function is where it happened; that narrowing is the
    /// whole return on the seam.
    ///
    /// A failed outcome's slips are DROPPED rather than half-applied: a
    /// handler that reported failure has no business also mutating, and
    /// applying part of what it asked for is how an editor reaches a state
    /// nobody designed.
    pub fn interpret(&mut self, outcome: Outcome) {
        if let Some(m) = outcome.verdict.message() {
            self.messages.push(m.to_string());
            self.damage = self.damage.join(Damage::Viewport);
            self.bump_gen();
        }
        if outcome.verdict.is_failure() {
            return;
        }
        for slip in outcome.slips {
            self.honour(slip);
        }
    }

    /// Lower an [`Action`] to slips, when it has an exact slip equivalent.
    ///
    /// `None` means "editor mechanics" — 23 of the 30 variants are prompt
    /// editing, the operator-pending FSM, motion resolution, the jumplist,
    /// the dot register. Those are the KEYMAP's vocabulary, not the AUTHORED
    /// one, and forcing them into `Negai` would put `DeleteToLineStart` and
    /// `SearchPreviewStep` in front of every plugin author and make the
    /// capability question meaningless (what capability does a caret move
    /// read?). One type serving two vocabularies is the mistake this avoids.
    ///
    /// The plan's M3 predicate was "apply_resolved contains zero `self.`
    /// mutations", which would have forced exactly that. Amended: the
    /// invariant worth having is ONE IMPLEMENTATION PER MUTATION, not one
    /// vocabulary. See docs/backlog-plan.md §V Phase 1.
    fn lower(action: &Action, active: BufferId) -> Option<Vec<Negai>> {
        Some(match action {
            Action::Quit => vec![Negai::Quit],
            Action::ClearSearchHighlight => vec![Negai::ClearSearchHighlight],
            Action::Save => vec![Negai::Save { buffer: active }],
            Action::Undo => vec![Negai::Undo { buffer: active }],
            Action::Redo => vec![Negai::Redo { buffer: active }],
            // `apply_edit` was a STUB that did nothing, so this action was a
            // silent no-op while `Negai::Edit` applied for real. Lowering it
            // makes keymap-originated edits work for the first time — and
            // nothing binds it today, so the duplication goes away at zero
            // risk.
            Action::Edit(edit) => vec![Negai::Edit {
                buffer: active,
                edit: edit.clone(),
            }],
            _ => return None,
        })
    }

    /// What the world currently is, for freshness.
    ///
    /// One text axis per open buffer. A list sealed against this is fresh
    /// exactly while the buffers it depends on are unchanged — and a buffer
    /// that has since CLOSED drops out, which makes lists about it stale
    /// rather than silently kept.
    #[must_use]
    pub fn world(&self) -> escriba_shirube::Anchor {
        let mut a = escriba_shirube::Anchor::new();
        for id in self.buffers.ids() {
            if let Some(b) = self.buffers.get(id) {
                a = a.on(escriba_shirube::Axis::Text(id, b.text_rev()));
            }
        }
        // Every axis the world can move, ALWAYS present — not only the ones
        // some producer happens to use today.
        //
        // `Anchor::is_fresh` treats an ABSENT axis as stale, deliberately:
        // unknowable is not unchanged. The consequence, unnoticed until a
        // recon pass went looking, is that a list anchored on an axis this
        // function never emits is born PERMANENTLY stale — `]c` would answer
        // "that list is out of date" forever, and nothing would say why. The
        // two-axis model was built for git hunks and then only ever fed one
        // axis.
        //
        // Emitting them unconditionally means a producer can anchor on any
        // axis and get an honest answer. A counter that never moves reads as
        // "unchanged", which is exactly right for a plane escriba does not
        // track yet.
        a = a.on(escriba_shirube::Axis::Index(self.index_rev));
        // Both session kinds, unconditionally, for the reason above: a
        // producer anchoring on either must get an honest answer rather than
        // permanent staleness. They are separate axes because they move
        // independently — a picker closing must not invalidate diagnostics.
        a = a.on(escriba_shirube::Axis::Session(
            escriba_shirube::SessionKind::Lsp,
            self.lsp_gen,
        ));
        a.on(escriba_shirube::Axis::Session(
            escriba_shirube::SessionKind::Scan,
            self.scan_gen,
        ))
    }

    /// Where the cursor is, WITH the buffer it is in.
    ///
    /// Every jumplist push goes through this. A bare `Position` is what let
    /// `<C-o>` return to the right line in the wrong file.
    #[must_use]
    pub fn spot(&self) -> escriba_core::Spot {
        escriba_core::Spot::new(self.active, self.cursor())
    }

    /// Move to a `Spot`, switching buffer if it names another one.
    ///
    /// The read half of [`spot`](Self::spot). `<C-o>` and `<C-i>` both land
    /// here so neither can forget the buffer.
    fn goto_spot(&mut self, s: escriba_core::Spot) {
        if s.buffer != self.active && self.buffers.get(s.buffer).is_some() {
            self.active = s.buffer;
        }
        let clamped = self
            .buffers
            .get(self.active)
            .map_or(s.pos, |b| b.clamp(s.pos));
        self.set_cursor(clamped);
    }

    /// Advance the git-index generation — every list anchored on
    /// `Axis::Index` goes stale.
    ///
    /// Not called yet; a git layer calls it after a stage/reset. Present so
    /// the axis is WIRED rather than declared, because an axis nothing can
    /// move is indistinguishable from an axis that does not exist.
    pub fn bump_index_rev(&mut self) {
        self.index_rev = escriba_shirube::IndexRev(self.index_rev.0.wrapping_add(1));
    }

    /// Advance the language-server generation — a server restarted, so every
    /// diagnostic it produced describes a conversation that no longer exists.
    /// See [`bump_index_rev`](Self::bump_index_rev).
    pub fn bump_lsp_gen(&mut self) {
        self.lsp_gen = escriba_shirube::SessionGen(self.lsp_gen.0.wrapping_add(1));
    }

    /// Advance the filesystem-scan generation.
    ///
    /// This is what makes a superseded scan's rows stale. A scan runs on its
    /// own thread and cannot be stopped mid-walk; bumping this means its
    /// remaining batches are *ignored on arrival*, which is a reply filter, not
    /// cancellation — the thread keeps going until it notices. Say the weaker
    /// thing, because the stronger one is not true.
    ///
    /// Deliberately separate from [`bump_lsp_gen`](Self::bump_lsp_gen): they
    /// move for unrelated reasons, and when they shared one axis, closing a
    /// picker staled every diagnostic in the gutter.
    pub fn bump_scan_gen(&mut self) {
        self.scan_gen = escriba_shirube::SessionGen(self.scan_gen.0.wrapping_add(1));
    }

    /// Install the courier's runners. Called once, by the composition root.
    pub fn hire(&mut self, crew: escriba_madoguchi::errand::Crew) {
        self.courier.hire(crew);
    }

    /// Diagnose every buffer that is already open.
    ///
    /// **Call this LAST, after the plan has been applied.** It has two
    /// prerequisites and they are in different places: the courier needs its
    /// crew ([`hire`](Self::hire)), and `language_of` needs the filetype table,
    /// which `apply_plan_to_filetypes` fills from the catalog's `defmode`
    /// forms. Both are set up by the composition root, in that order, and this
    /// belongs after both.
    ///
    /// # Why this exists at all
    ///
    /// `ask_for_diagnostics` had exactly one caller — `Negai::OpenPath`, the
    /// path a file takes when opened from INSIDE the editor. A file named on
    /// the command line never travels it: the composition root reads the file
    /// and hands the buffer to `new_with_buffer`. So `escriba main.rs` opened a
    /// buffer that was never diagnosed while `:e main.rs` on the same file was,
    /// and nothing on screen distinguished them — a file with errors simply
    /// looked clean.
    ///
    /// # The ordering trap, which cost an hour
    ///
    /// The first fix hung this off `hire`, reasoning that hiring is the moment
    /// diagnosis becomes possible. It is not: hiring makes the courier able to
    /// RUN, but the filetype table is still empty there, so every errand went
    /// out carrying `language: None`. The runner then fell back to its own
    /// two-extension `language_of`, which does not know `.b`, and declined —
    /// **silently, and correctly**, because "most files have no server" is the
    /// common case and saying so on every open would be noise.
    ///
    /// Nothing was broken enough to report. `blue check` said `diag.b:8:3`
    /// while the gutter stayed empty, and the only visible difference between
    /// "no server for this language" and "the language was not resolved yet"
    /// was a `None` in a struct field. **A prerequisite that is satisfied
    /// somewhere else, later, is not a prerequisite the caller can see.**
    pub fn diagnose_open_buffers(&mut self) {
        // Every buffer, not just the active one: a session restored with a
        // split, or a future `escriba a.rs b.rs`, opens more than one.
        for id in self.buffers.ids() {
            self.ask_for_diagnostics(id);
        }
    }

    /// What an errand of this class depends on — the ONE place a courier
    /// anchor is minted.
    ///
    /// A total match, so a new [`Freight`](escriba_madoguchi::errand::Freight)
    /// variant does not compile until somebody decides what makes its results
    /// stale. That is the point: the failure this prevents is not a wrong
    /// answer, it is an errand class shipping with no freshness rule at all and
    /// nobody noticing, because "no rule" reads at runtime as "always fresh".
    ///
    /// The return type forbids the empty anchor by construction — see
    /// [`NonEmptyAnchor`](escriba_shirube::NonEmptyAnchor).
    fn seal(
        &self,
        freight: &escriba_madoguchi::errand::Freight,
    ) -> escriba_shirube::NonEmptyAnchor {
        use escriba_madoguchi::errand::Freight;
        use escriba_shirube::{Axis, NonEmptyAnchor, SessionKind};
        match freight {
            // A scan reads the filesystem, which no axis tracks, so text
            // revisions are irrelevant to it — anchoring one on the buffers
            // would make it die on the next keystroke for no reason. What DOES
            // supersede it is the surface it feeds opening or closing, which is
            // exactly what `scan_gen` counts.
            Freight::Scan { .. } => {
                NonEmptyAnchor::on(Axis::Session(SessionKind::Scan, self.scan_gen))
            }
            // Diagnostics describe ONE buffer at one revision, from one server
            // session. Narrow on purpose: anchoring on the whole world would
            // mean an edit in an unrelated buffer discards them.
            Freight::Diagnostics { buffer, .. } => {
                let rev = self.buffers.get(*buffer).map_or_else(
                    escriba_buffer::TextRev::default,
                    escriba_buffer::Buffer::text_rev,
                );
                NonEmptyAnchor::on(Axis::Text(*buffer, rev))
                    .and(Axis::Session(SessionKind::Lsp, self.lsp_gen))
            }
            // A formatter reply REWRITES text. It must be judged against the
            // revision it read and nothing else — the whole hazard is applying
            // one to a buffer the operator kept typing into.
            Freight::Format { path, .. } => match self.buffers.find_by_path(path) {
                Some(id) => {
                    let rev = self.buffers.get(id).map_or_else(
                        escriba_buffer::TextRev::default,
                        escriba_buffer::Buffer::text_rev,
                    );
                    NonEmptyAnchor::on(Axis::Text(id, rev))
                }
                // No open buffer for that path: seal on the LSP session so the
                // reply is judged against SOMETHING. Never an empty anchor —
                // that would be fresh forever, which is the whole hazard.
                None => NonEmptyAnchor::on(Axis::Session(SessionKind::Lsp, self.lsp_gen)),
            },
        }
    }

    /// How many courier replies one tick may apply.
    ///
    /// Bounded so a chatty runner cannot hold a frame open. The remainder is
    /// not dropped — it lands on the next tick.
    const DELIVER_BUDGET: usize = 64;

    /// Apply whatever the courier has delivered since the last tick.
    ///
    /// Must run BEFORE input translation: a redraw event maps to
    /// `InputOutcome::None`, so a drain hung off the input path would never see
    /// a tick that carried no keystroke — which is every tick during a scan.
    pub fn deliver(&mut self) {
        let slips = self.courier.drain(Self::DELIVER_BUDGET);
        if slips.is_empty() {
            return;
        }
        for slip in slips {
            self.honour_one(slip);
        }
        // Something landed, so the screen is out of date.
        self.bump_gen();
    }

    /// Move the cursor to the next/previous finding in `list`.
    ///
    /// Reports the wrap, because `n`/`N` do and a reader losing their place
    /// in a long file is the same problem either way.
    fn walk_list(&mut self, list: &str, forward: bool) {
        let world = self.world();
        let Some(result) = self.results.get(list) else {
            let mut m = String::from("no list named ");
            m.push_str(list);
            self.messages.push(m);
            return;
        };
        if result.is_stale(&world) {
            self.messages
                .push("that list is out of date — run it again".to_string());
            return;
        }
        let here = (Some(self.active), self.cursor().line);
        let Some(found) = result.step(&world, here, forward, escriba_shirube::Bound::Exclusive)
        else {
            let mut m = String::from("no entries in ");
            m.push_str(list);
            self.messages.push(m);
            return;
        };
        let site = found.site.clone();
        let msg = found.message.clone();
        self.jump_to_site(&site);
        self.messages.push(msg);
    }

    /// Move the cursor to a located finding's SITE — the one operation that
    /// cannot drop the buffer half of a location.
    ///
    /// A `Site` is `(buffer, range)`. Every jumper before this re-derived the
    /// move itself and clamped against `self.active`, so a finding in another
    /// file landed on the right LINE in the WRONG file. `on_line` and
    /// `worst_on_line` already filter by buffer, so the gutter and the walker
    /// disagreed — latent only because the first producer scanned one buffer.
    ///
    /// Every future producer (diagnostics, hunks, grep hits, test failures)
    /// is cross-file by nature, which is why this is a shared operation
    /// rather than a fix at the one call site that has it wrong today.
    ///
    /// Always a FAR jump: it pushes the jumplist, so `<C-o>` returns from a
    /// `]t` exactly as it returns from an `n`.
    pub fn jump_to_site(&mut self, site: &escriba_shirube::Site) {
        self.jumps.push(self.spot());
        // Switch buffers FIRST — clamping against the wrong buffer is how the
        // position gets silently mangled before anyone can notice.
        if let Some(target) = site.buffer {
            if target != self.active && self.buffers.get(target).is_some() {
                self.active = target;
                self.refollow_cursor();
            }
        }
        let to = site.range.start;
        let clamped = self.buffers.get(self.active).map_or(to, |b| b.clamp(to));
        self.set_cursor(clamped);
    }

    /// Close a buffer, keeping "there is always an active buffer" true.
    ///
    /// The invariant is the whole reason this is not just
    /// `self.buffers.close(id)`. `EditorState::active` is a `BufferId`, not
    /// an `Option`, so a dangling active is not a degraded state — it is a
    /// state where every read of the active buffer returns `None` and the
    /// editor renders `<no buffer>` forever. Closing the last buffer opens a
    /// scratch rather than emptying the set, which is what vim's `:bd` does
    /// and what the type demands.
    fn close_buffer(&mut self, id: BufferId) {
        if self.buffers.close(id).is_none() {
            self.messages.push("no such buffer".to_string());
            return;
        }
        if self.active != id {
            return;
        }
        // The active buffer went. Prefer the next one by id so repeated
        // closes walk forward predictably rather than jumping around.
        let next = self.buffers.ids().into_iter().find(|b| *b > id);
        self.active = match next.or_else(|| self.buffers.ids().into_iter().next_back()) {
            Some(b) => b,
            None => self.buffers.scratch(""),
        };
        self.set_cursor(Position::ZERO);
        if let Some(w) = self.layout.active_window_mut() {
            w.buffer_id = self.active;
        }
    }

    /// Move to the next or previous buffer, wrapping.
    fn cycle_buffer(&mut self, forward: bool) {
        let ids = self.buffers.ids();
        if ids.len() < 2 {
            self.messages.push("only one buffer".to_string());
            return;
        }
        let at = ids.iter().position(|b| *b == self.active).unwrap_or(0);
        let next = if forward {
            (at + 1) % ids.len()
        } else {
            (at + ids.len() - 1) % ids.len()
        };
        self.active = ids[next];
        self.set_cursor(Position::ZERO);
        if let Some(w) = self.layout.active_window_mut() {
            w.buffer_id = self.active;
        }
    }

    /// Re-clamp the cursor and re-contain the viewport after a buffer
    /// mutation.
    ///
    /// An undo can SHRINK the buffer under a cursor that was legal a moment
    /// ago, leaving it out of bounds and its viewport scrolled past the end.
    /// The Action executor has always done this (`self.set_cursor(self.cursor())`
    /// after undo/redo/save); the M1 interpreter did NOT, so `u` re-followed
    /// and `:undo` did not — two implementations of one operation, already
    /// drifted within one milestone of being written. Naming it once is the
    /// fix; lowering the Action arms onto the same slips is what keeps it
    /// fixed.
    fn refollow(&mut self) {
        self.set_cursor(self.cursor());
    }

    /// Apply one slip and record what it damaged.
    ///
    /// The bookkeeping wrapper. The Action executor calls
    /// [`honour_one`](Self::honour_one) directly because it does its own,
    /// wider bookkeeping (the dot register, the S3 damage seal) around a
    /// whole action.
    fn honour(&mut self, slip: Negai) {
        let touches_text = slip.touches_text();
        self.honour_one(slip);
        self.damage = self.damage.join(if touches_text {
            Damage::Full
        } else {
            Damage::Viewport
        });
        self.bump_gen();
    }

    /// Apply one slip. THE single implementation of every mutation a slip
    /// can ask for.
    ///
    /// Total over `Negai`: a new request variant is a compile error here
    /// rather than a request silently ignored — the same failure Phase 0
    /// removed one layer up.
    fn honour_one(&mut self, slip: Negai) {
        match slip {
            Negai::Edit { buffer, edit } => {
                if let Some(b) = self.buffers.get_mut(buffer) {
                    let _ = b.apply(&edit);
                }
                self.refollow();
            }
            Negai::SetCursor { buffer, to } => {
                // Clamping is the interpreter's job, exactly so that no
                // handler has to re-implement it and get it wrong.
                let clamped = self.buffers.get(buffer).map_or(to, |b| b.clamp(to));
                self.set_cursor(clamped);
            }
            Negai::EnterMode(m) => self.modal.enter(m),
            Negai::OpenPicker(source) => self.open_picker(source),
            Negai::SplitWindow { stacked } => {
                let axis = if stacked {
                    escriba_ui::shikiri::Axis::Stacked
                } else {
                    escriba_ui::shikiri::Axis::SideBySide
                };
                self.layout.split_active(axis);
                // The new pane is narrower/shorter than the old one, so the
                // cursor can now be outside it. Every face re-reports its
                // frame on the next draw, but the invariant must hold NOW —
                // an operator who splits and immediately types should not be
                // editing off-screen.
                self.refollow_cursor();
                self.damage = self.damage.join(Damage::Viewport);
            }
            Negai::CloseWindow => {
                let id = self.layout.active();
                if self.layout.close(id) {
                    self.refollow_cursor();
                    self.damage = self.damage.join(Damage::Viewport);
                } else {
                    // vim's E444, and the same refusal: the last window is
                    // the editor. Closing it would mean "quit", which is a
                    // different verb the operator did not type.
                    self.messages
                        .push("E444: Cannot close last window".to_string());
                }
            }
            Negai::FocusDir { dx, dy } => {
                use escriba_ui::Dir;
                let dir = match (dx, dy) {
                    (d, _) if d < 0 => Dir::Left,
                    (d, _) if d > 0 => Dir::Right,
                    (_, d) if d < 0 => Dir::Up,
                    _ => Dir::Down,
                };
                if let Some(id) = self.layout.neighbour(dir) {
                    self.layout.focus(id);
                    // The window we moved to has its OWN buffer; the editor's
                    // active buffer follows focus, or the next keystroke
                    // would edit the file we just navigated away from.
                    if let Some(w) = self.layout.active_window() {
                        self.active = w.buffer_id;
                    }
                    self.refollow_cursor();
                    self.damage = self.damage.join(Damage::Viewport);
                }
                // No neighbour is not an error — it is the edge of the
                // layout, and vim says nothing there either.
            }
            Negai::GrepProject { pattern } => self.grep_project(&pattern),
            Negai::FormatBuffer => self.ask_for_format(self.active),
            Negai::ToggleBreakpoint => self.toggle_breakpoint(),
            Negai::CycleBuffer { forward } => self.cycle_buffer(forward),
            Negai::FocusBuffer(id) => {
                if self.buffers.get(id).is_some() {
                    self.active = id;
                }
            }
            Negai::OpenPath(path) => match self.buffers.open(&path) {
                Ok(id) => {
                    self.active = id;
                    self.ask_for_diagnostics(id);
                }
                Err(e) => self.messages.push(e.to_string()),
            },
            Negai::CloseBuffer(id) => self.close_buffer(id),
            Negai::Save { buffer } => {
                if let Some(b) = self.buffers.get_mut(buffer) {
                    if let Err(e) = b.save() {
                        self.messages.push(e.to_string());
                    }
                }
                self.refollow();
            }
            Negai::Undo { buffer } => {
                if let Some(b) = self.buffers.get_mut(buffer) {
                    let _ = b.undo();
                }
                self.refollow();
            }
            Negai::Redo { buffer } => {
                if let Some(b) = self.buffers.get_mut(buffer) {
                    let _ = b.redo();
                }
                self.refollow();
            }
            Negai::Yank { text, kind, .. } => self.register = Some(Register::new(text, kind)),
            Negai::ClearSearchHighlight => self.search.clear_highlight(),
            Negai::SetOption { name, value } => {
                self.options.insert(name, value);
            }
            Negai::InsertText(text) => self.insert_text(&text),
            Negai::RunCommand { name, args } => self.run_command(&name, &args),
            Negai::PublishFindings { list, findings } => {
                let world = self.world();
                self.results
                    .publish(list, escriba_shirube::ResultList::new(findings, world));
            }
            // Sealed at `world()`, exactly like the `PublishFindings` above and
            // for the same reason: an ON-TICK producer computed this against
            // the world as it is right now. The off-tick path is the
            // `ErrandReply` arm below, which must NOT reseal — see the note
            // there.
            Negai::PublishSemanticTokens { buffer, tokens } => {
                let world = self.world();
                self.semantic = Some(SemanticPaint::new(buffer, tokens, world));
            }
            // A reply from off the tick. Honour it only if the world it was
            // computed against still holds.
            //
            // The drop is silent BY DESIGN, and this is the one place in the
            // slip vocabulary where silence is right: a stale reply is not a
            // failure anyone can act on. The operator kept typing, which is
            // the correct thing to have done, and telling them "a diagnostic
            // you never asked for was discarded" is noise. The producer
            // re-runs against the new world; that is the whole contract.
            Negai::ErrandReply { anchor, then } => {
                if anchor.is_fresh(&self.world()) {
                    match *then {
                        // The reply's OWN anchor becomes the list's seal.
                        //
                        // Passing the gate and then re-sealing at `world()` —
                        // which is what the direct `PublishFindings` arm does,
                        // correctly, for an on-tick producer — is wrong for a
                        // reply that crossed a thread. It widens a narrow claim
                        // into a broad one: findings that depended on one
                        // buffer would be stored as depending on every open
                        // buffer, so an edit anywhere kills them. And it
                        // upgrades an unearned claim into a durable one.
                        //
                        // Special-cased on this one payload because
                        // `ErrandReply` WRAPS a slip rather than putting an
                        // anchor field on `PublishFindings`; adding one there
                        // now would reopen exactly the hole the wrapper closed.
                        Negai::PublishFindings { list, findings } => {
                            self.results.publish(
                                list.clone(),
                                escriba_shirube::ResultList::new(findings, anchor),
                            );
                            self.refresh_projected_picker(&list);
                        }
                        // The second payload of one language-server
                        // conversation, and special-cased here for exactly the
                        // reason `PublishFindings` is: it must keep the
                        // reply's OWN anchor. Falling through to
                        // `honour_one` would reseal it at `world()` — passing
                        // the gate and then widening the claim, which turns
                        // "these colours describe buffer 3 at revision 7" into
                        // "these colours describe the world", so the next
                        // keystroke in ANY buffer would blank them and an
                        // unearned claim would have been made durable.
                        Negai::PublishSemanticTokens { buffer, tokens } => {
                            self.semantic = Some(SemanticPaint::new(buffer, tokens, anchor));
                        }
                        other => self.honour_one(other),
                    }
                }
            }
            Negai::WalkList { list, forward } => self.walk_list(&list, forward),
            Negai::Message(m) => self.messages.push(m),
            Negai::Quit => self.quit_requested = true,
            // Hand the freight to the courier, sealed against the world it
            // was dispatched in.
            //
            // The seal happens HERE and nowhere else. A handler named a class
            // of work; it did not — and could not — say what world that work
            // depends on, because a handler holds a read-only snapshot. If the
            // slip carried an anchor, any handler could mint one depending on
            // nothing, which is fresh forever, and the freshness gate would
            // stop meaning anything.
            Negai::Errand(freight) => {
                let anchor = self.seal(&freight);
                self.courier.send(*freight, anchor);
            }
            // Still unwired: the AwaitKey resume (M3). Announced, never
            // silently dropped — a slip that vanishes is the class Phase 0
            // sealed.
            Negai::AwaitKey { .. } => {
                self.messages
                    .push("deferred work is not wired yet".to_string());
            }
        }
    }
}

/// Outcome of feeding one key to the multi-key pending-stroke loop.
enum SeqStep {
    /// Key consumed into an in-progress sequence; wait for the next.
    Pending,
    /// A full bound sequence resolved — run this action.
    Resolved(Action),
    /// Key is not part of any sequence; hand it to single-key dispatch.
    Passthrough,
}

/// Keys whose HELD repeat is a viewport storm, and which the repeat gate
/// therefore exists to debounce.
///
/// This is an ALLOW-LIST, and it used to be the complement — an exception list
/// of "discrete" keys that grew three times (`n`/`N`/`*`/`#`, then `.`/`u`/
/// `<C-r>`, then `/`/`?`/`:`), each time because a key had been silently
/// swallowed and someone noticed. The third growth is the signal that the
/// default was backwards: almost every key in a modal editor is a discrete,
/// deliberate press, and only a handful are ones you HOLD.
///
/// Inverting it makes the failure mode safe. Forgetting to list a key here now
/// means it is ungated — one extra keypress honoured — instead of silently
/// dropped, and a dropped key is indistinguishable from a dead one.
///
/// Measured cost of the old direction: `/foo<CR>` then `/<CR>` (vim's
/// reuse-the-previous-pattern) lost the second `/` outright, because the gate
/// is keyed by KEY and the two presses fell inside one debounce window.
const fn is_repeat_storm_candidate(key: &Key) -> bool {
    matches!(
        key,
        // The four navigation keys a user actually holds down. `h`/`l` and
        // `j`/`k` flood the motion path and thrash the viewport; everything
        // else is pressed once and meant once.
        Key::Char('h')
            | Key::Char('j')
            | Key::Char('k')
            | Key::Char('l')
            | Key::Left
            | Key::Right
            | Key::Up
            | Key::Down
    )
}

/// Turn a command failure into the sentence an operator should read.
///
/// The two failures mean genuinely different things and must not be reported
/// the same way:
///
/// - `:flurb` — the operator typed a name that does not exist. "command not
///   found" is exactly right; it says *you* made a typo.
/// - `<leader>ff` bound to `picker.files` — escriba's OWN shipped config
///   declares this, `--list-rc` counts it, and it is not built yet. Telling
///   the operator "command not found" blames them for a gap we shipped.
///
/// The discriminator is the dotted form. `:action` takes action SYMBOLS
/// (`picker.files`), never command names — that boundary is already pinned by
/// `action_naming_a_command_is_inert_not_recursive` in escriba-command. So a
/// dotted name that reached dispatch and resolved to nothing is a declared
/// capability with no implementation, which is precisely what the 85 entries
/// in `escriba/tests/action_resolution.rs` are.
fn describe_command_failure(name: &str, e: &escriba_command::CommandError) -> String {
    use escriba_command::CommandError as E;
    match e {
        // Already the right words — the registry knew it was declared.
        E::Unhandled(_) => e.to_string(),
        E::NotFound(n) if n.contains('.') => {
            let mut m = String::with_capacity(n.len() + 48);
            m.push('`');
            m.push_str(n);
            m.push_str("` is declared but not implemented yet");
            m
        }
        _ => {
            let _ = name;
            e.to_string()
        }
    }
}

/// What committing the open search prompt did.
///
/// The two commit paths — bare `/` and operated `d/` — used to own private
/// copies of the whole sequence (read origin+skip, `accept`, three-arm
/// match, `commit_step_skipping`), and they drifted: the operated one
/// never reported the wrap, so `d/foo<CR>` that wrapped the file was
/// silent where `/foo<CR>` printed "search hit BOTTOM, continuing at TOP".
///
/// Total, and matched exhaustively at BOTH call sites, so a new outcome is
/// a compile error in two places rather than a case one path quietly
/// forgets. It does not make divergence impossible — the two paths
/// genuinely differ at the landing step — it makes FORGETTING A CASE
/// impossible, which is the failure that actually happened.
enum CommitOutcome {
    /// The prompt committed and a match was found.
    Landed {
        origin: usize,
        step: escriba_search::Step,
    },
    /// Committed, but nothing matched. E486 already reported.
    NotFound,
    /// Nothing typed and no previous pattern. E35 already reported.
    NoPrevious,
    /// No prompt was open.
    NoPrompt,
}

/// A replayable text change.
#[derive(Debug, Clone)]
struct LastChange {
    /// The action that began the change.
    action: Action,
    /// How many times it ran.
    count: u32,
    /// Characters typed while the change held Insert mode open.
    inserted: String,
}

impl EditorState {
    /// Build a fresh editor with one buffer (scratch or file-backed).
    pub fn new_with_buffer(initial: BufferSet, active: BufferId) -> Self {
        let window = Window {
            id: WindowId(1),
            buffer_id: active,
            viewport: Viewport {
                top_line: 0,
                left_column: 0,
                visible_lines: 40,
                visible_columns: 160,
            },
        };
        Self {
            buffers: initial,
            modal: ModalState::new(),
            search: SearchState::new(escriba_search::CaseMode::Smart),
            search_at: None,
            last_change: None,
            recording_insert: false,
            jumps: JumpList::new(),
            keymap: Keymap::default_vim(),
            commands: CommandRegistry::default_set(),
            layout: Layout::single(window),
            active,
            cursors: Cursors::single(Position::ZERO),
            quit_requested: false,
            register: None,
            op_pending: zenmai::Stateful::new(OpState::Resting),
            pending_object: None,
            pending_find: None,
            pending_replace: false,
            last_find: None,
            pending_mark: None,
            marks: HashMap::new(),
            messages: Vec::new(),
            options: HashMap::new(),
            lisp_vm: None,
            pending_keys: Vec::new(),
            repeat_gate: KeyRepeatGate::new(),
            plugin_host: PluginHost::default(),
            edit_gen: EditGen::default(),
            damage: Damage::None,
            // The FLEET default until an rc says otherwise — never a
            // hand-written theme name, so a fleet re-point lands for free.
            dispatch_depth: 0,
            filetypes: escriba_core::FiletypeTable::new(),
            results: escriba_shirube::ListRegistry::new(),
            picker: None,
            index_rev: escriba_shirube::IndexRev::default(),
            lsp_gen: escriba_shirube::SessionGen::default(),
            scan_gen: escriba_shirube::SessionGen::default(),
            courier: courier::Courier::inert(),
            picker_projects: None,
            theme: FleetTheme::prescribed_default(),
            chrome: ChromePalette::prescribed(),
            splash: None,
            semantic: None,
            breakpoints: Breakpoints::default(),
        }
    }

    /// What a language server says `buffer`'s tokens mean, if that answer is
    /// still about this buffer at this revision.
    ///
    /// The ONLY reader — the field is private so a face cannot reach past the
    /// freshness check the way an earlier `results` reader could have. Empty is
    /// the honest answer for "no server", "not this buffer" and "you have typed
    /// since", and every one of them means the same thing to a renderer: paint
    /// with your own lexer.
    #[must_use]
    pub fn semantic_spans(&self, buffer: BufferId) -> &[escriba_madoguchi::SemanticSpan] {
        self.semantic
            .as_ref()
            .map_or(&[][..], |p| p.fresh(&self.world(), buffer))
    }

    /// Everything the gutter has to say about one line of one buffer.
    ///
    /// ONE function, so the three faces cannot disagree about which planes
    /// the gutter reads. Before this each face called `worst_on_line` for
    /// itself, which was fine while there was one plane and is exactly how a
    /// second plane lands on two faces out of three — the divergence
    /// `escriba_ui::gutter` was extracted to stop, reappearing one layer up
    /// in the ARGUMENTS rather than in the composition.
    ///
    /// Takes `world` rather than computing it so a face that already holds
    /// one for the frame does not rebuild it per line.
    #[must_use]
    pub fn gutter_marks(
        &self,
        world: &escriba_shirube::Anchor,
        buffer: BufferId,
        line: u32,
    ) -> escriba_ui::gutter::GutterMarks {
        escriba_ui::gutter::GutterMarks::new(
            self.results.worst_on_line(world, buffer, line),
            self.breakpoints.is_set(buffer, line),
        )
    }

    /// Where the operator asked the debugger to stop.
    ///
    /// Read-only: the only way to change it is [`Negai::ToggleBreakpoint`],
    /// so the refresh-generation bump that makes a face repaint cannot be
    /// forgotten by a caller that reached in and mutated the set.
    #[must_use]
    pub const fn breakpoints(&self) -> &Breakpoints {
        &self.breakpoints
    }

    /// The theme this editor is set to.
    #[must_use]
    pub const fn theme(&self) -> FleetTheme {
        self.theme
    }

    /// The colours every face paints with — read once per frame.
    #[must_use]
    pub const fn chrome(&self) -> ChromePalette {
        self.chrome
    }

    /// Point the editor at a theme. The wiring that makes
    /// `(deftheme :preset …)` real.
    ///
    /// Bumps the refresh generation, because a theme change repaints
    /// everything: the GPU face caches its shaped buffer against that
    /// generation and would otherwise keep the old colours until an
    /// unrelated edit happened to invalidate it.
    pub fn set_theme(&mut self, theme: FleetTheme) {
        if self.theme == theme {
            return;
        }
        self.theme = theme;
        self.chrome = ChromePalette::for_theme(theme);
        self.damage = self.damage.join(Damage::Viewport);
        self.bump_gen();
    }

    /// The start screen, if one is up. Renderers paint this INSTEAD of the
    /// buffer pane; `None` is the ordinary editor.
    #[must_use]
    pub fn splash(&self) -> Option<&Splash> {
        self.splash.as_ref()
    }

    /// Raise the start screen. The binary calls this at boot when no file
    /// was named; an empty splash is refused so a face never has to render
    /// a blank screen over a perfectly good buffer.
    pub fn set_splash(&mut self, splash: Splash) {
        if splash.is_empty() {
            return;
        }
        self.splash = Some(splash);
        self.damage = self.damage.join(Damage::Viewport);
        self.bump_gen();
    }

    /// Take the start screen down. Idempotent; bumps the refresh generation
    /// only when something actually changed, so dismissing twice does not
    /// cost a repaint.
    pub fn dismiss_splash(&mut self) {
        if self.splash.take().is_some() {
            self.damage = self.damage.join(Damage::Viewport);
            self.bump_gen();
        }
    }

    /// Offer `key` to the start screen.
    ///
    /// A menu key runs its entry; ANY other key simply takes the screen
    /// down and is then handled normally — so the first thing an operator
    /// types is never swallowed.
    /// The open picker, for a face to paint.
    #[must_use]
    pub fn picker(&self) -> Option<&escriba_ui::picker::Picker> {
        self.picker.as_ref()
    }

    /// Give an open picker the key.
    ///
    /// Runs BEFORE the keymap, and before the sequence stepper: while a
    /// picker is up it owns every key, including ones it has no meaning for.
    /// An overlay that let unknown keys fall through would edit the file
    /// behind itself.
    /// Ask the courier what a language server thinks of this buffer.
    ///
    /// Fires on open, and only on open. Re-asking on every keystroke is what a
    /// real diagnostics pump does, and it needs a session that outlives one
    /// errand plus `didChange` to keep the server's copy current — neither
    /// exists yet, and dispatching per keystroke against a one-shot runner
    /// would spawn a language server per character typed.
    ///
    /// A buffer with no path is skipped: a scratch buffer has no document for
    /// a server to have an opinion about.
    fn ask_for_diagnostics(&mut self, buffer: escriba_core::BufferId) {
        let Some(b) = self.buffers.get(buffer) else {
            return;
        };
        let Some(path) = b.path.clone() else {
            return;
        };
        let text = b.to_string();
        let language = self.language_of(&path);
        let freight = escriba_madoguchi::errand::Freight::Diagnostics {
            buffer,
            path,
            language,
            text,
        };
        let anchor = self.seal(&freight);
        self.courier.send(freight, anchor);
    }

    /// The language of `path`, as the CATALOG declared it.
    ///
    /// `FiletypeTable` is populated from every `(defmode … :extensions …)` the
    /// catalog carries, so this answer covers every language escriba has been
    /// told about — 11 of them today — rather than the two extensions
    /// `escriba_lsp_client::runner::language_of` hardcodes. That is why blue
    /// resolves here without anything in this crate naming blue: the
    /// declaration already existed, and nothing was reading it.
    ///
    /// `None` when the table has no entry, which is not an error — it is the
    /// answer for a plain `.txt`, and it is also what a `--no-defaults` boot
    /// gives for everything, so the runner keeps its own fallback.
    fn language_of(&self, path: &std::path::Path) -> Option<String> {
        self.filetypes.resolve(path).map(|f| f.name.clone())
    }

    /// Ask the formatter runner to format `buffer`.
    ///
    /// The buffer's CURRENT text goes with the errand and the anchor seals on
    /// its revision, so a reply computed against text the operator has since
    /// changed is refused rather than applied — see `seal`'s `Format` arm,
    /// which was written before anything could construct this errand.
    fn ask_for_format(&mut self, buffer: escriba_core::BufferId) {
        let Some(b) = self.buffers.get(buffer) else {
            return;
        };
        // A formatter needs a path: it is how the server is chosen and how the
        // project root is found. A scratch buffer has none, and saying so beats
        // silently doing nothing to a keystroke the operator pressed.
        let Some(path) = b.path.clone() else {
            self.messages
                .push("format: this buffer has no path".to_string());
            self.damage = self.damage.join(Damage::Viewport);
            self.bump_gen();
            return;
        };
        let text = b.to_string();
        let language = self.language_of(&path);
        let freight = escriba_madoguchi::errand::Freight::Format {
            buffer,
            path,
            language,
            text,
        };
        let anchor = self.seal(&freight);
        self.courier.send(freight, anchor);
    }

    /// Set or clear a breakpoint on the cursor's line of the active buffer.
    ///
    /// The SOLE writer of `self.breakpoints`, which is why the field is
    /// private and [`breakpoints`](Self::breakpoints) hands out a `&`: the
    /// mark has to reach the screen, and the GPU face rebuilds its cached
    /// shaped gutter buffer ONLY on a refresh-generation change
    /// (`escriba-render/src/gpu.rs:260`). A caller that reached in and
    /// mutated the set would change the state and not the picture until some
    /// unrelated edit invalidated the cache — the defect `set_theme` had.
    ///
    /// It does NOT bump the generation itself, and that is deliberate rather
    /// than an omission: [`honour`](Self::honour) widens the damage and bumps
    /// for EVERY slip, so doing it here as well would be a second
    /// implementation of one guarantee — measured 2026-08-12, the first cut
    /// of this method did exactly that and the redundancy was invisible
    /// because both spellings produce the same answer.
    fn toggle_breakpoint(&mut self) {
        let buffer = self.active;
        let line = self.cursor().line;
        // No buffer means no line to mark, and marking a row of nothing would
        // put a breakpoint somewhere a future DAP client cannot name.
        if self.buffers.get(buffer).is_none() {
            return;
        }
        let set = self.breakpoints.toggle(buffer, line);
        // Built by `push_str` + `Display`, not `format!` — ★★ TYPED EMISSION.
        // The number is 1-based, like the gutter it appears beside and like
        // every message vim prints; reporting the internal 0-based row would
        // disagree with the label painted next to the mark.
        let mut msg = String::from(if set {
            "breakpoint set at line "
        } else {
            "breakpoint cleared at line "
        });
        msg.push_str(&(line + 1).to_string());
        self.messages.push(msg);
    }

    /// Close the picker — the SOLE writer of `self.picker = None`.
    ///
    /// Sole on purpose. Closing has three consequences that must not come
    /// apart: the overlay goes, the screen repaints, and any scan feeding that
    /// overlay is superseded. Spread across call sites, one of them eventually
    /// forgets the third and a dismissed picker springs back open when a late
    /// batch arrives.
    ///
    /// `cancel_all` is asked as well as the generation bumped, but note which
    /// one is load-bearing: the bump is what makes late rows *ignored*, and it
    /// works whether or not any runner reads its flag. The cancel is a courtesy
    /// to a runner that checks.
    fn close_picker(&mut self) {
        self.picker = None;
        self.picker_projects = None;
        self.bump_scan_gen();
        self.courier.cancel_all();
        self.bump_gen();
    }

    fn consume_picker_key(&mut self, key: &Key) -> escriba_ui::picker::Consumed {
        use escriba_ui::picker::Consumed;
        let Some(p) = self.picker.as_mut() else {
            return Consumed::NotShowing;
        };
        let outcome = p.on_key(key);
        match &outcome {
            // BOTH arms close the picker — choosing a row dismisses it just as
            // surely as pressing Esc, and an earlier reading of this that only
            // considered Esc would have left a scan running after every pick.
            Consumed::Dismissed | Consumed::Chose(_) => self.close_picker(),
            Consumed::Held => self.bump_gen(),
            Consumed::NotShowing => {}
        }
        outcome
    }

    /// Lower an accepted pick into the ONE interpreter.
    ///
    /// The whole reason `Choice` is a closed enum: a new source must decide
    /// here, and the compiler says so.
    fn honour_choice(&mut self, choice: escriba_ui::picker::Choice) {
        use escriba_ui::picker::Choice;
        let slip = match choice {
            Choice::Buffer(id) => Negai::FocusBuffer(id),
            Choice::Command(name) => Negai::RunCommand {
                name,
                args: Vec::new(),
            },
            Choice::OpenFile(path) => Negai::OpenPath(path),
            Choice::Location { path, line } => {
                // Open FIRST, then jump: the buffer may not exist yet, and
                // `jump_to_site` needs a BufferId. Two slips, one interpret.
                self.interpret(Outcome::did(vec![Negai::OpenPath(path)]));
                let site = escriba_shirube::Site::in_buffer(
                    self.active,
                    escriba_core::Range::new(
                        escriba_core::Position::new(line, 0),
                        escriba_core::Position::new(line, 1),
                    ),
                );
                self.jump_to_site(&site);
                return;
            }
        };
        self.interpret(Outcome::did(vec![slip]));
    }

    /// How many files a project grep will read, and how many hits it keeps.
    ///
    /// BOUNDED, and the bound is here rather than hidden, because this is a
    /// SYNCHRONOUS scan on the editor's own thread. The interpreter already
    /// does synchronous filesystem I/O (`OpenPath`, `Save`), so the posture
    /// is not new — but those touch one file and this walks a tree, which is
    /// the first one big enough to freeze the editor.
    ///
    /// GREP NO LONGER USES THIS. The courier carries the scan now, with no
    /// ceiling at all, and `GREP_HIT_LIMIT` is gone with it — the bound was
    /// a symptom of walking the tree on the thread that draws the screen.
    ///
    /// It survives for the files and project pickers, which still build
    /// their row set synchronously at open. Moving those onto the courier
    /// is the same change again and has not been made.
    const GREP_FILE_LIMIT: usize = 2_000;

    /// Walk the working directory, bounded, returning `(files, truncated)`.
    ///
    /// ONE walker. grep, files and project each need to enumerate the tree,
    /// and three copies of a bounded traversal is three places to get the
    /// ceiling, the skip-list, or the truncation report subtly different.
    ///
    /// Skips dotfiles, `target` and `node_modules`. That is NOT a gitignore
    /// implementation and does not pretend to be — a real ignore crate comes
    /// with the courier.
    fn walk_project(limit: usize) -> (Vec<std::path::PathBuf>, bool) {
        Self::walk_from(std::path::Path::new("."), limit)
    }

    /// The same bounded walk, from an explicit root.
    ///
    /// `walk_project` is this with `.` — one traversal, two callers, rather
    /// than a second copy for "browse from somewhere else".
    fn walk_from(root: &std::path::Path, limit: usize) -> (Vec<std::path::PathBuf>, bool) {
        let mut out = Vec::new();
        let mut truncated = false;
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
                let path = entry.path();
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(path);
                    continue;
                }
                if out.len() >= limit {
                    truncated = true;
                    return (out, truncated);
                }
                out.push(path);
            }
        }
        (out, truncated)
    }

    /// Say plainly when a bounded scan stopped short.
    ///
    /// A truncated list presented as complete is the failure this codebase
    /// keeps finding in itself; it does not get to ship one.
    fn report_truncation(&mut self, truncated: bool) {
        if truncated {
            self.messages
                .push("scan stopped at the limit — results are INCOMPLETE".to_string());
        }
    }

    /// Scan the working directory for `pattern`, off the editor thread.
    ///
    /// This used to walk the tree inline, capped at 2,000 files and 500 hits
    /// because it ran on the thread that draws the screen — and it reported
    /// that truncation, honestly, as INCOMPLETE. Both ceilings are gone with
    /// the walk: the courier carries it, and results arrive in batches.
    ///
    /// The picker opens EMPTY and then projects the findings list. That is why
    /// there is no "no matches" message here any more — at dispatch time
    /// nothing is known yet, and guessing would mean reporting an empty result
    /// before the scan had read a single file.
    fn grep_project(&mut self, pattern: &str) {
        use escriba_ui::picker::{Picker, Source};
        if pattern.is_empty() {
            self.messages.push("grep: empty pattern".to_string());
            return;
        }
        // A fresh scan supersedes whatever was running, and clears the list so
        // the previous pattern's rows are not shown under the new one.
        self.bump_scan_gen();
        self.courier.cancel_all();
        self.results.clear(crate::scan::LIST);

        let freight = escriba_madoguchi::errand::Freight::Scan {
            raw: pattern.to_string(),
            case: escriba_search::CaseMode::Smart,
            root: std::path::PathBuf::from("."),
        };
        let anchor = self.seal(&freight);
        self.courier.send(freight, anchor);

        self.picker = Some(Picker::open(Source::Grep, Vec::new()));
        self.picker_projects = Some((true, Some(crate::scan::LIST.to_string())));
        self.bump_gen();
    }

    /// Re-list a projecting picker after `published` changed.
    ///
    /// A picker over a live producer is a VIEW of the registry, not a snapshot
    /// taken when it opened — otherwise a scan's later batches would have
    /// nowhere to land, since `Picker` has no append.
    fn refresh_projected_picker(&mut self, published: &str) {
        let Some((workspace, ref list)) = self.picker_projects else {
            return;
        };
        // Only the list this picker is projecting. A diagnostics publish must
        // not redraw a grep picker.
        if list.as_deref().is_some_and(|l| l != published) {
            return;
        }
        let items = self.finding_items(workspace, list.as_deref());
        if let Some(p) = self.picker.as_mut() {
            if p.refresh_items(items) {
                self.bump_gen();
            }
        }
    }

    /// Where accepting a finding should take the operator.
    ///
    /// Returns `None` for a finding whose site names neither a path nor a
    /// live buffer — that is a finding about nowhere, and offering it would
    /// give the operator a row that does nothing when pressed.
    fn finding_choice(&self, f: &escriba_shirube::Finding) -> Option<escriba_ui::picker::Choice> {
        use escriba_ui::picker::Choice;
        let line = f.site.range.start.line;
        if let Some(p) = &f.site.path {
            return Some(Choice::Location {
                path: p.clone(),
                line,
            });
        }
        let id = f.site.buffer?;
        let b = self.buffers.get(id)?;
        // A buffer WITH a path becomes a Location, so the line survives. A
        // scratch buffer has no path to name, so the best available answer
        // is the buffer itself — and the line is lost. Stated rather than
        // hidden: a `Choice` that carried a BufferId AND a line would be
        // the fix, and it belongs with the picker, not here.
        b.path.as_ref().map_or(Some(Choice::Buffer(id)), |p| {
            Some(Choice::Location {
                path: p.clone(),
                line,
            })
        })
    }

    /// How a finding's location reads in a list row.
    fn finding_label(&self, f: &escriba_shirube::Finding) -> String {
        if let Some(p) = &f.site.path {
            return p.to_string_lossy().into_owned();
        }
        f.site
            .buffer
            .and_then(|id| self.buffers.get(id))
            .and_then(|b| b.path.as_ref())
            .map_or_else(
                || String::from("[scratch]"),
                |p| p.to_string_lossy().into_owned(),
            )
    }

    /// Picker rows for every file under `root`.
    ///
    /// `picker.files` and `files.open-parent` differ only in the root, so
    /// they share this rather than carrying two copies of the same body —
    /// which is what they did for one commit, and what the line-count lint
    /// correctly complained about.
    fn file_items(
        &mut self,
        root: &std::path::Path,
    ) -> Vec<escriba_ui::picker::PickerItem<escriba_ui::picker::Choice>> {
        use escriba_ui::picker::{Choice, PickerItem};
        let (files, truncated) = Self::walk_from(root, Self::GREP_FILE_LIMIT);
        self.report_truncation(truncated);
        files
            .into_iter()
            .map(|p| {
                let label = p.to_string_lossy().into_owned();
                PickerItem::new(Choice::OpenFile(p), label)
            })
            .collect()
    }

    /// Picker rows for the located findings the `trouble.*` verbs show.
    ///
    /// Freshness is asked of the registry, not assumed: `fresh` filters
    /// against the CURRENT world, so a list anchored to a revision the
    /// buffer has moved past contributes nothing rather than offering a
    /// line that has since shifted.
    fn finding_items(
        &self,
        workspace: bool,
        only: Option<&str>,
    ) -> Vec<escriba_ui::picker::PickerItem<escriba_ui::picker::Choice>> {
        use escriba_ui::picker::PickerItem;
        let world = self.world();
        let active = Some(self.active);
        let mut items = Vec::new();
        // `None` means every list — the trouble.* behaviour, unchanged.
        // `Some(n)` narrows to one producer, so a grep picker does not also
        // render LSP diagnostics and the marker scan.
        let names: Vec<&str> = only.map_or_else(|| self.results.names(), |n| vec![n]);
        for name in names {
            let Some(list) = self.results.get(name) else {
                continue;
            };
            for f in list.fresh(&world) {
                // `trouble.document` narrows to the buffer in front of the
                // operator. A finding that names only a path is
                // workspace-scoped by construction — it has no buffer to be
                // "this" one.
                if !workspace && f.site.buffer != active {
                    continue;
                }
                let Some(choice) = self.finding_choice(f) else {
                    continue;
                };
                let line = f.site.range.start.line;
                let mut label = String::with_capacity(64);
                label.push_str(f.severity.label());
                label.push_str("  ");
                label.push_str(&self.finding_label(f));
                label.push(':');
                label.push_str(&(line + 1).to_string());
                label.push_str("  ");
                label.push_str(&f.message);
                items.push(PickerItem::new(choice, label));
            }
        }
        items
    }

    /// Build and open a picker over `source`.
    fn open_picker(&mut self, source: escriba_madoguchi::PickerSource) {
        use escriba_ui::picker::{Choice, Picker, PickerItem, Source};
        let (src, items) = match source {
            escriba_madoguchi::PickerSource::Buffers => (
                Source::Buffers,
                self.buffers
                    .ids()
                    .into_iter()
                    .filter_map(|id| {
                        let b = self.buffers.get(id)?;
                        let label = b.path.as_ref().map_or_else(
                            || String::from("[scratch]"),
                            |p| p.to_string_lossy().into_owned(),
                        );
                        Some(PickerItem::new(Choice::Buffer(id), label))
                    })
                    .collect::<Vec<_>>(),
            ),
            escriba_madoguchi::PickerSource::Help => (
                Source::Help,
                self.keymap
                    .entries_sorted()
                    .into_iter()
                    .map(|(mode, key, b)| {
                        // "NORMAL  gd   goto definition" — searchable by key,
                        // by mode, or by what it does, because a reader
                        // arrives from any of the three.
                        let mut label = String::with_capacity(48);
                        label.push_str(mode.as_str());
                        label.push_str("  ");
                        // `{key:?}` because there is no shared key FORMATTER
                        // in the fleet — awase owns the chord vocabulary but
                        // escriba-keymap's `Key` has no Display. That gap
                        // belongs to the keymap consolidation, not here, and
                        // inventing a fourth spelling would make it worse.
                        label.push_str(&format!("{key:?}"));
                        label.push_str("  ");
                        label.push_str(&b.description);
                        // Accepting runs the binding's action if it names a
                        // command; a typed Action has no name to run, so it
                        // reports rather than pretending.
                        let choice = match &b.action {
                            escriba_core::Action::Command { name, .. } => {
                                Choice::Command(name.clone())
                            }
                            other => Choice::Command(format!("{other:?}")),
                        };
                        PickerItem::new(choice, label)
                    })
                    .collect::<Vec<_>>(),
            ),
            escriba_madoguchi::PickerSource::Files => {
                (Source::Files, self.file_items(std::path::Path::new(".")))
            }
            escriba_madoguchi::PickerSource::Project => {
                // A project root is a directory carrying a marker. Derived
                // from the SAME walk rather than a second traversal — the
                // markers are files, so the walker already visited them.
                const MARKERS: &[&str] = &[
                    "Cargo.toml",
                    "flake.nix",
                    "package.json",
                    "go.mod",
                    "pyproject.toml",
                ];
                let (files, truncated) = Self::walk_project(Self::GREP_FILE_LIMIT);
                self.report_truncation(truncated);
                let mut roots: Vec<std::path::PathBuf> = files
                    .into_iter()
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|n| MARKERS.contains(&n.to_string_lossy().as_ref()))
                    })
                    .filter_map(|p| p.parent().map(std::path::Path::to_path_buf))
                    .collect();
                roots.sort();
                roots.dedup();
                (
                    Source::Project,
                    roots
                        .into_iter()
                        .map(|p| {
                            let label = p.to_string_lossy().into_owned();
                            PickerItem::new(Choice::OpenFile(p), label)
                        })
                        .collect::<Vec<_>>(),
                )
            }
            escriba_madoguchi::PickerSource::Commands => (
                Source::Commands,
                self.commands
                    .names()
                    .into_iter()
                    .map(|n| PickerItem::new(Choice::Command(n.to_string()), n.to_string()))
                    .collect::<Vec<_>>(),
            ),
            escriba_madoguchi::PickerSource::FilesUnder(root) => {
                (Source::Files, self.file_items(&root))
            }
            escriba_madoguchi::PickerSource::Findings { workspace } => {
                (Source::Findings, self.finding_items(workspace, None))
            }
        };
        if items.is_empty() {
            self.messages.push("nothing to pick from".to_string());
            return;
        }
        self.picker = Some(Picker::open(src, items));
        self.bump_gen();
    }

    /// Read one key as operator-pending object selection.
    ///
    /// Returns `None` when the key is nothing to do with objects, so the
    /// ordinary path runs untouched.
    fn consume_object_key(&mut self, key: Key) -> Option<ObjectKey> {
        use escriba_core::TextObject as O;
        let Key::Char(c) = key else {
            // Esc (or anything non-printable) abandons a half-typed object
            // rather than leaving the editor silently armed.
            if self.pending_object.take().is_some() {
                self.op_pending
                    .dispatch((Action::ChangeMode(Mode::Normal), 1));
                return Some(ObjectKey::Consumed);
            }
            return None;
        };

        // Second key: it names the object.
        if let Some(around) = self.pending_object.take() {
            let object = match c {
                'w' => Some(O::Word { around }),
                // vim's `b` and `B` aliases for the bracket pairs, plus the
                // brackets themselves in both directions.
                '(' | ')' | 'b' => Some(O::Delimited {
                    open: '(',
                    close: ')',
                    around,
                }),
                '{' | '}' | 'B' => Some(O::Delimited {
                    open: '{',
                    close: '}',
                    around,
                }),
                '[' | ']' => Some(O::Delimited {
                    open: '[',
                    close: ']',
                    around,
                }),
                '<' | '>' => Some(O::Delimited {
                    open: '<',
                    close: '>',
                    around,
                }),
                // Quotes: `open == close`, which is what tells the resolver
                // not to count nesting.
                '"' => Some(O::Delimited {
                    open: '"',
                    close: '"',
                    around,
                }),
                '\'' => Some(O::Delimited {
                    open: '\'',
                    close: '\'',
                    around,
                }),
                '`' => Some(O::Delimited {
                    open: '`',
                    close: '`',
                    around,
                }),
                _ => None,
            };
            let OpState::Awaiting { op, count } = *self.op_pending.state() else {
                return Some(ObjectKey::Consumed);
            };
            // Disarm either way: an unknown object key cancels the operator,
            // it does not leave it armed for the next unrelated keystroke.
            self.op_pending
                .dispatch((Action::ChangeMode(Mode::Normal), 1));
            let Some(object) = object else {
                return Some(ObjectKey::Consumed);
            };
            let composed = Action::ApplyOperatorObject { op, object };
            // `2diw` applies the object twice. The caller runs it once, so
            // the extra repeats happen here.
            for _ in 1..count {
                self.apply(&composed);
            }
            return Some(ObjectKey::Compose(composed));
        }

        // First key: `i` or `a` while an operator waits.
        if matches!(c, 'i' | 'a') && matches!(self.op_pending.state(), OpState::Awaiting { .. }) {
            self.pending_object = Some(c == 'a');
            return Some(ObjectKey::Consumed);
        }
        None
    }

    /// Claim the operand of a pending `f`/`F`/`t`/`T`, or arm one.
    ///
    /// Runs BEFORE the sequence stepper and before the keymap, for the same
    /// reason [`Self::consume_object_key`] does: the character is an OPERAND,
    /// not a binding. `dfx` is undecidable from actions — `x` would resolve as
    /// whatever `x` is bound to — and `f` then `f` has to reach here rather
    /// than resolve as a two-key sequence.
    ///
    /// Composition with an operator is free: the armed motion is emitted as an
    /// ordinary `Action::Move`, so the operator-pending FSM composes `dfx`
    /// exactly the way it composes `dw`.
    fn consume_find_key(&mut self, key: Key) -> Option<ObjectKey> {
        if let Some(spec) = self.pending_find.take() {
            let Key::Char(ch) = key else {
                // Esc (or any non-printable) abandons a half-typed find rather
                // than leaving the editor armed for the next keystroke.
                if matches!(self.op_pending.state(), OpState::Awaiting { .. }) {
                    self.op_pending
                        .dispatch((Action::ChangeMode(Mode::Normal), 1));
                }
                return Some(ObjectKey::Consumed);
            };
            let spec = FindSpec { ch, ..spec };
            self.last_find = Some(spec);
            return Some(ObjectKey::Compose(Action::Move(Motion::FindChar {
                ch,
                backward: spec.backward,
                till: spec.till,
            })));
        }
        if self.modal.mode() != Mode::Normal && self.modal.mode() != Mode::Visual {
            return None;
        }
        // A key that is CONTINUING a sequence belongs to the sequence.
        //
        // Without this, `zt` was unreachable: `z` starts a pending sequence,
        // then `t` was claimed here as a till-find and the sequence never
        // completed. The rule "an operand key outranks a binding" is right
        // for the FIRST key of a gesture and wrong for a later one — by then
        // the gesture has already been chosen. Note the operand branch above
        // runs before this guard, so `zt` and `ft` are both reachable.
        if !self.pending_keys.is_empty() {
            return None;
        }
        let Key::Char(c) = key else { return None };
        let (backward, till) = match c {
            'f' => (false, false),
            'F' => (true, false),
            't' => (false, true),
            'T' => (true, true),
            _ => return None,
        };
        self.pending_find = Some(FindSpec {
            ch: '\0',
            backward,
            till,
        });
        Some(ObjectKey::Consumed)
    }

    /// Claim the operand of a pending `r`, or arm one.
    ///
    /// Same key-layer shape as [`Self::consume_find_key`], down to the
    /// mid-sequence guard: `r` is unbound in the keymap on purpose, because a
    /// binding on it would be a table entry no keypress can reach — which
    /// reads as configured and behaves as absent, the exact trap `f`/`t`
    /// documented.
    ///
    /// It runs AFTER the find capture and before the sequence stepper, and
    /// the relative order with find does not matter — the two arm on disjoint
    /// keys and neither can be pending while the other is.
    fn consume_replace_key(&mut self, key: Key) -> Option<ObjectKey> {
        if self.pending_replace {
            self.pending_replace = false;
            let Key::Char(ch) = key else {
                // Esc (or any non-printable) abandons a half-typed `r` rather
                // than replacing with something unprintable.
                return Some(ObjectKey::Consumed);
            };
            return Some(ObjectKey::Compose(Action::ReplaceChar(ch)));
        }
        if self.modal.mode() != Mode::Normal && self.modal.mode() != Mode::Visual {
            return None;
        }
        // A key CONTINUING a sequence belongs to the sequence — the `zt` rule.
        if !self.pending_keys.is_empty() {
            return None;
        }
        if key != Key::Char('r') {
            return None;
        }
        // `r` is not a motion, so `dr` is a typo — and vim treats it as one by
        // CANCELLING the operator. Falling through to the keymap instead is
        // not the same thing and is the worse reading: `r` is unbound (it has
        // to be, see above), so it resolves to `Action::Pending`, and the FSM
        // deliberately lets a stray `Pending` leave the operator armed for the
        // multi-key-sequence case. The next motion would then delete.
        if matches!(self.op_pending.state(), OpState::Awaiting { .. }) {
            self.op_pending
                .dispatch((Action::ChangeMode(Mode::Normal), 1));
            return Some(ObjectKey::Consumed);
        }
        self.pending_replace = true;
        Some(ObjectKey::Consumed)
    }

    fn consume_splash_key(&mut self, key: &Key) -> SplashKey {
        let Some(splash) = self.splash.as_ref() else {
            return SplashKey::NotShowing;
        };
        let chosen = match key {
            Key::Char(c) => splash.entry_for(*c).map(|e| e.action.clone()),
            _ => None,
        };
        self.dismiss_splash();
        chosen.map_or(SplashKey::Dismissed, SplashKey::Ran)
    }

    /// The current refresh generation. A renderer caches its products against
    /// this; equality is the freshness test (an unchanged generation ⇒ the
    /// last frame is still valid, so skip the re-highlight + re-shape).
    #[must_use]
    pub fn edit_gen(&self) -> EditGen {
        self.edit_gen
    }

    /// Advance the refresh generation (a mutation happened).
    fn bump_gen(&mut self) {
        self.edit_gen = self.edit_gen.next();
    }

    /// The accumulated dirty region (read-only). See [`take_damage`](Self::take_damage).
    #[must_use]
    pub fn damage(&self) -> Damage {
        self.damage
    }

    /// Drain the accumulated dirty region, resetting to [`Damage::None`]. The
    /// renderer calls this once per frame to learn what to repaint, then the
    /// accumulator restarts — so damage never double-counts across frames.
    pub fn take_damage(&mut self) -> Damage {
        std::mem::replace(&mut self.damage, Damage::None)
    }

    /// The line count of the active buffer (0 if none) — used to compute the
    /// [`Damage`] scope of a mutation.
    fn active_line_count(&self) -> u32 {
        self.buffers
            .get(self.active)
            .map_or(0, escriba_buffer::Buffer::line_count)
    }

    /// Register a lazy USER plugin: its escriba entry is deferred until
    /// one of its `triggers` fires. Bundled defaults do NOT go through
    /// here — they are applied eagerly at boot. Empty `triggers` means
    /// the plugin never lazily activates (the binary applies eager
    /// plugins directly).
    pub fn register_lazy_plugin(
        &mut self,
        name: impl Into<String>,
        triggers: Vec<LazyTrigger>,
        entry_src: impl Into<String>,
    ) {
        self.plugin_host.register(name, triggers, entry_src);
    }

    /// Apply a plugin entry's escriba-lisp to live state — the same
    /// keymap / command / option apply paths a user rc uses. Options are
    /// applied before keybinds so a plugin that sets `mapleader` resolves
    /// `<leader>` correctly. Returns the count of commands + keybinds it
    /// registered (best-effort; a malformed entry is skipped, not fatal).
    fn apply_plugin_entry(&mut self, entry_src: &str) -> usize {
        let Ok(plan) = escriba_lisp::apply_source(entry_src) else {
            return 0;
        };
        let cmd = escriba_lisp::apply_plan_to_commands(&plan, &mut self.commands);
        escriba_lisp::apply_plan_to_options(&plan, &mut self.options);
        if let Some(value) = self.options.get("mapleader") {
            if let Some(key) = escriba_lisp::parse_leader_key(value) {
                self.keymap.set_leader(key);
            }
        }
        let km = escriba_lisp::apply_plan_to_keymap(&plan, &mut self.keymap);
        (cmd.registered + km.keybinds_applied) as usize
    }

    /// Fire any lazy plugin gated on a `FileType` trigger for `filetype`.
    /// Returns the number of plugins activated. Call when a buffer of a
    /// known filetype is opened.
    pub fn activate_filetype_plugins(&mut self, filetype: &str) -> usize {
        let pending = self.plugin_host.pending_for_filetype(filetype);
        let n = pending.len();
        for src in pending {
            self.apply_plugin_entry(&src);
        }
        n
    }

    /// Fire any lazy plugin gated on an `Event` trigger for `event`.
    /// Returns the number of plugins activated.
    pub fn activate_event_plugins(&mut self, event: &str) -> usize {
        let pending = self.plugin_host.pending_for_event(event);
        let n = pending.len();
        for src in pending {
            self.apply_plugin_entry(&src);
        }
        n
    }

    /// Advance one frame's worth of state given a raw madori event.
    ///
    /// Key events pass through the [`KeyRepeatGate`] first (see
    /// [`Self::tick_at`]); everything else is handled directly.
    pub fn tick(&mut self, event: &AppEvent) {
        self.tick_at(event, Instant::now());
    }

    /// [`Self::tick`] with an explicit timestamp for the key-repeat gate —
    /// lets tests drive the debounce window without depending on the
    /// wall clock.
    pub fn tick_at(&mut self, event: &AppEvent, now: Instant) {
        match translate_app_event(event) {
            InputOutcome::Key(k) => {
                if self.gate_key(&k, now) {
                    self.on_key(&k);
                }
            }
            InputOutcome::Resized { .. } => {
                // Damage only. Each face owns its own geometry: the GPU
                // backend derives the grid in `RenderCallback::resize`, and
                // the ratatui face reads its area every frame. This arm used
                // to write `Window.rect`, which nothing ever read — so the
                // resize path was already doing no real work, it just looked
                // like it was.
                self.damage = self.damage.join(Damage::Viewport);
                self.bump_gen();
            }
            InputOutcome::Quit => self.quit_requested = true,
            InputOutcome::Focus(_) | InputOutcome::None => {}
        }
    }

    /// Decide whether `key` survives the key-repeat gate at time `now`.
    ///
    /// Returns `true` when the key should be processed, `false` when it is
    /// an OS key-repeat storm tick that should be dropped. Gating applies
    /// ONLY in the navigation modes (Normal / Visual / VisualLine) — those
    /// are where a held `j`/`l` floods the motion path and thrashes the
    /// viewport. Insert and Command modes pass every key through ungated,
    /// because there "hold a key to repeat the character" is the intended
    /// behavior, not a storm to suppress.
    fn gate_key(&mut self, key: &Key, now: Instant) -> bool {
        match self.modal.mode() {
            Mode::Normal | Mode::Visual | Mode::VisualLine => {
                // The gate exists for HELD keys that flood the motion path and
                // thrash the viewport (`j`, `l`). It is wrong for the discrete
                // jumps: two `n` presses 10 ms apart mean two matches, and
                // swallowing the second is indistinguishable from a dead key —
                // the exact symptom the gate was added to prevent elsewhere.
                if is_repeat_storm_candidate(key) {
                    return self.repeat_gate.try_pass_at(*key, now);
                }
                true
            }
            Mode::Insert | Mode::Command => true,
        }
    }

    /// Dispatch a single key through the keymap + apply the resulting action.
    pub fn on_key(&mut self, key: &Key) {
        // An open picker owns EVERY key while it is up — before the splash,
        // before the sequence stepper, before the keymap.
        match self.consume_picker_key(key) {
            escriba_ui::picker::Consumed::NotShowing => {}
            escriba_ui::picker::Consumed::Held | escriba_ui::picker::Consumed::Dismissed => return,
            escriba_ui::picker::Consumed::Chose(c) => {
                self.honour_choice(c);
                return;
            }
        }
        // The start screen owns the first keypress and nothing after it.
        match self.consume_splash_key(key) {
            SplashKey::NotShowing | SplashKey::Dismissed => {}
            SplashKey::Ran(action) => {
                self.apply(&action);
                return;
            }
        }
        // ── OPERAND CAPTURE — the keys that are ARGUMENTS, not bindings ──
        //
        // `di(`, `fx`, `` `a ``, `rZ`: in each, the second keystroke is an
        // operand of a half-typed gesture and must be claimed before the
        // sequence stepper and before the keymap, or it resolves as whatever
        // it happens to be bound to (`i` enters Insert, `w` moves a word).
        //
        // This was four near-identical inlined blocks. It is a TABLE now
        // because **the order is the correctness property**, and an order that
        // lives in the arrangement of code is protected by nothing —
        // `operand_capture_order.rs` asserts this list, which the four blocks
        // could not be. Each adjacency below is a real dependency, not a
        // preference; see that test for the failure each one prevents.
        for cap in OPERAND_CHAIN {
            let Some(outcome) = (cap.claim)(self, key.clone()) else {
                continue;
            };
            match outcome {
                ObjectKey::Consumed => return,
                ObjectKey::Compose(a) => {
                    match cap.count {
                        // The object path applies its own repeats before
                        // returning (`2diw`), so re-counting here would square
                        // the count.
                        OperandCount::SelfCounted => self.apply(&a),
                        OperandCount::Drained => {
                            let n = self.modal.pending_count().unwrap_or(1);
                            self.modal.clear_count();
                            self.apply_counted(&a, n);
                        }
                    }
                    return;
                }
            }
        }
        // Multi-key sequence resolution runs first: a key that begins or
        // continues a bound sequence (`<leader>ff`, `gg`) is held or
        // resolved here before the single-key path sees it.
        match self.step_sequence(key) {
            SeqStep::Pending => return,
            SeqStep::Resolved(action) => {
                let count = self.modal.pending_count().unwrap_or(1);
                self.modal.clear_count();
                for _ in 0..count {
                    self.apply(&action);
                    if self.quit_requested {
                        return;
                    }
                }
                return;
            }
            SeqStep::Passthrough => {}
        }
        let counted = self.keymap.dispatch(&self.modal, key);
        // Count prefixes accumulate into modal state.
        if matches!(counted.action, Action::Pending) {
            if let Key::Char(c) = key {
                if c.is_ascii_digit() {
                    let d = u32::from(*c as u8 - b'0');
                    self.modal.append_count(d);
                }
            }
            return;
        }
        // The count flows through the operator-pending FSM (apply_counted), which
        // owns repetition: a bare motion runs count× , an operator captures its
        // count, and an operated motion multiplies the two. No naive outer loop.
        self.apply_counted(&counted.action, counted.count);
        // After applying, reset pending count.
        self.modal.clear_count();
    }

    /// Advance the multi-key pending-stroke state machine for `key`.
    ///
    /// Sequences only apply in normal / visual modes — insert and
    /// command modes treat keys as literal text. Rules:
    /// - Mid-sequence: extend the pending prefix. Exact match →
    ///   [`SeqStep::Resolved`]; still a live prefix → [`SeqStep::Pending`];
    ///   otherwise abort the sequence and re-process this key fresh.
    /// - Not mid-sequence: if `key` begins a bound sequence AND is not
    ///   itself a complete single binding (single bindings win, so no
    ///   chord timeout is needed) → start pending. Otherwise
    ///   [`SeqStep::Passthrough`] to the single-key dispatcher.
    fn step_sequence(&mut self, key: &Key) -> SeqStep {
        let mode = self.modal.mode();
        if !matches!(mode, Mode::Normal | Mode::Visual | Mode::VisualLine) {
            return SeqStep::Passthrough;
        }
        if !self.pending_keys.is_empty() {
            let mut seq = self.pending_keys.clone();
            seq.push(key.clone());
            if let Some(b) = self.keymap.lookup_sequence(mode, &seq) {
                let action = b.action.clone();
                self.pending_keys.clear();
                return SeqStep::Resolved(action);
            }
            if self.keymap.is_sequence_prefix(mode, &seq) {
                self.pending_keys = seq;
                return SeqStep::Pending;
            }
            // The key broke the in-progress sequence — abort it and let
            // the key be re-processed as a fresh stroke below.
            self.pending_keys.clear();
        }
        let start = [key.clone()];
        if self.keymap.is_sequence_prefix(mode, &start) && self.keymap.lookup(mode, key).is_none() {
            self.pending_keys = start.to_vec();
            return SeqStep::Pending;
        }
        SeqStep::Passthrough
    }

    /// The primary cursor position. The single read accessor — every
    /// renderer + motion path goes through it, so the underlying
    /// representation (today a single-cursor [`Cursors`]) can grow to
    /// multi-caret without changing read sites.
    #[must_use]
    pub fn cursor(&self) -> Position {
        self.cursors.primary()
    }

    /// The **single** cursor-mutation path. Clamp the requested position to
    /// the active buffer's bounds, then scroll the active window's viewport
    /// to contain it on BOTH axes. Routing every cursor change through this
    /// (and through [`Cursors::set_primary`]) makes "cursor outside its
    /// viewport" an unrepresentable state, AND keeps cursor state in ONE
    /// typed home — there is no code path that advances the cursor without
    /// re-deriving the viewport from it, and no second `Position` field to
    /// fall out of sync.
    /// Re-assert the cursor-visibility invariant against the CURRENT
    /// viewport.
    ///
    /// A resize changes how much a face can show without moving the cursor,
    /// so nothing would otherwise re-run `scroll_to_contain` — the cursor
    /// would sit off-screen until the operator happened to move it. Every
    /// face calls this after telling the runtime its new size.
    pub fn refollow_cursor(&mut self) {
        self.set_cursor(self.cursors.primary());
    }

    fn set_cursor(&mut self, pos: Position) {
        self.place_cursor(pos, CursorRest::OnCharacter);
    }

    /// The **single** cursor-mutation body. `rest` says what kind of place the
    /// caller is asking for — see [`CursorRest`].
    fn place_cursor(&mut self, pos: Position, rest: CursorRest) {
        let clamped = if let Some(buf) = self.buffers.get(self.active) {
            let on_buffer = buf.clamp(pos);
            // In Normal mode the cursor sits ON a character; only Insert may
            // park past the last one, because that is where the next typed
            // character goes.
            //
            // `Buffer::clamp` cannot make this call — it answers "is this
            // position inside the text", which is a question about the BUFFER,
            // and one-past-the-end legitimately is. Whether the cursor may
            // REST there is a question about the MODE, so it is asked here,
            // once, on the single cursor-mutation path. Every motion inherits
            // it: `w` onto the last word, `$`, `x` at end of line.
            if rest == CursorRest::OnCharacter && self.modal.mode() == Mode::Normal {
                Position::new(
                    on_buffer.line,
                    on_buffer
                        .column
                        .min(buf.line_len_chars(on_buffer.line).saturating_sub(1)),
                )
            } else {
                on_buffer
            }
        } else {
            pos
        };
        self.cursors.set_primary(clamped);
        if let Some(w) = self.layout.active_window_mut() {
            w.viewport = w.viewport.scroll_to_contain(self.cursors.primary(), 2);
        }
    }

    /// Dispatch one resolved action at count 1. See [`apply_counted`](Self::apply_counted).
    fn apply(&mut self, action: &Action) {
        self.apply_counted(action, 1);
    }

    /// Dispatch one resolved action with its count. Routes `(action, count)`
    /// through the operator-pending FSM ([`OperatorPending`], on `zenmai`): most
    /// actions pass straight to [`apply_resolved`](Self::apply_resolved) carrying
    /// their count (so `5j` runs the motion 5×), an operator key is held, and an
    /// operator-then-motion pair is rewritten into a counted
    /// [`Action::ApplyOperator`] (so `3dw` deletes 3 words). The FSM owns count
    /// composition — there is no naive outer repeat loop.
    fn apply_counted(&mut self, action: &Action, count: u32) {
        // An uncompilable pattern must not reach the operator machine.
        //
        // `SearchState::accept` puts the prompt BACK on a compile error so the
        // typed text is not lost — but the FSM had already transitioned out of
        // `AwaitingSearch` on the way in, so the prompt survived and the
        // OPERATOR did not, with nothing said about it. The `d` was simply
        // gone, and the corrected pattern then ran as a bare search.
        //
        // The machine is a pure `(State, Event) -> (State, effects)` and
        // cannot observe the result of an effect, so it cannot decide this
        // itself. The fix is to stop handing it an event it has no business
        // deciding: the runtime classifies the submit first, from state it
        // already holds. `prompt_error` returns `None` for an EMPTY prompt, so
        // the bare-`/<CR>` reuse path is untouched.
        //
        // Tier-honest: parse-rejected at the boundary, not
        // truly-unrepresentable.
        if matches!(action, Action::SubmitCommand) {
            if let Some(e) = self.search.prompt_error() {
                let mut m = String::from("E383: Invalid search string: ");
                m.push_str(&e.to_string());
                self.messages.push(m);
                return;
            }
        }

        // `|` is the one motion whose count is an ARGUMENT rather than a
        // repetition: `40|` means column 40, not "column 1, forty times"
        // (which is column 1). Folded in before the FSM sees it, so the
        // machine keeps one rule — counts repeat — and the exception lives
        // where the exception is.
        let action = &match action {
            Action::Move(Motion::Column(_)) => Action::Move(Motion::Column(count)),
            a => a.clone(),
        };
        let count = match action {
            Action::Move(Motion::Column(_)) => 1,
            _ => count,
        };
        for (resolved, times) in self.op_pending.dispatch((action.clone(), count)) {
            // Two ways an action can carry a count, and the split is real:
            //
            //   REPEAT (`5j`) — run it `times` over. The default.
            //   ABSORB (`3dw`, `2dd`, `3p`) — ONE operation over an extent
            //     resolved `times` over.
            //
            // The distinction is not pedantry. Repeating an operator works by
            // accident for delete — the text vanishes, so the cursor lands
            // somewhere new each round — and is simply wrong for yank, which
            // does not move: `2yw` re-yanked the FIRST word twice and put
            // "one one " in the register. It is wrong for `2dd` in the same
            // shape (the register kept only the second line, so `2ddp` put
            // back half), and it would make `3p` three undo steps.
            //
            // Both kinds now go THROUGH `apply_resolved`, and that is the
            // load-bearing part. The absorbing three used to short-circuit
            // straight to their executors from here, skipping the damage
            // classification and the dot-register recording at that function's
            // tail — so `.` after `3p` or `dw` replayed nothing, and the
            // repaint span was whatever the previous action had asked for.
            let absorbs = absorbs_count(&resolved);
            let (reps, n) = if absorbs { (1, times) } else { (times, 1) };
            for _ in 0..reps {
                self.apply_resolved(&resolved, n);
                if self.quit_requested {
                    return;
                }
            }
        }
    }

    /// The active buffer's text. Search is a pure function of it.
    /// The active buffer's text revision — the token an offset measured
    /// against it should carry.
    #[must_use]
    fn text_rev(&self) -> TextRev {
        self.buffers
            .get(self.active)
            .map_or_else(TextRev::default, escriba_buffer::Buffer::text_rev)
    }

    fn active_text(&self) -> String {
        self.buffers
            .get(self.active)
            .map(escriba_buffer::Buffer::to_string)
            .unwrap_or_default()
    }

    /// The cursor as a char offset — the coordinate search speaks.
    fn cursor_char(&self) -> usize {
        self.buffers
            .get(self.active)
            .and_then(|b| b.position_to_char(self.cursor()).ok())
            .unwrap_or(0)
    }

    /// Move the cursor onto a match and report a wrap the way vim does.
    /// The status line as data — what every face draws.
    ///
    /// One model, so the two faces can only disagree about styling. Before
    /// this existed the GPU face built its own line from a fixed `format!()`
    /// and drew neither the prompt nor any message, which made a fully
    /// working `/` look like a dead key on escriba's default renderer.
    #[must_use]
    pub fn status_model(&self) -> StatusModel<'_> {
        let cursor = self.cursor();
        let prompt = self.search.prompt();

        let kind = match prompt.map(|p| p.direction) {
            Some(escriba_search::Direction::Forward) => PromptKind::SearchForward,
            Some(escriba_search::Direction::Backward) => PromptKind::SearchBackward,
            // Command mode with no search prompt open is an ex-command; the
            // typed `Option<Prompt>` is the discriminator, never a mode flag.
            None if self.modal.mode() == Mode::Command => PromptKind::Ex,
            None => PromptKind::None,
        };

        StatusModel {
            mode: self.modal.mode(),
            line: cursor.line.saturating_add(1) as usize,
            column: cursor.column.saturating_add(1) as usize,
            prompt: kind,
            prompt_text: prompt
                .map_or_else(|| self.modal.minibuffer(), escriba_search::Prompt::text),
            prompt_caret: prompt.map_or_else(
                || self.modal.minibuffer_caret(),
                escriba_search::Prompt::caret,
            ),
            count: self.match_count(),
            message: self.messages.last().map(String::as_str),
        }
    }

    /// `[3/17]` for the current pattern.
    ///
    /// While a prompt is open the count describes the PREVIEW — the answer to
    /// "what would Enter do", which is the question being asked mid-typing.
    /// Once committed it describes where the cursor actually is.
    #[must_use]
    fn match_count(&self) -> MatchCount {
        if self.search.is_prompting() {
            let text = self.active_text();
            // ONE scan, four outcomes. `Incomplete` and `NoMatch` used to be
            // the same `None`, so a half-typed character class reported
            // `[0/0]` — telling the user their pattern matches nothing while
            // they are still writing it.
            return match self.search.preview(&text) {
                escriba_search::Preview::Landed { step, total } => {
                    MatchCount::new(step.index, total)
                }
                escriba_search::Preview::NoMatch => MatchCount::None,
                escriba_search::Preview::Incomplete | escriba_search::Preview::Idle => {
                    MatchCount::Idle
                }
            };
        }
        if self.search.pattern().is_none() {
            return MatchCount::Idle;
        }
        let total = self.search.matches().len();
        // Read THROUGH the anchor: an ordinal computed against text that has
        // since changed reads as absent, so a stale count cannot be displayed.
        let rev = self.text_rev();
        self.search_at.as_ref().and_then(|a| a.get(rev)).map_or(
            if total == 0 {
                MatchCount::None
            } else {
                MatchCount::Idle
            },
            |&i| MatchCount::new(i, total),
        )
    }

    /// `.` — replay the last change at the cursor.
    ///
    /// Two steps, because a change can be two: run the action, then re-type
    /// whatever followed it. `cgn` + `.` is exactly this — change the next
    /// match, then repeat that whole gesture on the one after.
    fn repeat_last_change(&mut self) {
        let Some(change) = self.last_change.clone() else {
            self.messages
                .push("E32: No previous change to repeat".to_string());
            return;
        };

        // Replayed the same way it ran: an absorbing action takes its count as
        // an argument, a repeating one takes it as a loop. Getting this
        // backwards makes `3dw` then `.` delete one word — which is what it
        // did while the recorded count was hardcoded to 1.
        let n = change.count.max(1);
        if absorbs_count(&change.action) {
            self.apply_resolved(&change.action, n);
        } else {
            for _ in 0..n {
                self.apply_resolved(&change.action, 1);
            }
        }
        for c in change.inserted.chars() {
            self.apply_resolved(&Action::InsertChar(c), 1);
        }
        if self.modal.mode() == Mode::Insert {
            // A replayed change must not leave the editor in Insert — the
            // original ended with an Esc the recording deliberately does not
            // store, since it is punctuation rather than part of the change.
            self.apply_resolved(&Action::ChangeMode(Mode::Normal), 1);
        }
        // The replay wrote through `apply_resolved`, which re-records
        // `last_change` from the inner action. Put the ORIGINAL back so a
        // second `.` repeats the same change rather than a fragment of it.
        self.last_change = Some(change);
        self.recording_insert = false;
    }

    /// Resolve a text object to the range it names.
    ///
    /// `gn` uses the INCLUSIVE step, so a cursor already sitting inside a
    /// match operates on THAT match rather than skipping to the next — which
    /// is what makes `cgn` then `.` walk matches one at a time instead of
    /// every other one.
    /// `dd` — the current line INCLUDING its terminator.
    ///
    /// Taking the newline is what makes `dd` remove a line rather than blank
    /// it. On the last line there is no following newline to take, so it
    /// falls back to the preceding one — otherwise `dd` on the final line
    /// leaves an empty line behind, which is the one case a naive
    /// "start-of-line to start-of-next-line" range gets wrong.
    fn object_line(&self) -> Option<Range> {
        self.line_extent(1).map(|e| e.capture)
    }

    /// `{n}dd` — `n` whole lines from the cursor down.
    ///
    /// A counted linewise operator is ONE operation over an `n`-line extent,
    /// not `n` operations over one line — the same rule `apply_operator_n`
    /// enforces for motions, and it broke here in exactly the way that note
    /// predicts. `2dd` ran the single-line object twice: the text came out
    /// right (deleting a line brings the next one under the cursor, so the
    /// repeat lands correctly by accident) and the REGISTER held only the
    /// second line, so `2ddp` silently put back half of what it took.
    ///
    /// Returns an [`Extent`], not a range, because a linewise CHANGE cuts
    /// something different from what a linewise DELETE cuts: `dd` takes the
    /// line and its terminator, `cc`/`S` clear the line's text and keep the
    /// line. Both put the same thing in the register.
    fn line_extent(&self, n: u32) -> Option<Extent> {
        let line = self.cursor().line;
        self.line_span(line, line.saturating_add(n.max(1).saturating_sub(1)))
    }

    /// The linewise extent covering `first..=last`, in either order.
    ///
    /// Split out of [`Self::line_extent`] when linewise MOTIONS landed
    /// (2026-08-14). `dd` names its lines by counting down from the cursor;
    /// `dgg` and `dk` name theirs by reaching BACKWARDS to a resolved target.
    /// Both are the same extent question, and answering it twice is how the
    /// two would come to disagree about the trailing-newline and phantom-row
    /// cases below — which are the whole difficulty here and were already
    /// paid for once.
    fn line_span(&self, first: u32, last_line: u32) -> Option<Extent> {
        let buf = self.buffers.get(self.active)?;
        let (line, requested_end) = if first <= last_line {
            (first, last_line)
        } else {
            (last_line, first)
        };
        let last = buf.line_count().saturating_sub(1);
        // The last line the extent reaches, clamped so `999dd` near the end of
        // a file takes what is there rather than resolving to nothing.
        //
        // Clamped to `last_text_line`, NOT to `last`: on a file ending in `\n`
        // those differ by the phantom row the rope reports, and letting the
        // extent reach it sent the whole resolution down the "no following
        // newline" branch below — so `dd` on the last real line ate the file's
        // trailing newline instead of the line. It also makes a `dd` issued
        // FROM the phantom row resolve to an empty range (a no-op) rather than
        // to a destructive one.
        let end = requested_end.min(last_text_line(buf));
        if line > last_text_line(buf) {
            // The phantom row. There is no line here to operate on.
            return None;
        }
        // What a CHANGE cuts: the text of the named lines, terminators intact.
        // Independent of which capture branch runs below, because the lines
        // named are the same in all three.
        let removal = Range::new(
            Position::new(line, 0),
            Position::new(end, buf.line_len_chars(end)),
        );
        let capture = if end < last {
            Range::new(Position::new(line, 0), Position::new(end + 1, 0))
        } else if line > 0 {
            // Final line of a file with NO trailing newline: there is no
            // following line start to take a terminator from, so swallow the
            // PRECEDING one — otherwise `dd` blanks the line and leaves it.
            Range::new(
                Position::new(line - 1, buf.line_len_chars(line - 1)),
                Position::new(end, buf.line_len_chars(end)),
            )
        } else {
            // The extent is the whole buffer and there is no terminator to
            // take at either end: clear the text, keep the line itself.
            removal
        };
        Some(Extent {
            capture,
            removal,
            kind: RegisterKind::Linewise,
        })
    }

    /// `iw` / `aw` — the word under the cursor.
    ///
    /// vim's `w` classes are word / punctuation / whitespace, and a text
    /// object never crosses a line. `around` additionally takes the trailing
    /// whitespace run, falling back to LEADING whitespace when there is none
    /// after — which is what vim does at end of line.
    fn object_word(&self, around: bool) -> Option<Range> {
        let buf = self.buffers.get(self.active)?;
        let pos = self.cursor();
        let text: Vec<char> = buf.line(pos.line)?.chars().collect();
        if text.is_empty() {
            return None;
        }
        let col = (pos.column as usize).min(text.len().saturating_sub(1));

        #[derive(PartialEq, Clone, Copy)]
        enum Class {
            Word,
            Punct,
            Space,
        }
        let class = |c: char| {
            if c.is_alphanumeric() || c == '_' {
                Class::Word
            } else if c.is_whitespace() {
                Class::Space
            } else {
                Class::Punct
            }
        };

        let here = class(text[col]);
        let mut start = col;
        while start > 0 && class(text[start - 1]) == here {
            start -= 1;
        }
        let mut end = col + 1;
        while end < text.len() && class(text[end]) == here {
            end += 1;
        }

        if around {
            let after = end;
            while end < text.len() && class(text[end]) == Class::Space {
                end += 1;
            }
            // No trailing run: take the leading one instead, as vim does.
            if end == after {
                while start > 0 && class(text[start - 1]) == Class::Space {
                    start -= 1;
                }
            }
        }

        Some(Range::new(
            Position::new(pos.line, start as u32),
            Position::new(pos.line, end as u32),
        ))
    }

    /// `i(` / `a"` … — the region between a matched pair, on one line.
    ///
    /// Brackets NEST and quotes do not, and that is the only difference:
    /// with `open == close` the scan cannot count depth, so it takes the
    /// nearest delimiter on each side instead.
    fn object_delimited(&self, open: char, close: char, around: bool) -> Option<Range> {
        let buf = self.buffers.get(self.active)?;
        let pos = self.cursor();
        let text: Vec<char> = buf.line(pos.line)?.chars().collect();
        if text.is_empty() {
            return None;
        }
        let col = (pos.column as usize).min(text.len().saturating_sub(1));

        let (l, r) = if open == close {
            // Quotes: nearest on each side, no nesting to track.
            let l = (0..=col).rev().find(|&i| text[i] == open)?;
            let r = ((col.max(l) + 1)..text.len()).find(|&i| text[i] == close)?;
            (l, r)
        } else {
            // Brackets: walk out counting depth, so an inner pair does not
            // terminate the search for the enclosing one.
            let mut depth = 0i32;
            let l = (0..=col).rev().find(|&i| {
                if text[i] == close && i != col {
                    depth += 1;
                    false
                } else if text[i] == open {
                    if depth == 0 {
                        true
                    } else {
                        depth -= 1;
                        false
                    }
                } else {
                    false
                }
            })?;
            depth = 0;
            let r = ((l + 1)..text.len()).find(|&i| {
                if text[i] == open {
                    depth += 1;
                    false
                } else if text[i] == close {
                    if depth == 0 {
                        true
                    } else {
                        depth -= 1;
                        false
                    }
                } else {
                    false
                }
            })?;
            (l, r)
        };

        // `i` is strictly between the delimiters; `a` includes them.
        let (s, e) = if around { (l, r + 1) } else { (l + 1, r) };
        Some(Range::new(
            Position::new(pos.line, s as u32),
            Position::new(pos.line, e as u32),
        ))
    }

    fn resolve_object(&self, object: escriba_core::TextObject) -> Option<Range> {
        use escriba_core::TextObject as O;

        // The text-scanning objects resolve against the BUFFER; the two
        // search objects resolve against the match set. Splitting here keeps
        // the search logic below exactly as it was rather than threading a
        // second concern through it.
        match object {
            O::Line => return self.object_line(),
            O::Word { around } => return self.object_word(around),
            O::Delimited {
                open,
                close,
                around,
            } => return self.object_delimited(open, close, around),
            O::NextMatch | O::PrevMatch => {}
        }

        let at = self.cursor_char();
        let matches = self.search.matches();

        // A match CONTAINING the cursor wins outright, whichever direction the
        // object names.
        //
        // Comparing only against `m.start` — which is what a `starts`-vector
        // plus `Bound::Inclusive` does — is right only when the cursor sits on
        // a match's FIRST character. One column further in, `start < at` and
        // the match is rejected, so `cgn` skipped the very instance the
        // operator was standing in and the rename silently missed it. vim
        // operates on the containing match from every interior column, and the
        // `starts`-only comparison cannot express "contains" because it never
        // looks at `m.end`.
        let idx = matches.iter().position(|m| m.contains(at)).or_else(|| {
            let starts: Vec<usize> = matches.iter().map(|m| m.start).collect();
            match object {
                O::PrevMatch => Bound::Inclusive.first_matching(&starts, at, false),
                // Every other variant returned above; `NextMatch` is the only
                // one that can reach here besides `PrevMatch`.
                _ => Bound::Inclusive.first_matching(&starts, at, true),
            }
        })?;

        let m = matches.get(idx)?;
        let buf = self.buffers.get(self.active)?;
        Some(Range {
            start: buf.char_to_position(m.start),
            end: buf.char_to_position(m.end),
        })
    }

    fn land_on(&mut self, step: escriba_search::Step) {
        if let Some(buf) = self.buffers.get(self.active) {
            let pos = buf.char_to_position(step.target.start);
            self.set_cursor(pos);
        }
        // The `[3/17]` numerator. `Step` has carried this index since the
        // engine was written — `engine.rs` even names the counter as the
        // reason it exists — and every consumer discarded it until now.
        self.search_at = Some(Anchored::new(step.index, self.text_rev()));
    }

    /// vim's "search hit BOTTOM, continuing at TOP".
    ///
    /// One reporter, called by the two places a search can wrap: the shared
    /// commit and `n`/`N`. `land_on` deliberately does NOT report, or the bare
    /// commit would say it twice.
    fn report_wrap(&mut self, step: &escriba_search::Step) {
        if let Some(msg) = escriba_search::wrap_message(step.wrapped) {
            self.messages.push(msg.to_string());
        }
    }

    /// `n` / `N`. Reports vim's E486 when the pattern matches nothing, rather
    /// than failing silently — a search that appears to do nothing is
    /// indistinguishable from a dropped keystroke.
    fn jump_search(&mut self, reverse: bool) {
        // Using the matches re-lights them: `n` after an auto-clear shows you
        // what you are walking through.
        self.search.relight();
        // `n` is a far jump — record where we leave from so `<C-o>` works.
        self.jumps.push(self.spot());
        let at = self.cursor_char();
        match self.search.repeat(at, reverse) {
            Some(step) => {
                // `n` wrapping the file says so, same as a commit does.
                self.report_wrap(&step);
                self.land_on(step);
            }
            None => {
                let msg = self.search.pattern().map_or_else(
                    || "E35: No previous regular expression".to_string(),
                    |p| {
                        let mut m = String::from("E486: Pattern not found: ");
                        m.push_str(p.raw());
                        m
                    },
                );
                self.messages.push(msg);
            }
        }
    }

    /// Move the cursor to where the in-progress pattern would land, without
    /// committing anything. vim's `incsearch`.
    ///
    /// A pattern that does not compile yet (`/a[`, mid-typing) previews
    /// nothing and reports nothing — an error toast on every keystroke of a
    /// character class would be unusable.
    fn preview_search(&mut self) {
        let text = self.active_text();
        let Some(origin) = self.search.prompt().map(|p| p.origin) else {
            return;
        };
        let target = match self.search.preview(&text) {
            escriba_search::Preview::Landed { step, .. } => step.target.start,
            // Nothing to show: back to where the search started. Covers a
            // half-typed pattern and a pattern that finds nothing alike —
            // both mean "there is no match to preview".
            escriba_search::Preview::Idle
            | escriba_search::Preview::Incomplete
            | escriba_search::Preview::NoMatch => origin,
        };
        // A pattern that STOPS matching returns the cursor to the origin.
        //
        // Preview used to only ever move forward, so typing `ch` (a match) and
        // then `chz` (none) left the cursor parked on the `ch` match — a
        // preview showing a position the pattern no longer justifies, while
        // the count beside it read `[0/0]`. Restoring is also what makes
        // Escape's promise legible: at every keystroke the cursor is either on
        // a real match or back where you started, never on a stale one.
        if let Some(buf) = self.buffers.get(self.active) {
            let pos = buf.char_to_position(target);
            self.set_cursor(pos);
        }
    }

    /// `d/foo<CR>` — commit the prompt and operate from the prompt's origin to
    /// where the search lands, as ONE action.
    ///
    /// Split from [`Self::submit_search`] rather than sharing it because the
    /// two want opposite things from the commit: the bare `/` MOVES the cursor
    /// to the match, and an operated `/` must NOT — the cursor is the
    /// operator's start point, and moving it first would leave the operator
    /// with a zero-width range.
    /// Commit the open search prompt. The ONE copy of the sequence.
    ///
    /// Reports its own failures (E486 / E35) so neither caller has to carry a
    /// third copy of the message strings. `Accepted::Invalid` cannot reach
    /// here — `apply_counted` rejects an uncompilable pattern at the dispatch
    /// boundary before the FSM or this method ever sees the submit.
    fn commit_search_prompt(&mut self) -> CommitOutcome {
        let text = self.active_text();
        let Some((origin, skip)) = self.search.prompt().map(|p| (p.origin, p.preview_skip()))
        else {
            return CommitOutcome::NoPrompt;
        };

        match self.search.accept(&text) {
            escriba_search::Accepted::Committed | escriba_search::Accepted::ReusedPrevious => {
                self.modal.clear_minibuffer();
                self.modal.enter(Mode::Normal);
                match self.search.commit_step_skipping(origin, skip) {
                    Some(step) => {
                        // The wrap notice belongs HERE, once, for both commit
                        // paths. Reporting it in each caller is what let the
                        // operated path lose it in the first place — and my
                        // first attempt at this refactor duplicated it again
                        // rather than moving it, which the red proof caught.
                        self.report_wrap(&step);
                        CommitOutcome::Landed { origin, step }
                    }
                    None => {
                        self.report_pattern_not_found();
                        CommitOutcome::NotFound
                    }
                }
            }
            escriba_search::Accepted::NothingToRepeat => {
                self.modal.clear_minibuffer();
                self.modal.enter(Mode::Normal);
                self.messages
                    .push("E35: No previous regular expression".to_string());
                CommitOutcome::NoPrevious
            }
            // Unreachable: the boundary guard in `apply_counted` returns early
            // on an uncompilable pattern, leaving the prompt open. Reported
            // rather than `unreachable!()` — a panic in the editor's commit
            // path is a worse failure than a duplicate message.
            escriba_search::Accepted::Invalid(e) => {
                let mut m = String::from("E383: Invalid search string: ");
                m.push_str(&e.to_string());
                self.messages.push(m);
                CommitOutcome::NoPrompt
            }
        }
    }

    /// vim's E486, with the pattern named. One place, so every path that fails
    /// to find reports identically.
    fn report_pattern_not_found(&mut self) {
        let mut m = String::from("E486: Pattern not found");
        if let Some(p) = self.search.pattern() {
            m.push_str(": ");
            m.push_str(p.raw());
        }
        self.messages.push(m);
    }

    /// Bare `/foo<CR>` — commit and MOVE the cursor to the match.
    ///
    /// The only difference from the operated path is that this one lands;
    /// everything else lives in `commit_search_prompt`.
    fn submit_search(&mut self) {
        match self.commit_search_prompt() {
            CommitOutcome::Landed { origin, step } => {
                if let Some(buf) = self.buffers.get(self.active) {
                    let from = buf.char_to_position(origin);
                    self.jumps.push(escriba_core::Spot::new(self.active, from));
                }
                self.land_on(step);
            }
            CommitOutcome::NotFound | CommitOutcome::NoPrevious | CommitOutcome::NoPrompt => {}
        }
    }

    /// `d/foo<CR>` — commit, then operate from the prompt's origin to where the
    /// search lands, as ONE action.
    ///
    /// The cursor must NOT move to the match first: it is the operator's start
    /// point. That is the whole reason this differs from the bare path, and
    /// now the only reason.
    fn submit_search_operated(&mut self, op: Operator) {
        match self.commit_search_prompt() {
            CommitOutcome::Landed { origin, step } => {
                if let Some(buf) = self.buffers.get(self.active) {
                    let from = buf.char_to_position(origin);
                    let target = buf.char_to_position(step.target.start);
                    // Operating over a search is itself a far jump.
                    self.jumps.push(escriba_core::Spot::new(self.active, from));
                    self.set_cursor(from);
                    self.apply_operator_to(op, target);
                }
            }
            CommitOutcome::NotFound | CommitOutcome::NoPrevious | CommitOutcome::NoPrompt => {}
        }
    }

    /// Execute one resolved action, with the count it ABSORBS.
    ///
    /// `count` is 1 for everything that repeats — the caller loops those — and
    /// is the gesture's full count for the arms listed in [`absorbs_count`].
    /// Every path lands here so the damage classification and dot-register
    /// recording at the tail run exactly once per gesture; the counted
    /// operators used to bypass this function and skipped both.
    fn apply_resolved(&mut self, action: &Action, count: u32) {
        // Snapshot the scope inputs before the mutation so the resulting
        // Damage covers the changed region (the S3 seal — conservative widen).
        let lines_before = self.active_line_count();
        // Snapshot for the dot register: the only reliable witness that this
        // action changed text is that the buffer's revision moved.
        let rev_before = self.text_rev();
        let cline_before = self.cursor().line;
        match action {
            // Every action with an exact slip equivalent goes through the
            // interpreter, so "undo" has ONE implementation rather than one
            // per entry point. These had already drifted: the executor
            // re-followed the viewport after undo and the M1 interpreter did
            // not, so `u` and `:undo` behaved differently within a milestone
            // of each other.
            // Listed EXPLICITLY rather than behind a `if lower(..).is_some()`
            // guard: a guard arm does not count toward exhaustiveness, so the
            // guarded form silently gave up the total match — the compiler
            // said so, and it was right. `lowering_and_dispatch_agree` pins
            // that this list and `lower` stay the same set.
            Action::Quit
            | Action::ClearSearchHighlight
            | Action::Save
            | Action::Undo
            | Action::Redo
            | Action::Edit(_) => {
                for slip in Self::lower(action, self.active).unwrap_or_default() {
                    self.honour_one(slip);
                }
            }
            Action::Move(m) => self.apply_motion(*m),
            Action::SearchOpen(dir) => {
                // vim's `/` is the command-line with a different prompt char,
                // so we reuse Command mode; `search.prompt` is what tells a
                // later <CR> this is a search and not an ex-command.
                let origin = self.cursor_char();
                self.search.open(*dir, origin);
                self.modal.enter(Mode::Command);
            }
            Action::SearchRepeat { reverse } => self.jump_search(*reverse),
            Action::SearchWord { reverse } => {
                let dir = if *reverse {
                    SearchDirection::Backward
                } else {
                    SearchDirection::Forward
                };
                let (text, at) = (self.active_text(), self.cursor_char());
                // `*` jumps, so it records too.
                self.jumps.push(self.spot());
                match self.search.search_word(&text, at, dir) {
                    Some(step) => self.land_on(step),
                    // vim beeps and stays put when there is no word under the
                    // cursor; a silent no-op would look like a broken key.
                    None => self
                        .messages
                        .push("E348: No string under cursor".to_string()),
                }
            }
            Action::SearchSubmitOperated { op } => self.submit_search_operated(*op),
            Action::TextObject(object) => {
                // Bare `gn` moves onto the match. vim additionally starts a
                // Visual selection of it; escriba's Visual plumbing does not
                // carry a selection an operator can consume yet, so this
                // stops at the jump rather than faking a selection that
                // nothing would honour.
                if let Some(range) = self.resolve_object(*object) {
                    self.jumps.push(self.spot());
                    self.set_cursor(range.start);
                } else {
                    self.report_pattern_not_found();
                }
            }
            // The linewise object is the one that can express an `n`-fold
            // extent, so it reads the count directly; every other object still
            // repeats (see `absorbs_count`).
            Action::ApplyOperatorObject {
                op,
                object: escriba_core::TextObject::Line,
            } => {
                if let Some(extent) = self.line_extent(count) {
                    self.apply_operator_over(*op, extent);
                }
            }
            Action::ApplyOperatorObject { op, object } => match self.resolve_object(*object) {
                // The kind comes from the OBJECT (`TextObject::register_kind`,
                // total over the enum), which is the only thing that knows:
                // `dd` on line 1 and a charwise `[(1,0), (2,0))` are the same
                // two positions.
                Some(range) => {
                    self.apply_operator_over(
                        *op,
                        Extent::from_object(range, object.register_kind()),
                    );
                }
                None => self.report_pattern_not_found(),
            },
            Action::Put { before } => self.put(*before, count),
            Action::ReplaceChar(ch) => self.replace_char(*ch, count),
            Action::JoinLines { space } => self.join_lines(*space, count),
            Action::RepeatLastChange => self.repeat_last_change(),
            Action::JumpBack => {
                let here = self.spot();
                if let Some(spot) = self.jumps.back(here) {
                    self.goto_spot(spot);
                } else {
                    self.messages
                        .push("E662: At start of changelist".to_string());
                }
            }
            Action::JumpForward => {
                if let Some(spot) = self.jumps.forward() {
                    self.goto_spot(spot);
                } else {
                    self.messages.push("E663: At end of changelist".to_string());
                }
            }
            Action::ChangeMode(m) => {
                // Leaving the cmdline abandons any open search prompt and
                // returns the cursor home. The COMMITTED pattern survives —
                // cancelling a new search must not erase the old highlights.
                if *m == Mode::Normal && self.search.is_prompting() {
                    if let Some(origin) = self.search.cancel() {
                        if let Some(buf) = self.buffers.get(self.active) {
                            let pos = buf.char_to_position(origin);
                            self.set_cursor(pos);
                        }
                    }
                }
                self.modal.enter(*m);
            }
            Action::EnterInsert(at) => self.enter_insert_at(*at),
            Action::InsertChar(c) => self.insert_char(*c),

            Action::SubmitCommand => {
                if self.search.is_prompting() {
                    self.submit_search();
                } else {
                    self.submit_command();
                }
            }
            Action::Command { name, args } => self.run_command(name, args),
            Action::ApplyOperator { op, motion } => self.apply_operator_n(*op, *motion, count),
            // The operator-pending FSM consumes Operator keys (begins pending);
            // they never reach the executor. Defensive no-op for exhaustiveness.
            Action::Operator(_) => {}
            Action::PromptCaret { to } => {
                // Both prompts have a caret now, and the same keys move it.
                if self.search.is_prompting() {
                    self.search.move_caret(*to);
                } else {
                    self.modal.move_minibuffer_caret(*to);
                }
            }
            Action::SearchPreviewStep { forward } => {
                if self.search.is_prompting() {
                    self.search.preview_step(*forward);
                    self.preview_search();
                }
            }
            Action::DeleteForward => {
                if self.modal.mode() == Mode::Command {
                    if self.search.is_prompting() {
                        self.search.delete_at_caret();
                        self.preview_search();
                    } else {
                        self.modal.delete_minibuffer_at_caret();
                    }
                } else {
                    self.delete_after_cursor();
                }
            }
            Action::DeleteWordBefore => {
                if self.modal.mode() == Mode::Command {
                    if self.search.is_prompting() {
                        self.search.delete_word_before_caret();
                        self.preview_search();
                    }
                } else {
                    self.delete_word_before_cursor();
                }
            }
            Action::DeleteToLineStart => {
                if self.modal.mode() == Mode::Command {
                    if self.search.is_prompting() {
                        self.search.clear_before_caret();
                        self.preview_search();
                    }
                } else {
                    self.delete_to_line_start();
                }
            }
            Action::Backspace => {
                if self.modal.mode() == Mode::Command {
                    self.prompt_backspace();
                    // Shortening the pattern changes which matches exist, so
                    // the preview must re-run — otherwise the cursor sits on a
                    // match of a pattern that is no longer typed.
                    if self.search.is_prompting() {
                        self.preview_search();
                    }
                } else {
                    self.delete_before_cursor();
                }
            }
            Action::PromptHistory { back } => {
                if self.search.is_prompting() {
                    self.search.history_step(*back);
                    // No minibuffer resync: the shadow is the ex-line's store
                    // and nothing reads it while a search prompt is open, so
                    // rewriting it here was maintaining a copy for no reader.
                    self.preview_search();
                }
            }
            // `m{a-z}`. Only `a-z`: `A-Z` are vim's cross-file marks and this
            // map is per-editor, so accepting one would promise a jump back
            // to another FILE and deliver a jump to that line in this one.
            Action::SetMark(name) => {
                if name.is_ascii_lowercase() {
                    let at = self.cursor();
                    self.marks.insert(*name, at);
                } else {
                    self.messages
                        .push(format!("E191: mark `{name}` is not a-z"));
                }
            }
            Action::ScrollView(align) => self.scroll_view(*align),
            Action::Pending => {}
        }
        // Widen the dirty region by what this action touched (M1). Content
        // mutations that changed the line count run to end-of-document (every
        // line below shifted); an in-place edit or a cursor move is local;
        // arbitrary commands are conservatively Full. Never narrows.
        let lines_after = self.active_line_count();
        let cline_after = self.cursor().line;
        let d = match action {
            // A search repaints every highlight in the viewport, not just the
            // line the cursor left — so it must widen to Full. Treating it as a
            // cursor move would leave stale highlights on untouched lines.
            Action::SearchOpen(_)
            | Action::PromptHistory { .. }
            | Action::Backspace
            | Action::PromptCaret { .. }
            | Action::SearchPreviewStep { .. }
            | Action::DeleteForward
            | Action::DeleteWordBefore
            | Action::DeleteToLineStart
            | Action::SearchRepeat { .. }
            | Action::SearchWord { .. }
            | Action::ClearSearchHighlight
            | Action::SearchSubmitOperated { .. }
            // A replayed change can edit anywhere the original could, and a
            // match object can be anywhere in the document.
            | Action::RepeatLastChange
            | Action::TextObject(_)
            | Action::ApplyOperatorObject { .. }
            // A jump can land anywhere, so the viewport may scroll wholesale.
            | Action::JumpBack
            | Action::JumpForward
            // A re-frame repaints every row even though no byte changed —
            // which is exactly the case a line-scoped damage would miss.
            | Action::ScrollView(_) => Damage::Full,
            Action::InsertChar(_)
            | Action::Edit(_)
            | Action::Undo
            | Action::Redo
            // Insert-entry belongs in THIS group rather than beside
            // `ChangeMode` below, even though four of its six members only move
            // the caret: `o`/`O` add a line, and this arm's body is already the
            // one that asks whether the line COUNT changed. Grouping it with
            // the pure mode change would repaint a one-line span after `o` and
            // leave every line below the new one stale.
            | Action::EnterInsert(_)
            // Same reading as `EnterInsert`: a charwise put touches one line
            // and a linewise one adds several, and this arm's body is already
            // the one that asks which happened by comparing the line count.
            // `J` removes lines and `r` removes none — the same question.
            | Action::Put { .. }
            | Action::ReplaceChar(_)
            | Action::JoinLines { .. }
            | Action::ApplyOperator { .. } => {
                if lines_after == lines_before {
                    Damage::span(cline_before, cline_after)
                } else {
                    Damage::Lines {
                        from: cline_before.min(cline_after),
                        to: u32::MAX,
                    }
                }
            }
            Action::Move(_) | Action::ChangeMode(_) => Damage::span(cline_before, cline_after),
            Action::Save => Damage::Viewport,
            Action::Command { .. } | Action::SubmitCommand => Damage::Full,
            // Setting a mark changes no pixel — there is no gutter sign for
            // one yet. When one lands, this arm becomes `Damage::span`.
            Action::Quit | Action::Operator(_) | Action::SetMark(_) | Action::Pending => {
                Damage::None
            }
        };
        self.damage = self.damage.join(d);
        // Remember this change for `.`.
        //
        // Recorded from an OBSERVED MUTATION, not from the action's variant.
        // `text_effect()` is the wrong predicate here even though it looks
        // like the right one: it exists to decide cache invalidation, where
        // OVER-reporting is the safe direction, and the dot register needs the
        // opposite bias. Leaning on it meant `last_change` was set by actions
        // that changed no text at all, with two measured consequences:
        //
        //   `iZ<Esc>` then `/a<CR>` then `.`  — did nothing; the register held
        //       `SubmitCommand`, whose replay reads an already-cleared
        //       minibuffer.
        //   `iZ<Esc>` then `/q<Esc>` then `.` — TYPED `q` INTO THE BUFFER. An
        //       abandoned prompt left the register holding `InsertChar('q')`,
        //       and `.` in Normal mode routes that to the text. A corrupting
        //       register, not merely a lost one.
        //
        // Comparing the buffer's `TextRev` across the action answers the only
        // question that matters — did this actually change the text — and gets
        // the failed-operator case (`dgn` with no pattern) right for free.
        if self.recording_insert {
            match action {
                Action::InsertChar(c) => {
                    if let Some(lc) = self.last_change.as_mut() {
                        lc.inserted.push(*c);
                    }
                }
                // Leaving Insert ends the session; the change is now whole.
                Action::ChangeMode(m) if *m != Mode::Insert => self.recording_insert = false,
                _ => {}
            }
        } else if self.text_rev() != rev_before
            && !matches!(
                action,
                Action::RepeatLastChange | Action::Undo | Action::Redo
            )
        {
            self.last_change = Some(LastChange {
                // The count the gesture actually carried, not a hardcoded 1.
                // `.` replays it through the same absorb-or-repeat split the
                // original ran under (see `repeat_last_change`), so `3dw` then
                // `.` deletes three words rather than one.
                action: action.clone(),
                count,
                inserted: String::new(),
            });
            self.recording_insert = self.modal.mode() == Mode::Insert;
        }

        // The search is over the moment you move on or edit — clear the
        // highlight rather than leaving the buffer as confetti until an
        // explicit `:noh`, which is the remap nearly every vimrc carries.
        // Clearing suppresses without forgetting, so `n` still works.
        if action.highlight_effect() == HighlightEffect::Clear {
            self.search.clear_highlight();
        }
        // Text changed ⇒ every match offset cached against the old text is
        // wrong. `SearchState::refresh` existed for exactly this and had ZERO
        // callers, so inserting four characters left both renderers painting
        // the highlight four columns off.
        //
        // Gated on the typed classifier rather than on `bump_gen` (which fires
        // for pure cursor moves too): re-scanning the document on every `j`
        // would be a per-keystroke full pass for no reason.
        if action.text_effect() == TextEffect::Mutates && self.search.pattern().is_some() {
            let text = self.active_text();
            self.search.refresh(&text);
            // NO manual invalidation of `search_at` here, deliberately. It is
            // `Anchored` to the text revision, so an ordinal computed against
            // the old text now reads as `None` on its own. This is the line
            // that used to have to be remembered.
        }
        // An action reached the executor ⇒ visible state may have changed.
        // Advance the refresh generation so the renderer repaints (and
        // re-highlights) exactly once. A gated-out key never reaches here, so
        // a key-repeat storm does not spin the renderer.
        self.bump_gen();
    }

    /// Resolve a [`Motion`] from `from` to its target [`Position`] against the
    /// active buffer — **pure**: no cursor mutation, no side effects. This is
    /// the single motion-resolution source of truth that both [`apply_motion`]
    /// (move the cursor *to* the target) and [`apply_operator`] (use the target
    /// as the *other end* of an operated range) stand on. `None` only if there
    /// is no active buffer.
    ///
    /// [`apply_motion`]: Self::apply_motion
    /// [`apply_operator`]: Self::apply_operator
    fn resolve_motion(&self, from: Position, motion: Motion) -> Option<Position> {
        let buf = self.buffers.get(self.active)?;
        let pos = from;
        Some(match motion {
            // Search-as-motion: what makes `dn` / `d/foo<CR>` work. Resolved
            // against the committed match list, so it is `None` (motion fails,
            // operator aborts, buffer untouched) when nothing is committed —
            // never a silent move to 0, which would delete to the file start.
            Motion::SearchNext | Motion::SearchPrev => {
                let at = buf.position_to_char(pos).ok()?;
                let step = self
                    .search
                    .repeat(at, matches!(motion, Motion::SearchPrev))?;
                buf.char_to_position(step.target.start)
            }
            Motion::Left => Position::new(pos.line, pos.column.saturating_sub(1)),
            // Clamped to the line, which is what makes `x` (`dl`) safe to
            // express as a composition. Unclamped, an operator range built
            // over `Right` crosses the line TERMINATOR on an empty line — so
            // `x` there would join the next line onto this one instead of
            // doing nothing. The cursor path is unaffected: `place_cursor`
            // was already pulling `l` back onto the last character.
            Motion::Right => Position::new(
                pos.line,
                pos.column
                    .saturating_add(1)
                    .min(buf.line_len_chars(pos.line)),
            ),
            Motion::Up => Position::new(pos.line.saturating_sub(1), pos.column),
            Motion::Down => Position::new(pos.line.saturating_add(1), pos.column),
            Motion::LineStart => Position::new(pos.line, 0),
            Motion::LineEnd => Position::new(pos.line, buf.line_len_chars(pos.line)),
            Motion::LineFirstNonBlank => first_non_blank(buf, pos.line),
            // `_` — the same landing character as `^`, and the CURSOR path
            // cannot tell them apart. The difference is entirely in the kind,
            // which only an operator reads (`Motion::is_linewise`).
            Motion::LinewiseDown => first_non_blank(buf, pos.line),
            // `g_` — the last non-blank. Inclusive, so the operator widens it;
            // the resolver names the CHARACTER, which is where `$` differs.
            Motion::LineLastNonBlank => {
                let chars = line_chars(buf, pos.line);
                let col = chars
                    .iter()
                    .rposition(|c| !c.is_whitespace())
                    .and_then(|i| u32::try_from(i).ok())
                    .unwrap_or(0);
                Position::new(pos.line, col)
            }
            // `|` is 1-based, and clamped to the line rather than refused —
            // vim puts `500|` on the last character.
            Motion::Column(n) => Position::new(
                pos.line,
                n.saturating_sub(1).min(buf.line_len_chars(pos.line)),
            ),
            Motion::LineDownFirstNonBlank => {
                first_non_blank(buf, pos.line.saturating_add(1).min(last_text_line(buf)))
            }
            Motion::LineUpFirstNonBlank => first_non_blank(buf, pos.line.saturating_sub(1)),
            Motion::DocStart => Position::ZERO,
            Motion::DocEnd => Position::new(
                buf.line_count().saturating_sub(1),
                buf.line_len_chars(buf.line_count().saturating_sub(1)),
            ),
            Motion::WordStartNext => word_next(buf, pos, Width::Small),
            Motion::WordEndNext => word_end(buf, pos, Width::Small),
            Motion::WordStartPrev => word_prev(buf, pos, Width::Small),
            Motion::WordEndPrev => word_end_prev(buf, pos, Width::Small),
            Motion::BigWordStartNext => word_next(buf, pos, Width::Big),
            Motion::BigWordEndNext => word_end(buf, pos, Width::Big),
            Motion::BigWordStartPrev => word_prev(buf, pos, Width::Big),
            Motion::BigWordEndPrev => word_end_prev(buf, pos, Width::Big),
            Motion::FindChar { ch, backward, till } => find_char(buf, pos, ch, backward, till)?,
            // `;` / `,` resolve through the LAST `f`/`t`, which is runtime
            // state — the same shape as the search motions above, and the
            // reason neither can be resolved by the enum alone.
            Motion::RepeatFind { reverse } => {
                let last = self.last_find?;
                let backward = last.backward != reverse;
                find_char(buf, pos, last.ch, backward, last.till)?
            }
            Motion::MatchPair => self.resolve_match(buf, pos)?,
            // A mark that was never set is a FAILED motion, not a move to the
            // origin: `` `q `` with no `q` must leave the cursor alone, and
            // ``d`q`` must not delete to the top of the file.
            Motion::MarkExact(name) => {
                let at = *self.marks.get(&name)?;
                Position::new(
                    at.line.min(last_text_line(buf)),
                    at.column
                        .min(buf.line_len_chars(at.line.min(last_text_line(buf)))),
                )
            }
            Motion::MarkLine(name) => {
                let at = *self.marks.get(&name)?;
                first_non_blank(buf, at.line.min(last_text_line(buf)))
            }
            Motion::ParagraphNext => paragraph(buf, pos, true),
            Motion::ParagraphPrev => paragraph(buf, pos, false),
            Motion::SentenceNext => sentence(buf, pos, true),
            Motion::SentencePrev => sentence(buf, pos, false),
            // `H` / `M` / `L` are about the VIEWPORT, not the buffer — which
            // is what makes them the only motions whose target changes when
            // nothing in the text did.
            Motion::ScreenTop | Motion::ScreenMiddle | Motion::ScreenBottom => {
                let vp = self.layout.active_window().map_or(
                    Viewport {
                        top_line: 0,
                        left_column: 0,
                        visible_lines: 1,
                        visible_columns: 1,
                    },
                    |w| w.viewport,
                );
                let last = last_text_line(buf);
                let bottom = vp
                    .top_line
                    .saturating_add(vp.visible_lines.saturating_sub(1))
                    .min(last);
                let line = match motion {
                    Motion::ScreenTop => vp.top_line.min(last),
                    Motion::ScreenBottom => bottom,
                    _ => vp.top_line.min(last) + (bottom - vp.top_line.min(last)) / 2,
                };
                first_non_blank(buf, line)
            }
            Motion::PageDown | Motion::HalfPageDown => {
                Position::new(pos.line.saturating_add(10), pos.column)
            }
            Motion::PageUp | Motion::HalfPageUp => {
                Position::new(pos.line.saturating_sub(10), pos.column)
            }
            Motion::GotoLine(n) => Position::new(n.saturating_sub(1), 0),
            // Structural Lisp motions — stubs for phase 1.B; full paredit
            // semantics land when caixa-ast is wired to the active buffer.
            Motion::ForwardSexp
            | Motion::BackwardSexp
            | Motion::UpList
            | Motion::DownList
            | Motion::BeginningOfDefun
            | Motion::EndOfDefun
            | Motion::BeginningOfSexp
            | Motion::EndOfSexp => pos,
        })
    }

    /// `zt` / `zz` / `zb` — re-frame the window around the cursor's line
    /// WITHOUT moving the cursor.
    ///
    /// The cursor is deliberately untouched: `zz` is what you press when you
    /// are already where you want to be and only the framing is wrong. Note
    /// that `set_cursor` would undo this — it scrolls the viewport to contain
    /// the cursor with a 2-line margin — so this must not route through it,
    /// and the next motion legitimately re-frames again.
    fn scroll_view(&mut self, align: escriba_core::ViewAlign) {
        use escriba_core::ViewAlign;
        let line = self.cursor().line;
        let Some(w) = self.layout.active_window_mut() else {
            return;
        };
        let h = w.viewport.visible_lines.max(1);
        w.viewport.top_line = match align {
            ViewAlign::Top => line,
            ViewAlign::Center => line.saturating_sub(h / 2),
            ViewAlign::Bottom => line.saturating_sub(h.saturating_sub(1)),
        };
        self.damage = self.damage.join(Damage::Full);
        self.bump_gen();
    }

    /// Claim the operand of a pending `m` / `` ` `` / `'`, or arm one.
    ///
    /// Same key-layer shape as [`Self::consume_find_key`] and for the same
    /// reason: `ma` is `m` plus an OPERAND, and `a` is bound (append). Without
    /// claiming it first, `ma` would set no mark and enter Insert mode.
    ///
    /// Runs BEFORE `consume_object_key`, because `` d`a `` needs it: the
    /// object path claims `i` and `a` whenever an operator is armed, and the
    /// mark LETTER can be either of them.
    ///
    /// The two do not fight over the first key. This arms only while
    /// `pending_object` is clear, so `di'` — where `'` is a text-object
    /// delimiter rather than a mark jump — still reaches the object path. The
    /// guard states that dependency locally instead of leaving it implied by
    /// call order.
    fn consume_mark_key(&mut self, key: Key) -> Option<ObjectKey> {
        if let Some(kind) = self.pending_mark.take() {
            let Key::Char(name) = key else {
                if matches!(self.op_pending.state(), OpState::Awaiting { .. }) {
                    self.op_pending
                        .dispatch((Action::ChangeMode(Mode::Normal), 1));
                }
                return Some(ObjectKey::Consumed);
            };
            return Some(ObjectKey::Compose(match kind {
                MarkKey::Set => Action::SetMark(name),
                MarkKey::GotoExact => Action::Move(Motion::MarkExact(name)),
                MarkKey::GotoLine => Action::Move(Motion::MarkLine(name)),
            }));
        }
        if !matches!(self.modal.mode(), Mode::Normal | Mode::Visual) {
            return None;
        }
        // Half-typed text object (`di` waiting for its `'`) belongs to the
        // object path, not here; a key continuing a sequence belongs to the
        // sequence (see `consume_find_key` for the `zt` case that proves it).
        if self.pending_object.is_some() || !self.pending_keys.is_empty() {
            return None;
        }
        let Key::Char(c) = key else { return None };
        let kind = match c {
            'm' => MarkKey::Set,
            '`' => MarkKey::GotoExact,
            '\'' => MarkKey::GotoLine,
            _ => return None,
        };
        self.pending_mark = Some(kind);
        Some(ObjectKey::Consumed)
    }

    /// `%` — brackets, plus this buffer's language word pairs if it has any.
    ///
    /// When both are candidates the NEARER one on the line wins, because that
    /// is the one under the operator's eye: on `if foo() then`, `%` on the
    /// `if` means the block and `%` on the `(` means the call. Deciding by
    /// distance rather than by precedence is what keeps both usable from the
    /// same key without a mode.
    fn resolve_match(&self, buf: &escriba_buffer::Buffer, pos: Position) -> Option<Position> {
        let Some(pairs) = self.word_pairs_for_active() else {
            return match_pair(buf, pos);
        };
        let bracket_col = line_chars(buf, pos.line)
            .into_iter()
            .enumerate()
            .skip(pos.column as usize)
            .find(|(_, c)| MATCH_PAIRS.iter().any(|&(o, cl)| *c == o || *c == cl))
            .and_then(|(i, _)| u32::try_from(i).ok());
        let word_col = word_hits(buf, pos.line, pairs)
            .into_iter()
            .find(|h| h.end > pos.column)
            .map(|h| h.col);
        match (bracket_col, word_col) {
            (Some(b), Some(w)) if w < b => match_word_pair(buf, pos, pairs),
            (Some(_), _) => match_pair(buf, pos),
            (None, Some(_)) => match_word_pair(buf, pos, pairs),
            (None, None) => None,
        }
    }

    /// The word pairs for the active buffer's filetype, if the language has
    /// any. `None` for brace languages — Rust's `%` is bracket-only, and that
    /// is correct rather than missing.
    fn word_pairs_for_active(&self) -> Option<WordPairs> {
        let path = self.buffers.get(self.active)?.path.as_deref()?;
        let name = &self.filetypes.resolve(path)?.name;
        WORD_PAIRS
            .iter()
            .find(|(ft, _)| ft == name)
            .map(|(_, pairs)| *pairs)
    }

    fn apply_motion(&mut self, motion: Motion) {
        // A bare search motion is a FAR JUMP and it REPORTS — it records into
        // the jumplist, prints vim's "hit BOTTOM" on a wrap, and says E486
        // when nothing matches. `resolve_motion` can do none of that: it is
        // deliberately pure because the OPERATOR path calls it to find a range
        // without moving the cursor. So `n` routes to the one executor that
        // owns those side effects, and `Action::SearchRepeat` routes to the
        // same place — one code path, two spellings.
        if matches!(motion, Motion::SearchNext | Motion::SearchPrev) {
            self.jump_search(matches!(motion, Motion::SearchPrev));
            return;
        }
        let Some(pos) = self.resolve_motion(self.cursor(), motion) else {
            return;
        };
        // The single cursor-mutation path clamps to the buffer and scrolls
        // the viewport to contain the cursor on both axes.
        self.set_cursor(pos);
    }

    /// Apply an operator over a motion — the vim `{operator}{motion}` verbs
    /// (`dw` delete-word, `c$` change-to-line-end, `y0` yank-to-line-start).
    /// Composition is explicit: the motion resolves a target via
    /// [`resolve_motion`](Self::resolve_motion); the operator acts over the
    /// `[cursor, target)` range. Register-leaving operators
    /// ([`Operator::leaves_register`]) capture the text first.
    /// Apply `op` over `motion` resolved `n` times from the cursor.
    ///
    /// `n == 1` is the ordinary path. Larger `n` walks the motion forward
    /// first and operates over the whole span in one go, which is what vim
    /// means by `3dw` — and the only way a non-moving operator like yank can
    /// honour a count at all.
    fn apply_operator_n(&mut self, op: Operator, motion: Motion, n: u32) {
        if n <= 1 {
            self.apply_operator(op, motion);
            return;
        }
        let from = self.cursor();
        let mut to = from;
        for _ in 0..n {
            match self.resolve_motion(to, motion) {
                Some(next) if next != to => to = next,
                // The motion stopped making progress (start/end of buffer):
                // operate over what we reached rather than aborting, which is
                // what vim does for `999dw` near the end of a file.
                _ => break,
            }
        }
        if to == from {
            // Nothing to operate over. Fall through to the single-step path
            // so its error reporting (E35, pattern-not-found) still runs.
            self.apply_operator(op, motion);
            return;
        }
        if let Some(extent) = self.operated_extent(motion, from, to) {
            self.apply_operator_over(op, extent);
        }
    }

    /// `;` inherits the inclusiveness of the find it repeats — resolve it to
    /// that concrete motion rather than teaching [`Motion::is_inclusive`] about
    /// state it cannot see. `d;` after `fx` must delete THROUGH the `x`.
    ///
    /// Returns the motion unchanged when there is no find to repeat, so the
    /// caller's `is_inclusive` question gets `false` rather than a wrong answer.
    fn concrete_motion(&self, motion: Motion) -> Motion {
        match motion {
            Motion::RepeatFind { reverse } => match self.last_find {
                Some(f) => Motion::FindChar {
                    ch: f.ch,
                    backward: f.backward != reverse,
                    till: f.till,
                },
                None => motion,
            },
            m => m,
        }
    }

    /// One character to the right, clamped to the line.
    ///
    /// The whole of what "inclusive" means to an operator: a range is
    /// `[start, end)`, so acting ON a character means the range must end
    /// AFTER it.
    fn widen_one(&self, pos: Position) -> Position {
        let line_len = self
            .buffers
            .get(self.active)
            .map_or(pos.column, |b| b.line_len_chars(pos.line));
        Position::new(pos.line, pos.column.saturating_add(1).min(line_len))
    }

    /// Widen an INCLUSIVE motion's target to the exclusive end an operator
    /// range needs. See [`Motion::is_inclusive`].
    ///
    /// Applied at the OPERATOR, never inside `resolve_motion`: the same
    /// resolution has to serve the cursor path, where `e` must land ON the
    /// last character, and the range path, where the range must end after it.
    /// One target, two readings — putting the widening in the resolver would
    /// move `e` itself one character too far.
    ///
    /// **Forward motions only.** A backward-inclusive motion (`ge`) widens the
    /// CURSOR instead, which this function cannot express because it is handed
    /// one position; [`Self::operated_extent`] owns that split.
    fn operated_end(&self, motion: Motion, to: Position) -> Position {
        if !self.concrete_motion(motion).is_inclusive() {
            return to;
        }
        self.widen_one(to)
    }

    fn apply_operator(&mut self, op: Operator, motion: Motion) {
        let from = self.cursor();
        let Some(to) = self.resolve_motion(from, motion) else {
            // A motion that cannot resolve aborts the operator with the buffer
            // untouched. A search motion says WHY — `dn` with no pattern armed
            // is otherwise indistinguishable from a dropped keystroke, which
            // is the same complaint that motivated E486 on the bare path.
            if matches!(motion, Motion::SearchNext | Motion::SearchPrev) {
                if self.search.pattern().is_none() {
                    self.messages
                        .push("E35: No previous regular expression".to_string());
                } else {
                    self.report_pattern_not_found();
                }
            }
            return;
        };
        if let Some(extent) = self.operated_extent(motion, from, to) {
            self.apply_operator_over(op, extent);
        }
    }

    /// Apply `op` over `[cursor, to)`.
    ///
    /// Split out of [`Self::apply_operator`] so the operated-search path can
    /// reach the same range machinery with a target it resolved itself — the
    /// alternative was a second copy of the delete/yank/register logic, which
    /// is how the two would drift.
    /// The extent an operated MOTION names — vim's three motion kinds
    /// resolved in ONE place.
    ///
    /// Both motion call sites route through here rather than building an
    /// extent themselves, because "which kind is this motion" is a property of
    /// the motion and must have exactly one answer. It already had two rules
    /// (`operated_end`'s inclusive widening); `is_linewise` is the third, and
    /// a fourth lands here rather than at whichever call site notices it.
    ///
    /// A linewise motion that resolves to the SAME line still names that line:
    /// `dj` on the last line is a failed motion (`resolve_motion` returns the
    /// cursor) and vim refuses it, but `d_` and `dH`-on-the-top-line are one
    /// whole line, which is what the caller's own emptiness check decides.
    fn operated_extent(&self, motion: Motion, from: Position, to: Position) -> Option<Extent> {
        if motion.is_linewise() {
            return self.line_span(from.line, to.line);
        }
        // vim's rule for an inclusive motion is "the last character towards the
        // END OF THE BUFFER is included" — not "the target is included". For a
        // forward motion those are the same sentence and the distinction never
        // shows. For a BACKWARD one (`ge`, `gE`) the buffer-end of the range is
        // the CURSOR, so the widening flips sides: `dge` must take the
        // character under the cursor, and widening the target instead would
        // eat one character too far back and leave the cursor's behind.
        //
        // Which way the motion ran is knowable only here, after resolution —
        // it is a fact about this press, not about the motion.
        let inclusive = self.concrete_motion(motion).is_inclusive();
        let range = if inclusive && to < from {
            Range {
                start: to,
                end: self.widen_one(from),
            }
        } else {
            Range {
                start: from,
                end: self.operated_end(motion, to),
            }
        };
        Some(Extent::charwise(range))
    }

    fn apply_operator_to(&mut self, op: Operator, to: Position) {
        let from = self.cursor();
        // A motion-shaped operation is charwise by construction: it acts over
        // `[cursor, point)`, which is a run of characters even when that run
        // happens to span a line break (`d}`).
        self.apply_operator_over(
            op,
            Extent::charwise(Range {
                start: from,
                end: to,
            }),
        );
    }

    /// Apply `op` over an explicit range, capturing it as `kind`.
    ///
    /// The object path needs this: `gn`'s extent need not begin at the cursor,
    /// so it cannot go through the `[cursor, target)` shape the motion path
    /// uses. One implementation of the delete/yank/register logic, reached two
    /// ways.
    ///
    /// `kind` is a PARAMETER rather than something inferred from the range,
    /// and it has to be: `[(1,0), (2,0))` is the range `dd` produces on line 1
    /// AND the range `dj`-ish charwise motions produce, and nothing about the
    /// two positions distinguishes them. Only the caller knows which gesture
    /// it was. It travels to the register, and the register is what a later
    /// `p` reads to decide between splicing and opening a line.
    fn apply_operator_over(&mut self, op: Operator, extent: Extent) {
        let Extent {
            capture,
            removal,
            kind,
        } = extent.normalized();
        if capture.is_empty() {
            return;
        }
        // Capture the operated text (for the register) before mutating.
        let text = self
            .buffers
            .get(self.active)
            .and_then(|buf| buf.slice(capture).ok());
        if op.leaves_register() {
            if let Some(t) = &text {
                let captured = match kind {
                    RegisterKind::Charwise => t.clone(),
                    RegisterKind::Linewise => as_linewise_capture(t),
                };
                self.register = Some(Register::new(captured, kind));
            }
        }
        match op {
            // Delete + Change remove the range; Change then enters Insert so
            // the operator pairs with immediate typing (`ciw`, `c$`).
            //
            // They remove DIFFERENT ranges for a linewise extent, which is the
            // whole reason `Extent` carries two: `dd` takes the line and its
            // terminator, `cc` clears the line's text and KEEPS the line,
            // because you are changing its contents rather than removing it.
            // Both leave the same thing in the register.
            Operator::Delete | Operator::Change => {
                let cut = if op == Operator::Change {
                    removal
                } else {
                    capture
                };
                if cut.is_empty() {
                    // `cc` on an already-empty line: nothing to clear, but the
                    // gesture still means "type here".
                    self.rest_after_operator(kind, cut.start);
                    self.modal.enter(Mode::Insert);
                    return;
                }
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    let _ = buf.apply(&Edit::delete(cut));
                }
                self.rest_after_operator(kind, cut.start);
                if op == Operator::Change {
                    self.modal.enter(Mode::Insert);
                }
            }
            // Yank copies to the register without mutating the buffer, so it
            // gets its OWN resting rule rather than the delete rule above: the
            // line is still there, and vim moves the cursor only when the yank
            // reached BACKWARDS past it.
            //
            // Keyed on the kind for the same reason everything else here is.
            // A linewise yank compares LINES — `yy` and `3yy` both start on
            // the cursor's own line, so neither moves, which is why comparing
            // POSITIONS was wrong: `yy`'s range starts at column 0, so it read
            // as "backwards" and knocked the cursor to the left margin every
            // time you copied a line. Invisible for `yw`, whose range starts
            // exactly at the cursor, so the move was a no-op there.
            Operator::Yank => {
                let here = self.cursor();
                match kind {
                    RegisterKind::Linewise if capture.start.line < here.line => {
                        // Keep the column: a backwards linewise yank rests on
                        // the first line taken, not at its margin.
                        self.set_cursor(Position::new(capture.start.line, here.column));
                    }
                    RegisterKind::Charwise
                        if (capture.start.line, capture.start.column)
                            < (here.line, here.column) =>
                    {
                        self.set_cursor(capture.start);
                    }
                    _ => {}
                }
            }
            // Indent/Format/structural operators are not yet wired — named,
            // not faked (no buffer mutation, register already captured for the
            // register-leaving ones above).
            _ => {
                self.messages
                    .push("operator not yet implemented".to_owned());
            }
        }
    }

    /// `r{char}` — overwrite `count` characters from the cursor with `char`.
    ///
    /// Three things it deliberately is NOT, each of which a `Change`-operator
    /// composition would get wrong: it does not enter Insert, it does not
    /// touch the register, and it REFUSES rather than truncating when the
    /// count runs past the end of the line. vim's rule is that `5rx` on a
    /// three-character tail does nothing at all — a partial replace would
    /// silently destroy two characters you did not mean to name.
    fn replace_char(&mut self, ch: char, count: u32) {
        let n = count.max(1);
        let here = self.cursor();
        let Some(buf) = self.buffers.get(self.active) else {
            return;
        };
        let len = buf.line_len_chars(here.line);
        if here.column.saturating_add(n) > len {
            // Silent, like vim. The line is short — there is nothing to say
            // that the unchanged text does not already say.
            return;
        }
        let end = Position::new(here.line, here.column + n);
        let mut text = String::with_capacity(n as usize);
        for _ in 0..n {
            text.push(ch);
        }
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        if buf
            .apply(&Edit::replace(Range::new(here, end), text))
            .is_err()
        {
            return;
        }
        // vim leaves the cursor on the LAST character replaced, not after it.
        self.set_cursor(Position::new(here.line, here.column + n - 1));
    }

    /// `J` / `gJ` — join `count` lines into one.
    ///
    /// One `Edit::replace` over the whole span rather than `n` splices, so a
    /// `3J` is one `u` away from gone and the damage classifier sees a single
    /// line-count change.
    ///
    /// `space: true` (`J`) drops the next line's leading whitespace and puts a
    /// single space in the newline's place, with vim's two exceptions: no
    /// space is added when the line already ends in one, or when the next line
    /// starts with `)`. `space: false` (`gJ`) splices verbatim — the reason to
    /// reach for it is that `J` is lossy.
    fn join_lines(&mut self, space: bool, count: u32) {
        // `J` and `2J` both mean "join ONE following line": vim counts LINES
        // involved, not joins performed, so the join count is `count - 1`
        // floored at 1.
        let joins = count.max(2) - 1;
        let here = self.cursor();
        let Some(buf) = self.buffers.get(self.active) else {
            return;
        };
        let last = last_text_line(buf);
        if here.line >= last {
            // Nothing below to join. vim beeps; escriba says so, because a key
            // that silently does nothing is indistinguishable from an unbound
            // one — which is how `<C-h>` hid for a month.
            self.messages
                .push("E36: Not enough lines to join".to_string());
            return;
        }
        let end_line = here.line.saturating_add(joins).min(last);
        // `Buffer::line` INCLUDES the trailing newline and `line_len_chars`
        // excludes it — a mismatch that made the first cut of this splice the
        // terminators back in and then leave the originals behind, so `J`
        // produced the file unchanged plus a blank line. `line_chars` is the
        // newline-free reading every motion already uses.
        let line_text = |l: u32| line_chars(buf, l).into_iter().collect::<String>();
        let mut joined = line_text(here.line);
        // Where the cursor lands: vim puts it ON the join — the position the
        // newline used to occupy, which is the space it inserted.
        let mut caret = u32::try_from(joined.chars().count()).unwrap_or(0);
        for l in (here.line + 1)..=end_line {
            let next = line_text(l);
            caret = u32::try_from(joined.chars().count()).unwrap_or(0);
            if space {
                let trimmed = next.trim_start();
                let needs_space = !joined.is_empty()
                    && !joined.ends_with(char::is_whitespace)
                    && !trimmed.starts_with(')')
                    && !trimmed.is_empty();
                if needs_space {
                    joined.push(' ');
                }
                joined.push_str(trimmed);
            } else {
                joined.push_str(&next);
            }
        }
        let span = Range::new(
            Position::new(here.line, 0),
            Position::new(end_line, buf.line_len_chars(end_line)),
        );
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        if buf.apply(&Edit::replace(span, joined)).is_err() {
            return;
        }
        self.set_cursor(Position::new(here.line, caret));
    }

    /// Where the cursor rests once an operator has finished.
    ///
    /// Keyed on the operated KIND rather than on the operator, because that is
    /// what vim keys it on: every linewise operation lands the cursor the same
    /// way regardless of which operator produced it.
    ///
    /// The charwise arm is the old unconditional behaviour — the range start,
    /// which is where the text used to begin.
    fn rest_after_operator(&mut self, kind: RegisterKind, start: Position) {
        match kind {
            RegisterKind::Charwise => self.set_cursor(start),
            RegisterKind::Linewise => {
                // vim's linewise rule: the cursor lands on the FIRST NON-BLANK
                // of the line that now occupies the operated line's index, and
                // never past the last line that holds text.
                //
                // Both halves were wrong, and each in a way no unit test could
                // see, because both are about WHERE THE CURSOR IS rather than
                // what the text says — and every `dd` test asserted the text.
                //
                //   - Column 0 instead of the first non-blank is untidy on flat
                //     prose and actively wrong on indented code: `dd` inside a
                //     nested block dropped the cursor into the indentation, so
                //     the next `i` typed at the margin.
                //   - Landing past the last line of text was worse. A file
                //     ending in `\n` makes the rope report a phantom final
                //     line (see `last_text_line`); `dd` on the last REAL line
                //     parked the cursor on that phantom row, where `x` and `i`
                //     had nothing to act on and the next `dd` deleted the
                //     file's trailing NEWLINE rather than a line.
                let at = match self.buffers.get(self.active) {
                    Some(buf) => first_non_blank(buf, start.line.min(last_text_line(buf))),
                    None => return,
                };
                self.set_cursor(at);
            }
        }
    }

    /// `p` / `P` — put `count` copies of the register back into the buffer.
    ///
    /// **The register's [`RegisterKind`] chooses the operation, not the key.**
    /// `p` after `dw` splices characters in at a column; `p` after `dd` opens
    /// a whole line below. That is why the capture had to become typed before
    /// this could exist at all: a `String` register leaves `p` guessing, and
    /// the only guess available — splice — drops a whole line, terminator and
    /// all, into the middle of whatever line the cursor is on.
    ///
    /// One [`Edit`] regardless of `count`, so `3p` is one `u` away from gone.
    fn put(&mut self, before: bool, count: u32) {
        let Some(reg) = self.register.clone() else {
            // vim says nothing for a put with an empty register, and neither
            // does this — but it must not fall through to an insert of `""`
            // either, which would record a `last_change` that `.` then
            // replays as a no-op edit.
            return;
        };
        let text = reg.replayed(count);
        if text.is_empty() {
            return;
        }
        let Some(buf) = self.buffers.get(self.active) else {
            return;
        };
        let here = self.cursor();
        let (at, rest) = match reg.kind {
            RegisterKind::Linewise => {
                // `p` opens BELOW the cursor's line, `P` above. The insertion
                // point is the start of a line either way, and the text ends
                // in a newline (`Register::replayed` guarantees it), so the
                // splice pushes the existing line down rather than joining it.
                //
                // `line + 1` is a valid insertion point even on the last line:
                // a file ending in `\n` has the phantom row there, and one
                // that does not gets the newline from `replayed`.
                let line = if before {
                    here.line
                } else {
                    here.line.saturating_add(1)
                };
                let at = Position::new(line.min(buf.line_count()), 0);
                // vim rests on the first non-blank of the FIRST line put.
                (at, PutRest::LineStart(at.line))
            }
            RegisterKind::Charwise => {
                // `p` lands AFTER the character under the cursor, `P` on it.
                // Appending past the end of the line is legal here — that is
                // what makes `p` on the last character of a line work — so
                // this clamps to the line length, not to the last character.
                let col = if before {
                    here.column
                } else {
                    here.column
                        .saturating_add(1)
                        .min(buf.line_len_chars(here.line))
                };
                (Position::new(here.line, col), PutRest::LastCharPut)
            }
        };
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        if buf.apply(&Edit::insert(at, text.clone())).is_err() {
            return;
        }
        match rest {
            PutRest::LineStart(line) => {
                let to = match self.buffers.get(self.active) {
                    Some(b) => first_non_blank(b, line.min(last_text_line(b))),
                    None => return,
                };
                self.set_cursor(to);
            }
            // vim leaves the cursor ON the last character put, not after it —
            // which is what makes `p` then `.`-less repeated puts stack rather
            // than march right. Routed through `set_cursor` (an `OnCharacter`
            // rest) so Normal mode's on-a-character invariant still applies.
            PutRest::LastCharPut => {
                let added_lines = u32::try_from(text.matches('\n').count()).unwrap_or(0);
                let end = if let Some(nl) = text.rfind('\n') {
                    let tail = u32::try_from(text[nl + 1..].chars().count()).unwrap_or(0);
                    Position::new(at.line + added_lines, tail)
                } else {
                    let n = u32::try_from(text.chars().count()).unwrap_or(0);
                    Position::new(at.line, at.column.saturating_add(n))
                };
                self.set_cursor(Position::new(end.line, end.column.saturating_sub(1)));
            }
        }
    }

    /// The text last yanked or deleted into the unnamed register, if any.
    /// `p`/`P` read this — through [`Register`], so they can tell a captured
    /// LINE from a captured run of characters.
    #[must_use]
    pub fn register(&self) -> Option<&Register> {
        self.register.as_ref()
    }

    /// The register's raw text, for callers that only want the characters.
    #[must_use]
    pub fn register_text(&self) -> Option<&str> {
        self.register.as_ref().map(|r| r.text.as_str())
    }

    fn insert_char(&mut self, c: char) {
        if self.modal.mode() == Mode::Command {
            // A search prompt and an ex-command share Command mode (vim's
            // cmdline). `search.is_prompting()` is the typed discriminator —
            // it can only be true when `/` or `?` actually opened a prompt.
            if self.search.is_prompting() {
                // The search prompt is the SOLE store while it is open.
                //
                // This used to also `push_minibuffer(c)`, and the two stores
                // insert differently — `search.push` at the caret, the
                // minibuffer always at the end — so `/fo<Left>X` left them
                // reading `fXo` and `foX`. That was one of FIVE desync paths;
                // the caret moves, forward-delete, delete-word and
                // clear-to-start never touched the shadow at all.
                //
                // Deleting the write costs nothing because `status_model`
                // already selects the minibuffer only on the `prompt == None`
                // branch — the shadow is the EX-LINE's store, and while a
                // search prompt is open nothing reads it.
                self.search.push(c);
                self.preview_search();
            } else {
                self.modal.push_minibuffer(c);
            }
            return;
        }
        let cursor = self.cursor();
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        let edit = Edit::insert(cursor, c.to_string());
        if buf.apply(&edit).is_ok() {
            let next = if c == '\n' {
                Position::new(cursor.line.saturating_add(1), 0)
            } else {
                cursor.shift_right(1)
            };
            // Route through the single cursor-mutation path so the viewport
            // follows the cursor (both axes) and the cursor stays clamped.
            self.place_cursor(next, CursorRest::AtInsertPoint);
        }
    }

    /// Enter Insert mode at `at` — the ONE body behind `i` `I` `a` `A` `o` `O`.
    ///
    /// # What lets `A` park past the last character
    ///
    /// [`Self::place_cursor`] pulls a caret back to `len - 1` — but only when
    /// **both** halves of its guard hold: `rest == CursorRest::OnCharacter`
    /// *and* the mode is `Normal`. `A` and `a`-at-end-of-line need that clamp
    /// lifted, and this function lifts it twice over: it enters Insert before
    /// placing anything, and it asks for `CursorRest::AtInsertPoint`.
    ///
    /// **Either one alone is sufficient**, which is worth writing down because
    /// it is the opposite of what it looks like. An earlier version of this
    /// comment claimed the ORDER was load-bearing on its own; the red run
    /// refuted it — reversing the order while keeping `AtInsertPoint` stays
    /// green, and so does `OnCharacter` while entering Insert first. Only
    /// removing BOTH goes red, and then `a` on the `o` of "hello" reports
    /// column 4 instead of 5: the caret sits back on the character it was meant
    /// to append after. Belt and braces here is deliberate — the two guards
    /// answer different questions ("what kind of place is this?" and "what mode
    /// are we in?") and a later refactor is free to change one.
    ///
    /// `o`/`O` are in this function rather than in an `Edit` action because
    /// they are ONE gesture: vim's `o` is not "insert a newline, then enter
    /// insert" — the caret must land on the new line, and an operator watching
    /// two separate actions would record two dot-repeat entries for one press.
    fn enter_insert_at(&mut self, at: InsertAt) {
        self.modal.enter_insert();
        let cursor = self.cursor();
        // Resolve everything that needs the buffer BEFORE mutating, so the
        // immutable borrow ends before `place_cursor`/`apply` want `&mut self`.
        let Some(buf) = self.buffers.get(self.active) else {
            return;
        };
        let line_len = buf.line_len_chars(cursor.line);
        let target = match at {
            // `i` — the caret is already the insert point.
            InsertAt::Caret => Some(cursor),
            // One past the last char is a legal insert point, which is what
            // lets `a` on the final character append rather than stall.
            InsertAt::AfterCaret => Some(Position::new(
                cursor.line,
                cursor.column.saturating_add(1).min(line_len),
            )),
            InsertAt::LineEnd => Some(Position::new(cursor.line, line_len)),
            InsertAt::FirstNonBlank => Some(first_non_blank(buf, cursor.line)),
            // Handled below — these two edit the buffer first.
            InsertAt::OpenBelow | InsertAt::OpenAbove => None,
        };
        if let Some(pos) = target {
            self.place_cursor(pos, CursorRest::AtInsertPoint);
            return;
        }
        // `o`/`O` — open a line by inserting the terminator at the boundary the
        // direction names, then land on the fresh line. Expressed as an
        // `Edit::insert` through `Buffer::apply` so it joins the undo history
        // the same way typed text does.
        let (at_pos, land_on) = match at {
            InsertAt::OpenBelow => (
                Position::new(cursor.line, line_len),
                Position::new(cursor.line.saturating_add(1), 0),
            ),
            // Inserting at column 0 pushes the current line DOWN, so the fresh
            // line takes the caret's own line number.
            _ => (Position::new(cursor.line, 0), Position::new(cursor.line, 0)),
        };
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        if buf.apply(&Edit::insert(at_pos, "\n")).is_ok() {
            self.place_cursor(land_on, CursorRest::AtInsertPoint);
        }
    }

    /// `<BS>` against the BUFFER — the Insert-mode arm of [`Action::Backspace`].
    ///
    /// Deletes `[target, cursor)` where `target` is the previous character
    /// position, so column 0 JOINS with the line above rather than stopping
    /// dead: the range spans the newline and one `Edit::delete` removes it.
    /// `Motion::Left` cannot express that — it saturates at column 0, which is
    /// why this does not route through `apply_operator`.
    ///
    /// The other reason it does not: `Operator::Delete` captures the unnamed
    /// register, and vim's insert-mode backspace does not. Erasing a typo
    /// should not silently overwrite what you yanked to paste.
    fn delete_before_cursor(&mut self) {
        let cursor = self.cursor();
        let Some(buf) = self.buffers.get(self.active) else {
            return;
        };
        let target = if cursor.column > 0 {
            Position::new(cursor.line, cursor.column.saturating_sub(1))
        } else if cursor.line > 0 {
            let above = cursor.line.saturating_sub(1);
            Position::new(above, buf.line_len_chars(above))
        } else {
            // Start of the document — nothing to the left. A no-op, not a
            // clamp onto something else.
            return;
        };
        self.erase_back_to(target);
    }

    /// Delete `[target, cursor)` and park the caret on `target`.
    ///
    /// The shared body of every BACKWARD erase against the buffer — `<BS>`,
    /// `<C-w>`, `<C-u>`. They differ only in how far back they reach, so the
    /// two properties that must hold for all three live here once rather than
    /// three times: the edit does NOT route through `apply_operator` (see
    /// [`Self::delete_before_cursor`] for both reasons), and the caret lands
    /// via `set_cursor` so the viewport follows and the clamp still runs.
    ///
    /// A `target` at or after the cursor is a no-op. That is the guard that
    /// makes the callers safe to write as "resolve a position, hand it over":
    /// `word_prev` returns the cursor unchanged at column 0 and
    /// `first_non_blank` returns a position AHEAD of the cursor inside an
    /// indent, and a reversed `Range` would be a delete of unknown extent
    /// rather than nothing.
    fn erase_back_to(&mut self, target: Position) {
        let cursor = self.cursor();
        if (target.line, target.column) >= (cursor.line, cursor.column) {
            return;
        }
        let edit = Edit::delete(Range {
            start: target,
            end: cursor,
        });
        if let Some(buf) = self.buffers.get_mut(self.active) {
            if buf.apply(&edit).is_ok() {
                self.set_cursor(target);
            }
        }
    }

    /// `<C-w>` against the BUFFER — the Insert-mode arm of
    /// [`Action::DeleteWordBefore`].
    ///
    /// Reaches back over `Motion::WordStartPrev`, the SAME resolver the cursor
    /// move and the operator range already stand on, so `<C-w>` and `db` agree
    /// on where a word starts by construction instead of by two hand-written
    /// scans that drift.
    ///
    /// `word_prev` is single-line and returns the cursor unchanged at column 0,
    /// which would make `<C-w>` a dead key at the start of a line. vim erases
    /// the line break there, so the zero-width case falls through to
    /// [`Self::delete_before_cursor`] — one character back, which at column 0
    /// IS the newline.
    fn delete_word_before_cursor(&mut self) {
        let cursor = self.cursor();
        let Some(target) = self.resolve_motion(cursor, Motion::WordStartPrev) else {
            return;
        };
        if (target.line, target.column) >= (cursor.line, cursor.column) {
            self.delete_before_cursor();
            return;
        }
        self.erase_back_to(target);
    }

    /// `<C-u>` against the BUFFER — the Insert-mode arm of
    /// [`Action::DeleteToLineStart`].
    ///
    /// Two-step, as vim is: the first press erases back to the first non-blank
    /// (what you typed), and a second press — now sitting ON the first
    /// non-blank, so that target is no longer behind the cursor — erases the
    /// indent. Collapsing the two into "always column 0" would destroy
    /// alignment on the first press, which is the one the hands reach for.
    ///
    /// Never joins with the line above: `<C-u>` is a line-scoped verb, and at
    /// column 0 it is a no-op rather than a silent line-merge.
    fn delete_to_line_start(&mut self) {
        let cursor = self.cursor();
        let Some(indent) = self.resolve_motion(cursor, Motion::LineFirstNonBlank) else {
            return;
        };
        let target = if (indent.line, indent.column) < (cursor.line, cursor.column) {
            indent
        } else {
            Position::new(cursor.line, 0)
        };
        self.erase_back_to(target);
    }

    /// `<Del>` against the BUFFER — the Insert-mode arm of
    /// [`Action::DeleteForward`]. The cursor does NOT move: forward-delete
    /// pulls the rest of the line leftwards under a stationary caret.
    fn delete_after_cursor(&mut self) {
        let cursor = self.cursor();
        let Some(buf) = self.buffers.get(self.active) else {
            return;
        };
        let target = if cursor.column < buf.line_len_chars(cursor.line) {
            Position::new(cursor.line, cursor.column.saturating_add(1))
        } else if cursor.line.saturating_add(1) < buf.line_count() {
            // At end-of-line the character ahead IS the newline, so this
            // joins the line below — the mirror of `delete_before_cursor`.
            Position::new(cursor.line.saturating_add(1), 0)
        } else {
            return;
        };
        let edit = Edit::delete(Range {
            start: cursor,
            end: target,
        });
        if let Some(buf) = self.buffers.get_mut(self.active) {
            let _ = buf.apply(&edit);
        }
    }

    /// Backspace inside a prompt. Keeps the search buffer and the displayed
    /// minibuffer in lockstep — if only one shrank, the pattern submitted
    /// would differ from the text on screen.
    fn prompt_backspace(&mut self) -> bool {
        if self.modal.mode() != Mode::Command {
            return false;
        }
        if self.search.is_prompting() {
            // Backspacing past the `/` closes the prompt, as vim does. No
            // `pop_minibuffer` here for the same reason as `insert_char`: the
            // shadow is the ex-line's, and popping its TAIL when the caret is
            // mid-pattern was another desync path.
            if self.search.backspace() {
                self.modal.clear_minibuffer();
                self.modal.enter(Mode::Normal);
            }
            // Never `pop_minibuffer` on the search path: it pops the TAIL,
            // while `search.backspace()` removes the char before the CARET.
            return true;
        }
        self.modal.pop_minibuffer();
        true
    }

    fn submit_command(&mut self) {
        // Read the command line BEFORE leaving Command mode — the minibuffer
        // exists only in the `Command` variant, so the escape must come
        // after the capture.
        let line = self.modal.minibuffer().to_string();
        self.modal.escape();
        // The ex-name grammar — vim's abbreviations and its `!` — lives in
        // `escriba_command::ex` and NOT here. It used to be three arms in a
        // `match` at the bottom of this file (`"w" => "save"`, …), which is
        // why `:wq` reported "command not found" while `:w` and `:q` both
        // worked: there was nowhere for a compound spelling to be known.
        let Some(inv) = escriba_command::ex::parse(&line) else {
            return;
        };
        self.run_command(&inv.command, &inv.args);
    }

    fn run_command(&mut self, name: &str, args: &[String]) {
        // Bound the command -> RunCommand slip -> command cycle. Refused and
        // reported, never a stack overflow: an editor that dies under the
        // operator loses their buffer, and a script that loops is a mistake
        // they should be told about, not punished for.
        if self.dispatch_depth >= Self::MAX_DISPATCH_DEPTH {
            let mut m = String::from("command recursion too deep at `");
            m.push_str(name);
            m.push_str("` — refusing");
            self.messages.push(m);
            self.damage = self.damage.join(Damage::Viewport);
            self.bump_gen();
            return;
        }
        self.dispatch_depth += 1;
        self.run_command_inner(name, args);
        self.dispatch_depth -= 1;
    }

    /// How many nested command dispatches are allowed. Deep enough that no
    /// legitimate script notices, shallow enough to fail fast.
    const MAX_DISPATCH_DEPTH: u8 = 8;

    fn run_command_inner(&mut self, name: &str, args: &[String]) {
        // Lazy-activation seam (lazy.nvim `cmd =` model): a user plugin
        // gated on `Command: <name>` has its entry applied the first time
        // that command runs, BEFORE dispatch — so the activated plugin
        // can register the very command being invoked and it resolves on
        // this same call.
        if self.plugin_host.pending() > 0 {
            let pending = self.plugin_host.pending_for_command(name);
            for src in pending {
                self.apply_plugin_entry(&src);
            }
        }
        // Read through the counter, then interpret. Two immutable borrows of
        // `self` (the window and the registry) coexist; the `&mut` comes
        // afterwards, once the outcome is owned. That sequencing IS the
        // seam: there is no moment where a command body and `&mut self` are
        // live at the same time.
        let outcome = {
            let window = self.window();
            self.commands.run(name, &window, args)
        };
        match outcome {
            Ok(o) => self.interpret(o),
            // Reported, never fatal (Phase 0). A failed command must not
            // take the editor down, but it must not be invisible either.
            Err(e) => {
                self.messages.push(describe_command_failure(name, &e));
                self.damage = self.damage.join(Damage::Viewport);
                self.bump_gen();
            }
        }
    }

    // ── tatara-lisp runtime bridge (imperative programmability tier) ──

    /// Capture a read snapshot of the editor for the tatara-lisp host.
    /// Lisp reads (`cursor-line`, `current-line`, …) answer from this.
    #[must_use]
    pub fn snapshot(&self) -> EditorSnapshot {
        let current_line = self
            .buffers
            .get(self.active)
            .and_then(|b| b.line(self.cursor().line))
            .map(|s| s.trim_end_matches('\n').to_string())
            .unwrap_or_default();
        let buffer_name = self
            .buffers
            .get(self.active)
            .and_then(|b| b.path.as_ref())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "[scratch]".to_string());
        EditorSnapshot {
            cursor_line: i64::from(self.cursor().line),
            cursor_column: i64::from(self.cursor().column),
            current_line,
            mode: self.modal.mode().as_str().to_string(),
            buffer_name,
        }
    }

    /// Evaluate tatara-lisp `src` against this editor: capture a
    /// snapshot, run it in the embedded VM, then apply the typed effects
    /// the program emitted. This is the imperative programmability tier
    /// — live Lisp that reads state and drives the editor through the
    /// sandboxed effect boundary.
    ///
    /// **Snapshot semantics:** the read snapshot is captured ONCE before
    /// eval, and effects are applied AFTER the program returns. So within
    /// a single `run_lisp` call a program cannot observe its own writes —
    /// `(insert "x") (cursor-column)` reads the pre-insert column. This
    /// snapshot-isolation is deliberate (it's what makes the effect
    /// boundary a clean sandbox seam); a program that must read its own
    /// effects splits the work across calls. The VM is cached
    /// ([`Self::lisp_vm`]) so the stdlib is installed once and top-level
    /// `define`s persist across calls (REPL-like).
    pub fn run_lisp(&mut self, src: &str) -> Result<(), VmError> {
        let mut host = EscribaHost::with_snapshot(self.snapshot());
        let vm = self.lisp_vm.get_or_insert_with(EscribaVm::new);
        vm.eval(src, &mut host)?;
        let effects = host.take_effects();
        self.apply_host_effects(effects);
        Ok(())
    }

    /// Apply tatara-lisp effects to live editor state.
    ///
    /// A thin adapter now. It used to be `apply_host_effects`, a THIRD
    /// implementation of message-push / option-insert / insert-text beside
    /// the Action executor and the slip interpreter — the same duplication
    /// that let `u` and `:undo` drift apart in M3. The VM emits slips; this
    /// hands them to the one interpreter.
    pub fn apply_host_effects(&mut self, effects: Vec<Negai>) {
        self.interpret(Outcome::did(effects));
    }

    /// Insert a (possibly multi-line) string at the cursor and advance
    /// the cursor past it. Used by the `(insert …)` effect.
    fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let cursor = self.cursor();
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        let edit = Edit::insert(cursor, text.to_string());
        if buf.apply(&edit).is_ok() {
            let next = if let Some(nl) = text.rfind('\n') {
                let added_lines = u32::try_from(text.matches('\n').count()).unwrap_or(0);
                let last_line_len = u32::try_from(text[nl + 1..].chars().count()).unwrap_or(0);
                Position::new(cursor.line + added_lines, last_line_len)
            } else {
                let n = u32::try_from(text.chars().count()).unwrap_or(0);
                cursor.shift_right(n)
            };
            // Route through the single cursor-mutation path so the viewport
            // follows the cursor (both axes) and the cursor stays clamped.
            self.place_cursor(next, CursorRest::AtInsertPoint);
        }
    }
}

fn first_non_blank(buf: &escriba_buffer::Buffer, line: u32) -> Position {
    let Some(text) = buf.line(line) else {
        return Position::new(line, 0);
    };
    let col = text
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .count();
    Position::new(line, u32::try_from(col).unwrap_or(0))
}

/// A character search: which character, which direction, and whether it stops
/// ON it (`f`/`F`) or just BEFORE it (`t`/`T`).
///
/// The same value serves the pending operand and the `;`/`,` memory, so the
/// thing repeated is the thing that ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FindSpec {
    ch: char,
    backward: bool,
    till: bool,
}

/// What the next keystroke means after `m`, `` ` `` or `'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkKey {
    /// `m{a-z}` — set.
    Set,
    /// `` `{a-z} `` — jump to the exact position.
    GotoExact,
    /// `'{a-z}` — jump to the line's first non-blank.
    GotoLine,
}

/// What kind of place a cursor move is asking for.
///
/// The Normal-mode rule "the cursor sits ON a character" is about where the
/// cursor comes to REST. It is not about where text goes next: a write that
/// appends `abc` leaves the cursor after the `c`, and that position is one
/// past the last character by construction — clamping it back would make the
/// next append land inside the text just written. The lisp `(insert …)`
/// effect is the case that proves it, because it runs in Normal mode.
///
/// A parameter rather than two functions, so both readings stay in front of
/// whoever changes the clamp.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum CursorRest {
    /// A motion's destination — Normal mode pulls it onto a character.
    OnCharacter,
    /// Where the next character goes — never pulled back.
    AtInsertPoint,
}

/// What an operator acts over — a CAPTURE range, a REMOVAL range, and the
/// kind they were resolved as.
///
/// Two ranges because for a linewise extent they genuinely differ, and every
/// attempt to derive one from the other re-decides which branch produced it:
///
///   - `dd` removes the line AND its terminator; `cc` clears the line's text
///     and KEEPS the line, because you are changing its contents rather than
///     removing it. Same lines, same register content, different cut.
///   - On the last line of a file with no trailing newline the removal has to
///     swallow the PRECEDING newline (there is none following), so it starts
///     on a different LINE than the extent names.
///
/// Charwise extents have `capture == removal`, which is why the old
/// single-range signature was right for everything except the linewise change
/// and wrong there — `cc` deleted the line.
#[derive(Clone, Copy, Debug)]
struct Extent {
    /// What goes in the register, and what a delete removes.
    capture: Range,
    /// What a CHANGE removes. Equal to `capture` unless the kind is linewise.
    removal: Range,
    kind: RegisterKind,
}

impl Extent {
    /// A run of characters: one range, both roles.
    const fn charwise(r: Range) -> Self {
        Self {
            capture: r,
            removal: r,
            kind: RegisterKind::Charwise,
        }
    }

    /// An extent resolved by a text object, keyed on what the object says it
    /// is. The linewise case needs the explicit constructor below, so this is
    /// the charwise-or-nothing door.
    fn from_object(r: Range, kind: RegisterKind) -> Self {
        match kind {
            RegisterKind::Charwise => Self::charwise(r),
            // A caller that has only a range cannot supply the text-only
            // removal, so the two coincide — which is the pre-`cc` behaviour
            // and is correct for every linewise object except a change. The
            // `Line` object goes through `line_extent` instead.
            RegisterKind::Linewise => Self {
                capture: r,
                removal: r,
                kind,
            },
        }
    }

    fn normalized(self) -> Self {
        Self {
            capture: self.capture.normalized(),
            removal: self.removal.normalized(),
            kind: self.kind,
        }
    }
}

/// Normalize a linewise capture: newline-TERMINATED, never newline-LED.
///
/// The removal range and the register capture are two different views of one
/// gesture, and the last line of a file with no trailing newline is where they
/// come apart. There is no following terminator to take, so `dd` has to
/// swallow the PRECEDING one — the right thing to REMOVE, and the wrong thing
/// to put back: the raw slice reads `"\nbravo"`, so `yyp` opened a blank line
/// and then a `bravo` with no terminator of its own.
///
/// Stated once, here, where the kind is known. `Register::replayed` handles
/// the other half (a capture that ends without a newline).
fn as_linewise_capture(slice: &str) -> String {
    match slice.strip_prefix('\n') {
        Some(rest) => {
            let mut s = String::with_capacity(slice.len());
            s.push_str(rest);
            s.push('\n');
            s
        }
        None => slice.to_owned(),
    }
}

/// How a captured operand's composed action reaches the executor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OperandCount {
    /// The capture already applied its own repeats; run the composed action
    /// once. Only the object path does this (`2diw` repeats inside it).
    SelfCounted,
    /// Drain the pending count and hand it to `apply_counted`, so `3fx` /
    /// `3ra` / ``3`a`` repeat on the one path every other motion uses.
    Drained,
}

/// One step of the operand-capture chain.
struct OperandCapture {
    /// Stable label — what `operand_capture_order.rs` asserts against.
    name: &'static str,
    claim: fn(&mut EditorState, Key) -> Option<ObjectKey>,
    count: OperandCount,
}

/// **The operand-capture chain, in the order that matters.**
///
/// Every adjacency is a dependency with a named failure:
///
/// 1. **mark before object** — the object path claims `i`/`a` whenever an
///    operator is armed, and a mark LETTER can be either, so ``d`a`` lost its
///    `a` to it. They do not fight over the FIRST key (the mark path arms only
///    while `pending_object` is clear, so `di'` still reaches the object
///    path); they fight over the SECOND, and the gesture already half-typed
///    must win.
/// 2. **object before find** — `di(` must not read as `d`, then `i` (insert),
///    then a literal `(`.
/// 3. **find before replace** — no live conflict; `f`/`t` and `r` arm on
///    disjoint keys and neither can be pending while the other is. Ordered
///    for stability rather than necessity, and said so rather than implying a
///    constraint that is not there.
/// 4. **all four before the sequence stepper and the keymap** — this is the
///    whole point. Each capture also declines while `pending_keys` is
///    non-empty, so a LATER key of a gesture (`zt`'s `t`) belongs to the
///    sequence rather than arming a till-find.
static OPERAND_CHAIN: &[OperandCapture] = &[
    OperandCapture {
        name: "mark",
        claim: EditorState::consume_mark_key,
        count: OperandCount::Drained,
    },
    OperandCapture {
        name: "object",
        claim: EditorState::consume_object_key,
        count: OperandCount::SelfCounted,
    },
    OperandCapture {
        name: "find",
        claim: EditorState::consume_find_key,
        count: OperandCount::Drained,
    },
    OperandCapture {
        name: "replace",
        claim: EditorState::consume_replace_key,
        count: OperandCount::Drained,
    },
];

/// The chain's order, for the gate in `tests/operand_capture_order.rs`.
#[must_use]
pub fn operand_capture_order() -> Vec<&'static str> {
    OPERAND_CHAIN.iter().map(|c| c.name).collect()
}

/// Does this action ABSORB its count into one operation, or REPEAT?
///
/// A free function rather than a method on `Action` deliberately: the answer
/// is a property of THIS EXECUTOR's arms, not of the action's meaning. An arm
/// absorbs its count exactly when it takes one, and a name listed here that no
/// arm reads is worse than no list — it reads as handled and behaves as
/// repeated. Keep the two in step; the tests below pin every member.
fn absorbs_count(action: &Action) -> bool {
    matches!(
        action,
        Action::ApplyOperator { .. }
            | Action::Put { .. }
            // `3ra` is one replace of three characters (and refuses if there
            // are not three), `3J` is one join of three lines. Repeating
            // either would walk the cursor and do the wrong thing three times.
            | Action::ReplaceChar(_)
            | Action::JoinLines { .. }
            // Only the LINEWISE object can express an `n`-fold extent today.
            // `2diw` still repeats, which is the same over-count-a-yank defect
            // waiting on a general "resolve this object n times" — named here
            // rather than half-fixed.
            | Action::ApplyOperatorObject {
                object: escriba_core::TextObject::Line,
                ..
            }
    )
}

/// Where a put leaves the cursor.
///
/// Decided BEFORE the insert (from the register's kind) and consumed after,
/// because the two arms need different information and only one of them
/// survives the edit: the linewise arm needs the line it opened, which the
/// pre-edit position names, while the charwise arm needs the extent of the
/// text it wrote. Computing either from the post-edit buffer alone means
/// re-deriving which gesture happened, which is exactly what
/// [`escriba_core::RegisterKind`] exists to stop.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum PutRest {
    /// Linewise: the first non-blank of the first line put.
    LineStart(u32),
    /// Charwise: ON the last character put — vim's rule, and the one that
    /// makes a following `p` stack the copies rather than walk rightward.
    LastCharPut,
}

/// vim's three character classes — the whole of what "a word" means to `w`,
/// `b`, `e` and `iw`.
///
/// One classifier, not four. `object_word` grew its own copy while the word
/// MOTIONS were still splitting on whitespace alone, so `diw` on `foo.bar`
/// took `foo` and `dw` took `foo.bar` — two answers to "where does this word
/// end" from one editor, on the same keystroke's worth of text.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum WordClass {
    Word,
    Punct,
    Space,
}

fn word_class(c: char) -> WordClass {
    if c.is_alphanumeric() || c == '_' {
        WordClass::Word
    } else if c.is_whitespace() {
        WordClass::Space
    } else {
        WordClass::Punct
    }
}

/// vim's two word WIDTHS. `w` splits on the three [`WordClass`]es; `W` splits
/// on whitespace alone, so `foo.bar` is three words and one WORD.
///
/// A parameter on the scanners rather than a second family of them: `w` and
/// `W` differ in exactly one place — how a character is classified — and two
/// copies of the cross-line, empty-line and end-of-buffer rules is how they
/// would drift.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Width {
    /// `w` / `e` / `b` / `ge` — alphanumeric, punctuation and space.
    Small,
    /// `W` / `E` / `B` / `gE` — non-space and space, nothing else.
    Big,
}

fn class_at(c: char, width: Width) -> WordClass {
    match (width, word_class(c)) {
        (Width::Big, WordClass::Punct) => WordClass::Word,
        (_, k) => k,
    }
}

/// A line's characters WITHOUT its terminator.
///
/// The newline is not a character the cursor can sit on, and every word scan
/// wants the line's own text; `line_len_chars` already strips it for exactly
/// this reason, so the two agree on where a line ends by construction.
fn line_chars(buf: &escriba_buffer::Buffer, line: u32) -> Vec<char> {
    let Some(text) = buf.line(line) else {
        return Vec::new();
    };
    let len = buf.line_len_chars(line) as usize;
    text.chars().take(len).collect()
}

/// The last line that HOLDS text.
///
/// A file ending in `\n` is one line of text plus a terminator, but the rope
/// reports two lines, the second empty — so `line_count() - 1` names a line
/// that is not there. A forward word motion walking onto it moves the cursor
/// off the end of the file onto a row with nothing on it, which is what `w`
/// on the last word of an ordinary file did.
///
/// Scoped to the word motions on purpose. That phantom row is also DRAWN — it
/// gets a gutter number in every face — and hiding it is a buffer-model change
/// with a much wider blast radius than a motion fix; it is a separate defect,
/// named rather than half-fixed here. What is fixed here is the claim these
/// motions make: there is no next word after the last character of the text.
fn last_text_line(buf: &escriba_buffer::Buffer) -> u32 {
    let last = buf.line_count().saturating_sub(1);
    if last > 0 && buf.line_len_chars(last) == 0 {
        last - 1
    } else {
        last
    }
}

/// Where a forward word motion runs out of text — the EXCLUSIVE end, so an
/// operator reaches the final character. See [`word_next`].
fn buffer_end(buf: &escriba_buffer::Buffer) -> Position {
    let line = last_text_line(buf);
    Position::new(line, buf.line_len_chars(line))
}

/// `w` — to the start of the next word.
///
/// Three vim behaviours this had to grow, each of which was a visible wrong
/// answer before:
///
/// - **Punctuation starts a word.** `w` on `foo.bar` stops at `.` and again
///   at `b`; the whitespace-only scan sailed past both to the end.
/// - **It crosses lines onto the first non-blank**, not onto column 0. Landing
///   on the indent means the next `w` is spent walking out of it.
/// - **An empty line is a word.** vim stops on one, and that is what makes `w`
///   usable for walking paragraphs.
///
/// When there is no next word it returns the position PAST the last character
/// — not the last character itself. That looks like the bug it is next to and
/// is the opposite: an operator needs the exclusive end (`dw` on the final
/// word must delete the whole word), and it is the Normal-mode cursor that
/// must not sit there. So the clamp lives in [`EditorState::set_cursor`],
/// which knows the mode, and this stays a pure range endpoint.
fn word_next(buf: &escriba_buffer::Buffer, pos: Position, width: Width) -> Position {
    let mut line = pos.line;
    let mut chars = line_chars(buf, line);
    let mut col = (pos.column as usize).min(chars.len());

    // Leave the run the cursor is standing in. Starting on a blank skips this
    // — there is no run to leave, only blanks to cross.
    if col < chars.len() {
        let start = class_at(chars[col], width);
        if start != WordClass::Space {
            while col < chars.len() && class_at(chars[col], width) == start {
                col += 1;
            }
        }
    }

    loop {
        while col < chars.len() && class_at(chars[col], width) == WordClass::Space {
            col += 1;
        }
        if col < chars.len() {
            return Position::new(line, u32::try_from(col).unwrap_or(pos.column));
        }
        if line >= last_text_line(buf) {
            // Out of text: the exclusive end of the last word.
            return Position::new(line, u32::try_from(chars.len()).unwrap_or(pos.column));
        }
        line += 1;
        col = 0;
        chars = line_chars(buf, line);
        if chars.is_empty() {
            return Position::new(line, 0);
        }
    }
}

/// `b` — back to the start of the current or previous word.
///
/// Class-aware like [`word_next`], so `b` and `w` agree on where a word
/// begins; a disagreement between them is felt as `dw` and `db` deleting
/// different things from the same spot.
///
/// Single-line, and that is load-bearing: `<C-w>` reaches back over this
/// motion and relies on it returning the cursor UNCHANGED at column 0, which
/// is what makes the insert-mode erase fall through to `delete_before_cursor`
/// and join with the line above. Teaching this to cross lines would silently
/// change that key.
fn word_prev(buf: &escriba_buffer::Buffer, pos: Position, width: Width) -> Position {
    let chars = line_chars(buf, pos.line);
    let mut i = (pos.column as usize).min(chars.len());
    while i > 0 && class_at(chars[i - 1], width) == WordClass::Space {
        i -= 1;
    }
    if i > 0 {
        let run = class_at(chars[i - 1], width);
        while i > 0 && class_at(chars[i - 1], width) == run {
            i -= 1;
        }
    }
    Position::new(pos.line, u32::try_from(i).unwrap_or(0))
}

/// `ge` / `gE` — back to the LAST character of the previous word.
///
/// The mirror of [`word_end`], and INCLUSIVE like it: `dge` deletes through
/// the character it lands on. Single-line for the same reason [`word_prev`]
/// is — the backward scanners are what the insert-mode erases stand on, and
/// teaching them to cross lines changes those keys silently.
fn word_end_prev(buf: &escriba_buffer::Buffer, pos: Position, width: Width) -> Position {
    let chars = line_chars(buf, pos.line);
    let start = (pos.column as usize).min(chars.len());
    // `ge` always retreats at least one character before it starts looking,
    // so standing on the last character of a word does not stand still.
    let Some(mut i) = start.checked_sub(1) else {
        return pos;
    };
    // Leave the run the cursor is standing in FIRST. Without this, `ge` from
    // the middle (or the end) of a word lands one character to its left —
    // inside the same word, which is the one place `ge` must never stop.
    if let Some(&here) = chars.get(start) {
        let run = class_at(here, width);
        if run != WordClass::Space {
            while i > 0 && class_at(chars[i], width) == run {
                i -= 1;
            }
        }
    }
    while i > 0 && class_at(chars[i], width) == WordClass::Space {
        i -= 1;
    }
    Position::new(pos.line, u32::try_from(i).unwrap_or(0))
}

/// `e` — to the LAST character of the current or next word.
///
/// Always moves, which is what separates it from "the end of this word": on
/// the last character of a word, `e` goes to the last character of the NEXT
/// one rather than standing still.
///
/// This motion is INCLUSIVE — it names a character to act on, not a boundary
/// to stop before — see [`Motion::is_inclusive`]. `WordEndNext` used to
/// resolve through [`word_next`], so `e` and `w` were the same key with two
/// names.
fn word_end(buf: &escriba_buffer::Buffer, pos: Position, width: Width) -> Position {
    let mut line = pos.line;
    let mut chars = line_chars(buf, line);
    // `e` always advances at least one character before it starts looking.
    let mut col = (pos.column as usize).saturating_add(1);

    loop {
        while col < chars.len() && class_at(chars[col], width) == WordClass::Space {
            col += 1;
        }
        if col < chars.len() {
            break;
        }
        if line >= last_text_line(buf) {
            return buffer_end(buf);
        }
        line += 1;
        col = 0;
        chars = line_chars(buf, line);
    }

    let run = class_at(chars[col], width);
    while col + 1 < chars.len() && class_at(chars[col + 1], width) == run {
        col += 1;
    }
    Position::new(line, u32::try_from(col).unwrap_or(pos.column))
}

/// `f` / `F` / `t` / `T` — the character search, resolved on ONE line.
///
/// vim's character search never crosses a line, which is what makes it safe
/// to compose with an operator: `df;` can only ever delete within the line.
/// `None` when the character is not there — the motion fails and the operator
/// aborts with the buffer untouched, rather than deleting to the line edge.
fn find_char(
    buf: &escriba_buffer::Buffer,
    pos: Position,
    ch: char,
    backward: bool,
    till: bool,
) -> Option<Position> {
    let chars = line_chars(buf, pos.line);
    let cur = (pos.column as usize).min(chars.len());
    let hit = if backward {
        // `T` stops AFTER the character, so it has to start one further back
        // or a repeated `T` would never leave the spot it already reached.
        let from = if till { cur.checked_sub(1)? } else { cur };
        (0..from).rev().find(|&i| chars[i] == ch)?
    } else {
        let from = if till { cur.saturating_add(2) } else { cur + 1 };
        (from.min(chars.len())..chars.len()).find(|&i| chars[i] == ch)?
    };
    let col = match (backward, till) {
        (false, true) => hit - 1,
        (true, true) => hit + 1,
        _ => hit,
    };
    Some(Position::new(pos.line, u32::try_from(col).ok()?))
}

/// The four bracket pairs `%` knows.
const MATCH_PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

/// A language's WORD pairs for `%` — vim's `matchit`, typed.
///
/// `(open, middles, close)`. A middle (`else`, `elif`, `when`) is a word `%`
/// steps THROUGH on its way round the group; without them `%` on a shell `if`
/// jumps straight past `elif` to `fi`, which is right for a scanner and wrong
/// for a reader.
///
/// A TABLE keyed by filetype name, not a per-language scanner: every entry is
/// the same depth-counting walk over a different word list, so a new language
/// is a row. Deliberately small — these are the languages whose blocks are
/// words rather than braces, which is exactly the set where bracket-only `%`
/// is useless.
type WordPairs = &'static [(&'static str, &'static [&'static str], &'static str)];

const WORD_PAIRS: &[(&str, WordPairs)] = &[
    (
        "lua",
        &[
            ("if", &["elseif", "else"], "end"),
            ("for", &[], "end"),
            ("while", &[], "end"),
            ("function", &[], "end"),
            ("do", &[], "end"),
            ("repeat", &[], "until"),
        ],
    ),
    (
        "ruby",
        &[
            ("if", &["elsif", "else"], "end"),
            ("unless", &["else"], "end"),
            ("case", &["when", "else"], "end"),
            ("begin", &["rescue", "ensure", "else"], "end"),
            ("def", &[], "end"),
            ("class", &[], "end"),
            ("module", &[], "end"),
            ("do", &[], "end"),
            ("while", &[], "end"),
        ],
    ),
    (
        "sh",
        &[
            ("if", &["elif", "else"], "fi"),
            ("case", &[], "esac"),
            ("do", &[], "done"),
        ],
    ),
    (
        "bash",
        &[
            ("if", &["elif", "else"], "fi"),
            ("case", &[], "esac"),
            ("do", &[], "done"),
        ],
    ),
    (
        "elixir",
        &[
            ("do", &["else", "rescue", "after", "catch"], "end"),
            ("fn", &[], "end"),
        ],
    ),
    (
        "vim",
        &[
            ("if", &["elseif", "else"], "endif"),
            ("function", &[], "endfunction"),
            ("while", &[], "endwhile"),
            ("for", &[], "endfor"),
            ("try", &["catch", "finally"], "endtry"),
        ],
    ),
];

/// A word occurrence: its position, and which group + role it plays.
#[derive(Clone, Copy)]
struct WordHit {
    line: u32,
    col: u32,
    end: u32,
    group: usize,
    /// `0` = opener, `1` = middle, `2` = closer.
    role: u8,
}

/// `%` — to the match of the bracket under the cursor, or of the first
/// bracket to its right on the same line (vim scans forward to find one).
///
/// Depth-counting and buffer-wide, because a brace pair that fits on one line
/// is the case `%` is least needed for.
fn match_pair(buf: &escriba_buffer::Buffer, pos: Position) -> Option<Position> {
    let chars = line_chars(buf, pos.line);
    let start = (pos.column as usize).min(chars.len());
    let (col, open, close, forward) = (start..chars.len()).find_map(|i| {
        MATCH_PAIRS.iter().find_map(|&(o, c)| {
            if chars[i] == o {
                Some((i, o, c, true))
            } else if chars[i] == c {
                Some((i, o, c, false))
            } else {
                None
            }
        })
    })?;

    let last = buf.line_count().saturating_sub(1);
    let mut depth = 0i32;
    let (mut line, mut i) = (pos.line, col);
    let mut text = chars;
    loop {
        let c = text[i];
        if c == open {
            depth += if forward { 1 } else { -1 };
        } else if c == close {
            depth += if forward { -1 } else { 1 };
        }
        if depth == 0 {
            return Some(Position::new(line, u32::try_from(i).ok()?));
        }
        if forward {
            i += 1;
            while i >= text.len() {
                if line >= last {
                    return None;
                }
                line += 1;
                text = line_chars(buf, line);
                i = 0;
            }
        } else {
            while i == 0 {
                if line == 0 {
                    return None;
                }
                line -= 1;
                text = line_chars(buf, line);
                i = text.len();
            }
            i -= 1;
        }
    }
}

/// Every word-pair keyword on `line`, in column order.
///
/// Word-bounded on both sides, so `endif` is not read as `end`, `define` is
/// not read as `def`, and a `do` inside `window` is not a block opener. That
/// boundary check is the whole difference between matchit and a substring
/// search, and skipping it is worse than having no word pairs at all — a `%`
/// that jumps to the middle of an identifier is a silent wrong answer.
fn word_hits(buf: &escriba_buffer::Buffer, line: u32, pairs: WordPairs) -> Vec<WordHit> {
    let chars = line_chars(buf, line);
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if word_class(chars[i]) != WordClass::Word {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && word_class(chars[i]) == WordClass::Word {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        for (group, (open, middles, close)) in pairs.iter().enumerate() {
            let role = if word == *open {
                0
            } else if word == *close {
                2
            } else if middles.contains(&word.as_str()) {
                1
            } else {
                continue;
            };
            out.push(WordHit {
                line,
                col: u32::try_from(start).unwrap_or(0),
                end: u32::try_from(i).unwrap_or(0),
                group,
                role,
            });
            break;
        }
    }
    out
}

/// `%` over WORD pairs — matchit's half of the motion.
///
/// Finds the keyword at (or right of) the cursor and walks to the next member
/// of its group at the same depth: opener → first middle → … → closer →
/// opener. Cycling rather than jumping straight to the closer is what makes
/// `%` usable for reading an `if`/`elif`/`else`/`fi` chain.
fn match_word_pair(
    buf: &escriba_buffer::Buffer,
    pos: Position,
    pairs: WordPairs,
) -> Option<Position> {
    let here = word_hits(buf, pos.line, pairs)
        .into_iter()
        .find(|h| h.end > pos.column)?;
    let last = last_text_line(buf);
    let forward = here.role != 2;
    let mut depth = 0i32;
    let mut line = here.line;
    loop {
        let hits = word_hits(buf, line, pairs);
        // Only the hits strictly beyond the starting keyword on its own line.
        let scan: Vec<WordHit> = if line == here.line {
            let mut v: Vec<WordHit> = hits
                .into_iter()
                .filter(|h| {
                    if forward {
                        h.col > here.col
                    } else {
                        h.col < here.col
                    }
                })
                .collect();
            if !forward {
                v.reverse();
            }
            v
        } else {
            let mut v = hits;
            if !forward {
                v.reverse();
            }
            v
        };
        for h in scan {
            if h.group != here.group {
                continue;
            }
            match (h.role, forward) {
                (0, true) | (2, false) => depth += 1,
                (2, true) | (0, false) => {
                    if depth == 0 {
                        return Some(Position::new(h.line, h.col));
                    }
                    depth -= 1;
                }
                // A middle at the SAME depth is the next stop; nested ones are
                // somebody else's `else`.
                (1, _) if depth == 0 => return Some(Position::new(h.line, h.col)),
                _ => {}
            }
        }
        if forward {
            if line >= last {
                return None;
            }
            line += 1;
        } else {
            if line == 0 {
                return None;
            }
            line -= 1;
        }
    }
}

/// `{` / `}` — to the nearest blank line in `dir`, or the buffer edge.
///
/// vim's paragraph boundary is an EMPTY line, not an indentation change; a
/// line of spaces is not one. `line_len_chars` already excludes the
/// terminator, so "empty" is exactly `len == 0`.
fn paragraph(buf: &escriba_buffer::Buffer, pos: Position, forward: bool) -> Position {
    let last = last_text_line(buf);
    let mut line = pos.line;
    loop {
        if forward {
            if line >= last {
                return buffer_end(buf);
            }
            line += 1;
        } else {
            if line == 0 {
                return Position::ZERO;
            }
            line -= 1;
        }
        if buf.line_len_chars(line) == 0 {
            return Position::new(line, 0);
        }
    }
}

/// `(` / `)` — to the start of the adjacent sentence.
///
/// A sentence ends at `.`/`!`/`?` followed by whitespace or end-of-line; the
/// next one starts at the following non-blank. A paragraph boundary is also a
/// sentence boundary, which is what stops `)` at the end of a block of prose
/// instead of sailing into the next one.
fn sentence(buf: &escriba_buffer::Buffer, pos: Position, forward: bool) -> Position {
    let starts = sentence_starts(buf);
    let here = (pos.line, pos.column);
    if forward {
        starts
            .iter()
            .find(|&&(l, c)| (l, c) > here)
            .map_or_else(|| buffer_end(buf), |&(l, c)| Position::new(l, c))
    } else {
        starts
            .iter()
            .rev()
            .find(|&&(l, c)| (l, c) < here)
            .map_or(Position::ZERO, |&(l, c)| Position::new(l, c))
    }
}

/// Every sentence start in the buffer, in order.
///
/// Computed wholesale rather than scanned directionally: the backward and
/// forward cases are then the same list read two ways, so `(` and `)` cannot
/// disagree about where a sentence begins.
fn sentence_starts(buf: &escriba_buffer::Buffer) -> Vec<(u32, u32)> {
    let mut out = vec![(0u32, 0u32)];
    let mut ended = false;
    for line in 0..=last_text_line(buf) {
        let chars = line_chars(buf, line);
        if chars.is_empty() {
            // A blank line is a paragraph break, and so a sentence break.
            out.push((line, 0));
            ended = false;
            continue;
        }
        for (i, &c) in chars.iter().enumerate() {
            if ended && !c.is_whitespace() {
                out.push((line, u32::try_from(i).unwrap_or(0)));
                ended = false;
            }
            if matches!(c, '.' | '!' | '?') {
                ended = true;
            } else if !matches!(c, ')' | ']' | '"' | '\'') && !c.is_whitespace() {
                ended = false;
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use madori::event::{KeyCode, KeyEvent, Modifiers};

    // ── search wiring (escriba-search integration) ────────────────────
    //
    // The engine is proven in escriba-search's own 61 tests. These prove the
    // WIRING: that keys reach it, that the cursor lands where it says, and
    // that a search prompt and an ex-command can share Command mode without
    // being confused for one another.

    fn type_search(st: &mut EditorState, dir: SearchDirection, pat: &str) {
        st.apply(&Action::SearchOpen(dir));
        for c in pat.chars() {
            st.apply(&Action::InsertChar(c));
        }
        st.apply(&Action::SubmitCommand);
    }

    #[test]
    fn slash_search_moves_the_cursor_to_the_match() {
        let mut st = new_state_with("alpha\nbravo\ncharlie\n");
        type_search(&mut st, SearchDirection::Forward, "charlie");
        assert_eq!(st.cursor().line, 2, "cursor lands on the matching line");
        assert_eq!(st.modal.mode(), Mode::Normal, "prompt closes on submit");
        assert_eq!(st.search.matches().len(), 1);
    }

    #[test]
    // `N` is a DIFFERENT vim key from `n` — see escriba-search.
    #[allow(non_snake_case)]
    fn n_and_N_walk_matches_in_both_directions() {
        let mut st = new_state_with("foo\nbar\nfoo\nbaz\nfoo\n");
        type_search(&mut st, SearchDirection::Forward, "foo");
        let first = st.cursor().line;
        st.apply(&Action::SearchRepeat { reverse: false });
        let second = st.cursor().line;
        assert!(second > first, "n advances ({first} -> {second})");
        st.apply(&Action::SearchRepeat { reverse: true });
        assert_eq!(st.cursor().line, first, "N comes back");
    }

    #[test]
    fn star_searches_the_word_under_the_cursor() {
        let mut st = new_state_with("needle\nhaystack\nneedle\n");
        st.apply(&Action::SearchWord { reverse: false });
        assert_eq!(st.search.pattern().unwrap().raw(), r"\bneedle\b");
        assert_eq!(st.cursor().line, 2, "jumps to the other occurrence");
    }

    #[test]
    fn escape_abandons_the_prompt_and_keeps_the_previous_search() {
        let mut st = new_state_with("foo\nbar\nfoo\n");
        type_search(&mut st, SearchDirection::Forward, "foo");
        let matches_before = st.search.matches().len();

        st.apply(&Action::SearchOpen(SearchDirection::Forward));
        st.apply(&Action::InsertChar('z'));
        st.apply(&Action::ChangeMode(Mode::Normal));

        assert!(!st.search.is_prompting(), "prompt gone");
        assert_eq!(
            st.search.pattern().unwrap().raw(),
            "foo",
            "old pattern survives"
        );
        assert_eq!(
            st.search.matches().len(),
            matches_before,
            "old highlights survive"
        );
    }

    #[test]
    fn a_search_prompt_and_an_ex_command_are_not_confused() {
        let mut st = new_state_with("foo\n");
        // No `/` pressed: Command mode belongs to the ex-command line.
        st.apply(&Action::ChangeMode(Mode::Command));
        assert!(!st.search.is_prompting(), "`:` must not open a search");
        st.apply(&Action::InsertChar('w'));
        assert!(
            st.search.prompt().is_none(),
            "typed char went to the ex line"
        );
    }

    #[test]
    fn a_missing_pattern_reports_instead_of_failing_silently() {
        let mut st = new_state_with("alpha\nbravo\n");
        type_search(&mut st, SearchDirection::Forward, "zzz");
        assert!(
            st.messages.iter().any(|m| m.contains("E486")),
            "must report not-found, got {:?}",
            st.messages
        );
    }

    #[test]
    fn n_without_any_search_reports_rather_than_moving() {
        let mut st = new_state_with("alpha\nbravo\n");
        let before = st.cursor();
        st.apply(&Action::SearchRepeat { reverse: false });
        assert_eq!(st.cursor(), before, "cursor must not move");
        assert!(
            st.messages.iter().any(|m| m.contains("E35")),
            "got {:?}",
            st.messages
        );
    }

    #[test]
    fn search_as_a_motion_composes_with_an_operator() {
        // The point of Motion::SearchNext: `d` + search deletes to the match.
        let mut st = new_state_with("alpha bravo charlie\n");
        type_search(&mut st, SearchDirection::Forward, "charlie");
        st.set_cursor(Position::new(0, 0));
        let target = st.resolve_motion(Position::new(0, 0), Motion::SearchNext);
        assert!(target.is_some(), "search must resolve as a motion");
        assert_eq!(target.unwrap().column, 12, "at `charlie`");
    }

    #[test]
    fn search_motion_without_a_pattern_fails_the_motion_instead_of_moving_to_zero() {
        // A silent fallback to offset 0 would make `d` + search delete to the
        // start of the file — the worst possible failure for an operator.
        let st = new_state_with("alpha bravo\n");
        assert!(
            st.resolve_motion(Position::new(0, 5), Motion::SearchNext)
                .is_none()
        );
    }

    #[test]
    fn clear_highlight_keeps_the_pattern_usable() {
        let mut st = new_state_with("foo\nbar\nfoo\n");
        type_search(&mut st, SearchDirection::Forward, "foo");
        st.apply(&Action::ClearSearchHighlight);
        assert!(st.search.highlights().is_empty(), "nothing lit");
        st.apply(&Action::SearchRepeat { reverse: false });
        assert!(st.search.pattern().is_some(), "but n still works");
    }

    #[test]
    fn typing_previews_incrementally_before_commit() {
        let mut st = new_state_with("alpha\nbravo\ncharlie\n");
        st.apply(&Action::SearchOpen(SearchDirection::Forward));
        for c in "charlie".chars() {
            st.apply(&Action::InsertChar(c));
        }
        // incsearch: the cursor has already moved, with nothing committed.
        assert_eq!(st.cursor().line, 2, "preview moved the cursor");
        assert!(st.search.pattern().is_none(), "but nothing is committed");
    }

    #[test]
    fn backspace_corrects_the_prompt_and_reruns_the_preview() {
        let mut st = new_state_with("alpha\nbravo\n");
        st.apply(&Action::SearchOpen(SearchDirection::Forward));
        for c in "bravox".chars() {
            st.apply(&Action::InsertChar(c));
        }
        assert_eq!(st.search.prompt().unwrap().text(), "bravox");
        st.apply(&Action::Backspace);
        assert_eq!(
            st.search.prompt().unwrap().text(),
            "bravo",
            "typo corrected"
        );
        assert_eq!(
            st.status_model().prompt_text,
            "bravo",
            "the model reads the PROMPT — the minibuffer is the ex-line's store",
        );
        assert_eq!(st.cursor().line, 1, "preview re-ran and found it");
    }

    #[test]
    fn backspacing_past_the_slash_closes_the_prompt() {
        let mut st = new_state_with("alpha\n");
        st.apply(&Action::SearchOpen(SearchDirection::Forward));
        st.apply(&Action::InsertChar('a'));
        st.apply(&Action::Backspace);
        st.apply(&Action::Backspace);
        assert!(!st.search.is_prompting(), "prompt closed");
        assert_eq!(st.modal.mode(), Mode::Normal);
    }

    #[test]
    fn noh_clears_highlights_and_keeps_the_pattern() {
        let mut st = new_state_with("foo\nbar\nfoo\n");
        type_search(&mut st, SearchDirection::Forward, "foo");
        assert!(!st.search.highlights().is_empty());
        st.run_command("noh", &[]);
        assert!(st.search.highlights().is_empty(), ":noh turns them off");
        assert!(st.search.pattern().is_some(), "but n still works");
    }

    #[test]
    fn noh_accepts_the_vim_aliases() {
        for name in ["noh", "nohl", "nohlsearch"] {
            let mut st = new_state_with("foo\nfoo\n");
            type_search(&mut st, SearchDirection::Forward, "foo");
            st.run_command(name, &[]);
            assert!(st.search.highlights().is_empty(), "{name} must clear");
        }
    }

    #[test]
    fn backspace_on_the_ex_line_does_not_touch_search_state() {
        let mut st = new_state_with("foo\n");
        st.apply(&Action::ChangeMode(Mode::Command));
        st.apply(&Action::InsertChar('w'));
        st.apply(&Action::InsertChar('q'));
        st.apply(&Action::Backspace);
        assert_eq!(st.status_model().prompt_text, "w");
        assert!(st.search.prompt().is_none(), "no search was involved");
    }

    #[test]
    fn up_arrow_recalls_the_previous_search() {
        let mut st = new_state_with("alpha\nbravo\n");
        type_search(&mut st, SearchDirection::Forward, "bravo");
        st.apply(&Action::SearchOpen(SearchDirection::Forward));
        st.apply(&Action::PromptHistory { back: true });
        assert_eq!(st.search.prompt().unwrap().text(), "bravo");
        assert_eq!(
            st.status_model().prompt_text,
            "bravo",
            "display follows the prompt"
        );
    }

    #[test]
    fn arrowing_back_down_restores_the_half_typed_pattern() {
        let mut st = new_state_with("alpha\nbravo\n");
        type_search(&mut st, SearchDirection::Forward, "bravo");
        st.apply(&Action::SearchOpen(SearchDirection::Forward));
        st.apply(&Action::InsertChar('a'));
        st.apply(&Action::PromptHistory { back: true });
        assert_eq!(st.search.prompt().unwrap().text(), "bravo");
        st.apply(&Action::PromptHistory { back: false });
        assert_eq!(
            st.search.prompt().unwrap().text(),
            "a",
            "the draft comes back"
        );
        assert_eq!(st.status_model().prompt_text, "a");
    }

    #[test]
    fn history_arrows_do_nothing_on_the_ex_line() {
        let mut st = new_state_with("alpha\n");
        st.apply(&Action::ChangeMode(Mode::Command));
        st.apply(&Action::InsertChar('w'));
        st.apply(&Action::PromptHistory { back: true });
        assert_eq!(st.status_model().prompt_text, "w", "ex line untouched");
    }

    // ── trouble.* — the findings view ────────────────────────────────
    //
    // These assert on the ROWS the picker would be built from, not on the
    // registry: the registry is already tested, and what could be wrong
    // here is the projection — scoping, freshness, and whether a row goes
    // anywhere when pressed.

    fn finding_at(buffer: BufferId, line: u32, msg: &str) -> escriba_shirube::Finding {
        use escriba_core::{Position, Range};
        escriba_shirube::Finding::new(
            escriba_shirube::Site::in_buffer(
                buffer,
                Range::new(Position::new(line, 0), Position::new(line, 1)),
            ),
            escriba_shirube::Severity::Error,
            msg.to_string(),
            escriba_shirube::Origin::Text("test"),
        )
    }

    #[test]
    fn published_findings_become_picker_rows() {
        let mut st = new_state_with("a\nb\nc\n");
        let world = st.world();
        st.results.publish(
            "test",
            escriba_shirube::ResultList::new(vec![finding_at(st.active, 1, "boom")], world),
        );
        let rows = st.finding_items(true, None);
        assert_eq!(rows.len(), 1, "the published finding produces a row");
        // The row must SAY something an operator can act on: severity,
        // 1-based line, and the message.
        let label = &rows[0].label;
        assert!(label.contains("ERROR"), "{label}");
        assert!(label.contains(":2"), "lines are 1-based on screen: {label}");
        assert!(label.contains("boom"), "{label}");
    }

    #[test]
    fn a_stale_list_contributes_no_rows() {
        // THE load-bearing one. A list anchored to a revision the buffer has
        // moved past must vanish from the view rather than offer a line that
        // has since shifted — which is the whole reason findings carry an
        // anchor instead of just a position.
        let mut st = new_state_with("a\nb\nc\n");
        let world = st.world();
        st.results.publish(
            "test",
            escriba_shirube::ResultList::new(vec![finding_at(st.active, 1, "boom")], world),
        );
        assert_eq!(st.finding_items(true, None).len(), 1, "fresh to begin with");

        st.apply(&Action::InsertChar('x'));
        assert!(
            st.finding_items(true, None).is_empty(),
            "an edit moved the text on; the list is stale and must not be shown"
        );
    }

    #[test]
    fn document_scope_excludes_another_buffer() {
        // `trouble.document` vs `trouble.workspace` is one bool, so this is
        // the only thing that can distinguish them.
        let mut st = new_state_with("a\nb\n");
        let other = st.buffers.scratch("z\n");
        let world = st.world();
        st.results.publish(
            "test",
            escriba_shirube::ResultList::new(
                vec![
                    finding_at(st.active, 0, "mine"),
                    finding_at(other, 0, "theirs"),
                ],
                world,
            ),
        );
        let ws = st.finding_items(true, None);
        assert_eq!(ws.len(), 2, "workspace scope shows both");
        let doc = st.finding_items(false, None);
        assert_eq!(doc.len(), 1, "document scope shows only the active buffer");
        assert!(doc[0].label.contains("mine"), "{}", doc[0].label);
    }

    #[test]
    fn files_under_a_root_produces_rows() {
        // `files.open-parent` differs from `files.open` only in the root, so
        // what must hold is that a root is actually honoured.
        let mut st = new_state_with("");
        let rows = st.file_items(std::path::Path::new("."));
        assert!(!rows.is_empty(), "the working directory has files");
    }

    // ── vim text objects ─────────────────────────────────────────────
    //
    // Asserted through `apply` on real buffer text, so a wrong RANGE shows
    // up as wrong text rather than as a range that merely looks plausible.

    fn after(text: &str, line: u32, col: u32, act: Action) -> String {
        let mut st = new_state_with(text);
        st.set_cursor(Position::new(line, col));
        st.apply(&act);
        st.buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default()
    }

    fn del_obj(o: escriba_core::TextObject) -> Action {
        Action::ApplyOperatorObject {
            op: escriba_core::Operator::Delete,
            object: o,
        }
    }

    #[test]
    fn dd_removes_the_line_not_just_its_contents() {
        // The distinction the newline makes: without it, `dd` blanks a line
        // and leaves it behind.
        let got = after("a\nb\nc\n", 1, 0, del_obj(escriba_core::TextObject::Line));
        assert_eq!(got, "a\nc\n");
    }

    #[test]
    fn dd_on_the_last_line_leaves_no_blank_behind() {
        // The case a naive start-of-line..start-of-next range gets wrong:
        // there is no following newline to take, so it must take the
        // preceding one.
        let got = after("a\nb\nc\n", 2, 0, del_obj(escriba_core::TextObject::Line));
        assert_eq!(got, "a\nb\n", "no trailing empty line: {got:?}");
    }

    #[test]
    fn dd_on_the_only_line_clears_it_but_keeps_the_line() {
        let got = after("solo\n", 0, 2, del_obj(escriba_core::TextObject::Line));
        assert!(got.starts_with('\n') || got.is_empty(), "{got:?}");
    }

    #[test]
    fn diw_takes_the_word_and_daw_takes_its_trailing_space() {
        let inner = after(
            "one two three\n",
            0,
            5,
            del_obj(escriba_core::TextObject::Word { around: false }),
        );
        assert_eq!(inner, "one  three\n", "iw leaves both spaces");
        let around = after(
            "one two three\n",
            0,
            5,
            del_obj(escriba_core::TextObject::Word { around: true }),
        );
        assert_eq!(around, "one three\n", "aw takes the trailing space");
    }

    #[test]
    fn iw_from_any_column_inside_the_word_takes_the_whole_word() {
        for col in 4..=6 {
            let got = after(
                "one two three\n",
                0,
                col,
                del_obj(escriba_core::TextObject::Word { around: false }),
            );
            assert_eq!(got, "one  three\n", "from column {col}");
        }
    }

    #[test]
    fn iw_on_punctuation_takes_the_punctuation_run() {
        // vim's three classes: word / punctuation / whitespace. A `::` is a
        // run of punctuation, not part of either identifier.
        let got = after(
            "foo::bar\n",
            0,
            3,
            del_obj(escriba_core::TextObject::Word { around: false }),
        );
        assert_eq!(got, "foobar\n");
    }

    #[test]
    fn i_paren_takes_the_inside_and_a_paren_takes_the_brackets_too() {
        let inner = after(
            "f(a, b)\n",
            0,
            3,
            del_obj(escriba_core::TextObject::Delimited {
                open: '(',
                close: ')',
                around: false,
            }),
        );
        assert_eq!(inner, "f()\n");
        let around = after(
            "f(a, b)\n",
            0,
            3,
            del_obj(escriba_core::TextObject::Delimited {
                open: '(',
                close: ')',
                around: true,
            }),
        );
        assert_eq!(around, "f\n");
    }

    #[test]
    fn nested_brackets_resolve_to_the_enclosing_pair() {
        // THE reason the bracket scan counts depth: an inner pair must not
        // terminate the search for the one the cursor is actually inside.
        let got = after(
            "f(g(x), y)\n",
            0,
            8,
            del_obj(escriba_core::TextObject::Delimited {
                open: '(',
                close: ')',
                around: false,
            }),
        );
        assert_eq!(got, "f()\n", "took the outer pair");
    }

    #[test]
    fn quotes_do_not_nest_so_the_nearest_pair_wins() {
        let got = after(
            r#"say "hi there" ok"#,
            0,
            7,
            del_obj(escriba_core::TextObject::Delimited {
                open: '"',
                close: '"',
                around: false,
            }),
        );
        assert_eq!(got, "say \"\" ok");
    }

    #[test]
    fn an_unmatched_delimiter_resolves_to_nothing_rather_than_guessing() {
        let mut st = new_state_with("f(a, b\n");
        st.set_cursor(Position::new(0, 3));
        let before = st
            .buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default();
        st.apply(&del_obj(escriba_core::TextObject::Delimited {
            open: '(',
            close: ')',
            around: false,
        }));
        let got_after = st
            .buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default();
        assert_eq!(got_after, before, "no closing bracket: change nothing");
    }

    fn new_state_with(text: &str) -> EditorState {
        let mut bufs = BufferSet::new();
        let id = bufs.scratch(text);
        EditorState::new_with_buffer(bufs, id)
    }

    /// A breakpoint toggle reaches the GPU face's rebuild gate.
    ///
    /// ## What this does and does not claim
    ///
    /// The GPU face is the only one that CACHES its gutter: it shapes a
    /// glyphon buffer and rebuilds it only when `s.edit_gen() != self.last_gen`
    /// (`escriba-render/src/gpu.rs:260`). Both testable faces repaint from
    /// scratch every draw, so no rendered-cells test can see this — measured
    /// 2026-08-12 by removing the bump and watching all eight breakpoint
    /// render tests stay green.
    ///
    /// The guarantee is STRUCTURAL, not local: [`EditorState::honour`] widens
    /// the damage and bumps the generation after every slip, so the property
    /// holds for `ToggleBreakpoint` the way it holds for the other thirty.
    /// This pins the INSTANCE, and says so rather than pretending to gate a
    /// line inside `toggle_breakpoint` — there is no such line, deliberately.
    ///
    /// RED RUN (2026-08-12): deleting `self.bump_gen()` from `honour` fails
    /// this (and much else, which is the honest shape of a structural
    /// guarantee). The evidence is the generation counter itself — the exact
    /// value the GPU gate compares — not a restatement of "was a method
    /// called".
    #[test]
    fn setting_a_breakpoint_repaints() {
        let mut s = new_state_with("alpha\nbravo\ncharlie\n");
        let before = s.edit_gen();
        s.run_command("dap.toggle-breakpoint", &[]);
        assert!(
            s.breakpoints().is_set(s.active, 0),
            "precondition: the toggle ran",
        );
        assert_ne!(
            s.edit_gen(),
            before,
            "the GPU face rebuilds its cached gutter ONLY on a generation \
             change — without this the mark never reaches that screen",
        );
        assert!(
            !s.damage().is_none(),
            "and a scoped-repaint face has to be told the viewport moved",
        );
    }

    #[test]
    fn a_breakpoint_toggle_with_no_open_buffer_marks_nothing() {
        // A mark on a buffer that does not exist is one no future DAP client
        // could ever name, and the honest report is silence rather than a
        // confirmation of something that did not happen. `active` names no
        // open buffer here, which is the state a `--no-defaults` boot and a
        // just-closed buffer both pass through.
        let mut s = new_state_with("alpha\n");
        s.active = BufferId(9_999);
        s.run_command("dap.toggle-breakpoint", &[]);
        assert!(!s.breakpoints().is_set(s.active, 0), "nothing was marked");
        assert!(
            !s.messages.iter().any(|m| m.contains("breakpoint")),
            "and nothing was claimed: {:?}",
            s.messages,
        );
    }

    /// The refresh-seal driver (theory/ESCRIBA.md §Refresh-Seal): an applied
    /// action advances `edit_gen` (so the renderer repaints), and merely
    /// reading the generation does not. This is what lets `gpu.rs` gate the
    /// re-highlight/re-shape on a generation change — an idle frame observes an
    /// unchanged generation and reuses its cached buffer.
    #[test]
    fn edit_gen_advances_on_applied_action_not_on_read() {
        let mut s = new_state_with("hello\nworld\n");
        let g0 = s.edit_gen();
        s.apply(&Action::InsertChar('X'));
        assert_ne!(
            s.edit_gen(),
            g0,
            "an applied action must advance the refresh generation",
        );
        // Reading the generation is not a mutation — idle frames stay put.
        let g1 = s.edit_gen();
        assert_eq!(s.edit_gen(), g1, "reading edit_gen must not advance it");
    }

    /// The M1 refresh node (theory/ESCRIBA.md §X): a mutation widens the typed
    /// `Damage` to cover exactly what changed — local for an in-place edit,
    /// to-end-of-document when the line count shifts — and the renderer drains
    /// it per frame. `Damage ⊇ changed` by construction; it never narrows.
    #[test]
    fn damage_tracks_edit_scope_and_drains() {
        let mut s = new_state_with("hello\nworld\n");
        assert!(s.damage().is_none(), "a fresh state has no damage");

        s.apply(&Action::InsertChar('X')); // in-place edit on line 0
        assert_eq!(
            s.damage(),
            Damage::Lines { from: 0, to: 0 },
            "a local edit damages just its line",
        );

        let drained = s.take_damage();
        assert_eq!(drained, Damage::Lines { from: 0, to: 0 });
        assert!(s.damage().is_none(), "take_damage drains to None");

        s.apply(&Action::InsertChar('\n')); // splits line 0 → line count grows
        assert_eq!(
            s.damage(),
            Damage::Lines {
                from: 0,
                to: u32::MAX,
            },
            "a line-count change damages to end-of-document",
        );
    }

    /// A state whose active window is a deliberately tiny viewport
    /// (`visible_lines` × `visible_columns`) so the scroll-to-contain
    /// invariant is exercised on small inputs.
    fn new_state_small_viewport(text: &str, vis_lines: u32, vis_cols: u32) -> EditorState {
        let mut s = new_state_with(text);
        for w in s.layout.windows_mut() {
            w.viewport.visible_lines = vis_lines;
            w.viewport.visible_columns = vis_cols;
        }
        s
    }

    /// The core regression invariant: the active window's viewport CONTAINS
    /// the cursor on BOTH axes. This is the operator's exact complaint —
    /// "typing past the bottom (or right) leaves the cursor off-screen" —
    /// made into a checkable property.
    fn assert_cursor_in_viewport(s: &EditorState, ctx: &str) {
        let w = s.layout.active_window().expect("active window");
        let v = w.viewport;
        let c = s.cursor();
        assert!(
            v.top_line <= c.line && c.line < v.top_line + v.visible_lines,
            "[{ctx}] cursor line {} not in vertical window [{}, {}); viewport={v:?}",
            c.line,
            v.top_line,
            v.top_line + v.visible_lines,
        );
        assert!(
            v.left_column <= c.column && c.column < v.left_column + v.visible_columns,
            "[{ctx}] cursor column {} not in horizontal window [{}, {}); viewport={v:?}",
            c.column,
            v.left_column,
            v.left_column + v.visible_columns,
        );
    }

    /// `dd` from the KEYBOARD, not from a synthesized action.
    ///
    /// The FSM composition is unit-tested, but what an operator actually
    /// does is press `d` twice — and that path goes through the keymap and
    /// the sequence stepper, either of which could swallow the second `d`.
    #[test]
    fn pressing_d_twice_deletes_the_line() {
        let mut st = new_state_with("alpha\nbeta\ngamma\n");
        st.set_cursor(Position::new(1, 0));
        st.tick(&press(KeyCode::Char('d')));
        st.tick(&press(KeyCode::Char('d')));
        let got = st
            .buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default();
        assert_eq!(got, "alpha\ngamma\n", "dd from the keyboard");
    }

    #[test]
    fn pressing_2_d_d_deletes_two_lines() {
        let mut st = new_state_with("a\nb\nc\nd\n");
        st.set_cursor(Position::new(0, 0));
        for k in ['2', 'd', 'd'] {
            st.tick(&press(KeyCode::Char(k)));
        }
        let got = st
            .buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default();
        assert_eq!(got, "c\nd\n", "count applies to the doubled operator");
    }

    // ── The insert-entry family: `i` `I` `a` `A` `o` `O` ─────────────────
    //
    // Measured before the family existed (escriba 0.1.71, live 80×24 TUI):
    // pressing `A` moved nothing, changed no mode and printed nothing, because
    // an unbound Normal key resolves to `Action::Pending`. Only `i` was bound.

    /// Press one key on `text` from `at`, and report (mode, cursor, buffer).
    ///
    /// The buffer is part of the tuple on purpose: four of the six entries must
    /// leave it byte-identical, and a caret-only assertion cannot see a stray
    /// edit — which is the exact shape of the phantom-space report that started
    /// this work.
    fn entry(text: &str, at: Position, key: char) -> (Mode, Position, String) {
        let mut st = new_state_with(text);
        st.set_cursor(at);
        st.tick(&press(KeyCode::Char(key)));
        (
            st.modal.mode(),
            st.cursor(),
            st.buffers
                .get(st.active)
                .map(|b| b.to_string())
                .unwrap_or_default(),
        )
    }

    /// The whole family, one row per entry — vim's caret placement exactly.
    ///
    /// A matrix rather than six tests so the ★★ CLOSED-LOOP MASS-SYNTHESIS
    /// rule has something to bite on: `every_insert_entry_has_a_key` below
    /// fails the build when a seventh `InsertAt` variant lands without a row.
    #[test]
    fn the_insert_entry_family_places_the_caret_like_vim() {
        // "  hello" — two leading blanks, so `I` and `0` differ, and 7 chars,
        // so "one past the end" is column 7.
        const TEXT: &str = "  hello\nworld\n";
        let from = Position::new(0, 4); // on the first `l`
        for (key, want_cursor, want_text, why) in [
            (
                'i',
                Position::new(0, 4),
                TEXT,
                "`i` inserts before the caret",
            ),
            (
                'I',
                Position::new(0, 2),
                TEXT,
                "`I` goes to the first NON-BLANK, not to column 0",
            ),
            (
                'a',
                Position::new(0, 5),
                TEXT,
                "`a` appends after the caret",
            ),
            (
                'A',
                Position::new(0, 7),
                TEXT,
                "`A` parks one PAST the last char — the whole point of the key",
            ),
            (
                'o',
                Position::new(1, 0),
                "  hello\n\nworld\n",
                "`o` opens below and lands on the new line",
            ),
            (
                'O',
                Position::new(0, 0),
                "\n  hello\nworld\n",
                "`O` opens above; the fresh line takes the caret's line number",
            ),
        ] {
            let (mode, cursor, text) = entry(TEXT, from, key);
            assert_eq!(mode, Mode::Insert, "`{key}` must enter Insert");
            assert_eq!(cursor, want_cursor, "{why}");
            assert_eq!(text, want_text, "`{key}`: {why}");
        }
    }

    /// Every `InsertAt` has a Normal-mode key, and every one of those keys
    /// actually resolves to it.
    ///
    /// The forcing function. Adding a variant to `InsertAt` without binding it
    /// fails here rather than shipping a key that silently does nothing — which
    /// is precisely how `a`, `A`, `I`, `o` and `O` were missing for so long
    /// without any test noticing.
    #[test]
    fn every_insert_entry_has_a_key() {
        let km = escriba_keymap::Keymap::default_vim();
        let bound: Vec<InsertAt> = km
            .entries_sorted()
            .into_iter()
            .filter_map(|(mode, _, b)| match (mode, &b.action) {
                (Mode::Normal, Action::EnterInsert(at)) => Some(*at),
                _ => None,
            })
            .collect();
        for at in InsertAt::ALL {
            assert!(
                bound.contains(&at),
                "InsertAt::{at:?} ({}) has no Normal-mode key",
                at.as_str()
            );
        }
        assert_eq!(
            bound.len(),
            InsertAt::ALL.len(),
            "one key per entry, no duplicates: {bound:?}"
        );
    }

    /// `A` then typing appends at the end — the end-to-end gesture, not just
    /// the caret placement.
    ///
    /// The caret assertion above would still pass if Insert mode refused to
    /// write at a column past the last character; this is what proves the
    /// `CursorRest::AtInsertPoint` rest actually holds through a keystroke.
    #[test]
    fn shift_a_then_typing_appends_at_the_end_of_the_line() {
        let mut st = new_state_with("hello\nworld\n");
        st.set_cursor(Position::new(0, 0));
        st.tick(&press(KeyCode::Char('A')));
        for c in "!!".chars() {
            st.tick(&press(KeyCode::Char(c)));
        }
        assert_eq!(
            st.buffers
                .get(st.active)
                .map(|b| b.to_string())
                .unwrap_or_default(),
            "hello!!\nworld\n"
        );
    }

    /// `a` on the LAST character still appends, rather than stalling.
    ///
    /// The case the Normal-mode clamp would break, and the test that measured
    /// how. RED RUN 2026-08-12: `place_cursor`'s clamp needs BOTH
    /// `CursorRest::OnCharacter` and `Mode::Normal`, so this stays green if
    /// either guard is removed and goes red only when both are — reporting
    /// `column: 4` (back on the `o`) instead of 5. See `enter_insert_at`, whose
    /// doc comment originally over-claimed that the ordering alone carried it.
    #[test]
    fn a_on_the_last_character_appends_after_it() {
        let mut st = new_state_with("hello\n");
        st.set_cursor(Position::new(0, 4)); // the `o`
        st.tick(&press(KeyCode::Char('a')));
        assert_eq!(st.cursor(), Position::new(0, 5), "one past the `o`");
        st.tick(&press(KeyCode::Char('?')));
        assert_eq!(
            st.buffers
                .get(st.active)
                .map(|b| b.to_string())
                .unwrap_or_default(),
            "hello?\n"
        );
    }

    /// No insert-entry key touches the buffer except `o`/`O`.
    ///
    /// The direct gate on the reported symptom: "hitting insert creates a
    /// space". It never did — the space was a RENDER defect (see
    /// `escriba-tui`'s `entering_insert_does_not_widen_the_rendered_line`) —
    /// and this test is what keeps the two explanations from being confused
    /// again, by pinning that the text really is untouched.
    #[test]
    fn entering_insert_types_nothing() {
        const TEXT: &str = "  hello\nworld\n";
        for key in ['i', 'I', 'a', 'A'] {
            let (_, _, text) = entry(TEXT, Position::new(0, 4), key);
            assert_eq!(text, TEXT, "`{key}` must not write a character");
        }
        for key in ['o', 'O'] {
            let (_, _, text) = entry(TEXT, Position::new(0, 4), key);
            assert_eq!(
                text.chars().filter(|c| *c == '\n').count(),
                3,
                "`{key}` adds exactly one line terminator and no other char"
            );
            assert!(
                text.contains("  hello") && text.contains("world"),
                "`{key}` must not disturb the existing lines: {text:?}"
            );
        }
    }

    /// Binding bare `a` and `i` must NOT shadow the text objects.
    ///
    /// The regression this whole family risked. `escriba-keymap`'s rule is that
    /// a single binding beats a sequence prefix, so a naive `a` binding would
    /// have made `daw` mean "delete, then append". It does not, because
    /// `consume_object_key` runs before both and claims `i`/`a` only while an
    /// operator is armed — this test is the evidence for that sentence.
    #[test]
    fn the_insert_entry_keys_do_not_shadow_text_objects() {
        assert_eq!(keys("one two three\n", 0, 5, "daw"), "one three\n");
        assert_eq!(keys("one two three\n", 0, 5, "diw"), "one  three\n");
        assert_eq!(keys("f(a, b)\n", 0, 3, "di("), "f()\n");
        // And the operator-free path still reaches the new bindings.
        let (mode, cursor, _) = entry("one two\n", Position::new(0, 0), 'a');
        assert_eq!(mode, Mode::Insert);
        assert_eq!(cursor, Position::new(0, 1), "no operator ⇒ `a` appends");
    }

    /// Text objects FROM THE KEYBOARD.
    ///
    /// Every bracket is unbound, and `i`/`a` are claimed by
    /// `consume_object_key` only while an operator waits, so all of this is
    /// decided on the KEY rather than in the binding table. (Until 2026-08-12
    /// this comment read "`i` is `ChangeMode(Insert)` in Normal and `a` … are
    /// unbound" — true when written, and made false by the insert-entry family
    /// above.)

    fn keys(text: &str, line: u32, col: u32, seq: &str) -> String {
        let mut st = new_state_with(text);
        st.set_cursor(Position::new(line, col));
        for c in seq.chars() {
            st.tick(&press(KeyCode::Char(c)));
        }
        st.buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default()
    }

    #[test]
    fn diw_from_the_keyboard() {
        assert_eq!(keys("one two three\n", 0, 5, "diw"), "one  three\n");
    }

    #[test]
    fn daw_from_the_keyboard_takes_the_space() {
        assert_eq!(keys("one two three\n", 0, 5, "daw"), "one three\n");
    }

    #[test]
    fn ciw_deletes_and_enters_insert() {
        let mut st = new_state_with("one two\n");
        st.set_cursor(Position::new(0, 5));
        for c in "ciw".chars() {
            st.tick(&press(KeyCode::Char(c)));
        }
        assert_eq!(st.modal.mode(), Mode::Insert, "change leaves you inserting");
        let got = st
            .buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default();
        assert_eq!(got, "one \n");
    }

    #[test]
    fn di_paren_and_da_paren_from_the_keyboard() {
        assert_eq!(keys("f(a, b)\n", 0, 3, "di("), "f()\n");
        assert_eq!(keys("f(a, b)\n", 0, 3, "da("), "f\n");
    }

    #[test]
    fn the_closing_bracket_and_b_are_aliases() {
        // vim accepts `i(`, `i)` and `ib` for the same object.
        for sel in ["di(", "di)", "dib"] {
            assert_eq!(keys("f(a, b)\n", 0, 3, sel), "f()\n", "{sel}");
        }
    }

    #[test]
    fn di_quote_from_the_keyboard() {
        assert_eq!(keys("say \"hi\" ok\n", 0, 6, "di\""), "say \"\" ok\n");
    }

    #[test]
    fn i_alone_still_enters_insert_when_no_operator_is_pending() {
        // The load-bearing negative: the object layer must not steal `i`
        // from ordinary use.
        let mut st = new_state_with("abc\n");
        st.tick(&press(KeyCode::Char('i')));
        assert_eq!(st.modal.mode(), Mode::Insert);
    }

    #[test]
    fn an_unknown_object_key_cancels_rather_than_staying_armed() {
        // `diz` is not an object. The operator must disarm, and the buffer
        // must be untouched — not left waiting to eat the next keystroke.
        let mut st = new_state_with("one two\n");
        st.set_cursor(Position::new(0, 5));
        for c in "diz".chars() {
            st.tick(&press(KeyCode::Char(c)));
        }
        let got = st
            .buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default();
        assert_eq!(got, "one two\n", "nothing was deleted");
        assert_eq!(*st.op_pending.state(), OpState::Resting, "and it disarmed");
    }

    // ── the register under a count ───────────────────────────────────

    #[test]
    fn a_counted_delete_puts_all_of_it_in_the_register() {
        // `3dw` is one delete of three words as far as the register is
        // concerned. Each repetition emits its own Yank, and each used to
        // overwrite — so `3dwP` put back only the third word and silently
        // lost two.
        let mut st = new_state_with("one two three four\n");
        st.set_cursor(Position::new(0, 0));
        for c in "3dw".chars() {
            st.tick(&press(KeyCode::Char(c)));
        }
        assert_eq!(
            st.register_text(),
            Some("one two three "),
            "all three words, in the order they were deleted"
        );
    }

    #[test]
    fn an_uncounted_delete_still_replaces_the_register() {
        // The combining flag must not leak: a later single delete replaces.
        let mut st = new_state_with("alpha beta\n");
        st.set_cursor(Position::new(0, 0));
        for c in "3dw".chars() {
            st.tick(&press(KeyCode::Char(c)));
        }
        let mut st2 = new_state_with("gamma delta\n");
        st2.set_cursor(Position::new(0, 0));
        for c in "dw".chars() {
            st2.tick(&press(KeyCode::Char(c)));
        }
        assert_eq!(st2.register_text(), Some("gamma "));
    }

    #[test]
    fn two_separate_counted_deletes_do_not_accumulate_into_each_other() {
        // The flag is cleared after each group, so the second `2dw` starts
        // from empty rather than appending to the first.
        let mut st = new_state_with("a b c d e f\n");
        st.set_cursor(Position::new(0, 0));
        for c in "2dw".chars() {
            st.tick(&press(KeyCode::Char(c)));
        }
        let first = st.register_text().map(str::to_owned);
        for c in "2dw".chars() {
            st.tick(&press(KeyCode::Char(c)));
        }
        assert_eq!(first.as_deref(), Some("a b "));
        assert_eq!(st.register_text(), Some("c d "), "not \"a b c d \"");
    }

    #[test]
    fn a_counted_yank_accumulates_without_changing_the_buffer() {
        let mut st = new_state_with("one two three\n");
        st.set_cursor(Position::new(0, 0));
        let before = st
            .buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default();
        for c in "2yw".chars() {
            st.tick(&press(KeyCode::Char(c)));
        }
        assert_eq!(st.register_text(), Some("one two "));
        let after = st
            .buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default();
        assert_eq!(after, before, "yank does not edit");
    }

    // ── the anchored reply (Negai::ErrandReply) ──────────────────────
    //
    // Landed BEFORE the courier that will produce these. The class being
    // closed: a reply computed off the tick, applied against a world that
    // has since moved, and RESEALED as fresh by the interpreter — which is
    // what every synchronous slip correctly does and what an async one must
    // never do.

    fn a_finding(buffer: BufferId, line: u32) -> escriba_shirube::Finding {
        use escriba_core::{Position, Range};
        escriba_shirube::Finding::new(
            escriba_shirube::Site::in_buffer(
                buffer,
                Range::new(Position::new(line, 0), Position::new(line, 1)),
            ),
            escriba_shirube::Severity::Error,
            "computed off the tick".to_string(),
            escriba_shirube::Origin::Text("test"),
        )
    }

    #[test]
    fn a_fresh_errand_reply_is_honoured() {
        let mut st = new_state_with("a\nb\nc\n");
        let anchor = st.world();
        st.honour_one(escriba_madoguchi::Negai::ErrandReply {
            anchor,
            then: Box::new(escriba_madoguchi::Negai::PublishFindings {
                list: "lsp".to_string(),
                findings: vec![a_finding(st.active, 1)],
            }),
        });
        assert_eq!(
            st.finding_items(true, None).len(),
            1,
            "the world had not moved"
        );
    }

    /// THE red run. Without the freshness check this passes findings
    /// straight through, and `PublishFindings` reseals them with the
    /// CURRENT world — so they are reported fresh at columns that moved.
    #[test]
    fn a_stale_errand_reply_is_dropped_not_resealed() {
        let mut st = new_state_with("a\nb\nc\n");
        // Capture the world the "server" computed against...
        let anchor = st.world();
        // ...then let the operator keep typing, which is the whole point.
        st.apply(&Action::InsertChar('x'));

        st.honour_one(escriba_madoguchi::Negai::ErrandReply {
            anchor,
            then: Box::new(escriba_madoguchi::Negai::PublishFindings {
                list: "lsp".to_string(),
                findings: vec![a_finding(st.active, 1)],
            }),
        });
        assert!(
            st.finding_items(true, None).is_empty(),
            "a reply computed against an older text revision must be DROPPED, \
             not resealed against the current one"
        );
    }

    /// The failure the wrapper exists for, and the reason it wraps a slip
    /// rather than adding an anchor field to PublishFindings: a stale EDIT
    /// corrupts the file, where a stale diagnostic merely mis-decorates it.
    #[test]
    fn a_stale_errand_reply_cannot_edit_the_buffer() {
        let mut st = new_state_with("hello\n");
        let anchor = st.world();
        st.apply(&Action::InsertChar('!'));
        let before = st
            .buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default();

        st.honour_one(escriba_madoguchi::Negai::ErrandReply {
            anchor,
            then: Box::new(escriba_madoguchi::Negai::Edit {
                buffer: st.active,
                edit: escriba_core::Edit {
                    range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                    kind: escriba_core::EditKind::Insert {
                        text: "FORMATTED".to_string(),
                    },
                },
            }),
        });
        let after = st
            .buffers
            .get(st.active)
            .map(|b| b.to_string())
            .unwrap_or_default();
        assert_eq!(
            after, before,
            "a stale formatter reply must not touch the text"
        );
    }

    fn press(kc: KeyCode) -> AppEvent {
        AppEvent::Key(KeyEvent {
            key: kc,
            pressed: true,
            modifiers: Modifiers::default(),
            text: None,
        })
    }

    // ── operator-over-motion (the `dw`/`c$`/`y0` verbs) ──────────────

    fn line0_len(s: &EditorState) -> u32 {
        s.buffers.get(s.active).unwrap().line_len_chars(0)
    }

    #[test]
    fn delete_to_line_end_clears_line_and_fills_register() {
        let mut s = new_state_with("hello world");
        s.apply(&Action::ApplyOperator {
            op: Operator::Delete,
            motion: Motion::LineEnd,
        });
        assert_eq!(line0_len(&s), 0, "d$ deletes to end of line");
        assert_eq!(
            s.register_text(),
            Some("hello world"),
            "delete fills the register"
        );
        assert_eq!(
            s.cursor(),
            Position::ZERO,
            "cursor lands at the range start"
        );
    }

    #[test]
    fn delete_over_right_motion_removes_one_char() {
        let mut s = new_state_with("abc");
        s.apply(&Action::ApplyOperator {
            op: Operator::Delete,
            motion: Motion::Right,
        });
        assert_eq!(
            s.buffers.get(s.active).unwrap().line(0).as_deref(),
            Some("bc")
        );
        assert_eq!(s.register_text(), Some("a"));
    }

    #[test]
    fn change_to_line_end_deletes_and_enters_insert() {
        let mut s = new_state_with("hello world");
        assert_eq!(s.modal.mode(), Mode::Normal);
        s.apply(&Action::ApplyOperator {
            op: Operator::Change,
            motion: Motion::LineEnd,
        });
        assert_eq!(line0_len(&s), 0, "c$ deletes the range");
        assert_eq!(
            s.modal.mode(),
            Mode::Insert,
            "change enters Insert to type the replacement"
        );
        assert_eq!(
            s.register_text(),
            Some("hello world"),
            "change fills the register"
        );
    }

    #[test]
    fn yank_to_line_end_fills_register_without_mutating() {
        let mut s = new_state_with("hello world");
        s.apply(&Action::ApplyOperator {
            op: Operator::Yank,
            motion: Motion::LineEnd,
        });
        assert_eq!(line0_len(&s), 11, "yank does not mutate the buffer");
        assert_eq!(
            s.register_text(),
            Some("hello world"),
            "yank fills the register"
        );
        assert_eq!(s.modal.mode(), Mode::Normal, "yank stays in Normal");
    }

    #[test]
    fn resolve_motion_is_the_shared_target_for_move_and_operator() {
        // The encapsulation proof: apply_motion (cursor move) and
        // apply_operator (range end) BOTH stand on resolve_motion.
        //
        // They read its answer differently AT THE BUFFER EDGE, and the
        // difference is vim's: `d$` deletes the last character, so the RANGE
        // ends after it; `$` puts the cursor ON it, because Normal mode has
        // nowhere past the last character to stand. One resolver, one target,
        // two readings — the reading is the mode's, not the motion's, which
        // is why the rest rule lives in `place_cursor` and not in here.
        let mut s = new_state_with("hello world");
        let target = s.resolve_motion(Position::ZERO, Motion::LineEnd).unwrap();
        assert_eq!(target, Position::new(0, 11), "the exclusive range end");

        s.apply_motion(Motion::LineEnd);
        assert_eq!(
            s.cursor(),
            Position::new(0, 10),
            "`$` rests on the last character, not past it",
        );

        let mut d = new_state_with("hello world");
        d.apply(&Action::ApplyOperator {
            op: Operator::Delete,
            motion: Motion::LineEnd,
        });
        assert_eq!(
            line0_len(&d),
            0,
            "`d$` deletes through the last character — the range ends where \
             resolve_motion said, not where the cursor may rest",
        );
    }

    #[test]
    fn empty_motion_range_is_a_no_op() {
        // An operator over a zero-width motion (cursor already at line start)
        // mutates nothing and leaves the register untouched.
        let mut s = new_state_with("abc");
        s.apply(&Action::ApplyOperator {
            op: Operator::Delete,
            motion: Motion::LineStart,
        });
        assert_eq!(
            s.buffers.get(s.active).unwrap().line(0).as_deref(),
            Some("abc")
        );
        assert_eq!(s.register_text(), None);
    }

    #[test]
    fn operator_then_motion_composes_through_the_pending_fsm() {
        // The full keymap→FSM→engine path: dispatching the `d` operator action
        // then a `$` motion composes `d$` via the zenmai operator-pending FSM —
        // the operator key alone does nothing until the motion arrives.
        let mut s = new_state_with("hello world");
        s.apply(&Action::Operator(Operator::Delete));
        assert_eq!(line0_len(&s), 11, "the operator key alone mutates nothing");
        s.apply(&Action::Move(Motion::LineEnd));
        assert_eq!(
            line0_len(&s),
            0,
            "d then $ composes d$ and deletes the line"
        );
        assert_eq!(s.register_text(), Some("hello world"));
    }

    #[test]
    fn change_operator_through_fsm_enters_insert() {
        let mut s = new_state_with("hello world");
        s.apply(&Action::Operator(Operator::Change));
        s.apply(&Action::Move(Motion::LineEnd));
        assert_eq!(s.modal.mode(), Mode::Insert, "c$ deletes and enters Insert");
    }

    #[test]
    fn lone_motion_after_no_operator_just_moves() {
        // Without a preceding operator the motion passes through unchanged —
        // and comes to rest on the last character, as Normal mode requires.
        let mut s = new_state_with("hello world");
        s.apply(&Action::Move(Motion::LineEnd));
        assert_eq!(s.cursor(), Position::new(0, 10));
        assert_eq!(line0_len(&s), 11, "a bare motion never mutates");
    }

    #[test]
    fn counted_operator_deletes_count_times() {
        // `3d` + a right-motion = `3dl` = delete 3 chars. The operator's count
        // flows through the FSM to the composed motion (the bug fix: previously
        // the count repeated the operator key and toggled the FSM).
        let mut s = new_state_with("abcdef");
        s.apply_counted(&Action::Operator(Operator::Delete), 3);
        assert_eq!(line0_len(&s), 6, "the operator key alone mutates nothing");
        s.apply(&Action::Move(Motion::Right));
        assert_eq!(
            s.buffers.get(s.active).unwrap().line(0).as_deref(),
            Some("def")
        );
    }

    #[test]
    fn operator_and_motion_counts_multiply_end_to_end() {
        // `2d3l` = delete 2×3 = 6 chars.
        let mut s = new_state_with("abcdefgh");
        s.apply_counted(&Action::Operator(Operator::Delete), 2);
        s.apply_counted(&Action::Move(Motion::Right), 3);
        assert_eq!(
            s.buffers.get(s.active).unwrap().line(0).as_deref(),
            Some("gh")
        );
    }

    #[test]
    fn bare_counted_motion_still_repeats_no_regression() {
        // `3j` still moves down 3 lines — the count passes through the FSM
        // unchanged when no operator is pending.
        let mut s = new_state_with("a\nb\nc\nd\ne");
        s.apply_counted(&Action::Move(Motion::Down), 3);
        assert_eq!(s.cursor().line, 3, "5j-style counted motion preserved");
    }

    /// A monotonic clock for the key-repeat gate in tests — each `next()`
    /// jumps a full second past the previous, so every press it stamps is
    /// well outside the 80ms debounce window and therefore an INTENTIONAL
    /// press (never a storm tick). Used by tests that fire the *same*
    /// navigation key twice and assert editor logic, not debounce timing.
    struct SpacedClock(std::time::Instant);
    impl SpacedClock {
        fn new() -> Self {
            Self(std::time::Instant::now())
        }
        fn next(&mut self) -> std::time::Instant {
            self.0 += std::time::Duration::from_secs(1);
            self.0
        }
    }

    #[test]
    fn hjkl_moves_cursor() {
        let mut s = new_state_with("hello\nworld");
        s.tick(&press(KeyCode::Char('l')));
        assert_eq!(s.cursor().column, 1);
        s.tick(&press(KeyCode::Char('j')));
        assert_eq!(s.cursor().line, 1);
        s.tick(&press(KeyCode::Char('h')));
        assert_eq!(s.cursor().column, 0);
    }

    #[test]
    fn insert_mode_inserts_chars() {
        let mut s = new_state_with("");
        s.tick(&press(KeyCode::Char('i')));
        assert_eq!(s.modal.mode(), Mode::Insert);
        s.tick(&press(KeyCode::Char('h')));
        s.tick(&press(KeyCode::Char('i')));
        assert_eq!(s.buffers.get(s.active).unwrap().to_string(), "hi");
        assert_eq!(s.cursor().column, 2);
    }

    #[test]
    fn esc_returns_to_normal() {
        let mut s = new_state_with("");
        s.tick(&press(KeyCode::Char('i')));
        s.tick(&press(KeyCode::Escape));
        assert_eq!(s.modal.mode(), Mode::Normal);
    }

    #[test]
    fn count_prefix_repeats_motion() {
        let mut s = new_state_with("abcdefghij");
        s.tick(&press(KeyCode::Char('5')));
        s.tick(&press(KeyCode::Char('l')));
        assert_eq!(s.cursor().column, 5);
    }

    #[test]
    fn close_event_requests_quit() {
        let mut s = new_state_with("");
        s.tick(&AppEvent::CloseRequested);
        assert!(s.quit_requested);
    }

    #[test]
    fn word_next_jumps_past_whitespace() {
        let mut s = new_state_with("foo bar baz");
        // Two INTENTIONAL `w` presses, spaced past the key-repeat window so
        // the gate passes both (a real user's two taps are ≥80ms apart).
        let mut clk = SpacedClock::new();
        s.tick_at(&press(KeyCode::Char('w')), clk.next());
        assert_eq!(s.cursor().column, 4);
        s.tick_at(&press(KeyCode::Char('w')), clk.next());
        assert_eq!(s.cursor().column, 8);
    }

    // ── Multi-key / leader pending-stroke ───────────────────────────

    #[test]
    fn leader_sequence_holds_then_resolves() {
        let mut s = new_state_with("a\nbb\nccc");
        s.keymap.bind_sequence(
            Mode::Normal,
            vec![Key::Char(','), Key::Char('g')],
            Action::Move(Motion::DocEnd),
            "doc end",
        );
        // `,` begins the sequence — held pending, nothing applied yet.
        s.on_key(&Key::Char(','));
        assert_eq!(s.pending_keys, vec![Key::Char(',')]);
        assert_eq!(s.cursor(), Position::ZERO);
        // `g` completes `<leader>g` → DocEnd; pending clears.
        s.on_key(&Key::Char('g'));
        assert!(s.pending_keys.is_empty());
        assert_eq!(s.cursor().line, 2);
    }

    #[test]
    fn two_key_gg_jumps_doc_start() {
        let mut s = new_state_with("a\nbb\nccc");
        s.keymap.bind_sequence(
            Mode::Normal,
            vec![Key::Char('g'), Key::Char('g')],
            Action::Move(Motion::DocStart),
            "doc start",
        );
        let mut clk = SpacedClock::new();
        s.tick_at(&press(KeyCode::Char('j')), clk.next());
        s.tick_at(&press(KeyCode::Char('j')), clk.next());
        assert_eq!(s.cursor().line, 2);
        s.on_key(&Key::Char('g')); // pending
        assert_eq!(s.pending_keys, vec![Key::Char('g')]);
        s.on_key(&Key::Char('g')); // resolve
        assert_eq!(s.cursor(), Position::ZERO);
    }

    #[test]
    fn broken_sequence_aborts_and_clears_pending() {
        let mut s = new_state_with("hello");
        s.keymap.bind_sequence(
            Mode::Normal,
            vec![Key::Char('g'), Key::Char('g')],
            Action::Move(Motion::DocEnd),
            "doc end",
        );
        s.on_key(&Key::Char('g')); // pending [g]
        assert_eq!(s.pending_keys, vec![Key::Char('g')]);
        s.on_key(&Key::Char('x')); // breaks gg → abort; x is unbound → no-op
        assert!(s.pending_keys.is_empty());
        assert_eq!(s.cursor(), Position::ZERO);
    }

    #[test]
    fn single_binding_wins_over_sequence_prefix() {
        // A key that is BOTH a complete single binding and the start of
        // a sequence fires the single binding immediately (no chord
        // timeout needed). Here `h` (move-left) also prefixes `hz`.
        let mut s = new_state_with("abcde");
        let mut clk = SpacedClock::new();
        s.tick_at(&press(KeyCode::Char('l')), clk.next());
        s.tick_at(&press(KeyCode::Char('l')), clk.next());
        assert_eq!(s.cursor().column, 2);
        s.keymap.bind_sequence(
            Mode::Normal,
            vec![Key::Char('h'), Key::Char('z')],
            Action::Move(Motion::DocEnd),
            "shadowed",
        );
        s.on_key(&Key::Char('h'));
        assert!(s.pending_keys.is_empty(), "single binding should not pend");
        assert_eq!(s.cursor().column, 1, "h moved left immediately");
    }

    // ── tatara-lisp runtime bridge (imperative programmability) ─────

    #[test]
    fn lisp_set_option_writes_live_options() {
        let mut s = new_state_with("");
        s.run_lisp(r#"(set-option "number" "true")"#).unwrap();
        assert_eq!(s.options.get("number").map(String::as_str), Some("true"));
    }

    #[test]
    fn lisp_insert_modifies_buffer_and_advances_cursor() {
        let mut s = new_state_with("");
        s.run_lisp(r#"(insert "abc")"#).unwrap();
        assert_eq!(s.buffers.get(s.active).unwrap().to_string(), "abc");
        assert_eq!(s.cursor(), Position::new(0, 3));
    }

    #[test]
    fn lisp_message_appends_to_messages() {
        let mut s = new_state_with("");
        s.run_lisp(r#"(message "hello from lisp")"#).unwrap();
        assert_eq!(s.messages, vec!["hello from lisp".to_string()]);
    }

    #[test]
    fn lisp_reads_snapshot_and_branches_to_effect() {
        // Genuine programmability: Lisp reads the live cursor line and
        // an `if` decides which option to set.
        let mut s = new_state_with("one\ntwo\nthree");
        // cursor at line 0 → "top" branch
        s.run_lisp(r#"(if (= (cursor-line) 0) (set-option "pos" "top") (set-option "pos" "mid"))"#)
            .unwrap();
        assert_eq!(s.options.get("pos").map(String::as_str), Some("top"));
    }

    #[test]
    fn lisp_run_command_effect_drives_registry() {
        // `(run-command "undo")` reaches the live command registry and
        // reverts a prior Lisp-driven insert — proving the RunCommand
        // effect dispatches through real editor commands.
        let mut s = new_state_with("");
        s.run_lisp(r#"(insert "abc")"#).unwrap();
        assert_eq!(s.buffers.get(s.active).unwrap().to_string(), "abc");
        s.run_lisp(r#"(run-command "undo")"#).unwrap();
        assert_eq!(s.buffers.get(s.active).unwrap().to_string(), "");
    }

    #[test]
    fn lisp_run_command_quit_sets_quit_requested_via_typed_flag() {
        // The full imperative-quit path: (run-command "quit") routes
        // through the registry's typed `quit_requested` signal — no string
        // sentinel, and no minibuffer pollution (the editor stays in a
        // clean Normal state, which has no minibuffer at all).
        let mut s = new_state_with("");
        s.run_lisp(r#"(run-command "quit")"#).unwrap();
        assert!(s.quit_requested, "lisp-driven quit must set quit_requested");
        assert_eq!(
            s.modal.minibuffer(),
            "",
            "quit must not pollute any command line — Normal mode has no minibuffer",
        );
    }

    // ── Lazy plugin activation (PluginHost) ────────────────────────

    #[test]
    fn lazy_plugin_activates_on_command_trigger() {
        // A user plugin gated on `Command: LazyGo` has its entry applied
        // the first time that command runs — proving the lazy.nvim
        // `cmd =` model works end-to-end against live editor state.
        let mut s = new_state_with("");
        s.register_lazy_plugin(
            "user-lazy",
            vec![LazyTrigger::Command("LazyGo".into())],
            r#"(defoption :name "lazy-loaded" :value "yes")
               (defcmd :name "LazyGo" :description "noop" :action "editor.noop")"#,
        );
        assert_eq!(s.plugin_host.pending(), 1);
        assert!(
            s.options.get("lazy-loaded").is_none(),
            "entry not applied yet"
        );

        // Drive the command through the public imperative path.
        s.run_lisp(r#"(run-command "LazyGo")"#).unwrap();

        assert_eq!(
            s.options.get("lazy-loaded").map(String::as_str),
            Some("yes"),
            "the command trigger applied the plugin's entry",
        );
        assert_eq!(s.plugin_host.pending(), 0, "plugin activated exactly once");
    }

    #[test]
    fn lazy_plugin_activates_on_filetype() {
        let mut s = new_state_with("");
        s.register_lazy_plugin(
            "user-rust",
            vec![LazyTrigger::FileType("rust".into())],
            r#"(defoption :name "rust-plugin" :value "on")"#,
        );
        let n = s.activate_filetype_plugins("rust");
        assert_eq!(n, 1);
        assert_eq!(s.options.get("rust-plugin").map(String::as_str), Some("on"));
        // A second open of the same filetype is a no-op (one-shot).
        assert_eq!(s.activate_filetype_plugins("rust"), 0);
    }

    #[test]
    fn cached_vm_serves_multiple_run_lisp_calls() {
        let mut s = new_state_with("");
        s.run_lisp(r#"(message "one")"#).unwrap();
        assert!(
            s.lisp_vm.is_some(),
            "VM should be cached after first run_lisp"
        );
        s.run_lisp(r#"(message "two")"#).unwrap();
        assert_eq!(s.messages, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn lisp_define_persists_across_run_lisp_calls() {
        // The cached VM's top-level env persists across calls (REPL
        // semantics): a `define` in one call is visible in the next.
        let mut s = new_state_with("");
        s.run_lisp(r#"(define greeting "hi")"#).unwrap();
        s.run_lisp(r#"(message greeting)"#).unwrap();
        assert_eq!(s.messages, vec!["hi".to_string()]);
    }

    #[test]
    fn snapshot_is_isolated_within_one_run_lisp_call_and_refreshes_across() {
        // Within ONE call a program cannot observe its own writes — the
        // read snapshot is captured before eval, effects apply after. A
        // later call sees the refreshed snapshot.
        let mut s = new_state_with("");
        s.run_lisp(
            r#"(insert "ab") (set-option "col" (if (= (cursor-column) 0) "stale-zero" "live"))"#,
        )
        .unwrap();
        assert_eq!(s.buffers.get(s.active).unwrap().to_string(), "ab");
        assert_eq!(
            s.options.get("col").map(String::as_str),
            Some("stale-zero"),
            "cursor-column within the same call reads the pre-eval snapshot",
        );
        // After the first call the cursor advanced to column 2; the next
        // call's snapshot reflects it.
        s.run_lisp(r#"(set-option "col2" (if (= (cursor-column) 2) "live-two" "other"))"#)
            .unwrap();
        assert_eq!(
            s.options.get("col2").map(String::as_str),
            Some("live-two"),
            "a later call sees the refreshed snapshot",
        );
    }

    #[test]
    fn insert_text_effect_multiline_lands_cursor_on_last_line() {
        let mut s = new_state_with("");
        s.apply_host_effects(vec![Negai::InsertText("foo\nbar".to_string())]);
        assert_eq!(s.buffers.get(s.active).unwrap().to_string(), "foo\nbar");
        assert_eq!(s.cursor(), Position::new(1, 3));
    }

    #[test]
    fn visual_mode_sequence_resolves() {
        let mut s = new_state_with("abc");
        s.modal.enter(Mode::Visual);
        s.keymap.bind_sequence(
            Mode::Visual,
            vec![Key::Char('g'), Key::Char('e')],
            Action::Move(Motion::DocEnd),
            "ge",
        );
        s.on_key(&Key::Char('g'));
        assert_eq!(s.pending_keys, vec![Key::Char('g')]);
        s.on_key(&Key::Char('e'));
        assert!(s.pending_keys.is_empty());
        assert_eq!(
            s.cursor().column,
            3,
            "ge resolved to doc-end in visual mode"
        );
    }

    #[test]
    fn sequence_abort_with_bound_breaking_key_redispatches() {
        // gg is a sequence; `l` (move-right) is a bound single key. After
        // `g` pends, `l` breaks gg, aborts, and is re-dispatched fresh.
        let mut s = new_state_with("abcde");
        s.keymap.bind_sequence(
            Mode::Normal,
            vec![Key::Char('g'), Key::Char('g')],
            Action::Move(Motion::DocEnd),
            "gg",
        );
        s.on_key(&Key::Char('g'));
        assert_eq!(s.pending_keys, vec![Key::Char('g')]);
        s.on_key(&Key::Char('l'));
        assert!(s.pending_keys.is_empty());
        assert_eq!(
            s.cursor().column,
            1,
            "the breaking key l should re-dispatch as move-right",
        );
    }

    // ── Viewport-follows-cursor invariant (both axes) ───────────────

    #[test]
    fn viewport_contains_cursor_after_every_op() {
        // Tiny window: 5 visible lines × 10 visible columns. Drive a
        // representative scripted sequence and assert the viewport contains
        // the cursor after EVERY mutating step.
        let mut s = new_state_small_viewport("", 5, 10);
        assert_cursor_in_viewport(&s, "initial");

        // Enter insert mode and type 30 newline-separated lines — this is
        // the exact "type past the bottom" complaint.
        s.tick(&press(KeyCode::Char('i')));
        assert_eq!(s.modal.mode(), Mode::Insert);
        for line in 0..30u32 {
            for c in "line".chars() {
                s.tick(&press(KeyCode::Char(c)));
                assert_cursor_in_viewport(&s, "typing chars");
            }
            s.tick(&press(KeyCode::Enter));
            assert_cursor_in_viewport(&s, &format!("newline after line {line}"));
        }

        // Type a long (200-char) line — the "type past the right edge"
        // complaint. The cursor must stay horizontally visible the whole way.
        for i in 0..200u32 {
            s.tick(&press(KeyCode::Char('x')));
            assert_cursor_in_viewport(&s, &format!("long-line char {i}"));
        }

        // Multi-line insert_text effect (the `(insert …)` Lisp path).
        s.insert_text("alpha\nbeta\ngamma delta epsilon zeta");
        assert_cursor_in_viewport(&s, "insert_text multiline");

        // Back to normal mode and move in all directions / to extremes.
        s.tick(&press(KeyCode::Escape));
        assert_eq!(s.modal.mode(), Mode::Normal);
        for m in [
            Motion::DocStart,
            Motion::DocEnd,
            Motion::Down,
            Motion::Down,
            Motion::Up,
            Motion::Right,
            Motion::Right,
            Motion::Left,
            Motion::LineEnd,
            Motion::LineStart,
            Motion::GotoLine(1),
            Motion::GotoLine(40),
            Motion::PageDown,
            Motion::PageUp,
        ] {
            s.apply_motion(m);
            assert_cursor_in_viewport(&s, &format!("after motion {m:?}"));
        }

        // Undo many times — the buffer shrinks; the viewport must re-follow
        // the (now clamped) cursor.
        for i in 0..50u32 {
            s.apply(&Action::Undo);
            assert_cursor_in_viewport(&s, &format!("undo {i}"));
        }
        // Redo back up.
        for i in 0..50u32 {
            s.apply(&Action::Redo);
            assert_cursor_in_viewport(&s, &format!("redo {i}"));
        }
    }

    #[test]
    fn insert_at_eof_keeps_cursor_in_bounds() {
        // Inserting at the end of the buffer must leave the cursor clamped
        // to a valid position (and inside the viewport).
        let mut s = new_state_small_viewport("abc", 5, 10);
        s.apply_motion(Motion::DocEnd);
        s.tick(&press(KeyCode::Char('i')));
        s.tick(&press(KeyCode::Char('d')));
        let buf = s.buffers.get(s.active).unwrap();
        let clamped = buf.clamp(s.cursor());
        assert_eq!(
            s.cursor(),
            clamped,
            "cursor must be clamped in-bounds at EOF"
        );
        assert_cursor_in_viewport(&s, "insert at eof");
    }

    #[test]
    fn count_prefix_then_sequence_repeats() {
        // `2` then `gj` (→ move-down) repeats the resolved action twice.
        let mut s = new_state_with("a\nb\nc\nd\ne");
        s.keymap.bind_sequence(
            Mode::Normal,
            vec![Key::Char('g'), Key::Char('j')],
            Action::Move(Motion::Down),
            "gj",
        );
        s.on_key(&Key::Char('2'));
        s.on_key(&Key::Char('g'));
        s.on_key(&Key::Char('j'));
        assert_eq!(s.cursor().line, 2, "count 2 should repeat the gj motion");
    }

    // ── Key-repeat gate (awase::KeyRepeatGate) ──────────────────────────

    #[test]
    fn held_key_repeat_storm_is_debounced_in_normal_mode() {
        // The audit's exact complaint: holding `j` floods motion events
        // and thrashes the viewport. Simulate an OS key-repeat storm — 20
        // identical `j` KeyDowns at 50ms intervals (typical repeat cadence)
        // — and assert only the gated subset (one per 80ms window) actually
        // moves the cursor.
        let mut s = new_state_with(&"x\n".repeat(40));
        let t0 = std::time::Instant::now();
        let mut delivered = 0u32;
        for i in 0..20u32 {
            let before = s.cursor().line;
            s.tick_at(
                &press(KeyCode::Char('j')),
                t0 + std::time::Duration::from_millis(u64::from(i) * 50),
            );
            if s.cursor().line != before {
                delivered += 1;
            }
        }
        // 20 events over ~1s at 50ms spacing, 80ms gate ⇒ ~13 pass — far
        // fewer than the 20 the ungated path would have applied.
        assert!(
            (10..=14).contains(&delivered),
            "expected the storm debounced to ~13 moves, got {delivered}",
        );
        assert!(
            delivered < 20,
            "the gate must drop SOME storm ticks, not pass all 20",
        );
    }

    #[test]
    fn spaced_intentional_taps_all_pass() {
        // Intentional taps spaced past the debounce window must ALL reach
        // the editor — the gate filters storms, never deliberate input.
        let mut s = new_state_with(&"x\n".repeat(10));
        let t0 = std::time::Instant::now();
        for i in 0..5u32 {
            s.tick_at(
                &press(KeyCode::Char('j')),
                // 100ms apart — comfortably past the 80ms window.
                t0 + std::time::Duration::from_millis(u64::from(i) * 100),
            );
        }
        assert_eq!(s.cursor().line, 5, "all 5 spaced `j` taps moved the cursor");
    }

    #[test]
    fn distinct_keys_have_independent_clocks() {
        // Holding `j` must not block a simultaneous `l` — the gate keys on
        // the Key, so independent keys have independent windows.
        let mut s = new_state_with("abc\ndef\nghi");
        let t = std::time::Instant::now();
        s.tick_at(&press(KeyCode::Char('j')), t);
        // `j` again within the window is dropped…
        s.tick_at(
            &press(KeyCode::Char('j')),
            t + std::time::Duration::from_millis(10),
        );
        assert_eq!(s.cursor().line, 1, "second `j` within window dropped");
        // …but `l` at the same instant passes (its own clock).
        s.tick_at(
            &press(KeyCode::Char('l')),
            t + std::time::Duration::from_millis(10),
        );
        assert_eq!(s.cursor().column, 1, "`l` is not blocked by `j`'s clock");
    }

    // ── Cursors newtype is the single cursor home ──────────────────────

    #[test]
    fn cursor_home_preserves_single_cursor_behavior() {
        // The typed `Cursors` wrapper behaves exactly like the old bare
        // `Position` field for single-cursor editing: the read accessor
        // tracks every mutation routed through `set_cursor`, and there is
        // exactly one caret.
        let mut s = new_state_with("hello\nworld\nthere");
        assert_eq!(s.cursor(), Position::ZERO);
        assert_eq!(s.cursors.count(), 1, "phase-1 holds exactly one caret");

        s.apply_motion(Motion::Down);
        s.apply_motion(Motion::Right);
        s.apply_motion(Motion::Right);
        assert_eq!(s.cursor(), Position::new(1, 2));
        // Still a single caret after a sequence of motions.
        assert_eq!(s.cursors.count(), 1);

        // The accessor is the SAME value the viewport-follow path read.
        let w = s.layout.active_window().unwrap();
        assert!(w.viewport.top_line <= s.cursor().line);
    }

    #[test]
    fn insert_mode_is_ungated_so_repeat_typing_works() {
        // Holding a key to repeat-type a character is intended in Insert
        // mode — the gate must NOT suppress it. 10 rapid identical `x`
        // keystrokes at the same instant must all land as text.
        let mut s = new_state_with("");
        s.tick(&press(KeyCode::Char('i')));
        assert_eq!(s.modal.mode(), Mode::Insert);
        let t = std::time::Instant::now();
        for _ in 0..10 {
            s.tick_at(&press(KeyCode::Char('x')), t);
        }
        assert_eq!(
            s.buffers.get(s.active).unwrap().to_string(),
            "xxxxxxxxxx",
            "insert-mode repeat typing is ungated",
        );
    }

    // ── the courier seam (denrei) ────────────────────────────────────
    //
    // What these pin is not "work happens off-thread" — that is the runner's
    // business. It is that a reply computed against one world cannot be
    // applied against a different one, and that the machinery says so out loud
    // when it declines to do something.

    mod courier_seam {
        use super::new_state_with;
        use escriba_madoguchi::Negai;
        use escriba_madoguchi::errand::{Crew, Errand, Freight, Parcel, Runner};
        use escriba_shirube::{Anchor, Axis, ResultList, SessionKind};
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        use std::sync::mpsc::Sender;

        fn a_scan() -> Freight {
            Freight::Scan {
                raw: "needle".into(),
                case: escriba_search::CaseMode::Smart,
                root: ".".into(),
            }
        }

        /// Replies with whatever slip it was built with, immediately and on the
        /// calling thread — so these tests assert the SEAM, not thread timing.
        struct Says(Negai);
        impl Runner for Says {
            fn start(&self, e: Errand, _c: Arc<AtomicBool>, reply: Sender<Parcel>) {
                let _ = reply.send(Parcel {
                    id: e.id,
                    slip: self.0.clone(),
                });
            }
        }

        /// Replies by wrapping its payload in the anchor the DISPATCHER sealed
        /// — which is what a real runner does: it echoes back the seal it was
        /// handed, because it has no way to mint its own.
        struct EchoesSeal(Negai);
        impl Runner for EchoesSeal {
            fn start(&self, e: Errand, _c: Arc<AtomicBool>, reply: Sender<Parcel>) {
                let _ = reply.send(Parcel {
                    id: e.id,
                    slip: Negai::ErrandReply {
                        anchor: e.anchor.into_anchor(),
                        then: Box::new(self.0.clone()),
                    },
                });
            }
        }

        fn crew_with_scan(r: impl Runner + 'static) -> Crew {
            Crew {
                scan: Box::new(r),
                diagnostics: Box::new(escriba_madoguchi::errand::Idle("t")),
                format: Box::new(escriba_madoguchi::errand::Idle("t")),
            }
        }

        /// The whole path in one test: a handler names a class of work, the
        /// dispatcher seals it, a runner answers, and the reply is applied at a
        /// tick boundary.
        #[test]
        fn an_errand_is_dispatched_sealed_and_its_reply_applied_at_the_drain() {
            let mut st = new_state_with("x\n");
            st.hire(crew_with_scan(EchoesSeal(Negai::Message("done".into()))));

            st.honour_one(Negai::Errand(Box::new(a_scan())));
            assert!(
                !st.messages.iter().any(|m| m == "done"),
                "nothing is applied before the drain"
            );

            st.deliver();
            assert!(
                st.messages.iter().any(|m| m == "done"),
                "the reply lands at the drain: {:?}",
                st.messages
            );
        }

        /// **The reason the whole seam exists.** A reply sealed against the
        /// world at dispatch must be discarded once that world has moved.
        #[test]
        fn a_reply_whose_world_moved_is_dropped() {
            let mut st = new_state_with("x\n");
            st.hire(crew_with_scan(EchoesSeal(Negai::Message("late".into()))));

            st.honour_one(Negai::Errand(Box::new(a_scan())));
            // The surface the scan feeds closed while it was running.
            st.bump_scan_gen();
            st.deliver();

            assert!(
                !st.messages.iter().any(|m| m == "late"),
                "a superseded reply must not be applied: {:?}",
                st.messages
            );
        }

        /// The converse, so the test above is not passing because nothing ever
        /// applies.
        #[test]
        fn a_reply_whose_world_held_is_applied() {
            let mut st = new_state_with("x\n");
            st.hire(crew_with_scan(EchoesSeal(Negai::Message("ok".into()))));
            st.honour_one(Negai::Errand(Box::new(a_scan())));
            st.deliver();
            assert!(st.messages.iter().any(|m| m == "ok"));
        }

        /// A scan must NOT be staled by typing. It reads the filesystem; no
        /// text revision has anything to say about it, and anchoring one on the
        /// buffers would kill every result on the next keystroke.
        #[test]
        fn typing_does_not_stale_a_scan_reply() {
            let mut st = new_state_with("x\n");
            st.hire(crew_with_scan(EchoesSeal(Negai::Message("rows".into()))));
            st.honour_one(Negai::Errand(Box::new(a_scan())));

            st.insert_text("hello");
            st.deliver();
            assert!(
                st.messages.iter().any(|m| m == "rows"),
                "a scan does not depend on buffer text: {:?}",
                st.messages
            );
        }

        /// The seal's OWN anchor becomes the list's seal. Re-sealing at the
        /// arrival world would widen a one-axis claim into an every-buffer one,
        /// so the findings would die on the next unrelated edit.
        #[test]
        fn findings_from_an_errand_keep_the_narrow_seal_they_were_computed_with() {
            let mut st = new_state_with("x\n");
            st.hire(crew_with_scan(EchoesSeal(Negai::PublishFindings {
                list: "grep".into(),
                findings: vec![],
            })));
            st.honour_one(Negai::Errand(Box::new(a_scan())));
            st.deliver();

            let sealed_with = st.results.get("grep").expect("published").anchor().clone();
            let axes = sealed_with.axes();
            assert_eq!(axes.len(), 1, "narrow, not the whole world: {axes:?}");
            assert!(
                matches!(axes[0], Axis::Session(SessionKind::Scan, _)),
                "sealed on the scan session: {axes:?}"
            );

            // …and the consequence that makes it worth doing: an edit
            // elsewhere does not discard it.
            st.insert_text("more");
            assert!(
                !st.results
                    .get("grep")
                    .expect("still there")
                    .is_stale(&st.world()),
                "an unrelated edit must not stale a scan list"
            );
        }

        /// A directly-dispatched `PublishFindings` — an on-tick producer like
        /// the marker scan — still seals at the world, which is correct for it.
        /// The special case must not have changed that.
        #[test]
        fn a_direct_publish_still_seals_at_the_world() {
            let mut st = new_state_with("x\n");
            st.honour_one(Negai::PublishFindings {
                list: "todo".into(),
                findings: vec![],
            });
            let axes = st.results.get("todo").expect("published").anchor().axes();
            assert!(
                axes.len() > 1,
                "the on-tick path anchors on the whole world: {axes:?}"
            );
        }

        /// An empty anchor is fresh against every world, so a forged reply
        /// carrying one bypasses the gate entirely. The courier cannot produce
        /// this — `seal` returns a `NonEmptyAnchor` — and the test exists to
        /// document why that type is not decoration.
        #[test]
        fn an_empty_anchor_would_bypass_the_gate_which_is_why_seal_cannot_mint_one() {
            let mut st = new_state_with("x\n");
            st.bump_scan_gen();
            st.bump_lsp_gen();
            st.insert_text("moved a long way");

            st.honour_one(Negai::ErrandReply {
                anchor: Anchor::new(),
                then: Box::new(Negai::Message("forged".into())),
            });
            assert!(
                st.messages.iter().any(|m| m == "forged"),
                "an empty anchor passes any world — the hazard NonEmptyAnchor removes"
            );
        }

        /// Closing the picker supersedes the scan feeding it. Both closing
        /// paths must do it — choosing a row closes the overlay exactly as Esc
        /// does, and only handling Esc leaves a scan running after every pick.
        #[test]
        fn closing_the_picker_supersedes_the_scan_it_was_feeding() {
            let mut st = new_state_with("x\n");
            st.hire(crew_with_scan(EchoesSeal(Negai::Message("rows".into()))));
            st.honour_one(Negai::Errand(Box::new(a_scan())));

            st.close_picker();
            st.deliver();
            assert!(
                !st.messages.iter().any(|m| m == "rows"),
                "rows must not reopen a picker the operator closed: {:?}",
                st.messages
            );
        }

        /// The default state. An errand with nobody hired must report that it
        /// went nowhere — a request that silently does nothing is the exact
        /// failure the pre-courier stub had.
        #[test]
        fn an_errand_with_no_crew_hired_says_so() {
            let mut st = new_state_with("x\n");
            st.honour_one(Negai::Errand(Box::new(a_scan())));
            st.deliver();
            assert!(
                st.messages.iter().any(|m| m.contains("scan")),
                "the inert crew announces: {:?}",
                st.messages
            );
        }

        /// A quiet tick must be free — `deliver` is called on every frame.
        #[test]
        fn delivering_nothing_does_not_repaint() {
            let mut st = new_state_with("x\n");
            let before = st.edit_gen();
            st.deliver();
            assert_eq!(st.edit_gen(), before, "an empty drain is not a change");
        }

        /// …and a tick that DID deliver must repaint, or the result sits in
        /// state that nothing draws.
        #[test]
        fn delivering_something_repaints() {
            let mut st = new_state_with("x\n");
            st.hire(crew_with_scan(Says(Negai::Message("hi".into()))));
            st.honour_one(Negai::Errand(Box::new(a_scan())));
            let before = st.edit_gen();
            st.deliver();
            assert_ne!(st.edit_gen(), before, "a delivered reply repaints");
        }

        /// The two session kinds must not alias at the runtime level either: an
        /// LSP restart must not discard scan results, and vice versa.
        #[test]
        fn the_two_session_generations_are_independent() {
            let mut st = new_state_with("x\n");
            let scan_sealed = ResultList::new(
                vec![],
                Anchor::new().on(Axis::Session(SessionKind::Scan, st.scan_gen)),
            );
            st.bump_lsp_gen();
            assert!(
                !scan_sealed.is_stale(&st.world()),
                "an LSP restart must not discard scan results"
            );
            st.bump_scan_gen();
            assert!(scan_sealed.is_stale(&st.world()), "…but a scan bump does");
        }

        #[test]
        fn every_freight_class_seals_on_something() {
            let mut st = new_state_with("x\n");
            let active = st.active;
            for freight in [
                a_scan(),
                Freight::Diagnostics {
                    buffer: active,
                    path: "a.nix".into(),
                    language: None,
                    text: String::new(),
                },
                Freight::Format {
                    buffer: active,
                    path: "a.nix".into(),
                    language: None,
                    text: String::new(),
                },
            ] {
                let sealed = st.seal(&freight);
                assert!(
                    !sealed.as_anchor().is_empty(),
                    "{} sealed on nothing",
                    freight.label()
                );
            }
            let _ = &mut st;
        }
    }
}
