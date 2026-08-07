//! Phase 0's gate: a keybinding that does nothing must SAY so.
//!
//! ## The class this seals
//!
//! escriba ships 85 keybindings that resolve to neither a typed `Action` nor a
//! registered command (`escriba/tests/action_resolution.rs` pins the exact
//! inventory). Until now they failed in two independent, silent ways:
//!
//! 1. `run_action`'s `_ => Ok(())` — a registered command whose action symbol
//!    nothing implements REPORTED SUCCESS.
//! 2. `run_command`'s `let _ = self.commands.run(…)` — an unregistered name
//!    produced `NotFound`, which was then DISCARDED.
//!
//! Either way a dead keybinding and a working one produced identical frames.
//! That is not primarily a UX bug: it made every later claim unfalsifiable.
//! "I wired the picker" could not be distinguished from "I bound a key to
//! nothing", by a human OR by a test. Every phase after this one depends on
//! being able to tell those apart, which is why this is Phase 0.
//!
//! ## What is asserted
//!
//! Reported, never fatal. The editor survives, keeps its buffer, and keeps
//! taking input — it simply stops pretending the action worked.

use escriba_buffer::BufferSet;
use escriba_core::{Action, Mode};
use escriba_keymap::Key;
use escriba_runtime::EditorState;

fn editor() -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("hello world\n");
    EditorState::new_with_buffer(bufs, id)
}

/// Bind `name` to a key and PRESS it — the path an operator actually takes.
///
/// Deliberately not a direct call into the executor: the defect lived in
/// `run_command`, which sits between key dispatch and the registry, so a test
/// that skipped the keyboard would have skipped the bug.
fn press_command(st: &mut EditorState, name: &str) {
    let key = Key::Char('\u{1}'); // an unbound control char: collides with nothing
    st.keymap.bind(
        Mode::Normal,
        key.clone(),
        Action::Command {
            name: name.to_string(),
            args: Vec::new(),
        },
        "test binding",
    );
    st.on_key(&key);
}

#[test]
fn an_unregistered_command_is_reported() {
    let mut st = editor();
    press_command(&mut st, "totally.not.a.command");
    let msg = st
        .messages
        .last()
        .expect("a dead command must say something");
    assert!(
        msg.contains("totally.not.a.command"),
        "the message must name what failed: {msg:?}",
    );
}

#[test]
fn a_registered_but_unimplemented_action_is_reported() {
    // The more misleading of the two: the editor ADVERTISED this — it is in
    // `--commands`, it is in the keymap, `--list-rc` counts it — and then did
    // nothing. This is the shape all 85 inert actions have.
    let mut st = editor();
    st.commands.register(escriba_command::Command::action(
        "pick",
        "Pick a file",
        "picker.files",
    ));
    press_command(&mut st, "pick");
    let msg = st
        .messages
        .last()
        .expect("an inert action must say something");
    assert!(
        msg.contains("picker.files"),
        "the message must name the unimplemented action: {msg:?}",
    );
    assert!(
        msg.contains("not implemented"),
        "and must distinguish 'not built yet' from 'no such command': {msg:?}",
    );
}

#[test]
fn a_failed_command_is_never_fatal() {
    // Reported, not fatal. The editor keeps its buffer, stays out of quit,
    // and keeps accepting input — a dead keybinding costs a message, never a
    // session.
    let mut st = editor();
    press_command(&mut st, "nope.not.here");
    assert!(
        !st.quit_requested,
        "a dead command must not exit the editor"
    );
    assert_eq!(
        st.buffers.get(st.active).map(|b| b.to_string()),
        Some("hello world\n".to_string()),
        "the buffer must be untouched",
    );
    // And the editor still works afterwards.
    st.on_key(&Key::Char('j'));
    assert_eq!(st.cursor().line, 1, "input still lands after a failure");
}

#[test]
fn a_working_command_says_nothing() {
    // The other half of the contract, and the one that makes the first half
    // meaningful: success must stay SILENT, or the message channel becomes
    // noise and operators learn to ignore it.
    let mut st = editor();
    let before = st.messages.len();
    press_command(&mut st, "buffer-info"); // a real built-in
    assert_eq!(
        st.messages.len().saturating_sub(before),
        0,
        "a command that worked must not report: {:?}",
        st.messages,
    );
}

#[test]
fn a_failure_forces_a_repaint() {
    // The message only helps if it reaches the screen. The GPU face caches
    // its shaped buffer against the refresh generation, so a message pushed
    // without bumping it would not appear until an unrelated edit happened
    // to invalidate the cache.
    let mut st = editor();
    let gen_before = st.edit_gen();
    press_command(&mut st, "nope.not.here");
    assert_ne!(
        gen_before,
        st.edit_gen(),
        "a reported failure must invalidate the cached frame",
    );
}

#[test]
fn the_message_reaches_the_shared_status_model() {
    // Both faces render `StatusModel::message`. Asserting on the model (not
    // on `messages` directly) is what proves the report is actually on the
    // path the TUI and GPU faces read.
    let mut st = editor();
    press_command(&mut st, "nope.not.here");
    let model = st.status_model();
    assert!(
        model.message.is_some_and(|m| m.contains("nope.not.here")),
        "the failure must reach the status model both faces render",
    );
}

#[test]
fn a_typo_and_an_unbuilt_capability_read_differently() {
    // The distinction that decides whether the operator blames themselves.
    //
    // `:flurb` is a typo — "command not found" is correct and useful.
    // `picker.files` is a capability escriba's OWN shipped config declares
    // and has not built; reporting that as "not found" blames the operator
    // for a gap we shipped. The dotted form is the discriminator, because
    // `:action` takes action SYMBOLS and never command names.
    let mut st = editor();
    press_command(&mut st, "flurb");
    let typo = st.messages.last().cloned().expect("a message");
    assert!(
        typo.contains("not found"),
        "a bare name the operator mistyped is not-found: {typo:?}",
    );

    let mut st = editor();
    press_command(&mut st, "picker.files");
    let unbuilt = st.messages.last().cloned().expect("a message");
    assert!(
        unbuilt.contains("not implemented"),
        "a declared-but-unbuilt action must not read as a typo: {unbuilt:?}",
    );
    assert!(
        !unbuilt.contains("not found"),
        "and must not blame the operator: {unbuilt:?}",
    );
}
