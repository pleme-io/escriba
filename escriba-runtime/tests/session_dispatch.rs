use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use escriba_buffer::BufferSet;
use escriba_core::{CompletionEntry, Filetype, Mode, Position};
use escriba_madoguchi::Negai;
use escriba_madoguchi::errand::{
    Crew, Errand, Freight, Idle, Parcel, Question, Runner, SessionFreight,
};
use escriba_runtime::EditorState;
use madori::event::{AppEvent, KeyCode, KeyEvent, Modifiers};

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<SessionFreight>>>);

impl Runner for Recorder {
    fn start(&self, errand: Errand, _cancel: Arc<AtomicBool>, _reply: Sender<Parcel>) {
        if let Freight::Session(freight) = errand.freight {
            self.0.lock().unwrap().push(freight);
        }
    }
}

impl Recorder {
    fn syncs(&self) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|f| matches!(f, SessionFreight::Sync { .. }))
            .count()
    }

    fn last_sync_text(&self) -> Option<String> {
        self.0.lock().unwrap().iter().rev().find_map(|f| match f {
            SessionFreight::Sync { text, .. } => Some(text.clone()),
            _ => None,
        })
    }

    fn asks(&self) -> Vec<Question> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|f| match f {
                SessionFreight::Ask { question, .. } => Some(*question),
                _ => None,
            })
            .collect()
    }

    fn closed(&self) -> bool {
        self.0
            .lock()
            .unwrap()
            .iter()
            .any(|f| matches!(f, SessionFreight::Close { .. }))
    }

    fn saved(&self) -> bool {
        self.0
            .lock()
            .unwrap()
            .iter()
            .any(|f| matches!(f, SessionFreight::Saved { .. }))
    }
}

struct Harness {
    st: EditorState,
    runner: Recorder,
    clock: Instant,
    _dir: tempfile::TempDir,
}

fn editor(text: &str, session: &str) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.b");
    std::fs::write(&path, text).unwrap();
    let mut buffers = BufferSet::new();
    let id = buffers.open(&path).unwrap();
    let mut st = EditorState::new_with_buffer(buffers, id);
    let runner = Recorder::default();
    st.hire(Crew {
        scan: Box::new(Idle("t")),
        diagnostics: Box::new(Idle("t")),
        format: Box::new(Idle("t")),
        session: Box::new(runner.clone()),
    });
    st.filetypes.insert(Filetype {
        name: "blue".to_string(),
        extensions: vec!["b".to_string()],
        comment: None,
    });
    st.options.insert("lsp.session".into(), session.into());
    st.options.insert("lsp.debounce_ms".into(), "1000".into());
    Harness {
        st,
        runner,
        clock: Instant::now(),
        _dir: dir,
    }
}

fn key(code: KeyCode, ctrl: bool) -> AppEvent {
    AppEvent::Key(KeyEvent {
        key: code,
        pressed: true,
        modifiers: Modifiers {
            ctrl,
            ..Modifiers::default()
        },
        text: None,
    })
}

impl Harness {
    fn press(&mut self, code: KeyCode) {
        self.clock += Duration::from_millis(1);
        self.st.tick_at(&key(code, false), self.clock);
    }

    fn typed(&mut self, text: &str) {
        for c in text.chars() {
            self.press(KeyCode::Char(c));
        }
    }

    fn text(&self) -> String {
        self.st
            .buffers
            .get(self.st.active)
            .map(escriba_buffer::Buffer::to_string)
            .unwrap_or_default()
    }

    fn offer(&mut self, labels: &[&str]) {
        let entries = labels.iter().map(|l| entry(l)).collect();
        self.st.interpret(escriba_madoguchi::Outcome::did(vec![
            Negai::OfferCompletion {
                buffer: self.st.active,
                at: self.st.cursor(),
                items: entries,
                manual: false,
            },
        ]));
    }
}

fn entry(label: &str) -> CompletionEntry {
    CompletionEntry {
        label: label.to_string(),
        detail: None,
        tier: None,
        documentation: None,
        filter: label.to_string(),
        sort: label.to_string(),
        insert: label.to_string(),
        edit: None,
        additional: Vec::new(),
    }
}

