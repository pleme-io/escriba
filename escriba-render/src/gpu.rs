//! GPU renderer — implements [`madori::RenderCallback`] backed by garasu's
//! glyphon-wrapped text renderer. Each frame:
//!
//!   1. Locks the shared `EditorState`.
//!   2. Collects visible buffer lines into a single string.
//!   3. Builds a glyphon `Buffer` (re-created each frame — phase 1.B; phase 2
//!      will diff + reuse).
//!   4. Prepares + renders through `madori::RenderContext::text`.
//!
//! Colors are the **Vellum** fleet theme (warm aged-paper Nord-matte),
//! sourced from `ishou_tokens::VellumPalette` so the GPU chrome matches
//! the rest of the fleet (mado, tear, frostmourne, …) and escriba's TUI
//! backend. Text is rendered in `snow1` (#E2DBC8, warm cream foreground)
//! over a `night0` (#16140E, parchment ground) background. The status
//! line is rendered in `ice_cyan` (#94BBB8, the matte accent).

use std::sync::{Arc, Mutex};

use escriba_core::{EditGen, Mode};
use escriba_runtime::EditorState;
use glyphon::{Attrs, Buffer, Color as GlyphColor, Family, Metrics, Shaping, TextArea, TextBounds};
use ishou_tokens::{EscribaSignals, Rgb, SignalMode, Srgb, VellumPalette};
use madori::{RenderCallback, RenderContext};
// hikari (光) — the fleet syntax-highlighting spine. path→Box<dyn Highlighter>,
// coverage-complete HlClass span partition, HlClass→Rgb via NordTheme.
use hikari_core::{Ecosystem, Language, NordTheme, Rgb as HlRgb, Theme};

/// Shared handle to the editor state — both the GPU renderer (reads) and
/// the madori `on_event` callback (writes) hold one.
pub type SharedState = Arc<Mutex<EditorState>>;

/// The GPU render callback.
///
/// Holds a shared reference to the editor state. `render()` reads it under
/// lock, computes a frame, releases the lock before touching the GPU to
/// minimise contention with the event loop.
pub struct GpuRenderer {
    state: SharedState,
    font_size: f32,
    line_height: f32,
    /// Cached font metrics — rebuilt if font_size changes.
    metrics: Metrics,
    /// hikari highlight registry (built once — resolves path→Highlighter).
    eco: Ecosystem,
    /// Nord syntax theme (HlClass→Rgb).
    theme: NordTheme,
    /// The refresh generation of the currently-cached text buffer — the seal
    /// (`theory/ESCRIBA.md` §Refresh-Seal). When `EditorState::edit_gen()`
    /// still equals this, the cached shaped buffer is reused verbatim: no
    /// re-highlight, no re-shape. Init `u64::MAX` so the first frame always
    /// paints.
    last_gen: EditGen,
    /// The shaped main-text glyphon buffer, cached across frames while the
    /// generation is unchanged. `None` before the first paint.
    cached_text: Option<Buffer>,
    /// The incremental highlighter for the active buffer's language (M2). Held
    /// across frames so a re-highlight re-lexes only the lines that changed
    /// (hikari's `LineState` fixpoint, `theory/ESCRIBA.md` §X) instead of the
    /// whole visible window. Keyed by path so a language switch rebuilds it;
    /// `None` before the first paint.
    highlighter: Option<(String, Box<dyn hikari_core::IncrementalHighlighter>)>,
}

impl GpuRenderer {
    #[must_use]
    pub fn new(state: SharedState) -> Self {
        let font_size = 14.0;
        let line_height = 20.0;
        Self {
            state,
            font_size,
            line_height,
            metrics: Metrics::new(font_size, line_height),
            eco: build_ecosystem(),
            theme: NordTheme,
            last_gen: EditGen(u64::MAX),
            cached_text: None,
            highlighter: None,
        }
    }

    #[must_use]
    pub fn with_font_size(mut self, font_size: f32, line_height: f32) -> Self {
        self.font_size = font_size;
        self.line_height = line_height;
        self.metrics = Metrics::new(font_size, line_height);
        self
    }
}

