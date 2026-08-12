//! `<leader>db` sets a breakpoint the operator can SEE — through the SHIPPED
//! catalog, the real keymap, and a real painted frame.
//!
//! ## Why this is not covered by the two ratchets it accompanies
//!
//! `action_resolution.rs` proves `dap.toggle-breakpoint` RESOLVES and
//! `alias_revival.rs` proves `:DapToggleBreakpoint` DISPATCHES. Neither says
//! anything reaches a face. `lsp.format` passes both and is invisible on every
//! face escriba has — which is correct for a formatter and would be a silent
//! failure for a mark whose entire purpose is to be looked at.
//!
//! So this drives the composite boot plan (`escriba-dap.escribaplugin.lisp` is
//! one of the 45 bundled caixas, and `<leader>` is `,` from the same rc) and
//! reads the ANSI frame. `escriba-tui/tests/breakpoint_gutter.rs` makes the
//! same claim against the ratatui face's literal cells; two faces, one model,
//! and the third (GPU) cannot run under `cargo test` at all.

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_keymap::Key;
use escriba_render::{Renderer, TextRenderer};
use escriba_runtime::EditorState;
use escriba_ui::gutter::breakpoint_glyph;

const SRC: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n";

/// An editor booted the way the binary boots it: a buffer, then the composite
/// shipped plan applied to the command registry and the keymap, with the
/// leader resolved in between — the order `escriba::run` uses, because
/// `<leader>` is resolved at BIND time and a keymap built before the option
/// store would bind `,db` to nothing.
fn shipped_editor() -> EditorState {
    let mut buffers = BufferSet::new();
    let id = buffers.scratch(SRC);
    let mut state = EditorState::new_with_buffer(buffers, id);
    let plan = escriba::default_plan(false).expect("shipped defaults parse");
    let _ = escriba_lisp::apply_plan_to_commands(&plan, &mut state.commands);
    let _ = escriba_lisp::apply_plan_to_options(&plan, &mut state.options);
    if let Some(v) = state.options.get("mapleader")
        && let Some(k) = escriba_lisp::parse_leader_key(v)
    {
        state.keymap.set_leader(k);
    }
    let _ = escriba_lisp::apply_plan_to_keymap(&plan, &mut state.keymap);
    state.dismiss_splash();
    state
}

/// The gutter columns of every painted row of the ANSI face.
///
/// Split on the rule rather than on `gutter_width`, because this face emits
/// SGR escapes between cells: a character count would slice through an escape
/// sequence and compare noise. The rule is the last gutter glyph, so
/// everything before it is gutter.
fn gutter_rows(st: &EditorState) -> Vec<String> {
    TextRenderer
        .render_frame(st)
        .lines()
        .filter(|l| l.contains('\u{2502}'))
        .map(|l| l.split('\u{2502}').next().unwrap_or("").to_string())
        .collect()
}

#[test]
fn the_shipped_leader_db_paints_a_breakpoint_on_the_cursors_line() {
    // RED RUN (2026-08-12): deleting the `dap.toggle-breakpoint` registration
    // from `CommandRegistry::default_set` fails this on the glyph assertion —
    // the keybind still resolves to `Action::Command`, still dispatches, and
    // paints nothing. That is the exact failure mode `action_resolution.rs`
    // cannot see, which is why this test exists beside it.
    let mut st = shipped_editor();
    st.on_key(&Key::Char('j'));
    st.on_key(&Key::Char('j'));
    assert_eq!(st.cursor().line, 2, "precondition: the cursor moved");
    assert_eq!(st.modal.mode(), Mode::Normal, "precondition: still Normal");

    let before = gutter_rows(&st);
    assert!(
        !before[2].contains(breakpoint_glyph()),
        "precondition: nothing is marked yet: {:?}",
        before[2],
    );

    // The catalog's own binding, pressed. `,` is held as a pending stroke,
    // `d` extends it, `b` resolves `<leader>db`.
    st.on_key(&Key::Char(','));
    assert_eq!(
        st.pending_keys,
        vec![Key::Char(',')],
        "the leader must be HELD, not dispatched — if `,` resolved on its own \
         the rest of this test would be measuring a different key",
    );
    st.on_key(&Key::Char('d'));
    st.on_key(&Key::Char('b'));
    assert!(
        st.pending_keys.is_empty(),
        "the sequence must have resolved: {:?}",
        st.pending_keys,
    );

    let after = gutter_rows(&st);
    assert!(
        after[2].contains(breakpoint_glyph()),
        "<leader>db must paint a breakpoint on line 3: {:?}",
        after[2],
    );
    for (row, (b, a)) in before.iter().zip(after.iter()).enumerate() {
        if row != 2 {
            assert_eq!(b, a, "row {row} must be untouched");
        }
    }

    // And it is a TOGGLE, driven by the same three keys.
    st.on_key(&Key::Char(','));
    st.on_key(&Key::Char('d'));
    st.on_key(&Key::Char('b'));
    assert_eq!(
        gutter_rows(&st),
        before,
        "pressing <leader>db again must restore the gutter exactly",
    );
}

