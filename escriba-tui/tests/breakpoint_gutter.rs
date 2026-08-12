//! A breakpoint you can toggle and SEE — asserted from the painted cells.
//!
//! ## Why every assertion here reads the FRAME
//!
//! The obvious test — toggle, then ask `EditorState::breakpoints()` whether a
//! breakpoint is set — is a tautology: it checks that a `BTreeSet` inserted
//! what it was told to insert, and it stays green for every way the mark can
//! fail to reach a face. `escriba` has this lesson recorded twice already
//! (the formatter's three round-trip laws all held while every call rendered
//! as a method send; `StatusModel::prompt_caret` was correct for months while
//! no face drew it). So these read the ratatui `TestBackend` buffer, which is
//! literal cells and cannot be satisfied by state that never paints.

use escriba_buffer::BufferSet;
use escriba_core::{Mode, Position, Range};
use escriba_keymap::Key;
use escriba_runtime::EditorState;
use escriba_shirube::{Finding, Origin, ResultList, Severity, Site};
use escriba_ui::gutter::{breakpoint_glyph, gutter_width, mark_glyph};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const W: u16 = 44;
const H: u16 = 10;

const SRC: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n";

fn editor(text: &str) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(text);
    EditorState::new_with_buffer(bufs, id)
}

/// Drive an ex line through the REAL key path, the way the operator does.
fn ex(st: &mut EditorState, line: &str) {
    st.on_key(&Key::Char(':'));
    for c in line.chars() {
        st.on_key(&Key::Char(c));
    }
    st.on_key(&Key::Enter);
    if st.modal.mode() == Mode::Command {
        st.on_key(&Key::Esc);
    }
}

fn frame(st: &EditorState) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(W, H)).expect("terminal");
    term.draw(|f| escriba_tui::draw_frame(f, st)).expect("draw");
    let buf = term.backend().buffer().clone();
    (0..H)
        .map(|y| (0..W).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect()
}

/// Just the gutter columns of every painted row.
///
/// Sliced at the width the MODEL declares rather than at a literal, so this
/// helper survives the gutter growing a third plane — and so a widening that
/// forgot to reserve its column would show up as a shifted text character
/// rather than as a silently-passing comparison.
fn gutters(st: &EditorState) -> Vec<String> {
    let cols = gutter_width(
        st.buffers
            .get(st.active)
            .expect("the buffer we opened")
            .line_count(),
    );
    frame(st)
        .into_iter()
        .map(|row| row.chars().take(cols).collect())
        .collect()
}

/// A hard error on `line`, published the way a language server would.
fn publish_error(st: &mut EditorState, line: u32) {
    let world = st.world();
    let finding = Finding::new(
        Site::in_buffer(st.active, Range::point(Position::new(line, 0))),
        Severity::Error,
        "boom",
        Origin::Text("test"),
    );
    st.results
        .publish("diagnostics", ResultList::new(vec![finding], world));
}

#[test]
fn toggling_a_breakpoint_changes_the_painted_gutter_and_toggling_back_restores_it() {
    // RED RUN (2026-08-12): dropping the `Breakpoint` arm from `gutter_cells`
    // (always emitting the blank `NoBreakpoint` cell) leaves `before == after`
    // and fails on "the gutter for line 2 must CHANGE".
    //
    // What this test canNOT see, stated so nobody assumes otherwise: the
    // refresh-generation bump. Both testable faces repaint from scratch every
    // draw, so removing it reddens nothing here — only the GPU face caches its
    // shaped gutter. That claim is gated in `escriba-runtime`'s
    // `setting_a_breakpoint_repaints`, against the generation counter itself.
    let mut st = editor(SRC);
    st.on_key(&Key::Char('j'));
    st.on_key(&Key::Char('j'));
    assert_eq!(st.cursor().line, 2, "precondition: the cursor moved");

    let before = gutters(&st);
    ex(&mut st, "dap.toggle-breakpoint");
    let after = gutters(&st);

    assert_ne!(
        before[2], after[2],
        "the gutter for line 2 must CHANGE when a breakpoint is set there",
    );
    assert!(
        after[2].contains(breakpoint_glyph()),
        "and it must change INTO a breakpoint: {:?}",
        after[2],
    );
    for row in 0..H as usize {
        if row == 2 {
            continue;
        }
        assert_eq!(
            before[row], after[row],
            "row {row}'s gutter must not move — one breakpoint marks one line",
        );
    }

    ex(&mut st, "dap.toggle-breakpoint");
    assert_eq!(
        gutters(&st),
        before,
        "toggling again must restore the gutter exactly",
    );
}