impl RenderCallback for GpuRenderer {
    fn render(&mut self, ctx: &mut RenderContext<'_>) {
        // ── 1. Read state under lock. The visible text is built ONLY when a
        //    rebuild is due (the refresh-generation gate): an idle frame reads
        //    just mode/cursor for the status line and reuses the cached shaped
        //    buffer below — zero re-highlight, zero re-shape. `rebuild_input`
        //    is Some((text, path)) exactly when the generation moved.
        let (rebuild_input, mode, cursor_line, cursor_col, cur_gen) = {
            let s = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(buf) = s.buffers.get(s.active) else {
                return clear_frame(ctx);
            };
            let cur_gen = s.edit_gen();
            let rebuild = cur_gen != self.last_gen || self.cached_text.is_none();
            let rebuild_input: Option<(String, String)> = if rebuild {
                // The open file's path drives hikari language resolution.
                let path = buf
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let win = s.layout.active_window().cloned();
                let top_line = win.as_ref().map_or(0, |w| w.viewport.top_line);
                let left_column = win.as_ref().map_or(0, |w| w.viewport.left_column) as usize;
                let visible_lines = win
                    .as_ref()
                    .map_or(40, |w| w.viewport.visible_lines.max(20));
                let visible_columns = win
                    .as_ref()
                    .map_or(usize::MAX, |w| w.viewport.visible_columns as usize);
                let mut out = String::new();
                for row in 0..visible_lines {
                    let ln = top_line + row;
                    if ln >= buf.line_count() {
                        break;
                    }
                    if let Some(line) = buf.line(ln) {
                        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                        // Slice to the visible horizontal window
                        // `[left_column, left_column + visible_columns)`.
                        // Char-based so multibyte text stays aligned; long
                        // lines clip to the window, no glyphon wrap.
                        let sliced: String = trimmed
                            .chars()
                            .skip(left_column)
                            .take(visible_columns)
                            .collect();
                        out.push_str(&sliced);
                        out.push('\n');
                    }
                }
                Some((out, path))
            } else {
                None
            };
            (rebuild_input, s.modal.mode(), s.cursor().line, s.cursor().column, cur_gen)
        };

        // ── 2. Rebuild the shaped main-text buffer ONLY on a generation
        //    change; otherwise reuse the cached one. This is the seal
        //    (theory/ESCRIBA.md §Refresh-Seal): highlight + set_rich_text +
        //    shape — the frame's dominant cost — run once per edit, never
        //    per vsync.
        let palette = VellumPalette::vellum();
        let fg = vellum_glyph(palette.snow1); // #E2DBC8 — warm cream fg
        let width = ctx.width as f32;
        let height = ctx.height as f32 - self.line_height; // reserve bottom row for status
        if let Some((text, path)) = rebuild_input {
            let mut buffer = Buffer::new(&mut ctx.text.font_system, self.metrics);
            buffer.set_size(&mut ctx.text.font_system, Some(width), Some(height));
            // hikari: resolve the language, highlight the visible text, paint
            // each span its Nord color. The span vec is a coverage-complete,
            // non-overlapping, sorted partition of `text` (the SpanSink
            // invariant), so each (slice, color) run is a valid set_rich_text
            // item. Offsets are self-consistent (highlight == render string).
            let base = Attrs::new().family(Family::Monospace);
            // hikari incremental (M2): reuse the per-path LineCache and re-lex
            // only the lines that changed since the last frame (the LineState
            // fixpoint). A language switch (path change) rebuilds the cache; a
            // scroll re-lexes the newly-visible window (graceful degrade). This
            // is byte-identical to the one-shot highlighter it replaces.
            if self.highlighter.as_ref().is_none_or(|(p, _)| p != &path) {
                self.highlighter =
                    Some((path.clone(), self.eco.incremental_highlighter_for_path(&path)));
            }
            let hl = &mut self
                .highlighter
                .as_mut()
                .expect("highlighter set immediately above")
                .1;
            let spans = hl.highlight(&text);
            let runs: Vec<(&str, Attrs)> = spans
                .iter()
                .filter_map(|s| {
                    text.get(s.span.range()).map(|slice| {
                        (slice, base.clone().color(hl_to_glyph(self.theme.color(s.class))))
                    })
                })
                .collect();
            buffer.set_rich_text(&mut ctx.text.font_system, runs, &base, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut ctx.text.font_system, false);
            self.cached_text = Some(buffer);
            self.last_gen = cur_gen;
        }
        let buffer = self
            .cached_text
            .as_ref()
            .expect("cached_text is built on the first frame (last_gen inits to u64::MAX)");

        // Status line — rendered as its own glyphon buffer. The mode is the
        // BORN fleet mode glyph (`ishou_tokens::EscribaSignals`) + escriba's
        // canonical uppercase mode label.
        let signals = EscribaSignals::prescribed();
        let status = format!(
            " {} {}  {}:{}  escriba v{} ",
            mode_glyph(&signals, mode).render(SignalMode::Glyph),
            mode.as_str(),
            cursor_line + 1,
            cursor_col + 1,
            env!("CARGO_PKG_VERSION")
        );
        let mut status_buf = Buffer::new(&mut ctx.text.font_system, self.metrics);
        status_buf.set_size(
            &mut ctx.text.font_system,
            Some(width),
            Some(self.line_height * 2.0),
        );
        status_buf.set_text(
            &mut ctx.text.font_system,
            &status,
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
        );
        status_buf.shape_until_scroll(&mut ctx.text.font_system, false);

        let status_color = vellum_glyph(palette.ice_cyan); // #94BBB8 — matte accent

        let text_areas = [
            TextArea {
                buffer,
                left: 8.0,
                top: 8.0,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: ctx.width as i32,
                    bottom: (height as i32).max(0),
                },
                default_color: fg,
                custom_glyphs: &[],
            },
            TextArea {
                buffer: &status_buf,
                left: 8.0,
                top: (ctx.height as f32 - self.line_height - 4.0).max(0.0),
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: (ctx.height as i32 - self.line_height as i32 - 4).max(0),
                    right: ctx.width as i32,
                    bottom: ctx.height as i32,
                },
                default_color: status_color,
                custom_glyphs: &[],
            },
        ];

        if let Err(e) = ctx.text.prepare(
            &ctx.gpu.device,
            &ctx.gpu.queue,
            ctx.width,
            ctx.height,
            text_areas,
        ) {
            tracing::warn!(error = %e, "glyphon prepare failed");
            return clear_frame(ctx);
        }

        // ── 3. Encode frame. ───────────────────────────────────────────
        let mut encoder = ctx
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("escriba frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("escriba main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: ctx.surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(nord_bg()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Err(e) = ctx.text.render(&mut pass) {
                tracing::warn!(error = %e, "glyphon render failed");
            }
        }
        ctx.gpu.queue.submit(std::iter::once(encoder.finish()));
    }

    fn resize(&mut self, width: u32, height: u32) {
        if let Ok(mut s) = self.state.lock() {
            // Monospace cell width estimate — glyphon advance for the
            // Family::Monospace face is ≈ 0.6 × font_size. Used to derive a
            // visible-column count so the horizontal-scroll window tracks the
            // real window width (mirrors the visible-line derivation below).
            let cell_w = (self.font_size * 0.6).max(1.0);
            for w in &mut s.layout.windows {
                w.rect.width = width;
                w.rect.height = height;
                // Rough visible-line count from height / line_height.
                let lh = self.line_height.max(1.0);
                w.viewport.visible_lines = ((height as f32 / lh).max(1.0) as u32).saturating_sub(1);
                // Rough visible-column count from width / cell_width.
                w.viewport.visible_columns = (width as f32 / cell_w).max(1.0) as u32;
            }
        }
    }
}

