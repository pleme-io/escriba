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

/// The GPU face's gutter geometry.
///
/// Pure arithmetic, and the one part of that face's gutter that can be WRONG
/// without a device: if the pixel offset and the reserved columns disagree,
/// the text either overlaps the line numbers or floats away from them, and
/// nothing in a headless test suite would notice.
mod gutter_geometry {
    use escriba_render::gpu::{cell_grid, gutter_px};
    use escriba_ui::gutter::gutter_width;

    #[test]
    fn the_pixel_offset_is_exactly_the_declared_columns() {
        // Derived from the SAME cell width `cell_grid` uses. Computed with a
        // second ratio, the text would start a fraction of a cell off the
        // gutter's edge — visible as a ragged left margin at some font sizes
        // and not others.
        for font_size in [10.0_f32, 14.0, 18.0, 24.0] {
            for line_count in [12_u32, 9_999, 10_000, 250_000] {
                let grid = cell_grid(1920, 1080, font_size, font_size * 1.4);
                let cell_w = 1920.0 / f32::from(grid.cols);
                let cols = gutter_width(line_count);
                let want = cell_w * cols as f32;
                let got = gutter_px(font_size, line_count);
                assert!(
                    (got - want).abs() <= cell_w,
                    "font {font_size}, {line_count} lines: gutter_px {got} is \
                     more than one cell off the {cols}-column width {want}",
                );
            }
        }
    }

    #[test]
    fn a_bigger_file_reserves_more_pixels() {
        // The whole reason the width is a function rather than a constant.
        // If this were flat, a five-digit line number would be painted into
        // the text column.
        let small = gutter_px(14.0, 9_999);
        let big = gutter_px(14.0, 10_000);
        assert!(
            big > small,
            "crossing into five digits must widen the gutter: {small} -> {big}",
        );
    }

    #[test]
    fn the_gutter_never_swallows_the_whole_window() {
        // A narrow window must still show text. `draw_buffer` subtracts the
        // gutter from the viewport with a saturating floor; this proves the
        // floor is reachable rather than theoretical.
        let grid = cell_grid(120, 400, 14.0, 20.0);
        assert!(
            u32::from(grid.cols) > 0,
            "a window always has at least one column",
        );
        let text_cols = u32::from(grid.cols).saturating_sub(gutter_width(10) as u32);
        assert!(
            text_cols.max(1) >= 1,
            "text columns must never reach zero — the file would vanish",
        );
    }
}

/// Syntax colours follow the editor's theme.
///
/// The GPU face held `hikari_core::NordTheme` by value, so `(deftheme :preset
/// "vellum")` repainted the chrome and left every keyword, string and comment
/// Nord — a theme that changed the frame and not the picture.
mod syntax_follows_the_theme {
    use escriba_ui::chrome::{ChromePalette, FleetTheme};
    use escriba_ui::syntax::{ALL_CLASSES, ChromeSyntax};
    use hikari_core::Theme;

    fn rgb(c: hikari_core::Rgb) -> (u8, u8, u8) {
        (c.r, c.g, c.b)
    }

    #[test]
    fn the_renderers_syntax_theme_is_built_from_the_chrome_it_paints() {
        // Same construction the render pass performs. If these diverged, the
        // code and the chrome would be resolving two different themes and the
        // window would be internally inconsistent.
        for theme in [
            FleetTheme::PlemeDark,
            FleetTheme::Vellum,
            FleetTheme::PolarVeil,
            FleetTheme::Bare,
        ] {
            let chrome = ChromePalette::for_theme(theme);
            let syn = ChromeSyntax::new(chrome);
            assert_eq!(
                syn.chrome().hex_tuple(),
                chrome.hex_tuple(),
                "{theme:?}: the syntax theme must resolve the chrome it was given",
            );
            // And a keyword must be the `link` role, not a constant.
            assert_eq!(
                rgb(syn.color(hikari_core::HlClass::Keyword)),
                (chrome.link.r, chrome.link.g, chrome.link.b),
            );
        }
    }

    #[test]
    fn switching_theme_moves_the_code_colours() {
        let a = ChromeSyntax::for_theme(FleetTheme::PlemeDark);
        let b = ChromeSyntax::for_theme(FleetTheme::Vellum);
        assert!(
            ALL_CLASSES
                .iter()
                .any(|c| rgb(a.color(*c)) != rgb(b.color(*c))),
            "a theme switch that leaves every syntax colour identical is the \
             bug this replaced",
        );
    }
}

// ─── paint_pieces: composing the lexer, the server and search ────────────
//
// Three colour sources meet at one partition, and getting the composition
// wrong is invisible: glyphon shapes whatever runs it is handed, so a gap, an
// overlap or a lost recolour renders as text that simply looks a bit off. The
// pieces are therefore checked directly rather than through anything drawn.

mod paint {
    use escriba_render::gpu::{Paint, paint_pieces};
    use hikari_core::HlClass;

    /// The invariant `set_rich_text` depends on, asserted on every case below:
    /// pieces are contiguous, in order, non-empty, and exactly cover the span.
    fn assert_partition(span: std::ops::Range<usize>, out: &[(std::ops::Range<usize>, Paint)]) {
        assert!(!out.is_empty(), "a span must yield at least one piece");
        assert_eq!(out[0].0.start, span.start, "starts where the span starts");
        assert_eq!(out[out.len() - 1].0.end, span.end, "ends where it ends");
        for (r, _) in out {
            assert!(r.start < r.end, "an empty piece is a shaping hazard: {r:?}");
        }
        for w in out.windows(2) {
            assert_eq!(w[0].0.end, w[1].0.start, "contiguous, no gap or overlap");
        }
    }