#[test]
fn a_breakpoint_and_an_error_on_one_line_are_both_painted() {
    // THE reason `gutter_cells` took a widened signature instead of reusing
    // its single mark cell. Both facts are true of line 2; a gutter that can
    // only say one of them hides the other with no indication it did.
    //
    // RED RUN (2026-08-12): collapsing the two cells back into one — emitting
    // the breakpoint glyph when set and the severity glyph otherwise — fails
    // here on the `✖` assertion, while
    // `toggling_a_breakpoint_changes_the_painted_gutter…` above stays GREEN.
    // That is the pair working: the first test proves the mark reaches the
    // screen, and only this one proves it did not evict something.
    let mut st = editor(SRC);
    publish_error(&mut st, 2);
    let with_error_only = gutters(&st);
    assert!(
        with_error_only[2].contains(mark_glyph(Severity::Error)),
        "precondition: the error paints on its own: {:?}",
        with_error_only[2],
    );

    st.on_key(&Key::Char('j'));
    st.on_key(&Key::Char('j'));
    ex(&mut st, "dap.toggle-breakpoint");

    let both = gutters(&st);
    assert!(
        both[2].contains(breakpoint_glyph()),
        "the breakpoint must paint: {:?}",
        both[2],
    );
    assert!(
        both[2].contains(mark_glyph(Severity::Error)),
        "and the error must STILL paint beside it: {:?}",
        both[2],
    );
}

#[test]
fn a_breakpoint_survives_the_edit_that_would_kill_a_finding() {
    // The whole difference between a breakpoint and a finding, and the reason
    // `Breakpoints` is a field rather than a published `ResultList`. Findings
    // dodge the shifting problem by DYING on the next keystroke; a breakpoint
    // the operator set must not vanish because they typed.
    //
    // RED RUN (2026-08-12): sealing the breakpoint against `self.world()` at
    // toggle time and reading it back through `Anchor::is_fresh` — which is
    // exactly what `ResultList::new(findings, world)` plus `worst_on_line`'s
    // `fresh(world)` flat-map does — fails THIS test and only this test; the
    // other three in this file stayed green. The error assertion below is the
    // control: it shows the edit really did invalidate an anchored list, so a
    // surviving breakpoint is not merely an edit that did nothing.
    let mut st = editor(SRC);
    publish_error(&mut st, 2);
    st.on_key(&Key::Char('j'));
    st.on_key(&Key::Char('j'));
    ex(&mut st, "dap.toggle-breakpoint");
    assert!(gutters(&st)[2].contains(breakpoint_glyph()), "precondition");

    // Type on ANOTHER line, so nothing has touched line 2's own text.
    st.on_key(&Key::Char('j'));
    st.on_key(&Key::Char('i'));
    st.on_key(&Key::Char('x'));
    st.on_key(&Key::Esc);

    let after = gutters(&st);
    assert!(
        after[2].contains(breakpoint_glyph()),
        "the operator's breakpoint must survive their typing: {:?}",
        after[2],
    );
    assert!(
        !after[2].contains(mark_glyph(Severity::Error)),
        "control: the anchored finding DID die on the same edit, so the line \
         above is a real survival and not an edit that changed nothing: {:?}",
        after[2],
    );
}

#[test]
fn setting_a_breakpoint_does_not_move_the_text_column() {
    // The breakpoint cell is reserved on every line of every buffer,
    // including buffers that have none. If it were grown on demand — vim's
    // `signcolumn=auto` — the whole file would slide one column sideways on
    // the keystroke that sets the first breakpoint.
    let mut st = editor(SRC);
    let rule_before: Vec<Option<usize>> = frame(&st)
        .iter()
        .map(|l| l.chars().position(|c| c == '│'))
        .collect();
    ex(&mut st, "dap.toggle-breakpoint");
    let rule_after: Vec<Option<usize>> = frame(&st)
        .iter()
        .map(|l| l.chars().position(|c| c == '│'))
        .collect();
    assert_eq!(
        rule_before, rule_after,
        "the gutter rule — and therefore the first text column — must not move",
    );
}

// The catalog's `:DapToggleBreakpoint` alias is driven end-to-end in
// `escriba/tests/breakpoint_end_to_end.rs` instead: the catalog lives in the
// binary crate, and registering a stand-in alias here would be testing a
// fixture rather than the shipped declaration.