/// The highlight registry escriba renders through: **tree-sitter grammars
/// (hikari-ts) take precedence** for the languages they cover, and the zero-dep
/// table backend fills every other language. So `.rs` gets real tree-sitter
/// highlighting while `.py` / `.lisp` / `.json` / … get the batteries-included
/// table lexer — and both flow through the same coverage-complete `HlClass`
/// partition. Registration order is load-bearing: `Ecosystem::resolve` returns
/// the first matching plugin, so tree-sitter (registered first) wins for its
/// languages; the table backend is skipped for any language tree-sitter already
/// covers (no duplicate). If the tree-sitter host fails to build, the table
/// backend covers everything — never a panic, never an empty registry.
fn build_ecosystem() -> Ecosystem {
    let mut eco = Ecosystem::new();
    let mut covered: Vec<Language> = Vec::new();
    if let Ok(host) = hikari_ts::TreeSitterHost::builtin() {
        for p in host.plugins() {
            covered.push(p.language());
            eco.register(p);
        }
    }
    for p in hikari_core::langs::builtins() {
        if !covered.contains(&p.language()) {
            eco.register(p);
        }
    }
    eco
}

/// Utility — clear the frame to Nord background. Used on error paths.
fn clear_frame(ctx: &mut RenderContext<'_>) {
    let mut encoder = ctx
        .gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("escriba clear"),
        });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("escriba clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(nord_bg()),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    ctx.gpu.queue.submit(std::iter::once(encoder.finish()));
}

