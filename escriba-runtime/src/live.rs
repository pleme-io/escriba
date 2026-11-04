use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use escriba_buffer::TextRev;
use escriba_core::{
    BufferId, CompletionEntry, Damage, Edit, InlayHint, Mode, Position, Range, ServerState,
    ServerStatus, TextChange,
};
use escriba_keymap::Key;
use escriba_madoguchi::errand::{Document, Freight, Question, SessionFreight};
use escriba_shirube::{Anchor, Severity};
use escriba_ui::completion::{CompletionPopup, is_word_char, word_start};
use escriba_ui::float::{FloatPanel, FloatRow};

use crate::{CursorRest, EditorState};

const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(80);
const DEFAULT_KEYWORD_LENGTH: u32 = 2;
const DEFAULT_MAX_DOCUMENT_CHARS: usize = 8 * 1024 * 1024;
const QUICK_WAKE_AFTER_SEND: Duration = Duration::from_secs(2);
const HOVER_ROWS: usize = 14;
const HOVER_COLUMNS: usize = 72;

#[derive(Debug, Clone)]
struct Seen {
    document: Document,
    rev: TextRev,
    synced: Option<TextRev>,
    dirty_since: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct Want {
    buffer: BufferId,
    manual: bool,
}

#[derive(Debug, Clone)]
struct Hints {
    hints: Vec<InlayHint>,
    anchor: Anchor,
}

#[derive(Debug, Default)]
pub(crate) struct LiveSession {
    seen: HashMap<BufferId, Seen>,
    want: Option<Want>,
    popup: Option<CompletionPopup>,
    hover: Option<Vec<String>>,
    hints: HashMap<BufferId, Hints>,
    servers: BTreeMap<(String, PathBuf), ServerStatus>,
    triggers: HashMap<String, (bool, Vec<char>)>,
    summary: String,
    last_send: Option<Instant>,
    too_large: HashSet<BufferId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationRole {
    Value,
    Diagnostic(Severity),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub text: String,
    pub role: AnnotationRole,
}

pub(crate) struct BeforeKey {
    buffer: BufferId,
    rev: Option<TextRev>,
}

fn truthy(value: Option<&String>, absent: bool) -> bool {
    value.map_or(absent, |v| {
        !matches!(v.as_str(), "false" | "0" | "off" | "no")
    })
}

impl EditorState {
    #[must_use]
    pub fn session_on(&self) -> bool {
        self.courier.carries_sessions() && truthy(self.options.get("lsp.session"), false)
    }

    fn completion_on(&self) -> bool {
        self.session_on()
            && truthy(self.options.get("cmp.enabled"), true)
            && self
                .options
                .get("cmp.sources")
                .is_none_or(|s| s.split(',').any(|source| source.trim() == "lsp"))
    }

    fn debounce(&self) -> Duration {
        self.options
            .get("lsp.debounce_ms")
            .and_then(|v| v.parse().ok())
            .map_or(DEFAULT_DEBOUNCE, Duration::from_millis)
    }

    fn max_document_chars(&self) -> usize {
        self.options
            .get("lsp.max_document_chars")
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_DOCUMENT_CHARS)
    }

    fn refuse_large_document(&mut self, id: BufferId) {
        if self.live.too_large.insert(id) {
            self.messages
                .push("lsp: this file is over lsp.max_document_chars; no live session".to_string());
        }
        if let Some(seen) = self.live.seen.remove(&id) {
            self.send_session(SessionFreight::Close {
                document: seen.document,
            });
        }
    }

