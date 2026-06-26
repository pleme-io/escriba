//! `escriba-runtime` — editor state machine.
//!
//! Wraps everything: `BufferSet`, `ModalState`, `Keymap`, `CommandRegistry`,
//! `Layout`. Exposes `tick(input)` which advances one frame's worth of
//! state given one input event. Pure — no rendering, no I/O beyond file
//! save/load through `BufferSet`.

extern crate self as escriba_runtime;

mod plugin_host;
pub use plugin_host::{LazyTrigger, PluginHost};

use std::collections::HashMap;

use escriba_buffer::BufferSet;
use escriba_command::{CommandRegistry, EditContext};
use escriba_core::{
    Action, BufferId, Cursors, Edit, Mode, Motion, Operator, Position, Range, WindowId,
};
use escriba_input::{InputOutcome, translate_app_event};
use escriba_keymap::{Key, Keymap};
use escriba_mode::ModalState;
use escriba_ui::{Layout, Rect, Viewport, Window};
use escriba_vm::{EditorSnapshot, EscribaHost, EscribaVm, HostEffect, VmError};
use awase::KeyRepeatGate;
use madori::AppEvent;
use std::time::Instant;

/// Full editor state — the single Rust value the binary hands to the
/// renderer each frame.
pub struct EditorState {
    pub buffers: BufferSet,
    pub modal: ModalState,
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
            keymap: Keymap::default_vim(),
            commands: CommandRegistry::default_set(),
            layout: Layout::single(window),
            active,
            cursors: Cursors::single(Position::ZERO),
            quit_requested: false,
            register: None,
            messages: Vec::new(),
            options: HashMap::new(),
            lisp_vm: None,
            pending_keys: Vec::new(),
            repeat_gate: KeyRepeatGate::new(),
            plugin_host: PluginHost::default(),
        }
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
                self.repeat_gate.try_pass_at(*key, now)
            }
            Mode::Insert | Mode::Command => true,
        }
    }

    /// Dispatch a single key through the keymap + apply the resulting action.
    pub fn on_key(&mut self, key: &Key) {
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
        for _ in 0..counted.count {
            self.apply(&counted.action);
            if self.quit_requested {
                return;
            }
        }
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
        if self.keymap.is_sequence_prefix(mode, &start)
            && self.keymap.lookup(mode, key).is_none()
        {
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

    fn apply(&mut self, action: &Action) {
        match action {
            Action::Move(m) => self.apply_motion(*m),
            Action::ChangeMode(m) => self.modal.enter(*m),
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
            Action::SubmitCommand => self.submit_command(),
            Action::Command { name, args } => self.run_command(name, args),
            Action::ApplyOperator { op, motion } => self.apply_operator(*op, *motion),
            Action::Pending => {}
        }
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
            return;
        };
        let range = Range { start: from, end: to }.normalized();
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
            self.modal.push_minibuffer(c);
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
        let active = Some(self.active);
        let mut quit = false;
        {
            let mut ctx = EditContext {
                buffers: &mut self.buffers,
                active,
                state: &mut self.modal,
                quit_requested: &mut quit,
            };
            let _ = self.commands.run(name, &mut ctx, args);
        }
        // The command's typed quit signal — no string sentinel, no
        // mode-specific buffer to clear.
        if quit {
            self.quit_requested = true;
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

    fn new_state_with(text: &str) -> EditorState {
        let mut bufs = BufferSet::new();
        let id = bufs.scratch(text);
        EditorState::new_with_buffer(bufs, id)
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
        s.apply(&Action::ApplyOperator { op: Operator::Delete, motion: Motion::LineEnd });
        assert_eq!(line0_len(&s), 0, "d$ deletes to end of line");
        assert_eq!(s.register(), Some("hello world"), "delete fills the register");
        assert_eq!(s.cursor(), Position::ZERO, "cursor lands at the range start");
    }

    #[test]
    fn delete_over_right_motion_removes_one_char() {
        let mut s = new_state_with("abc");
        s.apply(&Action::ApplyOperator { op: Operator::Delete, motion: Motion::Right });
        assert_eq!(s.buffers.get(s.active).unwrap().line(0).as_deref(), Some("bc"));
        assert_eq!(s.register(), Some("a"));
    }

    #[test]
    fn change_to_line_end_deletes_and_enters_insert() {
        let mut s = new_state_with("hello world");
        assert_eq!(s.modal.mode(), Mode::Normal);
        s.apply(&Action::ApplyOperator { op: Operator::Change, motion: Motion::LineEnd });
        assert_eq!(line0_len(&s), 0, "c$ deletes the range");
        assert_eq!(s.modal.mode(), Mode::Insert, "change enters Insert to type the replacement");
        assert_eq!(s.register(), Some("hello world"), "change fills the register");
    }

    #[test]
    fn yank_to_line_end_fills_register_without_mutating() {
        let mut s = new_state_with("hello world");
        s.apply(&Action::ApplyOperator { op: Operator::Yank, motion: Motion::LineEnd });
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
        assert_eq!(s.cursor(), target, "the move path resolves the same target the operator uses");
    }

    #[test]
    fn empty_motion_range_is_a_no_op() {
        // An operator over a zero-width motion (cursor already at line start)
        // mutates nothing and leaves the register untouched.
        let mut s = new_state_with("abc");
        s.apply(&Action::ApplyOperator { op: Operator::Delete, motion: Motion::LineStart });
        assert_eq!(s.buffers.get(s.active).unwrap().line(0).as_deref(), Some("abc"));
        assert_eq!(s.register(), None);
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
        assert!(s.options.get("lazy-loaded").is_none(), "entry not applied yet");

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
        assert!(s.lisp_vm.is_some(), "VM should be cached after first run_lisp");
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
        s.run_lisp(
            r#"(set-option "col2" (if (= (cursor-column) 2) "live-two" "other"))"#,
        )
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
        assert_eq!(s.cursor().column, 3, "ge resolved to doc-end in visual mode");
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
            s.cursor().column, 1,
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
        assert_eq!(s.cursor(), clamped, "cursor must be clamped in-bounds at EOF");
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
        s.tick_at(&press(KeyCode::Char('j')), t + std::time::Duration::from_millis(10));
        assert_eq!(s.cursor().line, 1, "second `j` within window dropped");
        // …but `l` at the same instant passes (its own clock).
        s.tick_at(&press(KeyCode::Char('l')), t + std::time::Duration::from_millis(10));
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