    /// With nothing overlaid, the lexer's answer stands — the fallback that
    /// every buffer with no language server relies on, which is most of them.
    #[test]
    fn with_no_overlay_the_lexer_keeps_the_whole_span() {
        let out = paint_pieces(0..10, HlClass::Variable, &[], &[]);
        assert_eq!(out, vec![(0..10, Paint::Class(HlClass::Variable))]);
        assert_partition(0..10, &out);
    }

    /// **The point of the whole feature.** hikari's lexer sees an identifier
    /// and says `Variable`; the server knows it is a call head and says
    /// `Function`. The server's answer must reach the pixel.
    ///
    /// RED RUN 2026-08-12: replacing the `is_token` arm's `class_at(…)` with
    /// plain `lexer` leaves the piece `Class(Variable)` and this fails — which
    /// is the shape of "the request went out, the reply decoded, and nothing
    /// on screen changed".
    #[test]
    fn a_server_token_overrides_the_lexers_guess() {
        let out = paint_pieces(0..3, HlClass::Variable, &[(0, 3, HlClass::Function)], &[]);
        assert_eq!(out, vec![(0..3, Paint::Class(HlClass::Function))]);
        assert_partition(0..3, &out);
    }

    /// A token covering PART of a lexer span recolours only its own bytes.
    ///
    /// The two partitions do not have to agree about where anything begins —
    /// hikari lexes `foo.bar` as one run where a server may claim only `bar` —
    /// so the cut has to happen inside the span, not around it.
    #[test]
    fn a_token_inside_a_lexer_span_recolours_only_its_own_bytes() {
        let out = paint_pieces(0..7, HlClass::Variable, &[(4, 7, HlClass::Function)], &[]);
        assert_eq!(
            out,
            vec![
                (0..4, Paint::Class(HlClass::Variable)),
                (4..7, Paint::Class(HlClass::Function)),
            ],
        );
        assert_partition(0..7, &out);
    }

    /// Two tokens inside one lexer span each keep their OWN class.
    ///
    /// The gate against resolving the class once per span instead of once per
    /// piece — which would paint both with whichever token happened to be
    /// found first and look entirely plausible.
    #[test]
    fn two_tokens_in_one_span_do_not_share_a_colour() {
        let out = paint_pieces(
            0..10,
            HlClass::Plain,
            &[(0, 3, HlClass::Keyword), (6, 10, HlClass::Str)],
            &[],
        );
        assert_eq!(
            out,
            vec![
                (0..3, Paint::Class(HlClass::Keyword)),
                (3..6, Paint::Class(HlClass::Plain)),
                (6..10, Paint::Class(HlClass::Str)),
            ],
        );
        assert_partition(0..10, &out);
    }

    /// **Search wins.** A hit inside a server-coloured token paints as a hit.
    ///
    /// Order matters and is the reason the LSP cut runs FIRST: a search
    /// highlight is a transient answer to "where is what I just typed", and a
    /// match that lost its colour to a token would make `n` step through
    /// matches the operator cannot see.
    ///
    /// RED RUN 2026-08-12: swapping the two `split_on_matches` passes (search
    /// first, then LSP) paints the matched bytes `Class(Function)` and fails
    /// here.
    #[test]
    fn a_search_match_outranks_a_server_token() {
        let out = paint_pieces(
            0..10,
            HlClass::Plain,
            &[(0, 10, HlClass::Function)],
            &[(4, 6)],
        );
        assert_eq!(
            out,
            vec![
                (0..4, Paint::Class(HlClass::Function)),
                (4..6, Paint::SearchMatch),
                (6..10, Paint::Class(HlClass::Function)),
            ],
        );
        assert_partition(0..10, &out);
    }

    /// A match straddling a token boundary stays one contiguous highlight and
    /// the token colour resumes either side of it.
    #[test]
    fn a_match_across_a_token_boundary_keeps_both_neighbours_intact() {
        let out = paint_pieces(
            0..12,
            HlClass::Plain,
            &[(0, 6, HlClass::Keyword), (6, 12, HlClass::Str)],
            &[(4, 8)],
        );
        assert_eq!(
            out,
            vec![
                (0..4, Paint::Class(HlClass::Keyword)),
                (4..6, Paint::SearchMatch),
                (6..8, Paint::SearchMatch),
                (8..12, Paint::Class(HlClass::Str)),
            ],
            "the two match halves are adjacent and identically painted, so \
             they read as one highlight",
        );
        assert_partition(0..12, &out);
    }

    /// Tokens that miss this span entirely change nothing — the common case
    /// once a whole screen's tokens are handed to every span in turn.
    #[test]
    fn tokens_outside_the_span_are_ignored() {
        let out = paint_pieces(
            10..20,
            HlClass::Comment { multiline: false },
            &[(0, 5, HlClass::Function), (30, 40, HlClass::Str)],
            &[],
        );
        assert_eq!(
            out,
            vec![(10..20, Paint::Class(HlClass::Comment { multiline: false }))],
        );
        assert_partition(10..20, &out);
    }

    /// A token boundary exactly on a span edge must not emit a zero-width
    /// piece — the same edge case the search overlay already pins, re-checked
    /// through the composed path because that is where it would now appear.
    #[test]
    fn boundaries_on_the_span_edges_produce_no_empty_pieces() {
        for lsp in [
            vec![(10usize, 20usize, HlClass::Str)],
            vec![(0, 10, HlClass::Str)],
            vec![(20, 30, HlClass::Str)],
            vec![(10, 15, HlClass::Str), (15, 20, HlClass::Keyword)],
        ] {
            let out = paint_pieces(10..20, HlClass::Plain, &lsp, &[(10, 12), (18, 20)]);
            assert_partition(10..20, &out);
        }
    }
}
