//! Search, driven by KEYSTROKES.
//!
//! Every pre-existing search test in the runtime calls
//! `st.apply(&Action::SearchOpen(..))` directly. Those tests are good, but
//! their own comment claimed they "prove the WIRING: that keys reach it" —
//! and they cannot, because `apply` is downstream of the part that decides
//! whether a key becomes that action at all. Deleting the `/` binding from
//! `escriba-keymap` left every one of them green.
//!
//! Everything here goes through `tick`, so it exercises the real path:
//!
//! ```text
//! AppEvent → translate_app_event → KeyRepeatGate → Keymap::dispatch → apply
//! ```
//!
//! That is four opportunities for a search to be unreachable that a direct
//! `apply` skips entirely.

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_runtime::{EditorState, PromptKind};
use madori::event::{AppEvent, KeyCode, KeyEvent, Modifiers};

const DOC: &str = "alpha bravo\ncharlie delta\nalpha echo\nfoxtrot alpha\n";

fn state() -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(DOC);
    EditorState::new_with_buffer(bufs, id)
}

fn key(code: KeyCode) -> AppEvent {
    AppEvent::Key(KeyEvent {
        key: code,
        pressed: true,
        modifiers: Modifiers::default(),
        text: None,
    })
}

/// Press one key through the full input path.
fn press(st: &mut EditorState, code: KeyCode) {
    st.tick(&key(code));
}

/// Type a literal string, one key at a time.
fn type_str(st: &mut EditorState, s: &str) {
    for c in s.chars() {
        press(st, KeyCode::Char(c));
    }
}

/// Press Ctrl+`c` through the full input path.
fn ctrl(st: &mut EditorState, c: char) {
    st.tick(&AppEvent::Key(KeyEvent {
        key: KeyCode::Char(c),
        pressed: true,
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
        text: None,
    }));
}

/// The active buffer's text.
fn text_of(st: &EditorState) -> String {
    st.buffers
        .get(st.active)
        .map(escriba_buffer::Buffer::to_string)
        .unwrap_or_default()
}

/// `/pattern<CR>` — entirely through keystrokes.
fn search(st: &mut EditorState, pattern: &str) {
    press(st, KeyCode::Char('/'));
    type_str(st, pattern);
    press(st, KeyCode::Enter);
}

// ── reachability: the bindings exist and produce a search ────────────────

#[test]
fn slash_opens_a_search_prompt() {
    let mut st = state();
    press(&mut st, KeyCode::Char('/'));

    assert_eq!(st.modal.mode(), Mode::Command, "`/` must enter the cmdline");
    assert!(
        st.search.is_prompting(),
        "`/` must open a SEARCH prompt, not an ex-line"
    );
    assert_eq!(st.status_model().prompt, PromptKind::SearchForward);
}

#[test]
fn question_mark_opens_a_backward_search_prompt() {
    let mut st = state();
    press(&mut st, KeyCode::Char('?'));

    assert!(st.search.is_prompting());
    assert_eq!(st.status_model().prompt, PromptKind::SearchBackward);
}

#[test]
fn typed_characters_reach_the_prompt_and_are_visible() {
    let mut st = state();
    press(&mut st, KeyCode::Char('/'));
    type_str(&mut st, "brav");

    let model = st.status_model();
    assert_eq!(
        model.prompt_text, "brav",
        "the pattern must reach the prompt"
    );

    // The whole point of the status model: a face can SEE it.
    let rendered = model.render();
    assert!(
        rendered.contains("/brav"),
        "prompt absent from status line: {rendered:?}"
    );
}

#[test]
fn slash_search_moves_the_cursor_through_real_keys() {
    let mut st = state();
    search(&mut st, "charlie");

    assert_eq!(st.cursor().line, 1, "landed on the matching line");
    assert_eq!(st.modal.mode(), Mode::Normal, "prompt closed on <CR>");
}

#[test]
fn n_and_shift_n_walk_the_matches() {
    let mut st = state();
    search(&mut st, "alpha"); // matches on lines 0, 2, 3

    let first = st.cursor().line;
    press(&mut st, KeyCode::Char('n'));
    let second = st.cursor().line;
    assert_ne!(first, second, "`n` must advance");

    press(&mut st, KeyCode::Char('N'));
    assert_eq!(st.cursor().line, first, "`N` must come back");
}