#[test]
fn opening_syncs_once_and_a_burst_of_keys_syncs_once_after_the_quiet() {
    let mut h = editor("hello\n", "true");
    h.st.diagnose_open_buffers();
    assert_eq!(h.runner.syncs(), 1, "the open is one didOpen");

    h.press(KeyCode::Char('i'));
    h.typed("abcdefgh");
    assert_eq!(h.st.modal.mode(), Mode::Insert);
    assert_eq!(h.runner.syncs(), 1, "no copy of the buffer per keystroke");

    h.clock += Duration::from_secs(2);
    h.st.tick_at(&AppEvent::RedrawRequested, h.clock);
    h.press(KeyCode::Escape);
    assert_eq!(h.runner.syncs(), 2, "one sync for the whole burst");
    assert_eq!(
        h.runner.last_sync_text().as_deref(),
        Some("abcdefghhello\n")
    );
}

#[test]
fn every_way_the_text_can_change_reaches_the_server() {
    let mut h = editor("hello\n", "true");
    h.st.diagnose_open_buffers();
    h.typed("dw");
    h.clock += Duration::from_secs(2);
    h.st.tick_at(&key(KeyCode::Escape, false), h.clock);
    h.clock += Duration::from_secs(2);
    h.st.tick_at(&key(KeyCode::Escape, false), h.clock);
    assert_eq!(h.runner.last_sync_text().as_deref(), Some("\n"));

    h.press(KeyCode::Char('u'));
    h.clock += Duration::from_secs(2);
    h.st.tick_at(&key(KeyCode::Escape, false), h.clock);
    h.clock += Duration::from_secs(2);
    h.st.tick_at(&key(KeyCode::Escape, false), h.clock);
    assert_eq!(
        h.runner.last_sync_text().as_deref(),
        Some("hello\n"),
        "undo is not typing and it still moves the document"
    );
}

#[test]
fn closing_a_buffer_closes_the_document_and_saving_says_so() {
    let mut h = editor("x\n", "true");
    h.st.diagnose_open_buffers();
    let id = h.st.active;
    h.st.interpret(escriba_madoguchi::Outcome::did(vec![Negai::Save {
        buffer: id,
    }]));
    assert!(h.runner.saved());
    h.st.interpret(escriba_madoguchi::Outcome::did(vec![Negai::CloseBuffer(
        id,
    )]));
    h.st.deliver();
    assert!(h.runner.closed());
}

#[test]
fn a_session_turned_off_sends_nothing_and_says_so_when_asked() {
    let mut h = editor("x\n", "false");
    h.st.diagnose_open_buffers();
    h.press(KeyCode::Char('i'));
    h.typed("abc");
    h.st.deliver();
    assert_eq!(h.runner.syncs(), 0);
    h.st.interpret(escriba_madoguchi::Outcome::did(vec![Negai::AskServer(
        Question::Hover,
    )]));
    assert!(
        h.st.messages.iter().any(|m| m.contains("lsp.session")),
        "{:?}",
        h.st.messages
    );
}

#[test]
fn typing_a_word_asks_only_after_the_keyword_length_and_never_on_a_space() {
    let mut h = editor("\n", "true");
    h.st.diagnose_open_buffers();
    h.press(KeyCode::Char('i'));
    h.typed("a");
    h.clock += Duration::from_secs(2);
    h.st.tick_at(&AppEvent::RedrawRequested, h.clock);
    h.st.tick_at(&AppEvent::RedrawRequested, h.clock);
    assert!(h.runner.asks().is_empty(), "one char is below the length");

    h.typed("b");
    h.clock += Duration::from_secs(2);
    h.st.tick_at(&key(KeyCode::Home, false), h.clock);
    h.press(KeyCode::End);
    assert_eq!(h.runner.asks().len(), 1, "{:?}", h.runner.asks());

    h.press(KeyCode::End);
    h.typed(" ");
    h.clock += Duration::from_secs(2);
    h.st.tick_at(&key(KeyCode::Home, false), h.clock);
    h.press(KeyCode::End);
    assert_eq!(h.runner.asks().len(), 1, "a space asks nothing");
}

