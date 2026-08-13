//! `:wq` and the rest of the write/quit family, driven the way an operator
//! drives it: `:`, the characters, `⏎`.
//!
//! The spelling table has its own all-variants proof in
//! `escriba_command::ex` — every abbreviation resolving to a command name is
//! settled there. What is settled HERE is that those names reach a body that
//! writes the file and asks to exit. The two used to be provably fine
//! separately and broken together: `:w` saved, `:q` quit, and `:wq` reported
//! "command not found", because the compound spelling had nowhere to be
//! known.

use escriba_buffer::BufferSet;
use escriba_core::Mode;
use escriba_keymap::Key;
use escriba_runtime::EditorState;

/// An editor over a real file on disk, so a write is observable.
fn on_file(contents: &str) -> (EditorState, tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("note.txt");
    std::fs::write(&path, contents).expect("seed the file");
    let mut bufs = BufferSet::new();
    let id = bufs.open(&path).expect("open");
    (EditorState::new_with_buffer(bufs, id), dir, path)
}

fn scratch(contents: &str) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(contents);
    EditorState::new_with_buffer(bufs, id)
}

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

/// Type `text` into the buffer through Insert mode, then return to Normal.
fn type_into(st: &mut EditorState, text: &str) {
    st.on_key(&Key::Char('i'));
    for c in text.chars() {
        st.on_key(&Key::Char(c));
    }
    st.on_key(&Key::Esc);
}

fn said(st: &EditorState, needle: &str) -> bool {
    st.messages.iter().any(|m| m.contains(needle))
}

/// The headline: every spelling of write-and-quit does both halves.
#[test]
fn every_spelling_of_write_and_quit_writes_and_quits() {
    for spelling in [
        "wq", "wq!", "wqa", "wqall", "xa", "xall", "x", "xit", "exi", "exit",
    ] {
        let (mut st, _dir, path) = on_file("before\n");
        type_into(&mut st, "AFTER ");
        ex(&mut st, spelling);

        assert!(
            st.quit_requested,
            "`:{spelling}` must ask to exit — it did not",
        );
        let on_disk = std::fs::read_to_string(&path).expect("read back");
        assert!(
            on_disk.contains("AFTER "),
            "`:{spelling}` must write the buffer first; disk says {on_disk:?}",
        );
    }
}

/// `:x` writes only when there is something to write. The difference from
/// `:wq` is the file's mtime, and something is always watching the mtime.
#[test]
fn exit_write_leaves_an_unmodified_file_alone() {
    let (mut st, _dir, path) = on_file("untouched\n");
    let before = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

    ex(&mut st, "x");
    assert!(st.quit_requested, "`:x` exits whether or not it wrote");

    let after = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    assert_eq!(before, after, "`:x` on an unmodified buffer must not write");
}

/// The bang has to MEAN something, or `:q` and `:q!` are one key sequence
/// with two lengths and the editor discards unsaved work without a word.
#[test]
fn quit_refuses_unsaved_work_and_the_bang_overrides() {
    for (plain, forced) in [
        ("q", "q!"),
        ("quit", "quit!"),
        ("qa", "qa!"),
        ("qall", "qall!"),
    ] {
        let mut st = scratch("");
        type_into(&mut st, "unsaved");

        ex(&mut st, plain);
        assert!(
            !st.quit_requested,
            "`:{plain}` must refuse while a buffer is modified",
        );
        assert!(
            said(&st, "E37"),
            "the refusal must SAY so — a silent one reads as a dropped key: {:?}",
            st.messages,
        );

        ex(&mut st, forced);
        assert!(st.quit_requested, "`:{forced}` must exit anyway");
    }
}

/// Nothing modified, nothing to refuse.
#[test]
fn quit_leaves_immediately_when_there_is_nothing_to_lose() {
    for spelling in ["q", "qu", "quit", "qa", "qall", "quita", "quitall"] {
        let mut st = scratch("clean\n");
        ex(&mut st, spelling);
        assert!(st.quit_requested, "`:{spelling}` must exit a clean editor");
    }
}

/// `:wq` promised a write. A buffer with nowhere to write it must not exit
/// as though it had — that is the shape of losing a file.
#[test]
fn write_quit_on_an_unnamed_buffer_refuses_rather_than_losing_it() {
    let mut st = scratch("");
    type_into(&mut st, "nowhere to put this");
    ex(&mut st, "wq");
    assert!(
        !st.quit_requested,
        "`:wq` with no file name must not exit — the write it promised is impossible",
    );
    assert!(said(&st, "E32"), "and it must say why: {:?}", st.messages);
}

/// Every write spelling reaches the file, without touching the quit half.
#[test]
fn the_write_only_spellings_write_and_stay() {
    for spelling in ["w", "wr", "writ", "write", "wa", "wall"] {
        let (mut st, _dir, path) = on_file("before\n");
        type_into(&mut st, "AFTER ");
        ex(&mut st, spelling);

        assert!(!st.quit_requested, "`:{spelling}` must not exit");
        let on_disk = std::fs::read_to_string(&path).expect("read back");
        assert!(
            on_disk.contains("AFTER "),
            "`:{spelling}` must write; disk says {on_disk:?}",
        );
    }
}

/// The grammar covers the vim vocabulary; it must not fence the namespace.
/// A registry name that is not an ex verb still dispatches.
#[test]
fn a_registry_name_still_reaches_its_command() {
    let mut st = scratch("one\n");
    ex(&mut st, "buffer-info");
    assert!(
        said(&st, "buffer"),
        "a non-vim command name must still dispatch: {:?}",
        st.messages,
    );
}