#[test]
fn star_searches_the_word_under_the_cursor() {
    let mut st = state();
    // Cursor starts at 0:0, on `alpha`.
    press(&mut st, KeyCode::Char('*'));

    assert!(st.search.pattern().is_some(), "`*` must arm a pattern");
    assert_ne!(st.cursor().line, 0, "`*` must jump to another occurrence");
}

// ── the preview/commit contract (T0.1) ───────────────────────────────────

#[test]
fn commit_lands_where_the_preview_showed() {
    // The regression: `submit_search` stepped from the LIVE cursor, which
    // incremental preview had already moved onto the match — so the exclusive
    // step skipped past it and committed to the NEXT match.
    let mut st = state();

    press(&mut st, KeyCode::Char('/'));
    type_str(&mut st, "alpha");
    let previewed = st.cursor().line;

    press(&mut st, KeyCode::Enter);
    assert_eq!(
        st.cursor().line,
        previewed,
        "committing must land on the match the preview was showing",
    );
}

#[test]
fn a_backward_search_commits_where_it_previewed() {
    let mut st = state();
    // Move down so a backward search has somewhere to go.
    for _ in 0..3 {
        press(&mut st, KeyCode::Char('j'));
    }

    press(&mut st, KeyCode::Char('?'));
    type_str(&mut st, "alpha");
    let previewed = st.cursor().line;

    press(&mut st, KeyCode::Enter);
    assert_eq!(
        st.cursor().line,
        previewed,
        "backward commit must not drift"
    );
}

#[test]
fn escape_abandons_the_prompt_and_restores_the_cursor() {
    let mut st = state();
    let origin = st.cursor().line;

    press(&mut st, KeyCode::Char('/'));
    type_str(&mut st, "foxtrot");
    assert_ne!(st.cursor().line, origin, "preview moved the cursor");

    press(&mut st, KeyCode::Escape);
    assert_eq!(
        st.cursor().line,
        origin,
        "Escape must return to where I searched from"
    );
    assert!(!st.search.is_prompting());
    assert_eq!(st.modal.mode(), Mode::Normal);
}

// ── the match count (T1.1) ───────────────────────────────────────────────

#[test]
fn the_count_reports_position_and_total() {
    use escriba_search::MatchCount;

    let mut st = state();
    search(&mut st, "alpha"); // three matches

    match st.status_model().count {
        MatchCount::Exact { current, total } => {
            assert_eq!(total, 3, "three `alpha`s in the fixture");
            assert_eq!(current, 1, "landed on the first");
        }
        other => panic!("expected an exact count, got {other:?}"),
    }

    press(&mut st, KeyCode::Char('n'));
    match st.status_model().count {
        MatchCount::Exact { current, .. } => assert_eq!(current, 2, "`n` advances the ordinal"),
        other => panic!("expected an exact count, got {other:?}"),
    }
}

#[test]
fn a_pattern_that_matches_nothing_says_so_before_enter() {
    use escriba_search::MatchCount;

    let mut st = state();
    press(&mut st, KeyCode::Char('/'));
    type_str(&mut st, "zzzz");

    assert_eq!(
        st.status_model().count,
        MatchCount::None,
        "`[0/0]` must appear while typing, not after committing",
    );
}

#[test]
fn an_empty_prompt_counts_nothing_rather_than_zero() {
    use escriba_search::MatchCount;

    let mut st = state();
    press(&mut st, KeyCode::Char('/'));

    assert_eq!(
        st.status_model().count,
        MatchCount::Idle,
        "an untyped prompt has nothing to report — [0/0] would be a false claim",
    );
}

#[test]
fn a_failed_search_reports_a_message_the_status_line_can_show() {
    let mut st = state();
    search(&mut st, "zzzz");

    let model = st.status_model();
    let msg = model.message.expect("a failed search must say so");
    assert!(msg.contains("E486"), "expected vim's E486, got {msg:?}");
    assert!(
        model.render().contains("E486"),
        "the message must reach the line"
    );
}

