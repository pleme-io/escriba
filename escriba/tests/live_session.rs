use std::path::PathBuf;
use std::time::{Duration, Instant};

use escriba_buffer::BufferSet;
use escriba_core::{Mode, ServerState};
use escriba_runtime::EditorState;
use madori::event::{AppEvent, KeyCode, KeyEvent, Modifiers};

const PATIENCE: Duration = Duration::from_secs(30);

fn blue_is_here() -> bool {
    if std::process::Command::new("blue")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        return true;
    }
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        let line = "live_session NOT RUN: `blue` is not on this runner's PATH\n";
        eprint!("{line}");
        if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
            let _ = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(summary)
                .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
        }
        return false;
    }
    panic!("live_session needs a real `blue lsp`, and `blue` is not on PATH");
}

struct Editor {
    st: EditorState,
    path: PathBuf,
    on_disk: String,
    _dir: tempfile::TempDir,
}

fn boot_with(text: &str, tune: impl FnOnce(&mut escriba_lisp::ApplyPlan)) -> Editor {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let path = dir.path().join("live.b");
    std::fs::write(&path, text).unwrap();
    let mut buffers = BufferSet::new();
    let id = buffers.open(&path).unwrap();
    let mut st = EditorState::new_with_buffer(buffers, id);
    let mut plan = escriba::default_plan(false).unwrap();
    tune(&mut plan);
    let _ = escriba::wire_plan(&mut st, &plan);
    Editor {
        st,
        path,
        on_disk: text.to_string(),
        _dir: dir,
    }
}

fn boot(text: &str) -> Editor {
    boot_with(text, |_| {})
}

