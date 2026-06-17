//! `escriba-runtime` — editor state machine.
//!
//! Wraps everything: `BufferSet`, `ModalState`, `Keymap`, `CommandRegistry`,
//! `Layout`. Exposes `tick(input)` which advances one frame's worth of
//! state given one input event. Pure — no rendering, no I/O beyond file
//! save/load through `BufferSet`.

extern crate self as escriba_runtime;

use std::collections::HashMap;

use escriba_buffer::BufferSet;
use escriba_command::{CommandRegistry, EditContext};
use escriba_core::{Action, BufferId, Edit, Mode, Motion, Position, WindowId};
use escriba_input::{InputOutcome, translate_app_event};
use escriba_keymap::{Key, Keymap};
use escriba_mode::ModalState;
use escriba_ui::{Layout, Rect, Viewport, Window};
use escriba_vm::{EditorSnapshot, EscribaHost, EscribaVm, HostEffect, VmError};
use madori::AppEvent;

/// Full editor state — the single Rust value the binary hands to the
/// renderer each frame.
pub struct EditorState {
    pub buffers: BufferSet,
    pub modal: ModalState,
    pub keymap: Keymap,
    pub commands: CommandRegistry,
    pub layout: Layout,
    pub active: BufferId,
    pub cursor: Position,
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
            cursor: Position::ZERO,
            quit_requested: false,
            messages: Vec::new(),
            options: HashMap::new(),
            lisp_vm: None,
            pending_keys: Vec::new(),
        }
    }

    /// Advance one frame's worth of state given a raw madori event.
    pub fn tick(&mut self, event: &AppEvent) {
        match translate_app_event(event) {
            InputOutcome::Key(k) => self.on_key(&k),
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

    /// Dispatch a single key through the keymap + apply the resulting action.
    pub fn on_key(&mut self, key: &Key) {
        // Multi-key sequence resolution runs first: a key that begins or
        // continues a bound sequence (`<leader>ff`, `gg`) is held or
        // resolved here before the single-key path sees it.
        match self.step_sequence(key) {
            SeqStep::Pending => return,
            SeqStep::Resolved(action) => {
                let count = self.modal.pending_count.unwrap_or(1);
                self.modal.pending_count = None;
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
        self.modal.pending_count = None;
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
        if !matches!(
            self.modal.mode,
            Mode::Normal | Mode::Visual | Mode::VisualLine
        ) {
            return SeqStep::Passthrough;
        }
        if !self.pending_keys.is_empty() {
            let mut seq = self.pending_keys.clone();
            seq.push(key.clone());
            if let Some(b) = self.keymap.lookup_sequence(self.modal.mode, &seq) {
                let action = b.action.clone();
                self.pending_keys.clear();
                return SeqStep::Resolved(action);
            }
            if self.keymap.is_sequence_prefix(self.modal.mode, &seq) {
                self.pending_keys = seq;
                return SeqStep::Pending;
            }
            // The key broke the in-progress sequence — abort it and let
            // the key be re-processed as a fresh stroke below.
            self.pending_keys.clear();
        }
        let start = [key.clone()];
        if self.keymap.is_sequence_prefix(self.modal.mode, &start)
            && self.keymap.lookup(self.modal.mode, key).is_none()
        {
            self.pending_keys = start.to_vec();
            return SeqStep::Pending;
        }
        SeqStep::Passthrough
    }

    /// The **single** cursor-mutation path. Clamp the requested position to
    /// the active buffer's bounds, then scroll the active window's viewport
    /// to contain it on BOTH axes. Routing every cursor change through this
    /// makes "cursor outside its viewport" an unrepresentable state: there
    /// is no code path that advances the cursor without re-deriving the
    /// viewport from it.
    fn set_cursor(&mut self, pos: Position) {
        self.cursor = if let Some(buf) = self.buffers.get(self.active) {
            buf.clamp(pos)
        } else {
            pos
        };
        if let Some(w) = self
            .layout
            .windows
            .iter_mut()
            .find(|w| w.id == self.layout.active)
        {
            w.viewport = w.viewport.scroll_to_contain(self.cursor, 2);
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
                self.set_cursor(self.cursor);
            }
            Action::Redo => {
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    let _ = buf.redo();
                }
                self.set_cursor(self.cursor);
            }
            Action::Save => {
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    let _ = buf.save();
                }
                self.set_cursor(self.cursor);
            }
            Action::Quit => self.quit_requested = true,
            Action::SubmitCommand => self.submit_command(),
            Action::Command { name, args } => self.run_command(name, args),
            Action::ApplyOperator { .. } => {
                // Phase 2: operator-over-motion composition.
            }
            Action::Pending => {}
        }
    }

    fn apply_motion(&mut self, motion: Motion) {
        let Some(buf) = self.buffers.get(self.active) else {
            return;
        };
        let mut pos = self.cursor;
        pos = match motion {
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
        };
        // The single cursor-mutation path clamps to the buffer and scrolls
        // the viewport to contain the cursor on both axes.
        self.set_cursor(pos);
    }

    fn insert_char(&mut self, c: char) {
        if self.modal.mode == Mode::Command {
            self.modal.push_minibuffer(c);
            return;
        }
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        let edit = Edit::insert(self.cursor, c.to_string());
        if buf.apply(&edit).is_ok() {
            let next = if c == '\n' {
                Position::new(self.cursor.line.saturating_add(1), 0)
            } else {
                self.cursor.shift_right(1)
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
        let line = self.modal.minibuffer.clone();
        self.modal.enter(Mode::Normal);
        let (name, args) = parse_command_line(&line);
        if name.is_empty() {
            return;
        }
        self.run_command(&name, &args);
    }

    fn run_command(&mut self, name: &str, args: &[String]) {
        let active = Some(self.active);
        let mut ctx = EditContext {
            buffers: &mut self.buffers,
            active,
            state: &mut self.modal,
        };
        let _ = self.commands.run(name, &mut ctx, args);
        if self.modal.minibuffer.contains("__quit__") {
            self.quit_requested = true;
            self.modal.minibuffer.clear();
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
            .and_then(|b| b.line(self.cursor.line))
            .map(|s| s.trim_end_matches('\n').to_string())
            .unwrap_or_default();
        let buffer_name = self
            .buffers
            .get(self.active)
            .and_then(|b| b.path.as_ref())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "[scratch]".to_string());
        EditorSnapshot {
            cursor_line: i64::from(self.cursor.line),
            cursor_column: i64::from(self.cursor.column),
            current_line,
            mode: self.modal.mode.as_str().to_string(),
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
        let Some(buf) = self.buffers.get_mut(self.active) else {
            return;
        };
        let edit = Edit::insert(self.cursor, text.to_string());
        if buf.apply(&edit).is_ok() {
            let next = if let Some(nl) = text.rfind('\n') {
                let added_lines = u32::try_from(text.matches('\n').count()).unwrap_or(0);
                let last_line_len = u32::try_from(text[nl + 1..].chars().count()).unwrap_or(0);
                Position::new(self.cursor.line + added_lines, last_line_len)
            } else {
                let n = u32::try_from(text.chars().count()).unwrap_or(0);
                self.cursor.shift_right(n)
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
        let c = s.cursor;
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

    #[test]
    fn hjkl_moves_cursor() {
        let mut s = new_state_with("hello\nworld");
        s.tick(&press(KeyCode::Char('l')));
        assert_eq!(s.cursor.column, 1);
        s.tick(&press(KeyCode::Char('j')));
        assert_eq!(s.cursor.line, 1);
        s.tick(&press(KeyCode::Char('h')));
        assert_eq!(s.cursor.column, 0);
    }

    #[test]
    fn insert_mode_inserts_chars() {
        let mut s = new_state_with("");
        s.tick(&press(KeyCode::Char('i')));
        assert_eq!(s.modal.mode, Mode::Insert);
        s.tick(&press(KeyCode::Char('h')));
        s.tick(&press(KeyCode::Char('i')));
        assert_eq!(s.buffers.get(s.active).unwrap().to_string(), "hi");
        assert_eq!(s.cursor.column, 2);
    }

    #[test]
    fn esc_returns_to_normal() {
        let mut s = new_state_with("");
        s.tick(&press(KeyCode::Char('i')));
        s.tick(&press(KeyCode::Escape));
        assert_eq!(s.modal.mode, Mode::Normal);
    }

    #[test]
    fn count_prefix_repeats_motion() {
        let mut s = new_state_with("abcdefghij");
        s.tick(&press(KeyCode::Char('5')));
        s.tick(&press(KeyCode::Char('l')));
        assert_eq!(s.cursor.column, 5);
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
        s.tick(&press(KeyCode::Char('w')));
        assert_eq!(s.cursor.column, 4);
        s.tick(&press(KeyCode::Char('w')));
        assert_eq!(s.cursor.column, 8);
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
        assert_eq!(s.cursor, Position::ZERO);
        // `g` completes `<leader>g` → DocEnd; pending clears.
        s.on_key(&Key::Char('g'));
        assert!(s.pending_keys.is_empty());
        assert_eq!(s.cursor.line, 2);
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
        s.tick(&press(KeyCode::Char('j')));
        s.tick(&press(KeyCode::Char('j')));
        assert_eq!(s.cursor.line, 2);
        s.on_key(&Key::Char('g')); // pending
        assert_eq!(s.pending_keys, vec![Key::Char('g')]);
        s.on_key(&Key::Char('g')); // resolve
        assert_eq!(s.cursor, Position::ZERO);
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
        assert_eq!(s.cursor, Position::ZERO);
    }

    #[test]
    fn single_binding_wins_over_sequence_prefix() {
        // A key that is BOTH a complete single binding and the start of
        // a sequence fires the single binding immediately (no chord
        // timeout needed). Here `h` (move-left) also prefixes `hz`.
        let mut s = new_state_with("abcde");
        s.tick(&press(KeyCode::Char('l')));
        s.tick(&press(KeyCode::Char('l')));
        assert_eq!(s.cursor.column, 2);
        s.keymap.bind_sequence(
            Mode::Normal,
            vec![Key::Char('h'), Key::Char('z')],
            Action::Move(Motion::DocEnd),
            "shadowed",
        );
        s.on_key(&Key::Char('h'));
        assert!(s.pending_keys.is_empty(), "single binding should not pend");
        assert_eq!(s.cursor.column, 1, "h moved left immediately");
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
        assert_eq!(s.cursor, Position::new(0, 3));
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
    fn lisp_run_command_quit_sets_quit_requested_and_clears_sentinel() {
        // The full imperative-quit path: (run-command "quit") routes
        // through the registry's __quit__ sentinel handshake.
        let mut s = new_state_with("");
        s.run_lisp(r#"(run-command "quit")"#).unwrap();
        assert!(s.quit_requested, "lisp-driven quit must set quit_requested");
        assert!(
            !s.modal.minibuffer.contains("__quit__"),
            "sentinel must be cleared so it can't re-trigger or pollute a command line",
        );
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
        assert_eq!(s.cursor, Position::new(1, 3));
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
        assert_eq!(s.cursor.column, 3, "ge resolved to doc-end in visual mode");
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
            s.cursor.column, 1,
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
        assert_eq!(s.modal.mode, Mode::Insert);
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
        assert_eq!(s.modal.mode, Mode::Normal);
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
        let clamped = buf.clamp(s.cursor);
        assert_eq!(s.cursor, clamped, "cursor must be clamped in-bounds at EOF");
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
        assert_eq!(s.cursor.line, 2, "count 2 should repeat the gj motion");
    }
}
