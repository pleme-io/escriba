//! The GPU face's LOGIC, tested without a GPU.
//!
//! ## Why this file exists
//!
//! `GpuRenderer::render` needs a `madori::RenderContext` — a live wgpu
//! device, queue and surface view — so it cannot run in `cargo test`. For a
//! while that was reported honestly as "the GPU face has no test", which is
//! true and also a bad place to stop: it left the arithmetic that decides
//! WHERE things go and the mapping that decides WHAT COLOUR they are inside
//! an untestable function, alongside plumbing that genuinely is upstream's
//! problem.
//!
//! So the fallible parts were pulled out into pure functions, and this file
//! covers them. What remains untested in `render()` is glyphon and wgpu call
//! sequencing — buffer sizing, shaping, encoder submission — which is
//! upstream's contract, not escriba's.
//!
//! The split is worth stating plainly, because "the GPU face is tested" and
//! "the GPU face's logic is tested" are different claims and only the second
//! one is true.

use escriba_core::{Action, Mode};
use escriba_render::gpu::{CellGrid, cell_grid, mode_color, splash_runs};
use escriba_ui::chrome::{ChromePalette, FleetTheme};
use escriba_ui::splash::{Splash, SplashEntry, SplashRole};

// ─── cell_grid: pixels → characters ──────────────────────────────────────

/// Standard-ish defaults: `GpuRenderer::new` uses 14px / 20px line height.
const FONT: f32 = 14.0;
const LINE: f32 = 20.0;

#[test]
fn a_typical_window_maps_to_a_sane_grid() {
    // 1200x800 is escriba's default window. 14px mono ≈ 8.4px advance.
    let g = cell_grid(1200, 800, FONT, LINE);
    assert_eq!(g.cols, 142, "1200 / 8.4 = 142");
    assert_eq!(g.rows, 39, "800 / 20 = 40, minus the status row");
}

#[test]
fn exactly_one_row_is_reserved_for_the_status_line() {
    // The bug this prevents: `resize` subtracted a row after dividing and
    // `render` subtracted a line-height before dividing. Same intent, two
    // spellings — and two chances to reserve zero rows or two.
    for h in [200u32, 400, 800, 1080] {
        let g = cell_grid(1000, h, FONT, LINE);
        let unreserved = (h as f32 / LINE).floor() as u16;
        assert_eq!(
            g.rows,
            unreserved - 1,
            "height {h}: expected one reserved row",
        );
    }
}

#[test]
fn a_degenerate_surface_never_produces_a_zero_or_negative_grid() {
    // A zero-size surface happens during window creation and on minimise.
    // A grid of 0 would make `Splash::rows` return nothing (survivable) —
    // but a WRAPPED subtraction would be a panic in release-mode overflow
    // checks and a giant bogus grid otherwise.
    for (w, h) in [(0u32, 0u32), (1, 1), (0, 800), (1200, 0), (5, 19)] {
        let g = cell_grid(w, h, FONT, LINE);
        assert!(g.cols >= 1, "cols must stay positive at {w}x{h}: {g:?}");
        assert!(g.rows >= 1, "rows must stay positive at {w}x{h}: {g:?}");
    }
}

#[test]
fn a_degenerate_font_metric_does_not_divide_by_zero() {
    // font_size and line_height come from `with_font_size`, which is public
    // and unvalidated.
    for (fs, lh) in [(0.0f32, 20.0f32), (14.0, 0.0), (0.0, 0.0), (-5.0, -5.0)] {
        let g = cell_grid(800, 600, fs, lh);
        assert!(g.cols >= 1 && g.rows >= 1, "{fs}/{lh} → {g:?}");
    }
}

#[test]
fn a_bigger_window_is_never_a_smaller_grid() {
    // Monotonicity — a resize that grows the window must not shrink the
    // visible area, which would scroll the buffer under the operator.
    let mut prev = CellGrid { cols: 0, rows: 0 };
    for px in [200u32, 400, 800, 1600, 3200] {
        let g = cell_grid(px, px, FONT, LINE);
        assert!(g.cols >= prev.cols, "cols shrank at {px}");
        assert!(g.rows >= prev.rows, "rows shrank at {px}");
        prev = g;
    }
}

#[test]
fn a_larger_font_yields_fewer_cells() {
    let small = cell_grid(1200, 800, 10.0, 14.0);
    let large = cell_grid(1200, 800, 28.0, 40.0);
    assert!(large.cols < small.cols, "{large:?} vs {small:?}");
    assert!(large.rows < small.rows, "{large:?} vs {small:?}");
}

// ─── splash_runs: roles → colours ────────────────────────────────────────

fn splash() -> Splash {
    Splash {
        art: vec!["ESCRIBA".into()],
        tagline: "a modal editor".into(),
        entries: vec![SplashEntry {
            key: 'q',
            label: "quit".into(),
            action: Action::Quit,
        }],
        facts: vec!["v0".into()],
    }
}

