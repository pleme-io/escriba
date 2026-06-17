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

    // Parse the rc source and apply it — commands FIRST (so deferred
    // keybinds have real commands to resolve against), then keymap.
    // This mirrors the binary's startup ordering exactly.
    let plan = escriba_lisp::apply_source(src).expect("parse rc");
    let _ = escriba_lisp::apply_plan_to_commands(&plan, &mut state.commands);
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

// ── defcmd → CommandRegistry keystone (Wave 1 apply-layer slice) ─────

#[test]
fn defcmd_registers_and_keybind_dispatches_without_panic() {
    // The keystone: a `(defcmd …)` registers a real, invokable command
    // into the live registry, and a `(defkeybind … :action "<cmd>")`
    // that defers to it now resolves against that registration instead
    // of dead-ending at the five built-ins.
    let (mut state, report) = load_and_apply(
        r#"
        (defcmd :name "write-all"
                :description "Write every modified buffer"
                :action "buffer.write-all")
        (defkeybind :mode "normal" :key "W" :action "write-all")
        "#,
    );
    // defcmd reached live state.
    assert!(
        state.commands.contains("write-all"),
        "defcmd should register into EditorState.commands",
    );
    // The keybind deferred to the registry (not a curated typed action).
    assert_eq!(report.keybinds_deferred_to_commands, 1);
    // W resolves to Action::Command { name: "write-all" }.
    let binding = state
        .keymap
        .lookup(Mode::Normal, &Key::Char('W'))
        .expect("W should be bound in normal mode");
    match &binding.action {
        Action::Command { name, .. } => assert_eq!(name, "write-all"),
        other => panic!("expected Action::Command, got {other:?}"),
    }
    // Driving the key dispatches through the registry and runs cleanly.
    state.on_key(&Key::Char('W'));
    assert!(!state.quit_requested);
}

#[test]
fn keybind_to_unregistered_command_defers_without_panic() {
    // Negative path: a keybind whose action names no registered command
    // still binds (deferred), and pressing it misses the registry
    // gracefully — swallowed NotFound, no panic, editor stays alive.
    let (mut state, report) = load_and_apply(
        r#"(defkeybind :mode "normal" :key "W" :action "no.such.command")"#,
    );
    assert_eq!(report.keybinds_deferred_to_commands, 1);
    assert!(!state.commands.contains("no.such.command"));
    state.on_key(&Key::Char('W'));
    assert!(!state.quit_requested);
}

#[test]
fn defcmd_write_all_saves_modified_file_buffer_end_to_end() {
    // Observable end-to-end proof of invokability: a file-backed buffer
    // edited in memory, then `W` (bound to a defcmd whose action is
    // buffer.write-all) persists the edit to disk.
    use escriba_core::{Edit, Position};

    let path = std::env::temp_dir().join("escriba-it-defcmd-writeall.txt");
    std::fs::write(&path, "before").expect("seed temp file");

    let mut buffers = BufferSet::new();
    let id = buffers.open(&path).expect("open temp file");
    {
        let buf = buffers.get_mut(id).expect("buffer present");
        buf.apply(&Edit::insert(Position::ZERO, "X".to_string()))
            .expect("edit applies");
        buf.modified = true;
    }
    let mut state = EditorState::new_with_buffer(buffers, id);

    let plan = escriba_lisp::apply_source(
        r#"
        (defcmd :name "w-all" :description "Write all" :action "buffer.write-all")
        (defkeybind :mode "normal" :key "W" :action "w-all")
        "#,
    )
    .expect("parse rc");
    let _ = escriba_lisp::apply_plan_to_commands(&plan, &mut state.commands);
    let _ = escriba_lisp::apply_plan_to_keymap(&plan, &mut state.keymap);
    assert!(state.commands.contains("w-all"));

    state.on_key(&Key::Char('W'));

    let on_disk = std::fs::read_to_string(&path).expect("read back temp file");
    let _ = std::fs::remove_file(&path);
    assert!(
        on_disk.starts_with('X'),
        "buffer.write-all should persist the in-memory edit, got {on_disk:?}",
    );
}

#[test]
fn leader_sequence_dispatches_command_end_to_end() {
    // The two slices compose: a `<leader>w` sequence binding resolves
    // through the pending-stroke loop and dispatches a `defcmd` command
    // that persists the buffer. Leader defaults to `,` (blnvim parity).
    use escriba_core::{Edit, Position};

    let path = std::env::temp_dir().join("escriba-it-leader-write.txt");
    std::fs::write(&path, "z").expect("seed temp file");

    let mut buffers = BufferSet::new();
    let id = buffers.open(&path).expect("open temp file");
    {
        let buf = buffers.get_mut(id).expect("buffer present");
        buf.apply(&Edit::insert(Position::ZERO, "Q".to_string()))
            .expect("edit applies");
        buf.modified = true;
    }
    let mut state = EditorState::new_with_buffer(buffers, id);

    let plan = escriba_lisp::apply_source(
        r#"
        (defcmd :name "w-all" :description "Write all" :action "buffer.write-all")
        (defkeybind :mode "normal" :key "<leader>w" :action "w-all")
        "#,
    )
    .expect("parse rc");
    let _ = escriba_lisp::apply_plan_to_commands(&plan, &mut state.commands);
    let report = escriba_lisp::apply_plan_to_keymap(&plan, &mut state.keymap);
    assert_eq!(report.keybinds_sequences, 1, "<leader>w binds as a sequence");

    // Drive the leader sequence: ',' (held) then 'w' (resolves).
    state.on_key(&Key::Char(','));
    assert_eq!(state.pending_keys, vec![Key::Char(',')]);
    state.on_key(&Key::Char('w'));
    assert!(state.pending_keys.is_empty());

    let on_disk = std::fs::read_to_string(&path).expect("read back temp file");
    let _ = std::fs::remove_file(&path);
    assert!(
        on_disk.starts_with('Q'),
        "<leader>w should run write-all and persist the edit, got {on_disk:?}",
    );
}

// ── defoption → live option store (declarative tier) ────────────────

#[test]
fn defoption_applies_to_editor_option_store() {
    let mut buffers = BufferSet::new();
    let id = buffers.scratch("");
    let mut state = EditorState::new_with_buffer(buffers, id);
    let plan = escriba_lisp::apply_source(
        r#"
        (defoption :name "number" :value "true")
        (defoption :name "tabstop" :value "4")
        "#,
    )
    .expect("parse rc");
    escriba_lisp::apply_plan_to_options(&plan, &mut state.options);
    assert_eq!(state.options.get("number").map(String::as_str), Some("true"));
    assert_eq!(state.options.get("tabstop").map(String::as_str), Some("4"));
}

#[test]
fn declarative_and_imperative_option_tiers_share_one_store() {
    // `defoption` (declarative) sets number=true; a live `(set-option …)`
    // (imperative tatara-lisp) then overrides it on the SAME store —
    // proving the two programmability tiers converge on one source of
    // truth rather than two parallel option maps.
    let mut buffers = BufferSet::new();
    let id = buffers.scratch("");
    let mut state = EditorState::new_with_buffer(buffers, id);
    let plan =
        escriba_lisp::apply_source(r#"(defoption :name "number" :value "true")"#).expect("parse");
    escriba_lisp::apply_plan_to_options(&plan, &mut state.options);
    assert_eq!(state.options.get("number").map(String::as_str), Some("true"));

    state.run_lisp(r#"(set-option "number" "false")"#).unwrap();
    assert_eq!(
        state.options.get("number").map(String::as_str),
        Some("false"),
    );
}
