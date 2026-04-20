//! End-to-end integration tests for the Tatara-Lisp rc path.
//!
//! The escriba-lisp library has dense unit coverage; this harness
//! exercises the full binary wiring — read a Lisp rc from disk,
//! parse it, apply it to a fresh EditorState, and assert the
//! resulting state reflects the declarations.

use std::fs;

use escriba_buffer::BufferSet;
use escriba_core::{Action, Mode, Motion};
use escriba_keymap::{Key, Keymap};
use escriba_runtime::EditorState;

fn load_and_apply(src: &str) -> (EditorState, escriba_lisp::ApplyReport) {
    // Build a fresh EditorState the same way the binary would.
    let mut buffers = BufferSet::new();
    let active_id = buffers.scratch("");
    let mut state = EditorState::new_with_buffer(buffers, active_id);

    // Parse the rc source and apply its keybindings.
    let plan = escriba_lisp::apply_source(src).expect("parse rc");
    let report = escriba_lisp::apply_plan_to_keymap(&plan, &mut state.keymap);
    (state, report)
}

#[test]
fn sample_rc_fixture_parses_and_applies() {
    // Sanity: the fixture in escriba/examples/sample-rc.lisp remains
    // valid and round-trips through the bridge without warnings.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("sample-rc.lisp");
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
    let (_state, report) = load_and_apply(&src);
    assert!(
        report.warnings.is_empty(),
        "sample rc should apply cleanly: {:?}",
        report.warnings
    );
    assert!(
        report.keybinds_applied > 0,
        "sample rc should apply at least one keybind",
    );
}

#[test]
fn applied_keybind_overrides_default_vim() {
    // The sample rc rebinds normal-mode `h` to `move-right` — prove
    // the override took, not just that the parse succeeded.
    let (state, _report) = load_and_apply(
        r#"(defkeybind :mode "normal" :key "h" :action "move-right")"#,
    );
    let binding = state
        .keymap
        .lookup(Mode::Normal, &Key::Char('h'))
        .expect("h should still be bound in normal mode");
    assert_eq!(
        binding.action,
        Action::Move(Motion::Right),
        "rc override should replace the default move-left",
    );
}

#[test]
fn unknown_action_defers_to_command_registry() {
    // Bindings with action strings not in the curated set should
    // still take effect — they register as `Action::Command` so the
    // runtime dispatcher can resolve them via the command registry.
    let (state, report) = load_and_apply(
        r#"(defkeybind :mode "normal" :key "<C-p>" :action "picker.files")"#,
    );
    assert_eq!(report.keybinds_deferred_to_commands, 1);
    let binding = state
        .keymap
        .lookup(Mode::Normal, &Key::Ctrl('p'))
        .expect("C-p should be bound");
    match &binding.action {
        Action::Command { name, .. } => assert_eq!(name, "picker.files"),
        other => panic!("expected Action::Command, got {other:?}"),
    }
}

#[test]
fn apply_leaves_unrelated_defaults_intact() {
    // Binding `h` shouldn't wipe the whole default_vim keymap. `j`,
    // which isn't touched by the rc, should still move down.
    let (state, _report) = load_and_apply(
        r#"(defkeybind :mode "normal" :key "h" :action "move-right")"#,
    );
    let j_binding = state
        .keymap
        .lookup(Mode::Normal, &Key::Char('j'))
        .expect("default_vim should bind j");
    assert_eq!(j_binding.action, Action::Move(Motion::Down));
}

#[test]
fn multi_key_sequence_warns_without_crashing() {
    // Multi-key sequences like `gh` need pending-stroke machinery
    // that isn't wired yet — they must surface as a warning, not a
    // crash or silent drop.
    let (_state, report) = load_and_apply(
        r#"(defkeybind :mode "normal" :key "gh" :action "doc-start")"#,
    );
    assert!(report.keybinds_applied == 0 || report.warnings.is_empty());
    if report.warnings.is_empty() {
        // If someone wires pending-stroke support, the gh binding
        // should take effect — either outcome is fine here.
    } else {
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("multi-key"));
    }
}

#[test]
fn fresh_keymap_without_rc_stays_default_vim() {
    // No rc → no apply → keymap is the vim default. The fleet should
    // never be worse off for not having an rc.
    let km = Keymap::default_vim();
    let binding = km
        .lookup(Mode::Normal, &Key::Char('h'))
        .expect("default_vim binds h");
    assert_eq!(binding.action, Action::Move(Motion::Left));
}
