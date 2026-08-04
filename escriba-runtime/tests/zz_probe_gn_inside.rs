//! SCRATCH probe — delete after.
use escriba_buffer::BufferSet;
use escriba_runtime::EditorState;
use madori::event::{AppEvent, KeyCode, KeyEvent, Modifiers};

fn key(code: KeyCode) -> AppEvent {
    AppEvent::Key(KeyEvent {
        key: code,
        pressed: true,
        modifiers: Modifiers::default(),
        text: None,
    })
}
fn press(st: &mut EditorState, code: KeyCode) {
    st.tick(&key(code));
}
fn type_str(st: &mut EditorState, s: &str) {
    for c in s.chars() {
        press(st, KeyCode::Char(c));
    }
}
fn text_of(st: &EditorState) -> String {
    st.buffers
        .get(st.active)
        .map(escriba_buffer::Buffer::to_string)
        .unwrap_or_default()
}
fn search(st: &mut EditorState, p: &str) {
    press(st, KeyCode::Char('/'));
    type_str(st, p);
    press(st, KeyCode::Enter);
}
fn st_with(doc: &str) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(doc);
    EditorState::new_with_buffer(bufs, id)
}

#[test]
fn probe_dgn_cursor_strictly_inside_first_match() {
    let mut st = st_with("foo bar foo\n");
    search(&mut st, "foo");
    eprintln!("after search cursor = {:?}", st.cursor());
    press(&mut st, KeyCode::Char('l'));
    eprintln!("after l cursor = {:?}", st.cursor());
    press(&mut st, KeyCode::Char('d'));
    press(&mut st, KeyCode::Char('g'));
    press(&mut st, KeyCode::Char('n'));
    eprintln!("PROBE dgn(inside) -> {:?}", text_of(&st));
}

#[test]
fn probe_dgn_cursor_on_match_start() {
    let mut st = st_with("foo bar foo\n");
    search(&mut st, "foo");
    press(&mut st, KeyCode::Char('d'));
    press(&mut st, KeyCode::Char('g'));
    press(&mut st, KeyCode::Char('n'));
    eprintln!("PROBE dgn(start) -> {:?}", text_of(&st));
}

#[test]
fn probe_dgN_cursor_strictly_inside_second_match() {
    let mut st = st_with("foo bar foo\n");
    search(&mut st, "foo");
    press(&mut st, KeyCode::Char('n')); // second match, col 8
    eprintln!("after n cursor = {:?}", st.cursor());
    press(&mut st, KeyCode::Char('l')); // col 9, inside
    eprintln!("after l cursor = {:?}", st.cursor());
    press(&mut st, KeyCode::Char('d'));
    press(&mut st, KeyCode::Char('g'));
    press(&mut st, KeyCode::Char('N'));
    eprintln!("PROBE dgN(inside) -> {:?}", text_of(&st));
}

#[test]
fn probe_cgn_dot_from_inside_a_match() {
    let mut st = st_with("foo one\nfoo two\nfoo three\n");
    search(&mut st, "foo");
    press(&mut st, KeyCode::Char('l')); // cursor inside match 1
    press(&mut st, KeyCode::Char('c'));
    press(&mut st, KeyCode::Char('g'));
    press(&mut st, KeyCode::Char('n'));
    type_str(&mut st, "bar");
    press(&mut st, KeyCode::Escape);
    eprintln!("PROBE cgn(inside) -> {:?}", text_of(&st));
}