fn event(code: KeyCode, ctrl: bool) -> AppEvent {
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

impl Editor {
    fn press(&mut self, code: KeyCode) {
        self.st.tick(&event(code, false));
    }

    fn ctrl(&mut self, c: char) {
        self.st.tick(&event(KeyCode::Char(c), true));
    }

    fn typed(&mut self, text: &str) {
        for c in text.chars() {
            self.press(KeyCode::Char(c));
        }
    }

    fn motion(&mut self, text: &str) {
        for c in text.chars() {
            std::thread::sleep(Duration::from_millis(120));
            self.press(KeyCode::Char(c));
        }
    }

    fn escape(&mut self) {
        self.press(KeyCode::Escape);
    }

    fn pump(&mut self) {
        self.st.deliver();
        self.st.tick(&AppEvent::RedrawRequested);
    }

    fn until(&mut self, what: &str, done: impl Fn(&EditorState) -> bool) {
        let began = Instant::now();
        while !done(&self.st) {
            assert!(
                began.elapsed() < PATIENCE,
                "never happened: {what}\nstatus: {}\nmessages: {:?}\nbuffer: {:?}",
                self.st.status_model().render(),
                self.st.messages,
                self.text(),
            );
            self.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn ready(&mut self) -> u32 {
        self.until("the server to answer", |st| {
            st.server_statuses()
                .iter()
                .any(|s| matches!(s.state, ServerState::Ready { .. }))
        });
        self.pid()
    }

    fn pid(&self) -> u32 {
        self.st
            .server_statuses()
            .iter()
            .find_map(|s| match s.state {
                ServerState::Ready { pid, .. } => pid,
                _ => None,
            })
            .expect("a ready server reports its process")
    }

    fn text(&self) -> String {
        self.st
            .buffers
            .get(self.st.active)
            .map(escriba_buffer::Buffer::to_string)
            .unwrap_or_default()
    }

    fn popup_labels(&self) -> Vec<String> {
        self.st.completion_popup().map_or_else(Vec::new, |p| {
            p.labels().into_iter().map(str::to_owned).collect()
        })
    }

    fn screen(&mut self) -> String {
        escriba_tui::render::frame_text(&mut self.st, 100, 30)
    }

    fn assert_disk_untouched(&self) {
        assert_eq!(
            std::fs::read_to_string(&self.path).unwrap(),
            self.on_disk,
            "this test is about an unsaved buffer"
        );
    }
}

fn has_error(st: &EditorState, line: u32) -> bool {
    st.results
        .worst_on_line(&st.world(), st.active, line)
        .is_some()
}

#[test]
fn typing_retsu_colons_offers_retsus_names_in_a_popup_on_the_screen() {
    if !blue_is_here() {
        return;
    }
    let mut e = boot("def main()\n  1\nend\n");
    e.ready();
    e.press(KeyCode::Char('o'));
    e.typed("  retsu::");
    e.until("retsu's names", |st| {
        st.completion_popup()
            .is_some_and(|p| p.labels().contains(&"zip_with"))
    });
    assert!(
        e.popup_labels().contains(&"all_but_last".to_string()),
        "{:?}",
        e.popup_labels()
    );
    let screen = e.screen();
    assert!(
        screen.contains("all_but_last"),
        "the popup is in the state and not on the screen:\n{screen}"
    );

    e.typed("zip_w");
    assert_eq!(
        e.popup_labels(),
        vec!["zip_with"],
        "narrowed by the keystroke"
    );
    e.press(KeyCode::Tab);
    assert!(e.text().contains("retsu::zip_with\n"), "{:?}", e.text());
    assert!(
        e.text().starts_with("use(\"retsu\")\n"),
        "the server's own edit adds the use: {:?}",
        e.text()
    );
    let line = e
        .text()
        .lines()
        .position(|l| l.contains("retsu::"))
        .unwrap();
    assert_eq!(
        e.st.cursor().line as usize,
        line,
        "the caret follows the text"
    );
    e.typed("(");
    assert!(e.text().contains("retsu::zip_with(\n"), "{:?}", e.text());
    e.assert_disk_untouched();
}

#[test]
fn a_name_comes_into_scope_when_its_use_line_is_typed_with_nothing_saved() {
    if !blue_is_here() {
        return;
    }
    let mut e = boot("def main()\n  zip_w\nend\n");
    e.ready();
    e.motion("jA");
    e.ctrl('n');
    e.until("zip_with one use away", |st| {
        st.completion_popup().is_some_and(|p| {
            p.entries().iter().any(|i| {
                i.label == "zip_with"
                    && i.tier.as_deref() == Some("needs a use")
                    && !i.additional.is_empty()
            })
        })
    });
    let screen = e.screen();
    assert!(screen.contains("needs a use"), "{screen}");
    e.escape();

    e.motion("gg");
    e.press(KeyCode::Char('O'));
    e.typed("use(\"retsu\", %i[zip_with])");
    e.escape();
    e.motion("j");
    e.motion("jA");
    assert_eq!(e.st.modal.mode(), Mode::Insert);
    e.ctrl('n');
    e.until("zip_with in scope", |st| {
        st.completion_popup().is_some_and(|p| {
            p.entries().iter().any(|i| {
                i.label == "zip_with"
                    && i.tier.as_deref() == Some("imported")
                    && i.additional.is_empty()
            })
        })
    });
    e.assert_disk_untouched();
}

#[test]
fn a_diagnostic_appears_as_it_is_typed_and_clears_as_it_is_fixed() {
    if !blue_is_here() {
        return;
    }
    let mut e = boot("def main()\n  1\nend\n");
    e.ready();
    e.press(KeyCode::Char('o'));
    e.typed("  undefined_thing");
    e.escape();
    e.until("the unbound name to be reported on its line", |st| {
        has_error(st, 1)
    });
    let screen = e.screen();
    assert!(
        screen.contains("unbound name `undefined_thing`"),
        "the words are in the state and not on the screen:\n{screen}"
    );

    e.motion("dd");
    let list = escriba_lsp_client::session::document_list("blue", &e.path);
    let answered_clean = |st: &EditorState| {
        st.results
            .get(&list)
            .is_some_and(|l| !l.is_stale(&st.world()) && l.is_empty())
    };
    e.until("the server to answer that it is clean", answered_clean);
    let began = Instant::now();
    while began.elapsed() < Duration::from_millis(300) {
        e.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(answered_clean(&e.st), "it stayed clear");
    assert!(!e.screen().contains("unbound name"));
    e.assert_disk_untouched();
}

#[test]
fn hover_shows_on_the_screen_and_definition_moves_the_cursor() {
    if !blue_is_here() {
        return;
    }
    let mut e = boot("def helper(x)\n  x\nend\n\ndef main()\n  helper(1)\nend\n");
    e.ready();
    e.motion("jjjjjw");
    assert_eq!(e.st.cursor().line, 5);

    e.motion("K");
    e.until("the hover", |st| {
        st.hover_text()
            .is_some_and(|t| t.iter().any(|l| l.contains("def helper(x)")))
    });
    let screen = e.screen();
    assert!(screen.contains("def helper(x)"), "{screen}");
    let shown = screen
        .lines()
        .filter(|l| l.contains("def helper(x)"))
        .count();
    assert!(shown >= 2, "the code line and the hover card: {screen}");

    e.motion("g");
    assert!(e.st.hover_text().is_none(), "any key takes the card down");
    e.motion("d");
    e.until("the jump", |st| st.cursor().line == 0);
    assert_eq!(e.st.cursor().column, 4, "on the name, not the line start");

    e.ctrl('o');
    assert_eq!(e.st.cursor().line, 5, "the jump is on the jumplist");
}

#[test]
fn a_top_level_value_shows_beside_its_line_as_the_person_types() {
    if !blue_is_here() {
        return;
    }
    let mut e = boot("\n");
    e.ready();
    e.press(KeyCode::Char('i'));
    e.typed("1 + 2");
    e.escape();
    e.until("the live value", |st| {
        st.inlay_hints(st.active).iter().any(|h| h.label == "=> 3")
    });
    assert!(e.screen().contains("=> 3"));

    e.press(KeyCode::Char('A'));
    e.typed(" + 4");
    e.escape();
    e.until("the value to follow the text", |st| {
        st.inlay_hints(st.active).iter().any(|h| h.label == "=> 7")
    });
    e.assert_disk_untouched();
}

#[test]
fn a_killed_server_is_shown_restarted_and_given_the_unsaved_text_again() {
    if !blue_is_here() {
        return;
    }
    let mut e = boot("def main()\n  1\nend\n");
    let first = e.ready();
    e.press(KeyCode::Char('o'));
    e.typed("  undefined_thing");
    e.escape();
    e.until("the report before the kill", |st| has_error(st, 1));

    assert_eq!(
        unsafe { libc::kill(i32::try_from(first).unwrap(), libc::SIGKILL) },
        0
    );
    let mut saw_restarting = false;
    let began = Instant::now();
    while began.elapsed() < PATIENCE {
        e.pump();
        saw_restarting |= e.st.status_model().render().contains("restarting");
        if e.st
            .server_statuses()
            .iter()
            .any(|s| matches!(s.state, ServerState::Ready { generation: 2, .. }))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(saw_restarting, "the outage is visible while it lasts");
    assert!(
        e.screen().contains("[blue]"),
        "the terminal face shows the state"
    );
    assert!(
        e.st.messages.iter().any(|m| m.contains("stopped")),
        "{:?}",
        e.st.messages
    );
    assert_ne!(e.pid(), first, "a new process");

    e.until("the unsaved text to be diagnosed by the new server", |st| {
        has_error(st, 1)
    });
    e.motion("o");
    e.typed("  retsu::");
    e.until("completion from the new server", |st| {
        st.completion_popup()
            .is_some_and(|p| p.labels().contains(&"zip_with"))
    });
    e.assert_disk_untouched();
}

#[test]
fn a_frozen_server_never_holds_a_key_and_answers_when_it_wakes() {
    if !blue_is_here() {
        return;
    }
    let mut e = boot("def main()\n  1\nend\n");
    let pid = i32::try_from(e.ready()).unwrap();
    e.press(KeyCode::Char('o'));
    assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);

    let mut worst = Duration::ZERO;
    let typed: String = std::iter::repeat_n("abc ", 200).collect();
    for c in typed.chars() {
        let began = Instant::now();
        e.press(KeyCode::Char(c));
        e.st.deliver();
        worst = worst.max(began.elapsed());
        std::thread::sleep(Duration::from_micros(1500));
    }
    e.typed("retsu::");
    let quiet = Instant::now();
    while quiet.elapsed() < Duration::from_millis(400) {
        let began = Instant::now();
        e.pump();
        worst = worst.max(began.elapsed());
        std::thread::sleep(Duration::from_millis(2));
    }

    assert!(
        e.st.completion_popup().is_none(),
        "a frozen server cannot have answered"
    );
    assert!(
        e.text().contains(&format!("{typed}retsu::")),
        "no key was lost"
    );
    assert!(
        worst < Duration::from_millis(250),
        "a keystroke waited {worst:?} on a frozen server"
    );

    assert_eq!(unsafe { libc::kill(pid, libc::SIGCONT) }, 0);
    e.until("the answer after the thaw", |st| {
        st.completion_popup()
            .is_some_and(|p| p.labels().contains(&"zip_with"))
    });
}

#[test]
fn a_server_that_stops_reading_its_input_is_killed_and_replaced() {
    if !blue_is_here() {
        return;
    }
    let padding = "# padding padding padding padding padding padding padding\n".repeat(8000);
    let mut e = boot_with(&padding, |plan| {
        plan.options.push(escriba_lisp::OptionSpec {
            name: "lsp.hang_after_ms".to_string(),
            value: "300".to_string(),
        });
    });
    let frozen = i32::try_from(e.ready()).unwrap();
    assert_eq!(unsafe { libc::kill(frozen, libc::SIGSTOP) }, 0);
    e.press(KeyCode::Char('i'));
    e.typed("x");
    e.until("the replacement server", |st| {
        st.server_statuses()
            .iter()
            .any(|s| matches!(s.state, ServerState::Ready { generation: 2, .. }))
    });
    assert_ne!(i32::try_from(e.pid()).unwrap(), frozen);
    assert!(e.text().starts_with("x# padding"), "no key was lost");
}

#[test]
fn a_server_that_is_not_installed_is_a_named_state_and_the_editor_keeps_typing() {
    let mut e = boot_with("x = 1\n", |plan| {
        for server in &mut plan.lsp_servers {
            if server.filetypes.iter().any(|f| f == "blue") {
                server.command = "escriba-test-no-such-language-server".to_string();
            }
        }
    });
    e.until("the missing state", |st| {
        st.status_model().render().contains("MISSING")
    });
    let screen = e.screen();
    assert!(
        screen.contains("MISSING escriba-test-no-such-language-server"),
        "the state is in the model and not on the screen:\n{screen}"
    );
    assert!(
        e.st.messages.iter().any(|m| m.contains("is not on PATH")),
        "{:?}",
        e.st.messages
    );
    e.press(KeyCode::Char('i'));
    e.typed("still typing ");
    e.pump();
    assert!(e.text().starts_with("still typing x = 1"), "{:?}", e.text());
    assert_eq!(
        e.st.server_statuses().len(),
        1,
        "typing did not start a second server"
    );
}

#[test]
fn the_one_shot_errand_stays_reachable_by_configuration() {
    if !blue_is_here() {
        return;
    }
    let mut e = boot_with("def main()\n  undefined_thing\nend\n", |plan| {
        plan.options.push(escriba_lisp::OptionSpec {
            name: "lsp.session".to_string(),
            value: "false".to_string(),
        });
    });
    assert!(!e.st.session_on());
    e.until("the one-shot report", |st| has_error(st, 1));
    assert!(e.st.server_statuses().is_empty(), "no session started");

    e.motion("K");
    assert!(
        e.st.messages.iter().any(|m| m.contains("lsp.session")),
        "{:?}",
        e.st.messages
    );
    e.press(KeyCode::Char('i'));
    e.typed("retsu::");
    let quiet = Instant::now();
    while quiet.elapsed() < Duration::from_millis(400) {
        e.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(e.st.completion_popup().is_none());
}
