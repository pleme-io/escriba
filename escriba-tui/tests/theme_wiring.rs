//! `(deftheme :preset …)` must reach the pixels.
//!
//! ## What was broken
//!
//! `ThemeSpec::resolve()` has always turned a preset name into a real
//! `ishou_tokens::FleetTheme`, and it was always correct. Nothing consumed
//! it. Every paint site — a dozen in the TUI face, several in the GPU one —
//! called `ChromePalette::prescribed()` for itself, so the editor painted
//! the FLEET default no matter what the operator authored. An operator
//! could write `(deftheme :preset "vellum")`, watch `--list-rc` report it,
//! and see Nord on screen.
//!
//! The seam (`ChromePalette::for_theme`) existed the whole time. The
//! missing piece was a caller.
//!
//! ## What these assert
//!
//! Cells, not palettes. A test that only checked `state.chrome()` would
//! have passed on the broken code too — the palette was never the problem,
//! the paint path's refusal to read it was.

use escriba_buffer::BufferSet;
use escriba_runtime::EditorState;
use escriba_ui::chrome::{ChromePalette, FleetTheme};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

const W: u16 = 40;
const H: u16 = 6;

fn rgb(c: ishou_tokens::Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

fn editor(theme: FleetTheme) -> EditorState {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("hello world\n");
    let mut st = EditorState::new_with_buffer(bufs, id);
    st.set_theme(theme);
    st
}

/// The foreground/background actually painted into the buffer pane's first
/// text cell — what an operator's eye lands on.
fn painted_cell(st: &EditorState) -> (Option<Color>, Option<Color>) {
    let mut term = Terminal::new(TestBackend::new(W, H)).expect("test terminal");
    term.draw(|f| escriba_tui::draw_frame(f, st)).expect("draw");
    let buf = term.backend().buffer().clone();
    // (0,0) is the gutter's first cell — styled from the same palette.
    let cell = &buf[(0, 0)];
    (cell.fg.into(), cell.bg.into())
}

#[test]
fn switching_the_theme_changes_the_painted_cells() {
    let nord = painted_cell(&editor(FleetTheme::PlemeDark));
    let vellum = painted_cell(&editor(FleetTheme::Vellum));
    assert_ne!(
        nord, vellum,
        "the two themes painted identical cells — the paint path is \
         ignoring the editor's theme again",
    );
}

#[test]
fn the_painted_ground_is_the_themes_own_ground() {
    // Not merely "different" — the RIGHT colour. A paint path that read
    // some other palette would still differ between themes.
    for theme in [
        FleetTheme::PlemeDark,
        FleetTheme::Vellum,
        FleetTheme::PolarVeil,
    ] {
        let (_, bg) = painted_cell(&editor(theme));
        assert_eq!(
            bg,
            Some(rgb(ChromePalette::for_theme(theme).background)),
            "{theme:?} painted the wrong ground",
        );
    }
}

#[test]
fn the_start_screen_follows_the_theme_too() {
    // The splash resolves its colours through the same ChromePalette seam,
    // so it must not be a face that quietly kept the default.
    use escriba_core::Action;
    use escriba_ui::splash::{Splash, SplashEntry};

    let splash = || Splash {
        art: vec!["ESCRIBA".into()],
        tagline: "t".into(),
        entries: vec![SplashEntry {
            key: 'q',
            label: "quit".into(),
            action: Action::Quit,
        }],
        facts: vec!["v0".into()],
    };
    let shot = |theme| {
        let mut st = editor(theme);
        st.set_splash(splash());
        let mut term = Terminal::new(TestBackend::new(60, 20)).expect("terminal");
        term.draw(|f| escriba_tui::draw_frame(f, &st))
            .expect("draw");
        let buf = term.backend().buffer().clone();
        // Find the wordmark row and read the colour it was painted in.
        (0..20u16)
            .flat_map(|y| (0..60u16).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "E")
            .map(|(x, y)| buf[(x, y)].fg)
    };
    let nord = shot(FleetTheme::PlemeDark);
    let vellum = shot(FleetTheme::Vellum);
    assert!(nord.is_some() && vellum.is_some(), "wordmark not found");
    assert_ne!(nord, vellum, "the start screen ignored the theme");
}

#[test]
fn a_theme_change_invalidates_the_cached_frame() {
    // The GPU face reuses its shaped glyphon buffer while the refresh
    // generation is unchanged. A theme change that did not bump it would
    // leave the old colours on screen until an unrelated edit happened to
    // invalidate the cache — a repaint bug that only shows up in the one
    // face with no headless test.
    let mut st = editor(FleetTheme::PlemeDark);
    let before = st.edit_gen();
    st.set_theme(FleetTheme::Vellum);
    assert_ne!(before, st.edit_gen(), "theme change must force a repaint");
}

#[test]
fn setting_the_same_theme_twice_costs_no_repaint() {
    let mut st = editor(FleetTheme::Vellum);
    let settled = st.edit_gen();
    st.set_theme(FleetTheme::Vellum);
    assert_eq!(settled, st.edit_gen(), "a no-op must not force a repaint");
}

#[test]
fn a_fresh_editor_starts_on_the_fleet_theme() {
    let mut bufs = BufferSet::new();
    let id = bufs.scratch("");
    let st = EditorState::new_with_buffer(bufs, id);
    assert_eq!(
        st.theme(),
        FleetTheme::prescribed_default(),
        "the default must be the FLEET's, never a hand-written name",
    );
    assert_eq!(
        st.chrome().hex_tuple(),
        ChromePalette::prescribed().hex_tuple(),
    );
}