// ── highlights track the buffer (T0.2) ───────────────────────────────────

#[test]
fn editing_the_buffer_refreshes_the_match_offsets() {
    // The regression: `SearchState::refresh` had zero callers, so inserting
    // text left every highlight pointing at pre-edit offsets.
    let mut st = state();
    search(&mut st, "alpha");
    let before: Vec<_> = st.search.matches().to_vec();
    assert!(!before.is_empty());

    // Insert four characters at the very start of the document. The cursor
    // is already at 0:0, so no multi-key motion is needed — this test is
    // about the refresh, not about `gg`.
    press(&mut st, KeyCode::Char('i'));
    type_str(&mut st, "XXXX");
    press(&mut st, KeyCode::Escape);

    let after: Vec<_> = st.search.matches().to_vec();
    assert_eq!(after.len(), before.len(), "the same words still match");
    assert_ne!(
        after.first().map(|m| m.start),
        before.first().map(|m| m.start),
        "offsets must move with the text, or the highlight paints the wrong columns",
    );
}

// ── prompt editing ───────────────────────────────────────────────────────

#[test]
fn backspace_edits_the_prompt_rather_than_the_buffer() {
    let mut st = state();

    press(&mut st, KeyCode::Char('/'));
    type_str(&mut st, "charlieX");
    press(&mut st, KeyCode::Backspace);

    assert_eq!(st.status_model().prompt_text, "charlie");

    press(&mut st, KeyCode::Enter);
    assert_eq!(
        st.cursor().line,
        1,
        "the corrected pattern is what committed"
    );
    // If Backspace had edited the BUFFER instead of the prompt, the fixture
    // would no longer contain this line intact.
    assert!(st.search.matches().len() == 1);
}

#[test]
fn an_ex_command_is_not_mistaken_for_a_search() {
    let mut st = state();
    press(&mut st, KeyCode::Char(':'));

    assert!(!st.search.is_prompting(), "`:` is not a search");
    assert_eq!(st.status_model().prompt, PromptKind::Ex);
}

// ── the jumplist: search is a place you can come back from ───────────────

#[test]
fn ctrl_o_returns_to_where_the_search_was_launched_from() {
    let mut st = state();
    let origin = st.cursor().line;

    search(&mut st, "foxtrot");
    assert_ne!(st.cursor().line, origin, "the search moved us");

    press(&mut st, KeyCode::Char('o'));
    // `o` is not the jump key — Ctrl is required. Guard against a plain-char
    // binding accidentally satisfying this test.
    let after_plain_o = st.cursor().line;
    assert_ne!(after_plain_o, origin, "plain `o` must not jump back");

    let mut st = state();
    search(&mut st, "foxtrot");
    ctrl(&mut st, 'o');
    assert_eq!(
        st.cursor().line,
        origin,
        "<C-o> returns to the launch point"
    );
}

#[test]
fn ctrl_i_goes_forward_again() {
    let mut st = state();
    search(&mut st, "foxtrot");
    let landed = st.cursor().line;

    ctrl(&mut st, 'o');
    ctrl(&mut st, 'i');
    assert_eq!(st.cursor().line, landed, "<C-i> returns to the match");
}

#[test]
fn n_records_a_jump_so_it_is_not_a_one_way_door() {
    let mut st = state();
    search(&mut st, "alpha");
    let first = st.cursor().line;

    press(&mut st, KeyCode::Char('n'));
    assert_ne!(st.cursor().line, first);

    ctrl(&mut st, 'o');
    assert_eq!(st.cursor().line, first, "<C-o> undoes an `n`");
}

#[test]
fn ctrl_o_with_an_empty_jumplist_reports_rather_than_moving() {
    let mut st = state();
    let before = st.cursor().line;
    ctrl(&mut st, 'o');

    assert_eq!(st.cursor().line, before, "nothing to jump back to");
    assert!(
        st.status_model()
            .message
            .is_some_and(|m| m.contains("E662")),
        "an impossible jump must say so, not look like a dead key",
    );
}

