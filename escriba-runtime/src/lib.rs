//! `escriba-runtime` — editor state machine.
//!
//! Wraps everything: `BufferSet`, `ModalState`, `Keymap`, `CommandRegistry`,
//! `Layout`. Exposes `tick(input)` which advances one frame's worth of
//! state given one input event. Pure — no rendering, no I/O beyond file
//! save/load through `BufferSet`.

extern crate self as escriba_runtime;

use escriba_buffer::BufferSet;
use escriba_command::{CommandRegistry, EditContext};
use escriba_core::{Action, BufferId, Edit, Mode, Motion, Position, WindowId};
use escriba_input::{InputOutcome, translate_app_event};
use escriba_keymap::{Key, Keymap};
use escriba_mode::ModalState;
use escriba_ui::{Layout, Rect, Viewport, Window};
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
            }
            Action::Redo => {
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    let _ = buf.redo();
                }
            }
            Action::Save => {
                if let Some(buf) = self.buffers.get_mut(self.active) {
                    let _ = buf.save();
                }
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
        self.cursor = buf.clamp(pos);
        // Scroll viewport to contain the cursor.
        if let Some(w) = self
            .layout
            .windows
            .iter_mut()
            .find(|w| w.id == self.layout.active)
        {
            w.viewport = w.viewport.scroll_to_contain(self.cursor, 2);
        }
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
            self.cursor = if c == '\n' {
                Position::new(self.cursor.line.saturating_add(1), 0)
            } else {
                self.cursor.shift_right(1)
            };
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
}