    fn keyword_length(&self) -> u32 {
        self.options
            .get("cmp.keyword_length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_KEYWORD_LENGTH)
    }

    #[must_use]
    pub fn completion_popup(&self) -> Option<&CompletionPopup> {
        self.live.popup.as_ref()
    }

    #[must_use]
    pub fn hover_text(&self) -> Option<&[String]> {
        self.live.hover.as_deref()
    }

    #[must_use]
    pub fn float_panel(&self) -> Option<FloatPanel> {
        if let Some(popup) = &self.live.popup {
            return Some(popup.panel());
        }
        self.live.hover.as_ref().map(|lines| FloatPanel {
            rows: lines
                .iter()
                .map(|line| {
                    let mut text = String::with_capacity(line.len() + 1);
                    text.push(' ');
                    text.push_str(line);
                    FloatRow {
                        text,
                        selected: false,
                    }
                })
                .collect(),
        })
    }

    #[must_use]
    pub fn lsp_summary(&self) -> Option<&str> {
        (!self.live.summary.is_empty()).then_some(self.live.summary.as_str())
    }

    #[must_use]
    pub fn server_statuses(&self) -> Vec<&ServerStatus> {
        self.live.servers.values().collect()
    }

    #[must_use]
    pub fn inlay_hints(&self, buffer: BufferId) -> &[InlayHint] {
        self.live
            .hints
            .get(&buffer)
            .filter(|h| h.anchor.is_fresh(&self.world()))
            .map_or(&[][..], |h| h.hints.as_slice())
    }

    #[must_use]
    pub fn annotations(&self, world: &Anchor, buffer: BufferId, line: u32) -> Vec<Annotation> {
        let mut out: Vec<Annotation> = self
            .results
            .on_line(world, buffer, line)
            .into_iter()
            .take(1)
            .map(|finding| Annotation {
                text: finding
                    .message
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string(),
                role: AnnotationRole::Diagnostic(finding.severity),
            })
            .collect();
        if let Some(h) = self
            .live
            .hints
            .get(&buffer)
            .filter(|h| h.anchor.is_fresh(world))
        {
            out.extend(
                h.hints
                    .iter()
                    .filter(|hint| hint.position.line == line)
                    .map(|hint| Annotation {
                        text: hint.label.clone(),
                        role: AnnotationRole::Value,
                    }),
            );
        }
        out
    }

    #[must_use]
    pub fn session_busy(&self) -> bool {
        self.live.want.is_some()
            || self.live.seen.values().any(|s| s.dirty_since.is_some())
            || self
                .live
                .last_send
                .is_some_and(|sent| sent.elapsed() < QUICK_WAKE_AFTER_SEND)
    }

    fn document_of(&self, buffer: BufferId) -> Option<Document> {
        let path = self.buffers.get(buffer)?.path.clone()?;
        let language = Some(self.language_of(&path)?);
        Some(Document {
            buffer,
            path,
            language,
        })
    }

    fn send_session(&mut self, freight: SessionFreight) {
        let freight = Freight::Session(freight);
        let anchor = self.seal(&freight);
        self.courier.send(freight, anchor);
        self.live.last_send = Some(Instant::now());
    }

    pub(crate) fn pump_session(&mut self, now: Instant) {
        if !self.session_on() {
            return;
        }
        let ids = self.buffers.ids();
        let gone: Vec<BufferId> = self
            .live
            .seen
            .keys()
            .filter(|id| !ids.contains(id))
            .copied()
            .collect();
        for id in gone {
            if let Some(seen) = self.live.seen.remove(&id) {
                self.send_session(SessionFreight::Close {
                    document: seen.document,
                });
            }
            self.live.hints.remove(&id);
        }
        let debounce = self.debounce();
        for id in ids {
            let Some(buffer) = self.buffers.get(id) else {
                continue;
            };
            let rev = buffer.text_rev();
            let unmoved = self.live.seen.get(&id).is_some_and(|s| {
                s.rev == rev
                    && s.dirty_since.is_none()
                    && buffer.path.as_ref() == Some(&s.document.path)
            });
            if unmoved {
                continue;
            }
            if buffer.char_count() > self.max_document_chars() {
                self.refuse_large_document(id);
                continue;
            }
            let Some(document) = self.document_of(id) else {
                continue;
            };
            let moved_file = self
                .live
                .seen
                .get(&id)
                .is_some_and(|s| s.document.path != document.path);
            if moved_file {
                if let Some(seen) = self.live.seen.remove(&id) {
                    self.send_session(SessionFreight::Close {
                        document: seen.document,
                    });
                }
            }
            let seen = self.live.seen.entry(id).or_insert_with(|| Seen {
                document,
                rev,
                synced: None,
                dirty_since: Some(now.checked_sub(debounce).unwrap_or(now)),
            });
            if seen.rev != rev {
                seen.rev = rev;
                seen.dirty_since = Some(now);
            }
            if seen
                .dirty_since
                .is_some_and(|since| now.saturating_duration_since(since) >= debounce)
            {
                self.sync_buffer(id);
            }
        }
        if let Some(want) = self.live.want {
            let settled = self
                .live
                .seen
                .get(&want.buffer)
                .is_none_or(|s| s.dirty_since.is_none());
            if settled {
                self.live.want = None;
                self.ask_completion(want);
            }
        }
    }

    fn sync_buffer(&mut self, id: BufferId) {
        let Some(buffer) = self.buffers.get(id) else {
            return;
        };
        let text = buffer.to_string();
        let rev = buffer.text_rev();
        let Some(seen) = self.live.seen.get_mut(&id) else {
            return;
        };
        seen.dirty_since = None;
        seen.rev = rev;
        seen.synced = Some(rev);
        let document = seen.document.clone();
        self.send_session(SessionFreight::Sync { document, text });
    }

    pub(crate) fn sync_now(&mut self, id: BufferId) {
        if !self.session_on() {
            return;
        }
        self.pump_session(Instant::now());
        if self
            .live
            .seen
            .get(&id)
            .is_some_and(|s| s.synced != Some(s.rev) || s.dirty_since.is_some())
        {
            self.sync_buffer(id);
        }
    }

    fn resync_all(&mut self) {
        let mut ids: Vec<BufferId> = self.live.seen.keys().copied().collect();
        ids.sort_by_key(|id| *id == self.active);
        for id in ids {
            self.sync_buffer(id);
        }
    }

    fn ask_completion(&mut self, want: Want) {
        if want.buffer != self.active || !self.completion_on() {
            return;
        }
        if !want.manual && self.modal.mode() != Mode::Insert {
            return;
        }
        let Some(document) = self.document_of(want.buffer) else {
            return;
        };
        let at = self.cursor();
        self.send_session(SessionFreight::Ask {
            document,
            at,
            question: Question::Completion {
                manual: want.manual,
            },
        });
    }

    pub(crate) fn ask_server(&mut self, question: Question) {
        if !self.session_on() {
            if question.is_explicit() {
                self.messages
                    .push("lsp: the live session is off (lsp.session)".to_string());
            }
            return;
        }
        if let Question::Completion { manual } = question {
            if !self.completion_on() {
                if manual {
                    self.messages
                        .push("completion is off (cmp.enabled, cmp.sources)".to_string());
                }
                return;
            }
        }
        let buffer = self.active;
        let Some(document) = self.document_of(buffer) else {
            if question.is_explicit() {
                self.messages
                    .push("lsp: this buffer has no path".to_string());
            }
            return;
        };
        self.sync_now(buffer);
        let at = self.cursor();
        self.send_session(SessionFreight::Ask {
            document,
            at,
            question,
        });
    }

    pub(crate) fn note_saved(&mut self, buffer: BufferId) {
        if !self.session_on() {
            return;
        }
        if let Some(document) = self.document_of(buffer) {
            self.sync_now(buffer);
            self.send_session(SessionFreight::Saved { document });
        }
    }

    pub(crate) fn restart_servers(&mut self) {
        if self.session_on() {
            self.send_session(SessionFreight::Restart);
        } else {
            self.messages
                .push("lsp: the live session is off (lsp.session)".to_string());
        }
    }

    pub(crate) fn report_servers(&mut self) {
        if self.live.servers.is_empty() {
            self.messages
                .push("lsp: no language server has started".to_string());
            return;
        }
        let mut line = String::from("lsp:");
        for status in self.live.servers.values() {
            line.push(' ');
            line.push_str(&status.command);
            line.push('=');
            status.state.render_into(&mut line);
            line.push('@');
            line.push_str(&status.root.to_string_lossy());
        }
        self.messages.push(line);
    }

    pub(crate) fn honour_server_status(&mut self, status: ServerStatus) {
        let key = (status.command.clone(), status.root.clone());
        let was_answering = self
            .live
            .servers
            .get(&key)
            .is_some_and(|s| s.state.answers());
        if let ServerState::Ready {
            completes,
            completion_triggers,
            ..
        } = &status.state
        {
            for language in &status.languages {
                self.live
                    .triggers
                    .insert(language.clone(), (*completes, completion_triggers.clone()));
            }
        }
        let mut message = None;
        let mut resync = false;
        match &status.state {
            ServerState::Starting | ServerState::Slow { .. } => {}
            ServerState::Ready { generation, .. } => {
                resync = *generation > 1;
                if resync {
                    message = Some(say(&status.command, "is back"));
                }
            }
            ServerState::Restarting { .. } => {
                resync = was_answering;
                if was_answering {
                    message = Some(say(&status.command, "stopped; restarting"));
                }
            }
            ServerState::Dead { reason } => {
                resync = was_answering;
                let mut m = say(&status.command, "is dead, :LspRestart to retry: ");
                m.push_str(reason);
                message = Some(m);
            }
            ServerState::Missing { command } => {
                let mut m = String::from("lsp: `");
                m.push_str(command);
                m.push_str("` is not on PATH; this language has no server");
                message = Some(m);
            }
        }
        self.live.servers.insert(key, status);
        self.rebuild_summary();
        if let Some(m) = message {
            self.messages.push(m);
        }
        if resync {
            self.bump_lsp_gen();
            self.resync_all();
        }
    }

    fn rebuild_summary(&mut self) {
        let mut out = String::new();
        for status in self.live.servers.values() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&status.command);
            if !matches!(status.state, ServerState::Ready { .. }) {
                out.push(':');
                status.state.render_into(&mut out);
            }
        }
        self.live.summary = out;
    }