// ── search as a motion: dn / d/foo<CR> (T1.2) ────────────────────────────

#[test]
fn d_then_n_deletes_to_the_next_match() {
    let mut st = state();
    search(&mut st, "alpha"); // lands on line 0
    let before = text_of(&st);

    press(&mut st, KeyCode::Char('d'));
    press(&mut st, KeyCode::Char('n'));

    let after = text_of(&st);
    assert_ne!(after, before, "`dn` must delete to the next match");
    assert!(after.len() < before.len(), "text got shorter");
}

#[test]
fn d_slash_pattern_cr_deletes_up_to_the_match() {
    // The mechanic the operator FSM's catch-all used to swallow entirely.
    let mut st = state();
    let before = text_of(&st);

    press(&mut st, KeyCode::Char('d'));
    press(&mut st, KeyCode::Char('/'));
    type_str(&mut st, "charlie");
    press(&mut st, KeyCode::Enter);

    let after = text_of(&st);
    assert_ne!(after, before, "`d/charlie<CR>` must delete");
    assert!(
        after.starts_with("charlie"),
        "everything up to the match should be gone, got {after:?}",
    );
}

#[test]
fn the_operator_survives_every_keystroke_of_the_pattern() {
    // Each typed character used to hit the catch-all and disarm `d`.
    let mut st = state();
    press(&mut st, KeyCode::Char('d'));
    press(&mut st, KeyCode::Char('/'));

    for c in "charlie".chars() {
        press(&mut st, KeyCode::Char(c));
        assert!(st.search.is_prompting(), "prompt died on {c:?}");
    }

    press(&mut st, KeyCode::Enter);
    assert!(
        text_of(&st).starts_with("charlie"),
        "operator was lost mid-pattern"
    );
}

#[test]
fn esc_during_an_operated_search_disarms_both() {
    let mut st = state();
    let before = text_of(&st);

    press(&mut st, KeyCode::Char('d'));
    press(&mut st, KeyCode::Char('/'));
    type_str(&mut st, "charlie");
    press(&mut st, KeyCode::Escape);

    assert_eq!(text_of(&st), before, "Esc must not delete anything");
    assert!(!st.search.is_prompting());

    // And the operator must be gone too — a following motion is a plain move.
    press(&mut st, KeyCode::Char('j'));
    assert_eq!(
        text_of(&st),
        before,
        "a stale operator would have deleted here"
    );
}

#[test]
fn dn_with_no_pattern_says_why_instead_of_doing_nothing() {
    let mut st = state();
    let before = text_of(&st);

    press(&mut st, KeyCode::Char('d'));
    press(&mut st, KeyCode::Char('n'));

    assert_eq!(text_of(&st), before, "nothing to delete to");
    assert!(
        st.status_model().message.is_some_and(|m| m.contains("E35")),
        "an aborted operator must report, got {:?}",
        st.status_model().message,
    );
}

#[test]
fn y_then_n_yanks_without_changing_the_buffer() {
    let mut st = state();
    search(&mut st, "alpha");
    let before = text_of(&st);

    press(&mut st, KeyCode::Char('y'));
    press(&mut st, KeyCode::Char('n'));

    assert_eq!(text_of(&st), before, "yank must not mutate");
    assert!(st.register().is_some(), "yank must fill the register");
}

// ── the repeat gate must not swallow a deliberate second press ───────────

#[test]
fn two_fast_n_presses_advance_twice() {
    // Measured before the fix: the KeyRepeatGate dropped the second `n` inside
    // its debounce window, which reads as a dead key.
    let mut st = state();
    search(&mut st, "alpha");
    let first = st.cursor().line;

    press(&mut st, KeyCode::Char('n'));
    let second = st.cursor().line;
    press(&mut st, KeyCode::Char('n'));
    let third = st.cursor().line;

    assert_ne!(first, second, "first `n` advanced");
    assert_ne!(
        second, third,
        "the immediately-following `n` must also advance"
    );
}

// ── highlighting decays on its own (T2.2) ────────────────────────────────