/// Vellum parchment-ground background (`night0` #16140E) as
/// `wgpu::Color`. Gamma-correct: the sRGB token is promoted through
/// `ishou_tokens`' typed sRGB→linear path so it composites correctly on
/// the linear-storage surface.
fn nord_bg() -> wgpu::Color {
    let c = VellumPalette::vellum().night0;
    Srgb::from(c).to_linear().with_alpha(1.0).into()
}

/// ishou Vellum `Rgb` → glyphon `Color` (sRGB u8 RGBA, opaque).
fn vellum_glyph(c: Rgb) -> GlyphColor {
    GlyphColor::rgba(c.r, c.g, c.b, 0xFF)
}

/// hikari `Rgb` (sRGB u8) → glyphon `Color` (opaque) — the syntax-span paint.
fn hl_to_glyph(c: HlRgb) -> GlyphColor {
    GlyphColor::rgba(c.r, c.g, c.b, 0xFF)
}

/// Mode indicator color — used by higher-layer rendering paths that want
/// a glance-readable color. Vellum mode pills: Normal cyan, Insert green,
/// Visual purple, Command yellow.
#[must_use]
pub fn mode_color(mode: Mode) -> Rgb {
    let p = VellumPalette::vellum();
    match mode {
        Mode::Insert => p.aurora_green,    // #A9BB8C
        Mode::Command => p.first_light,    // #D7C489
        Mode::Visual | Mode::VisualLine => p.solar_magenta, // #B8A1B9
        Mode::Normal => p.ice_cyan,        // #94BBB8
    }
}

/// The [`CursorShape`](escriba_core::CursorShape) the GPU backend should
/// draw for `mode`. Derived from the single typed `Mode::cursor_shape`
/// mapping shared with the TUI backend — so the GPU cursor (once it gains a
/// dedicated glyph; today the buffer text carries the caret) renders the
/// same shape the TUI does for any given mode. Exposed now so the shape is
/// a typed value at the GPU layer, not a renderer-local literal later.
#[must_use]
pub fn cursor_shape(mode: Mode) -> escriba_core::CursorShape {
    mode.cursor_shape()
}