    pub(crate) fn publish_hints(
        &mut self,
        buffer: BufferId,
        hints: Vec<InlayHint>,
        anchor: Anchor,
    ) {
        self.live.hints.insert(buffer, Hints { hints, anchor });
    }

    pub(crate) fn offer_completion(
        &mut self,
        buffer: BufferId,
        at: Position,
        items: Vec<CompletionEntry>,
        manual: bool,
    ) {
        if buffer != self.active || at != self.cursor() || self.modal.mode() != Mode::Insert {
            return;
        }
        let (start, prefix) = self.word_before_caret();
        self.live.popup = CompletionPopup::open(buffer, start, items, &prefix);
        if self.live.popup.is_none() && manual {
            self.messages.push("no completions here".to_string());
        }
    }

    pub(crate) fn show_hover(&mut self, text: &str) {
        if text.is_empty() {
            self.messages.push("lsp: nothing to say here".to_string());
            return;
        }
        self.live.hover = Some(
            text.lines()
                .take(HOVER_ROWS)
                .map(|line| line.chars().take(HOVER_COLUMNS).collect())
                .collect(),
        );
    }

    pub(crate) fn jump_to_definition(&mut self, path: PathBuf, at: Position) {
        let existing = self.buffers.find_by_path(&path);
        let target = match existing {
            Some(id) => id,
            None => match self.buffers.open(&path) {
                Ok(id) => id,
                Err(e) => {
                    self.messages.push(e.to_string());
                    return;
                }
            },
        };
        let site = escriba_shirube::Site::in_buffer(target, Range::new(at, at));
        self.jump_to_site(&site);
        if let Some(w) = self.layout.active_window_mut() {
            w.buffer_id = self.active;
        }
    }

