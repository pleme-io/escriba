//! Integration tests for the caixa-native plugin system: a plugin IS a
//! caixa (a dir with `caixa.lisp` + an `escriba/plugin.lisp` entry),
//! loaded by escriba's own parser and activated by applying its entry
//! def-forms to live editor state. Exercises the bundled example plugin
//! `examples/plugins/escriba-paredit` end to end.

use std::path::PathBuf;
use std::process::Command;

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_keymap::Key;
use escriba_runtime::EditorState;

fn example_plugin() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("plugins")
        .join("escriba-paredit")
}

#[test]
fn plugin_caixa_loads_and_applies_entry() {
    let root = example_plugin();
    let plugin = escriba_plugin::PluginCaixa::load(
        "escriba-paredit",
        "0.1.0",
        &["FileType: lisp".to_string()],
        &root,
    )
    .expect("load example plugin caixa");
    assert!(
        plugin.matches_filetype("lisp"),
        "paredit activates on lisp filetype"
    );
    assert!(!plugin.is_eager(), "a FileType-triggered plugin is lazy");

    // Activate: apply the plugin entry's def-forms to a fresh editor.
    let mut buffers = BufferSet::new();
    let id = buffers.scratch("");
    let mut state = EditorState::new_with_buffer(buffers, id);
    let plan = escriba_lisp::apply_source(&plugin.entry_src).expect("parse plugin entry");
    escriba_lisp::apply_plan_to_commands(&plan, &mut state.commands);
    escriba_lisp::apply_plan_to_options(&plan, &mut state.options);
    escriba_lisp::apply_plan_to_keymap(&plan, &mut state.keymap);

    // The paredit plugin's keybind + command + option all landed.
    assert!(
        state.keymap.lookup(Mode::Normal, &Key::Alt('f')).is_some(),
        "paredit <A-f> (forward-sexp) should be bound",
    );
    assert!(
        state.commands.contains("paredit-wrap-round"),
        "the plugin's defcmd should register into the command registry",
    );
    assert_eq!(
        state.options.get("paredit").map(String::as_str),
        Some("on"),
        "the plugin's defoption should land in the option store",
    );
}

#[test]
fn cli_plugin_load_reports_registration() {
    let out = Command::new(env!("CARGO_BIN_EXE_escriba"))
        .args(["plugin", "load"])
        .arg(example_plugin())
        .output()
        .expect("run `escriba plugin load`");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "plugin load should succeed: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(stdout.contains("escriba-paredit"), "got: {stdout:?}");
    assert!(stdout.contains("registered:"), "got: {stdout:?}");
}

#[test]
fn cli_plugin_list_runs() {
    let out = Command::new(env!("CARGO_BIN_EXE_escriba"))
        .args(["plugin", "list"])
        .output()
        .expect("run `escriba plugin list`");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("escriba plugins"), "got: {stdout:?}");
}