#[test]
fn a_file_over_the_limit_never_copies_to_a_server_and_says_so() {
    let mut h = editor("0123456789\n", "true");
    h.st.options
        .insert("lsp.max_document_chars".into(), "5".into());
    h.st.diagnose_open_buffers();
    h.st.deliver();
    assert_eq!(h.runner.syncs(), 0);
    assert!(
        h.st.messages
            .iter()
            .any(|m| m.contains("lsp.max_document_chars")),
        "{:?}",
        h.st.messages
    );
}

#[test]
fn a_late_answer_for_an_older_revision_never_opens_a_popup() {
    let mut h = editor("\n", "true");
    h.st.diagnose_open_buffers();
    h.press(KeyCode::Char('i'));
    h.typed("ab");
    let world = h.st.world();
    let at = h.st.cursor();
    let buffer = h.st.active;
    h.typed("c");
    h.st.interpret(escriba_madoguchi::Outcome::did(vec![Negai::ErrandReply {
        anchor: world,
        then: Box::new(Negai::OfferCompletion {
            buffer,
            at,
            items: vec![entry("abc")],
            manual: false,
        }),
    }]));
    assert!(h.st.completion_popup().is_none());
}

#[test]
fn enter_is_a_newline_until_the_person_moves_in_the_popup() {
    let mut h = editor("\n", "true");
    h.st.diagnose_open_buffers();
    h.press(KeyCode::Char('i'));
    h.typed("ab");
    h.offer(&["abacus", "abcde"]);
    assert_eq!(h.st.completion_popup().unwrap().labels().len(), 2);

    h.press(KeyCode::Enter);
    assert_eq!(h.text(), "ab\n\n", "no word typed by a person is replaced");
    assert!(h.st.completion_popup().is_none());
}

#[test]
fn arrow_then_enter_accepts_and_escape_leaves_without_accepting() {
    let mut h = editor("\n", "true");
    h.st.diagnose_open_buffers();
    h.press(KeyCode::Char('i'));
    h.typed("ab");
    h.offer(&["abacus", "abcde"]);
    h.st.tick_at(&key(KeyCode::Char('n'), true), h.clock);
    h.press(KeyCode::Enter);
    assert_eq!(h.text(), "abcde\n");
    assert_eq!(h.st.cursor(), Position::new(0, 5));

    h.typed(" ab");
    h.offer(&["abacus"]);
    h.press(KeyCode::Escape);
    assert_eq!(h.st.modal.mode(), Mode::Normal);
    assert!(h.st.completion_popup().is_none());
    assert_eq!(h.text(), "abcde ab\n");
}

#[test]
fn typing_narrows_the_open_popup_at_once_and_a_non_word_key_closes_it() {
    let mut h = editor("\n", "true");
    h.st.diagnose_open_buffers();
    h.press(KeyCode::Char('i'));
    h.typed("ab");
    h.offer(&["abacus", "abcde"]);
    h.typed("c");
    assert_eq!(h.st.completion_popup().unwrap().labels(), vec!["abcde"]);
    h.typed(" ");
    assert!(h.st.completion_popup().is_none());
    assert_eq!(h.text(), "abc \n");
}

#[test]
fn a_dead_server_is_named_on_the_status_line() {
    let mut h = editor("x\n", "true");
    let status = |state| {
        Negai::ServerStatus(escriba_core::ServerStatus {
            languages: vec!["blue".to_string()],
            command: "blue".to_string(),
            root: "/r".into(),
            state,
        })
    };
    h.st.interpret(escriba_madoguchi::Outcome::did(vec![status(
        escriba_core::ServerState::Dead {
            reason: "signal: 9".to_string(),
        },
    )]));
    let line = h.st.status_model().render();
    assert!(line.contains("blue:DEAD signal: 9"), "{line}");
}