    pub(crate) fn abort_completion(&mut self) {
        self.live.popup = None;
        self.live.want = None;
    }

    pub(crate) fn toggle_completion(&mut self) {
        let on = truthy(self.options.get("cmp.enabled"), true);
        self.options
            .insert("cmp.enabled".to_string(), (!on).to_string());
        if on {
            self.abort_completion();
        }
        self.messages.push(
            if on {
                "completion off"
            } else {
                "completion on"
            }
            .to_string(),
        );
    }

    fn word_before_caret(&self) -> (Position, String) {
        let cursor = self.cursor();
        let line = self
            .buffers
            .get(self.active)
            .and_then(|b| b.line(cursor.line))
            .unwrap_or_default();
        let start = word_start(&line, cursor.column);
        let prefix = line
            .chars()
            .skip(start as usize)
            .take((cursor.column.saturating_sub(start)) as usize)
            .collect();
        (Position::new(cursor.line, start), prefix)
    }

    pub(crate) fn before_key(&self) -> BeforeKey {
        BeforeKey {
            buffer: self.active,
            rev: self
                .buffers
                .get(self.active)
                .map(escriba_buffer::Buffer::text_rev),
        }
    }

    pub(crate) fn after_key(&mut self, key: &Key, before: &BeforeKey) {
        if self.modal.mode() != Mode::Insert || self.active != before.buffer {
            self.abort_completion();
            return;
        }
        let rev = self
            .buffers
            .get(self.active)
            .map(escriba_buffer::Buffer::text_rev);
        let changed = rev != before.rev;
        let (start, prefix) = self.word_before_caret();
        let popup_open = self.live.popup.is_some();
        if let Some(popup) = self.live.popup.as_mut() {
            let same_word = popup.start == start;
            if !same_word || !popup.narrow(&prefix) {
                self.live.popup = None;
            }
        }
        if !changed || !self.completion_on() {
            return;
        }
        let trigger = match key {
            Key::Char(c) => {
                let (completes, triggers) = self
                    .document_of(self.active)
                    .and_then(|d| d.language)
                    .and_then(|l| self.live.triggers.get(&l))
                    .map_or((true, &[][..]), |(c, t)| (*c, t.as_slice()));
                completes
                    && (triggers.contains(c)
                        || (is_word_char(*c)
                            && (popup_open
                                || u32::try_from(prefix.chars().count()).unwrap_or(0)
                                    >= self.keyword_length())))
            }
            Key::Backspace => popup_open,
            _ => false,
        };
        let auto = truthy(self.options.get("cmp.autocomplete"), true);
        self.live.want = (trigger && auto).then_some(Want {
            buffer: self.active,
            manual: false,
        });
    }

