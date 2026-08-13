use escriba_buffer::BufferSet;
use escriba_keymap::Key;
use escriba_runtime::EditorState;
use std::fmt::Write as _;

fn press(st: &mut EditorState, keys: &str) { for c in keys.chars() { st.on_key(&Key::Char(c)); } }

#[test]
fn probe() {
    let text = (0..200).fold(String::new(), |mut s, i| { let _ = writeln!(s, "line {i}"); s });
    let mut bufs = BufferSet::new();
    let id = bufs.scratch(&text);
    let mut st = EditorState::new_with_buffer(bufs, id);
    for _ in 0..60 { st.on_key(&Key::Char('j')); }
    let vp = |s: &EditorState| s.layout.active_window().map(|w| (w.viewport.top_line, w.viewport.visible_lines));
    println!("before: cursor={:?} vp={:?}", st.cursor(), vp(&st));
    press(&mut st, "dd");
    println!("after dd: cursor={:?} vp={:?}", st.cursor(), vp(&st));
    press(&mut st, "dd");
    println!("after dd2: cursor={:?} vp={:?}", st.cursor(), vp(&st));
    panic!("show");
}
