//! `escriba-runtime` — editor state machine.
//!
//! Wraps everything: `BufferSet`, `ModalState`, `Keymap`, `CommandRegistry`,
//! `Layout`. Exposes `tick(input)` which advances one frame's worth of
//! state given one input event. Pure — no rendering, no I/O beyond file
//! save/load through `BufferSet`.

extern crate self as escriba_runtime;

mod plugin_host;
pub use plugin_host::{LazyTrigger, PluginHost};

mod operator_pending;
pub mod status;

pub use operator_pending::{OpState, OperatorPending};
pub use status::{PromptKind, StatusModel};

use std::collections::HashMap;

use awase::KeyRepeatGate;
use escriba_buffer::BufferSet;
use escriba_buffer::TextRev;
use escriba_command::CommandRegistry;
use escriba_core::{
    Action, Anchored, Bound, BufferId, Cursors, Damage, Edit, EditGen, HighlightEffect, JumpList,
    Mode, Motion, Operator, Position, Range, TextEffect, WindowId,
};
use escriba_input::{InputOutcome, translate_app_event};
use escriba_keymap::{Key, Keymap};
use escriba_madoguchi::{Negai, Outcome};
use escriba_mode::ModalState;
use escriba_search::{Direction as SearchDirection, MatchCount, SearchState};
use escriba_ui::chrome::{ChromePalette, FleetTheme};
use escriba_ui::splash::Splash;
use escriba_ui::{Layout, Rect, Viewport, Window};
use escriba_vm::{EditorSnapshot, EscribaHost, EscribaVm, HostEffect, VmError};
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
    register: Option<String>,
    /// The operator-pending FSM (`d`/`c`/`y` then a motion → `dw`/`c$`/`y0`),
    /// standing on the fleet `zenmai` Mealy-machine primitive. Every dispatched
    /// action passes through it; only an operator-then-motion pair is rewritten
    /// into an [`Action::ApplyOperator`].
    op_pending: zenmai::Stateful<OperatorPending>,
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
    /// The start screen, while it is up.
    ///
    /// `Some` only between boot and the first keypress, and only when the
    /// editor opened with no file. It is deliberately NOT a `Mode`: a mode
    /// is a state keys are interpreted *in*, and the splash interprets
    /// exactly one key before it is gone. Modelling it as `Option<Splash>`
    /// keeps the modal state machine's variant set — and every exhaustive
    /// match over it — untouched.
    splash: Option<Splash>,
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

    /// Apply one slip. Total over `Negai`: a new request variant is a
    /// compile error here rather than a request that is silently ignored,
    /// which is the same failure Phase 0 removed one layer up.
    fn honour(&mut self, slip: Negai) {
        let touches_text = slip.touches_text();
        match slip {
            Negai::Edit { buffer, edit } => {
                if let Some(b) = self.buffers.get_mut(buffer) {
                    let _ = b.apply(&edit);
                }
            }
            Negai::SetCursor { buffer, to } => {
                // Clamping is the interpreter's job, exactly so that no
                // handler has to re-implement it and get it wrong.
                let clamped = self.buffers.get(buffer).map_or(to, |b| b.clamp(to));
                self.set_cursor(clamped);
            }
            Negai::EnterMode(m) => self.modal.enter(m),
            Negai::FocusBuffer(id) => {
                if self.buffers.get(id).is_some() {
                    self.active = id;
                }
            }
            Negai::OpenPath(path) => match self.buffers.open(&path) {
                Ok(id) => self.active = id,
                Err(e) => self.messages.push(e.to_string()),
            },
            Negai::CloseBuffer(_) => {
                // BufferSet has no close() yet; M5 lands it with
                // buffer.delete. Announced rather than silently ignored.
                self.messages
                    .push("closing buffers is not implemented yet".to_string());
            }
            Negai::Save { buffer } => {
                if let Some(b) = self.buffers.get_mut(buffer) {
                    if let Err(e) = b.save() {
                        self.messages.push(e.to_string());
                    }
                }
            }
            Negai::Undo { buffer } => {
                if let Some(b) = self.buffers.get_mut(buffer) {
                    let _ = b.undo();
                }
            }
            Negai::Redo { buffer } => {
                if let Some(b) = self.buffers.get_mut(buffer) {
                    let _ = b.redo();
                }
            }
            Negai::Yank { text, .. } => self.register = Some(text),
            Negai::ClearSearchHighlight => self.search.clear_highlight(),
            Negai::Message(m) => self.messages.push(m),
            Negai::Quit => self.quit_requested = true,
            // Both suspend the dispatch and need machinery that does not
            // exist yet — the courier (Phase 5) and the AwaitKey resume
            // (M3). Announced, never silently dropped: a slip that vanishes
            // is the class Phase 0 sealed.
            Negai::Errand(_) | Negai::AwaitKey { .. } => {
                self.messages
                    .push("deferred work is not wired yet".to_string());
            }
        }
        self.damage = self.damage.join(if touches_text {
            Damage::Full
        } else {
            Damage::Viewport
        });
        self.bump_gen();
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
            rect: Rect {
                x: 0,
                y: 0,
                width: 1200,
                height: 800,
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
            theme: FleetTheme::prescribed_default(),
            chrome: ChromePalette::prescribed(),
            splash: None,
        }
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
            InputOutcome::Resized { width, height } => {
                if let Some(w) = self
                    .layout
                    .windows
                    .iter_mut()
                    .find(|w| w.id == self.layout.active)
                {
                    w.rect.width = width;
                    w.rect.height = height;
                }
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
        // The start screen owns the first keypress and nothing after it.
        match self.consume_splash_key(key) {
            SplashKey::NotShowing | SplashKey::Dismissed => {}
            SplashKey::Ran(action) => {
                self.apply(&action);
                return;
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
    fn set_cursor(&mut self, pos: Position) {
        let clamped = if let Some(buf) = self.buffers.get(self.active) {
            buf.clamp(pos)
        } else {
            pos
        };
        self.cursors.set_primary(clamped);
        if let Some(w) = self
            .layout
            .windows
            .iter_mut()
            .find(|w| w.id == self.layout.active)
        {
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

        for (resolved, times) in self.op_pending.dispatch((action.clone(), count)) {
            for _ in 0..times {
                self.apply_resolved(&resolved);
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

        for _ in 0..change.count.max(1) {
            self.apply_resolved(&change.action);
        }
        for c in change.inserted.chars() {
            self.apply_resolved(&Action::InsertChar(c));
        }
        if self.modal.mode() == Mode::Insert {
            // A replayed change must not leave the editor in Insert — the
            // original ended with an Esc the recording deliberately does not
            // store, since it is punctuation rather than part of the change.
            self.apply_resolved(&Action::ChangeMode(Mode::Normal));
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
    fn resolve_object(&self, object: escriba_core::TextObject) -> Option<Range> {
        use escriba_core::TextObject as O;
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
                O::NextMatch => Bound::Inclusive.first_matching(&starts, at, true),
                O::PrevMatch => Bound::Inclusive.first_matching(&starts, at, false),
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
        if let Some(msg) = step.wrapped.message() {
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
        self.jumps.push(self.cursor());
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
                    self.jumps.push(from);
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
                    self.jumps.push(from);
                    self.set_cursor(from);
                    self.apply_operator_to(op, target);
                }
            }
            CommitOutcome::NotFound | CommitOutcome::NoPrevious | CommitOutcome::NoPrompt => {}
        }
    }

    fn apply_resolved(&mut self, action: &Action) {
        // Snapshot the scope inputs before the mutation so the resulting
        // Damage covers the changed region (the S3 seal — conservative widen).
        let lines_before = self.active_line_count();
        // Snapshot for the dot register: the only reliable witness that this
        // action changed text is that the buffer's revision moved.
        let rev_before = self.text_rev();
        let cline_before = self.cursor().line;
        match action {
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
                self.jumps.push(self.cursor());
                match self.search.search_word(&text, at, dir) {
                    Some(step) => self.land_on(step),
                    // vim beeps and stays put when there is no word under the
                    // cursor; a silent no-op would look like a broken key.
                    None => self
                        .messages
                        .push("E348: No string under cursor".to_string()),
                }
            }
            Action::ClearSearchHighlight => self.search.clear_highlight(),
            Action::SearchSubmitOperated { op } => self.submit_search_operated(*op),
            Action::TextObject(object) => {
                // Bare `gn` moves onto the match. vim additionally starts a
                // Visual selection of it; escriba's Visual plumbing does not
                // carry a selection an operator can consume yet, so this
                // stops at the jump rather than faking a selection that
                // nothing would honour.
                if let Some(range) = self.resolve_object(*object) {
                    self.jumps.push(self.cursor());
                    self.set_cursor(range.start);
                } else {
                    self.report_pattern_not_found();
                }
            }
            Action::ApplyOperatorObject { op, object } => match self.resolve_object(*object) {
                Some(range) => self.apply_operator_over(*op, range),
                None => self.report_pattern_not_found(),
            },
            Action::RepeatLastChange => self.repeat_last_change(),
            Action::JumpBack => {
                let here = self.cursor();
                if let Some(pos) = self.jumps.back(here) {
                    self.set_cursor(pos);
                } else {
                    self.messages
                        .push("E662: At start of changelist".to_string());
                }
            }
            Action::JumpForward => {
                if let Some(pos) = self.jumps.forward() {
                    self.set_cursor(pos);
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
            Action::InsertChar(c) => self.insert_char(*c),
            Action::Edit(edit) => self.apply_edit(edit),
            Action::Undo => {
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    let _ = buf.undo();
                }
                // The buffer may have shrunk — re-follow so the viewport
                // re-contains a now-out-of-bounds cursor.
                self.set_cursor(self.cursor());
            }
            Action::Redo => {
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    let _ = buf.redo();
                }
                self.set_cursor(self.cursor());
            }
            Action::Save => {
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    let _ = buf.save();
                }
                self.set_cursor(self.cursor());
            }
            Action::Quit => self.quit_requested = true,
            Action::SubmitCommand => {
                if self.search.is_prompting() {
                    self.submit_search();
                } else {
                    self.submit_command();
                }
            }
            Action::Command { name, args } => self.run_command(name, args),
            Action::ApplyOperator { op, motion } => self.apply_operator(*op, *motion),
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
            Action::PromptDelete => {
                if self.search.is_prompting() {
                    self.search.delete_at_caret();
                    self.preview_search();
                } else {
                    self.modal.delete_minibuffer_at_caret();
                }
            }
            Action::PromptDeleteWord => {
                if self.search.is_prompting() {
                    self.search.delete_word_before_caret();
                    self.preview_search();
                }
            }
            Action::PromptClearToStart => {
                if self.search.is_prompting() {
                    self.search.clear_before_caret();
                    self.preview_search();
                }
            }
            Action::PromptBackspace => {
                self.prompt_backspace();
                // Shortening the pattern changes which matches exist, so the
                // preview must re-run — otherwise the cursor sits on a match
                // of a pattern that is no longer typed.
                if self.search.is_prompting() {
                    self.preview_search();
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
            | Action::PromptBackspace
            | Action::PromptCaret { .. }
            | Action::SearchPreviewStep { .. }
            | Action::PromptDelete
            | Action::PromptDeleteWord
            | Action::PromptClearToStart
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
            | Action::JumpForward => Damage::Full,
            Action::InsertChar(_)
            | Action::Edit(_)
            | Action::Undo
            | Action::Redo
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
            Action::Quit | Action::Operator(_) | Action::Pending => Damage::None,
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
                action: action.clone(),
                count: 1,
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
            Motion::Right => Position::new(pos.line, pos.column.saturating_add(1)),
            Motion::Up => Position::new(pos.line.saturating_sub(1), pos.column),
            Motion::Down => Position::new(pos.line.saturating_add(1), pos.column),
            Motion::LineStart => Position::new(pos.line, 0),
            Motion::LineEnd => Position::new(pos.line, buf.line_len_chars(pos.line)),
            Motion::LineFirstNonBlank => first_non_blank(buf, pos.line),
            Motion::DocStart => Position::ZERO,
            Motion::DocEnd => Position::new(
                buf.line_count().saturating_sub(1),
                buf.line_len_chars(buf.line_count().saturating_sub(1)),
            ),
            Motion::WordStartNext | Motion::WordEndNext => word_next(buf, pos),
            Motion::WordStartPrev => word_prev(buf, pos),
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
        self.apply_operator_to(op, to);
    }

    /// Apply `op` over `[cursor, to)`.
    ///
    /// Split out of [`Self::apply_operator`] so the operated-search path can
    /// reach the same range machinery with a target it resolved itself — the
    /// alternative was a second copy of the delete/yank/register logic, which
    /// is how the two would drift.
    fn apply_operator_to(&mut self, op: Operator, to: Position) {
        let from = self.cursor();
        self.apply_operator_over(
            op,
            Range {
                start: from,
                end: to,
            },
        );
    }

    /// Apply `op` over an explicit range.
    ///
    /// The object path needs this: `gn`'s extent need not begin at the cursor,
    /// so it cannot go through the `[cursor, target)` shape the motion path
    /// uses. One implementation of the delete/yank/register logic, reached two
    /// ways.
    fn apply_operator_over(&mut self, op: Operator, range: Range) {
        let range = range.normalized();
        if range.is_empty() {
            return;
        }
        // Capture the operated text (for the register) before mutating.
        let text = self
            .buffers
            .get(self.active)
            .and_then(|buf| buf.slice(range).ok());
        if op.leaves_register() {
            if let Some(t) = &text {
                self.register = Some(t.clone());
            }
        }
        match op {
            // Delete + Change remove the range; Change then enters Insert so
            // the operator pairs with immediate typing (`ciw`, `c$`).
            Operator::Delete | Operator::Change => {
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    let _ = buf.apply(&Edit::delete(range));
                }
                self.set_cursor(range.start);
                if op == Operator::Change {
                    self.modal.enter(Mode::Insert);
                }
            }
            // Yank copies to the register without mutating the buffer; vim
            // leaves the cursor at the range start.
            Operator::Yank => {
                self.set_cursor(range.start);
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

    /// The text last yanked or deleted into the unnamed register, if any.
    /// The future `p`/`P` paste reads this.
    #[must_use]
    pub fn register(&self) -> Option<&str> {
        self.register.as_deref()
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
            self.set_cursor(next);
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

    fn apply_edit(&mut self, _edit: &Edit) {
        // Phase 2: actually apply arbitrary edits from the keymap. For now
        // the only keymap-originated edits are InsertChar (handled above)
        // and the Backspace sentinel that escriba-keymap emits.
    }

    fn submit_command(&mut self) {
        // Read the command line BEFORE leaving Command mode — the minibuffer
        // exists only in the `Command` variant, so the escape must come
        // after the capture.
        let line = self.modal.minibuffer().to_string();
        self.modal.escape();
        let (name, args) = parse_command_line(&line);
        if name.is_empty() {
            return;
        }
        self.run_command(&name, &args);
    }

    fn run_command(&mut self, name: &str, args: &[String]) {
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

    /// Apply tatara-lisp [`HostEffect`]s to live editor state. The
    /// single seam where Lisp-requested mutations land — extend here +
    /// in `escriba-vm` to add a capability.
    pub fn apply_host_effects(&mut self, effects: Vec<HostEffect>) {
        for eff in effects {
            match eff {
                HostEffect::Message(m) => self.messages.push(m),
                HostEffect::RunCommand { name, args } => self.run_command(&name, &args),
                HostEffect::SetOption { name, value } => {
                    self.options.insert(name, value);
                }
                HostEffect::InsertText(text) => self.insert_text(&text),
            }
        }
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
            self.set_cursor(next);
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

fn word_next(buf: &escriba_buffer::Buffer, pos: Position) -> Position {
    let Some(text) = buf.line(pos.line) else {
        return pos;
    };
    let chars: Vec<char> = text.chars().collect();
    let start = pos.column as usize;
    let mut i = start;
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() {
        // No more words on this line — jump to next line.
        if pos.line + 1 < buf.line_count() {
            return Position::new(pos.line + 1, 0);
        }
    }
    Position::new(pos.line, u32::try_from(i).unwrap_or(pos.column))
}

fn word_prev(buf: &escriba_buffer::Buffer, pos: Position) -> Position {
    let Some(text) = buf.line(pos.line) else {
        return pos;
    };
    let chars: Vec<char> = text.chars().collect();
    let mut i = (pos.column as usize).min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    Position::new(pos.line, u32::try_from(i).unwrap_or(0))
}

fn parse_command_line(line: &str) -> (String, Vec<String>) {
    let mut parts = line.split_whitespace();
    let Some(first) = parts.next() else {
        return (String::new(), Vec::new());
    };
    let head = first.strip_prefix(':').unwrap_or(first);
    let name = match head {
        "w" => "save",
        "q" => "quit",
        "u" => "undo",
        other => other,
    };
    (name.to_string(), parts.map(str::to_string).collect())
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
        st.apply(&Action::PromptBackspace);
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
        st.apply(&Action::PromptBackspace);
        st.apply(&Action::PromptBackspace);
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
        st.apply(&Action::PromptBackspace);
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

    fn new_state_with(text: &str) -> EditorState {
        let mut bufs = BufferSet::new();
        let id = bufs.scratch(text);
        EditorState::new_with_buffer(bufs, id)
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
        for w in &mut s.layout.windows {
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
            s.register(),
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
        assert_eq!(s.register(), Some("a"));
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
            s.register(),
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
        assert_eq!(s.register(), Some("hello world"), "yank fills the register");
        assert_eq!(s.modal.mode(), Mode::Normal, "yank stays in Normal");
    }

    #[test]
    fn resolve_motion_is_the_shared_target_for_move_and_operator() {
        // The encapsulation proof: apply_motion (cursor move) and
        // apply_operator (range end) BOTH stand on resolve_motion — so a move
        // to LineEnd lands at exactly the position the operator deletes to.
        let mut s = new_state_with("hello world");
        let target = s.resolve_motion(Position::ZERO, Motion::LineEnd).unwrap();
        assert_eq!(target, Position::new(0, 11));
        s.apply_motion(Motion::LineEnd);
        assert_eq!(
            s.cursor(),
            target,
            "the move path resolves the same target the operator uses"
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
        assert_eq!(s.register(), None);
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
        assert_eq!(s.register(), Some("hello world"));
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
        // Without a preceding operator the motion passes through unchanged.
        let mut s = new_state_with("hello world");
        s.apply(&Action::Move(Motion::LineEnd));
        assert_eq!(s.cursor(), Position::new(0, 11));
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
        s.apply_host_effects(vec![HostEffect::InsertText("foo\nbar".to_string())]);
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
}
