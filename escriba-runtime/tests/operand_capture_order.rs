//! **The operand-capture chain, and why its ORDER is a correctness property.**
//!
//! Four gestures take a keystroke that is an *argument*, not a binding —
//! `di(`, `fx`, `` `a ``, `rZ`. Each must claim that key before the sequence
//! stepper and before the keymap, or it resolves as whatever it happens to be
//! bound to: `i` enters Insert, `w` moves a word, `(` types a bracket.
//!
//! Until 2026-08-14 the chain was four near-identical inlined blocks in
//! `on_key`, and its order was protected by **nothing but the order the code
//! happened to be written in**. Comments explained each adjacency; no test
//! could see them, because an ordering expressed as statement sequence has
//! nothing to assert against.
//!
//! It is a table now, so this file can assert it. Every case below is a real
//! gesture driven through `on_key`, not a unit call — because the whole class
//! of bug here is "the wrong layer claimed the key", and only the real
//! dispatch path has layers.

use escriba_buffer::BufferSet;
use escriba_core::Position;
use escriba_keymap::Key;
use escriba_runtime::{EditorState, operand_capture_order};

fn editor(text: &str) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(text);
    EditorState::new_with_buffer(bufs, id)
}

fn press(st: &mut EditorState, keys: &str) {
    for c in keys.chars() {
        st.on_key(&Key::Char(c));
    }
}

fn text_of(st: &EditorState) -> String {
    st.buffers
        .get(st.active)
        .map(escriba_buffer::Buffer::to_string)
        .unwrap_or_default()
}

/// The order itself, pinned by name.
///
/// A reordering is now a failing test rather than a silently different editor.
/// If you change this list, the cases below tell you which gesture you broke.
#[test]
fn the_chain_order_is_mark_object_find_replace() {
    assert_eq!(
        operand_capture_order(),
        vec!["mark", "object", "find", "replace"],
        "the operand-capture order changed; each adjacency is a named \
         dependency — see this file's cases before accepting a new order",
    );
}

// ── adjacency 1: mark BEFORE object ──────────────────────────────────────

/// ``d`a`` — the failure this ordering exists for.
///
/// The object path claims `i`/`a` whenever an operator is armed, and a mark
/// letter can be `a`. With object first, the `a` of ``d`a`` is eaten as an
/// "around" object selector and the mark jump never happens.
#[test]
fn mark_before_object_so_a_backtick_a_delete_reaches_the_mark() {
    let mut st = editor("alpha\nbravo\ncharlie\n");
    press(&mut st, "ma"); // mark 'a' at (0,0)
    press(&mut st, "jj"); // to charlie
    assert_eq!(st.cursor(), Position::new(2, 0), "fixture");
    press(&mut st, "d`a"); // delete back to mark a
    assert_eq!(text_of(&st), "charlie\n", "the mark letter was eaten");
}

/// The other side of the same adjacency: putting mark first must NOT steal
/// `di'`. The mark path arms only while `pending_object` is clear, so the two
/// share a key without fighting over it.
#[test]
fn mark_first_does_not_steal_the_object_paths_quote() {
    let mut st = editor("say 'hello' now\n");
    press(&mut st, "0lllll"); // inside the quotes
    press(&mut st, "di'");
    assert_eq!(
        text_of(&st),
        "say '' now\n",
        "di' must still reach the object path"
    );
}

// ── adjacency 2: object BEFORE find ──────────────────────────────────────

/// `di(` must not read as `d`, then `i` (insert), then a literal `(`.
#[test]
fn object_before_find_so_di_paren_is_an_object_not_insert() {
    let mut st = editor("call(arg) end\n");
    press(&mut st, "0lllll");
    press(&mut st, "di(");
    assert_eq!(text_of(&st), "call() end\n");
    assert_eq!(
        st.modal.mode(),
        escriba_core::Mode::Normal,
        "`i` was consumed as an object selector, so Insert must not have been entered",
    );
}

// ── adjacency 3: find and replace are disjoint (stated, not assumed) ─────

/// `f` and `r` arm on different keys and neither can be pending while the
/// other is, so their relative order is stability rather than necessity. Both
/// still work with the chain as ordered — which is what makes the claim
/// checkable instead of just asserted in a comment.
#[test]
fn find_and_replace_do_not_contend() {
    let mut st = editor("abcdef\n");
    press(&mut st, "0fd"); // find 'd'
    assert_eq!(st.cursor(), Position::new(0, 3), "f reached its operand");

    let mut st2 = editor("abcdef\n");
    press(&mut st2, "0rZ"); // replace with 'Z'
    assert_eq!(text_of(&st2), "Zbcdef\n", "r reached its operand");
}

// ── adjacency 4: the whole chain BEFORE the sequence stepper ─────────────

/// `rZ` where `Z` could otherwise be a binding, and `fw` where `w` certainly
/// is. If the keymap saw these first, `w` would move a word.
#[test]
fn the_chain_runs_before_the_keymap() {
    // The line must contain a LATER WORD, or the assertion is vacuous: on a
    // one-word line `w` has nowhere to go, so an uncaptured `w` and a failed
    // find are the same cursor position and the test passes either way.
    let mut st = editor("abc def ghi\n");
    press(&mut st, "0fw"); // 'w' as a find OPERAND, not the word motion
    assert_eq!(
        st.cursor(),
        Position::new(0, 0),
        "no 'w' in the line, so the find fails and the cursor holds — a move \
         to column 4 means the keymap saw the key and ran the word motion",
    );

    let mut st2 = editor("abcdef\n");
    press(&mut st2, "0rw");
    assert_eq!(
        text_of(&st2),
        "wbcdef\n",
        "'w' was the replacement, not a motion"
    );
}

/// …but NOT before a gesture already in flight. Each capture declines while
/// `pending_keys` is non-empty, so `zt`'s `t` completes the `z` sequence
/// instead of arming a till-find. This is the bug that made `zt` unreachable
/// when the find capture first landed.
#[test]
fn a_later_key_of_a_sequence_is_not_captured_as_an_operand() {
    use std::fmt::Write as _;
    let long = (0..200).fold(String::new(), |mut s, i| {
        let _ = writeln!(s, "line {i}");
        s
    });
    let mut st = editor(&long);
    for _ in 0..60 {
        st.on_key(&Key::Char('j'));
    }
    let before = st.cursor();
    press(&mut st, "zt"); // re-frame; must NOT arm a till-find
    assert_eq!(
        st.cursor(),
        before,
        "zt re-frames without moving the cursor — if `t` armed a find, the \
         sequence never completed",
    );
}

/// The counting split is real and asymmetric, so it is pinned: the object path
/// applies its own repeats (`SelfCounted`) while the other three drain the
/// pending count (`Drained`). Getting this wrong squares the count.
#[test]
fn a_counted_find_repeats_once_per_count_not_count_squared() {
    let mut st = editor("a.b.c.d.e\n");
    press(&mut st, "0");
    press(&mut st, "3f."); // third '.'
    assert_eq!(
        st.cursor(),
        Position::new(0, 5),
        "3f. is the third dot, not the ninth"
    );
}