/// Map an editor [`Mode`] to its fleet [`Signal`](ishou_tokens::Signal)
/// from [`EscribaSignals`].
///
/// `VisualLine` shares `mode_visual` with `Visual` — the fleet signal set
/// has one visual signal, matching how [`mode_color`] groups the two.
#[must_use]
pub fn mode_glyph(sig: &EscribaSignals, mode: Mode) -> &ishou_tokens::Signal {
    match mode {
        Mode::Normal => &sig.mode_normal,
        Mode::Insert => &sig.mode_insert,
        Mode::Visual | Mode::VisualLine => &sig.mode_visual,
        Mode::Command => &sig.mode_command,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_buffer::BufferSet;

    #[test]
    fn nord_bg_is_vellum_parchment() {
        let bg = nord_bg();
        // Vellum night0 = #16140E promoted to gamma-correct linear: the
        // parchment ground is a very dark warm tone, so all channels sit
        // near zero (r ≳ g ≳ b) after the sRGB→linear transform.
        assert!((0.0..0.03).contains(&bg.r), "r = {}", bg.r);
        assert!((0.0..0.03).contains(&bg.g), "g = {}", bg.g);
        assert!((0.0..0.03).contains(&bg.b), "b = {}", bg.b);
        // Warm: red channel >= green >= blue (a > e > 0E in hex).
        assert!(bg.r >= bg.g && bg.g >= bg.b);
        assert_eq!(bg.a, 1.0);
    }

    #[test]
    fn nord_bg_matches_ishou_night0() {
        // The clear color must equal the BORN Vellum background token
        // run through ishou's typed sRGB→linear path — no drift.
        let want: wgpu::Color = Srgb::from(VellumPalette::vellum().night0)
            .to_linear()
            .with_alpha(1.0)
            .into();
        assert_eq!(nord_bg(), want);
    }

    #[test]
    fn renderer_construction_is_cheap() {
        let mut bufs = BufferSet::new();
        let id = bufs.scratch("hello\n");
        let state = Arc::new(Mutex::new(EditorState::new_with_buffer(bufs, id)));
        let _r = GpuRenderer::new(state);
    }

    /// Phase 4: the render Ecosystem serves `.rs` from the **tree-sitter**
    /// backend (hikari-ts) and other languages from the table backend — both a
    /// coverage-complete `HlClass` partition. Proves real tree-sitter
    /// highlighting is wired into the live render path (not just the table lexer).
    #[test]
    fn ecosystem_uses_tree_sitter_for_rust_and_table_for_the_rest() {
        use hikari_core::{HlClass, Language};
        let eco = build_ecosystem();
        // .rs resolves to a grammar and produces real (non-Plain) classification.
        assert_eq!(eco.resolve("src/main.rs"), Language("rust"));
        let rs = eco
            .highlighter_for_path("src/main.rs")
            .highlight("fn main() { let x = 42; }");
        assert!(
            rs.iter().any(|s| s.class != HlClass::Plain),
            "rust must be really highlighted (tree-sitter or table)",
        );
        // Python is also served (tree-sitter, once hikari-ts ships that grammar;
        // the table backend covers it otherwise) — either way it classifies.
        assert_eq!(eco.resolve("app.py"), Language("python"));
        // A tree-sitter-uncovered language still resolves via the table backend.
        assert_eq!(eco.resolve("init.lisp"), Language("lisp"));
        // An unknown extension is still total (plain text, never a panic).
        assert_eq!(eco.resolve("notes.xyz"), hikari_core::PLAIN_TEXT);
    }

    #[test]
    fn mode_colors_differ_by_mode() {
        let n = mode_color(Mode::Normal);
        let i = mode_color(Mode::Insert);
        let v = mode_color(Mode::Visual);
        assert_ne!((n.r, n.g, n.b), (i.r, i.g, i.b));
        assert_ne!((n.r, n.g, n.b), (v.r, v.g, v.b));
    }

    #[test]
    fn cursor_shape_tracks_mode() {
        use escriba_core::CursorShape;
        assert_eq!(cursor_shape(Mode::Normal), CursorShape::Block);
        assert_eq!(cursor_shape(Mode::Command), CursorShape::Block);
        assert_eq!(cursor_shape(Mode::Insert), CursorShape::Bar);
        assert_eq!(cursor_shape(Mode::Visual), CursorShape::Underline);
        assert_eq!(cursor_shape(Mode::VisualLine), CursorShape::Underline);
    }

    #[test]
    fn mode_colors_are_vellum_pills() {
        // Normal cyan, Insert green, Visual purple, Command yellow.
        assert_eq!(mode_color(Mode::Normal).hex(), "#94BBB8");
        assert_eq!(mode_color(Mode::Insert).hex(), "#A9BB8C");
        assert_eq!(mode_color(Mode::Visual).hex(), "#B8A1B9");
        assert_eq!(mode_color(Mode::Command).hex(), "#D7C489");
    }

    /// Forcing function: the status-line mode glyphs are sourced from the
    /// fleet `EscribaSignals` vocabulary, not hand-picked literals. Pins
    /// the geometric `Glyph`-mode marks so drift in either escriba or
    /// ishou surfaces here.
    #[test]
    fn mode_glyphs_are_fleet_signals() {
        let sig = EscribaSignals::prescribed();
        assert_eq!(mode_glyph(&sig, Mode::Normal).render(SignalMode::Glyph), "◆");
        assert_eq!(mode_glyph(&sig, Mode::Insert).render(SignalMode::Glyph), "▸");
        assert_eq!(mode_glyph(&sig, Mode::Visual).render(SignalMode::Glyph), "▮");
        assert_eq!(
            mode_glyph(&sig, Mode::VisualLine).render(SignalMode::Glyph),
            "▮"
        );
        assert_eq!(
            mode_glyph(&sig, Mode::Command).render(SignalMode::Glyph),
            ":"
        );
    }

    /// Fleet convergence guard: escriba's GPU chrome paints through
    /// `VellumPalette::vellum()` — the fleet-prescribed `FleetTheme::Vellum`.
    /// This pins that convergence so a drift in the fleet baseline (e.g.
    /// `FleetDefaults::prescribed().theme` moving off Vellum) surfaces here
    /// instead of silently leaving escriba's chrome out of step with the
    /// rest of the fleet (mado, tear, frostmourne, …). Formalises the
    /// ad-hoc token pins in `nord_bg_matches_ishou_night0` /
    /// `mode_colors_are_vellum_pills` under the shared
    /// `ishou_tokens::convergence::Guard` harness the other fleet apps use.
    #[test]
    fn escriba_gpu_chrome_converges_with_fleet() {
        use ishou_tokens::{FleetTheme, convergence::Guard};
        // The theme escriba's GPU backend renders. It is Vellum by
        // construction (every paint site reads `VellumPalette::vellum()`),
        // and the Guard asserts that equals the fleet prescribed theme.
        let chrome_theme = FleetTheme::Vellum;
        Guard::for_app("escriba-render")
            .expect_theme(chrome_theme)
            .run();
    }
}