#[test]
fn the_catalogs_dap_toggle_breakpoint_command_reaches_the_mark() {
    // `escriba-dap.escribaplugin.lisp` also declares
    // `(defcmd :name "DapToggleBreakpoint" :action "dap.toggle-breakpoint")`.
    // `alias_revival.rs` proves that alias no longer answers `Unhandled`;
    // this proves what it reaches is the thing that paints.
    let mut st = shipped_editor();
    st.on_key(&Key::Char(':'));
    for c in "DapToggleBreakpoint".chars() {
        st.on_key(&Key::Char(c));
    }
    st.on_key(&Key::Enter);
    if st.modal.mode() == Mode::Command {
        st.on_key(&Key::Esc);
    }
    assert!(
        gutter_rows(&st)[0].contains(breakpoint_glyph()),
        "`:DapToggleBreakpoint` must mark the cursor's line: {:?}",
        gutter_rows(&st)[0],
    );
}

#[test]
fn the_editor_says_what_it_did() {
    // A gutter cell an operator has never seen before is not self-explanatory,
    // and the status line is where every other refusal and confirmation in
    // this editor lands. 1-based, matching the number painted beside the mark.
    let mut st = shipped_editor();
    st.on_key(&Key::Char('j'));
    st.on_key(&Key::Char(','));
    st.on_key(&Key::Char('d'));
    st.on_key(&Key::Char('b'));
    assert_eq!(
        st.messages.last().map(String::as_str),
        Some("breakpoint set at line 2"),
        "{:?}",
        st.messages,
    );
    st.on_key(&Key::Char(','));
    st.on_key(&Key::Char('d'));
    st.on_key(&Key::Char('b'));
    assert_eq!(
        st.messages.last().map(String::as_str),
        Some("breakpoint cleared at line 2"),
        "{:?}",
        st.messages,
    );
}

#[test]
fn the_other_six_dap_verbs_are_still_inert_and_that_is_deliberate() {
    // Teeth against the obvious over-claim. This slice shipped a MARK, not a
    // debugger: there is no adapter, no session and nothing to step. If a
    // later wave wires them, `action_resolution.rs`'s set-equality ratchet
    // fails until its INERT list is trimmed — and this test fails too, in the
    // one file that says out loud why the six were left.
    let mut registry = escriba_command::CommandRegistry::default_set();
    let plan = escriba::default_plan(false).expect("shipped defaults parse");
    escriba_lisp::apply_plan_to_commands(&plan, &mut registry);
    for verb in [
        "dap.continue",
        "dap.repl",
        "dap.step-into",
        "dap.step-out",
        "dap.step-over",
        "dap.toggle-ui",
    ] {
        assert!(
            !registry.contains(verb),
            "`{verb}` is registered — if debugging landed, trim INERT in \
             action_resolution.rs and update this list",
        );
    }
    assert!(
        registry.contains("dap.toggle-breakpoint"),
        "and the one verb this slice DID wire must be there, or the negatives \
         above would pass on an empty registry",
    );
}