/// Exactly what the GPU face hands glyphon — the real function, not a
/// reconstruction of it. A test that rebuilt this mapping itself would keep
/// passing after the renderer stopped calling it.
fn runs(theme: FleetTheme) -> Vec<(String, glyphon::Color)> {
    let chunks = splash().screen_chunks(80, 24);
    splash_runs(&chunks, &ChromePalette::for_theme(theme))
        .into_iter()
        .map(|(t, c)| (t.to_string(), c))
        .collect()
}

/// ishou colour → the glyphon colour the renderer would paint it as.
fn glyph(c: ishou_tokens::Rgb) -> glyphon::Color {
    glyphon::Color::rgba(c.r, c.g, c.b, 0xFF)
}

#[test]
fn every_visible_role_resolves_to_the_palettes_own_colour() {
    // A mis-mapped role paints menu keys as body text and renders
    // perfectly — glyphon cannot notice. This is the check that can.
    for theme in [FleetTheme::PlemeDark, FleetTheme::Vellum] {
        let c = ChromePalette::for_theme(theme);
        assert_eq!(SplashRole::Art.color(&c).hex(), c.info.hex(), "{theme:?}");
        assert_eq!(SplashRole::MenuKey.color(&c).hex(), c.accent.hex());
        assert_eq!(SplashRole::MenuLabel.color(&c).hex(), c.text.hex());
        assert_eq!(SplashRole::Footer.color(&c).hex(), c.text_dim.hex());
    }
}

#[test]
fn the_run_stream_reconstructs_the_screen_exactly() {
    // `set_rich_text` concatenates its runs and requires the result to BE
    // the text. A gap is a hole in the rendered screen — on the one face
    // that cannot be checked headlessly, so it is checked here.
    let stream = runs(FleetTheme::PlemeDark);
    let joined: String = stream.iter().map(|(t, _)| t.as_str()).collect();
    let expected: String = splash()
        .screen_chunks(80, 24)
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(joined, expected);
    assert!(
        stream.iter().all(|(t, _)| !t.is_empty()),
        "an empty run is a wasted span",
    );
}

#[test]
fn the_wordmark_is_painted_in_the_themes_art_colour() {
    // The real end of the chain: the bytes handed to glyphon for the
    // wordmark must be the palette's `info` role, per theme.
    for theme in [FleetTheme::PlemeDark, FleetTheme::Vellum] {
        let c = ChromePalette::for_theme(theme);
        let stream = runs(theme);
        let (_, color) = stream
            .iter()
            .find(|(t, _)| t.contains("ESCRIBA"))
            .expect("the wordmark is in the stream");
        assert_eq!(
            *color,
            glyph(c.info),
            "{theme:?} painted the wrong art colour"
        );
    }
}

#[test]
fn the_run_stream_repaints_when_the_theme_changes() {
    // If the GPU face ignored the palette, these two streams would be
    // byte-identical — which is exactly what it did before the wiring.
    let nord: Vec<glyphon::Color> = runs(FleetTheme::PlemeDark)
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    let vellum: Vec<glyphon::Color> = runs(FleetTheme::Vellum)
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    assert_eq!(nord.len(), vellum.len(), "same screen, same run count");
    assert_ne!(nord, vellum, "the GPU face ignored the theme");
}

// ─── mode_color: the status pill ─────────────────────────────────────────

#[test]
fn mode_pills_follow_the_active_theme() {
    // `mode_color` used to read `ChromePalette::prescribed()` internally,
    // which is precisely how the GPU status pill stayed on the fleet theme
    // while the rest of the face moved.
    for theme in [FleetTheme::PlemeDark, FleetTheme::Vellum] {
        let c = ChromePalette::for_theme(theme);
        assert_eq!(
            mode_color(&c, Mode::Normal).hex(),
            c.info.hex(),
            "{theme:?}"
        );
        assert_eq!(mode_color(&c, Mode::Insert).hex(), c.success.hex());
        assert_eq!(mode_color(&c, Mode::Visual).hex(), c.accent.hex());
        assert_eq!(mode_color(&c, Mode::Command).hex(), c.warning.hex());
    }
}

#[test]
fn the_four_pills_stay_distinguishable_under_every_theme() {
    for theme in [
        FleetTheme::PlemeDark,
        FleetTheme::Vellum,
        FleetTheme::PolarVeil,
        FleetTheme::Bare,
    ] {
        let c = ChromePalette::for_theme(theme);
        let mut seen = std::collections::BTreeSet::new();
        for m in [Mode::Normal, Mode::Insert, Mode::Visual, Mode::Command] {
            assert!(
                seen.insert(mode_color(&c, m).hex()),
                "{theme:?}: {m:?} duplicates another pill — the mode stops \
                 being glance-readable",
            );
        }
    }
}