#[test]
fn moving_on_clears_the_highlight_but_keeps_the_pattern() {
    let mut st = state();
    search(&mut st, "alpha");
    assert!(st.search.highlight_enabled(), "a fresh search lights up");

    press(&mut st, KeyCode::Char('j'));
    assert!(
        !st.search.highlight_enabled(),
        "moving on ends the search — no `:noh` remap should be needed",
    );
    assert!(
        st.search.pattern().is_some(),
        "the PATTERN must survive the clear"
    );
}

#[test]
fn n_relights_after_an_auto_clear() {
    let mut st = state();
    search(&mut st, "alpha");
    press(&mut st, KeyCode::Char('j')); // clears
    assert!(!st.search.highlight_enabled());

    press(&mut st, KeyCode::Char('n'));
    assert!(
        st.search.highlight_enabled(),
        "using the matches shows them again"
    );
}

#[test]
fn arming_an_operator_does_not_clear_the_highlight() {
    // `d` is not "moving on" — `dn` still needs its matches visible.
    let mut st = state();
    search(&mut st, "alpha");
    press(&mut st, KeyCode::Char('d'));
    assert!(st.search.highlight_enabled());
}

// ── \c / \C case override (T2.4) ─────────────────────────────────────────

#[test]
fn backslash_c_forces_a_case_insensitive_match() {
    let mut bufs = escriba_buffer::BufferSet::new();
    let id = bufs.scratch("hello\nHELLO\n");
    let mut st = EditorState::new_with_buffer(bufs, id);

    // `HELLO` has uppercase, so smartcase alone would be case-SENSITIVE and
    // find one match. `\c` overrides that.
    search(&mut st, r"\cHELLO");
    assert_eq!(st.search.matches().len(), 2, "\\c must match both cases");
}

#[test]
fn backslash_capital_c_forces_case_sensitivity() {
    let mut bufs = escriba_buffer::BufferSet::new();
    let id = bufs.scratch("hello\nHELLO\n");
    let mut st = EditorState::new_with_buffer(bufs, id);

    // Lowercase pattern ⇒ smartcase would be insensitive and find both.
    search(&mut st, r"\Chello");
    assert_eq!(st.search.matches().len(), 1, "\\C must pin the case");
}

#[test]
fn the_override_is_kept_in_the_pattern_text_the_user_sees() {
    let mut st = state();
    search(&mut st, r"\calpha");
    assert_eq!(
        st.search.pattern().map(escriba_search::SearchPattern::raw),
        Some(r"\calpha"),
        "the prompt and history show what was typed, vim-style",
    );
}

// ── staleness is now a TYPE, not a discipline (memori::Anchored) ─────────

#[test]
fn an_edit_expires_the_match_ordinal_without_anyone_clearing_it() {
    use escriba_search::MatchCount;

    let mut st = state();
    search(&mut st, "alpha");
    assert!(
        matches!(st.status_model().count, MatchCount::Exact { .. }),
        "a landed search reports its ordinal",
    );

    // Change the text. NOTHING in the runtime clears `search_at` any more —
    // the ordinal is Anchored to the buffer's text revision, so it expires on
    // its own. Before this, an explicit invalidation line had to be
    // remembered, and `SearchState::refresh` is the cautionary tale: it
    // existed for exactly this purpose and had zero callers.
    press(&mut st, KeyCode::Char('i'));
    type_str(&mut st, "ZZZZ");
    press(&mut st, KeyCode::Escape);

    assert!(
        !matches!(st.status_model().count, MatchCount::Exact { .. }),
        "an ordinal into the OLD match set must not be displayed as current",
    );
}

#[test]
fn a_pure_cursor_move_does_not_expire_the_ordinal() {
    use escriba_search::MatchCount;

    // The distinction that makes the anchor useful rather than noisy: the
    // buffer's TEXT revision, not the refresh generation. `EditGen` bumps on
    // every action including this one, and keying on it would blank the count
    // after any keypress.
    let mut st = state();
    search(&mut st, "alpha");
    press(&mut st, KeyCode::Char('l'));

    assert!(
        matches!(st.status_model().count, MatchCount::Exact { .. }),
        "moving the cursor changed no text, so the ordinal is still true",
    );
}