    pub(crate) fn consume_completion_key(&mut self, key: &Key) -> bool {
        if self.modal.mode() != Mode::Insert {
            return false;
        }
        let Some(popup) = self.live.popup.as_mut() else {
            return false;
        };
        match key {
            Key::Ctrl('n') | Key::Down => popup.next(),
            Key::Ctrl('p') | Key::Up => popup.previous(),
            Key::Ctrl('e') => self.abort_completion(),
            Key::Ctrl('y') | Key::Tab => self.accept_completion(),
            Key::Enter if popup.navigated() => self.accept_completion(),
            _ => return false,
        }
        self.damage = self.damage.join(Damage::Viewport);
        self.bump_gen();
        true
    }

    fn accept_completion(&mut self) {
        let Some(popup) = self.live.popup.take() else {
            return;
        };
        let Some(entry) = popup.chosen().cloned() else {
            return;
        };
        self.live.want = None;
        let cursor = self.cursor();
        let main = entry.edit.clone().map_or_else(
            || TextChange {
                range: Range::new(popup.start, cursor),
                text: entry.insert.clone(),
            },
            |mut edit| {
                edit.range.end = cursor;
                edit
            },
        );
        let mut caret = end_of(main.range.start, &main.text);
        let mut changes = entry.additional;
        changes.push(main);
        changes.sort_by_key(|c| std::cmp::Reverse((c.range.start.line, c.range.start.column)));
        let Some(buffer) = self.buffers.get_mut(self.active) else {
            return;
        };
        for change in &changes {
            if buffer
                .apply(&Edit::replace(change.range, change.text.clone()))
                .is_ok()
                && change.range.end.line <= caret.line
                && change.range.start != popup.start
                && (change.range.end.line < caret.line || change.range.end.column <= caret.column)
            {
                caret = shifted(caret, change);
            }
        }
        self.place_cursor(caret, CursorRest::AtInsertPoint);
        self.damage = self.damage.join(Damage::Full);
    }

    pub(crate) fn dismiss_hover(&mut self) {
        if self.live.hover.take().is_some() {
            self.damage = self.damage.join(Damage::Viewport);
            self.bump_gen();
        }
    }

    pub(crate) fn want_manual_completion(&mut self) {
        self.live.want = None;
        self.ask_server(Question::Completion { manual: true });
    }
}

fn say(command: &str, what: &str) -> String {
    let mut m = String::from("lsp: ");
    m.push_str(command);
    m.push(' ');
    m.push_str(what);
    m
}

fn end_of(start: Position, text: &str) -> Position {
    match text.rfind('\n') {
        Some(nl) => Position::new(
            start.line + u32::try_from(text.matches('\n').count()).unwrap_or(0),
            u32::try_from(text[nl + 1..].chars().count()).unwrap_or(0),
        ),
        None => start.shift_right(u32::try_from(text.chars().count()).unwrap_or(0)),
    }
}

fn shifted(caret: Position, change: &TextChange) -> Position {
    let inserted_end = end_of(change.range.start, &change.text);
    let line = caret.line + inserted_end.line - change.range.end.line;
    let column = if caret.line == change.range.end.line {
        caret.column - change.range.end.column + inserted_end.column
    } else {
        caret.column
    };
    Position::new(line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(a: (u32, u32), b: (u32, u32), text: &str) -> TextChange {
        TextChange {
            range: Range::new(Position::new(a.0, a.1), Position::new(b.0, b.1)),
            text: text.to_string(),
        }
    }

    #[test]
    fn a_line_added_above_moves_the_caret_down_and_keeps_its_column() {
        let caret = shifted(
            Position::new(2, 9),
            &change((0, 0), (0, 0), "use(\"x\")\n\n"),
        );
        assert_eq!(caret, Position::new(4, 9));
    }

    #[test]
    fn an_edit_before_the_caret_on_its_line_moves_its_column() {
        let caret = shifted(Position::new(1, 9), &change((1, 2), (1, 4), "abcd"));
        assert_eq!(caret, Position::new(1, 11));
    }

    #[test]
    fn a_multi_line_replacement_above_lands_the_caret_correctly() {
        let caret = shifted(Position::new(5, 3), &change((1, 0), (2, 0), "x\ny\nz\n"));
        assert_eq!(caret, Position::new(7, 3));
    }

    #[test]
    fn off_spellings_read_off_and_anything_else_reads_on() {
        for off in ["false", "0", "off", "no"] {
            assert!(!truthy(Some(&off.to_string()), true), "{off}");
        }
        assert!(truthy(Some(&"true".to_string()), false));
        assert!(truthy(None, true));
        assert!(!truthy(None, false));
    }
}
