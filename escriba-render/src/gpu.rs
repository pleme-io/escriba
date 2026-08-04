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
//! sourced from `escriba_ui::chrome::ChromePalette` so the GPU chrome matches
//! the rest of the fleet (mado, tear, frostmourne, …) and escriba's TUI
//! backend. Text is rendered in `snow1` (#E2DBC8, warm cream foreground)
//! over a `night0` (#16140E, parchment ground) background. The status
//! line is rendered in `ice_cyan` (#94BBB8, the matte accent).

use std::sync::{Arc, Mutex};

use escriba_core::{EditGen, Mode};
use escriba_runtime::EditorState;
use escriba_ui::chrome::ChromePalette;
use glyphon::{Attrs, Buffer, Color as GlyphColor, Family, Metrics, Shaping, TextArea, TextBounds};
use ishou_tokens::{EscribaSignals, Rgb, SignalMode, Srgb};
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
    /// The resolved CHROME palette this renderer paints with.
    ///
    /// Held as state rather than re-derived per paint site, so a theme is
    /// a VALUE the renderer owns and `set_theme` can change at runtime.
    /// Every site previously called `ChromePalette::prescribed()` directly,
    /// which hardwired the paint path to the FLEET default and made
    /// `(deftheme :preset …)` inert no matter what the operator authored.
    chrome: ChromePalette,
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
            // Nord (the fleet prescribed default) until a config resolves
            // otherwise — never a hand-written constant, so a fleet
            // re-point lands here for free.
            chrome: ChromePalette::prescribed(),
            last_gen: EditGen(u64::MAX),
            cached_text: None,
            highlighter: None,
        }
    }

    /// Point the renderer at a theme — the wiring that makes
    /// `(deftheme :preset …)` real.
    ///
    /// `ChromePalette::for_theme` is total over `FleetTheme` (no wildcard
    /// arm), so a theme added upstream fails this to compile rather than
    /// silently painting the wrong thing.
    pub fn set_theme(&mut self, theme: ishou_tokens::FleetTheme) {
        self.chrome = ChromePalette::for_theme(theme);
    }

    /// The palette currently painted with.
    #[must_use]
    pub fn chrome(&self) -> ChromePalette {
        self.chrome
    }

    /// Builder form of [`Self::set_theme`].
    #[must_use]
    pub fn with_theme(mut self, theme: ishou_tokens::FleetTheme) -> Self {
        self.set_theme(theme);
        self
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
        let (rebuild_input, mode, status_core, cur_gen) = {
            let s = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(buf) = s.buffers.get(s.active) else {
                return clear_frame(ctx);
            };
            let cur_gen = s.edit_gen();
            let rebuild = cur_gen != self.last_gen || self.cached_text.is_none();
            // (rendered text, path, search-match byte ranges INTO that text).
            // The match ranges ride along with the text they index so the two
            // cannot be computed against different frames.
            let rebuild_input: Option<(String, String, Vec<(usize, usize)>)> = if rebuild {
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
                // Search matches are DOCUMENT char offsets; `out` is a
                // RECONSTRUCTED string (each row trimmed of \r\n, char-sliced
                // to the horizontal window, then \n-joined). There is
                // therefore NO single base offset relating the two — the map
                // has to be built per row, while we still know what each row
                // corresponds to. Converting here, at the one place both
                // coordinate systems are in scope, is what keeps byte/char
                // confusion out of the painting code below.
                let mut match_bytes: Vec<(usize, usize)> = Vec::new();
                let hl = s.search.highlights();
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
                        if !hl.is_empty() {
                            let seg_byte0 = out.len();
                            // Document char span this rendered segment covers.
                            let doc0 = buf
                                .position_to_char(escriba_core::Position::new(ln, 0))
                                .unwrap_or(0)
                                + left_column;
                            let seg_chars = sliced.chars().count();
                            // char index -> byte index within this segment.
                            let bytes: Vec<usize> = sliced
                                .char_indices()
                                .map(|(b, _)| b)
                                .chain(std::iter::once(sliced.len()))
                                .collect();
                            for m in hl {
                                let a = m.start.max(doc0);
                                let b = m.end.min(doc0 + seg_chars);
                                if a < b {
                                    match_bytes.push((
                                        seg_byte0 + bytes[a - doc0],
                                        seg_byte0 + bytes[b - doc0],
                                    ));
                                }
                            }
                        }
                        out.push_str(&sliced);
                        out.push('\n');
                    }
                }
                Some((out, path, match_bytes))
            } else {
                None
            };
            (
                rebuild_input,
                s.modal.mode(),
                s.status_model().render(),
                cur_gen,
            )
        };

        // ── 2. Rebuild the shaped main-text buffer ONLY on a generation
        //    change; otherwise reuse the cached one. This is the seal
        //    (theory/ESCRIBA.md §Refresh-Seal): highlight + set_rich_text +
        //    shape — the frame's dominant cost — run once per edit, never
        //    per vsync.
        let palette = self.chrome;
        let fg = chrome_glyph(palette.text);
        let width = ctx.width as f32;
        let height = ctx.height as f32 - self.line_height; // reserve bottom row for status
        if let Some((text, path, match_bytes)) = rebuild_input {
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
                self.highlighter = Some((
                    path.clone(),
                    self.eco.incremental_highlighter_for_path(&path),
                ));
            }
            let hl = &mut self
                .highlighter
                .as_mut()
                .expect("highlighter set immediately above")
                .1;
            let spans = hl.highlight(&text);
            // Overlay search matches on the syntax partition. Each syntax
            // span is cut at any match boundary crossing it and the matched
            // piece is recoloured; the result is still coverage-complete,
            // non-overlapping and sorted, which is what set_rich_text
            // requires — splitting a partition preserves that, replacing it
            // would not.
            let search_color = chrome_glyph(self.chrome.warning);
            let runs: Vec<(&str, Attrs)> = spans
                .iter()
                .flat_map(|sp| {
                    let syntax = base.clone().color(hl_to_glyph(self.theme.color(sp.class)));
                    split_on_matches(sp.span.range(), &match_bytes)
                        .into_iter()
                        .filter_map(|(r, is_match)| {
                            text.get(r).map(|slice| {
                                (
                                    slice,
                                    if is_match {
                                        base.clone().color(search_color)
                                    } else {
                                        syntax.clone()
                                    },
                                )
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            buffer.set_rich_text(
                &mut ctx.text.font_system,
                runs,
                &base,
                Shaping::Advanced,
                None,
            );
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
        // Built from `EditorState::status_model()` — the ONE model the
        // ratatui face renders too, so the two can differ only in styling.
        // This replaces a fixed `format!()` that carried mode/line/col/version
        // and read neither the prompt nor any message: typing `/foo` on this
        // face moved the cursor with nothing on screen to show for it, which
        // is why search looked absent on escriba's default renderer.
        //
        // `push_str`, not `format!` — ★★ TYPED EMISSION.
        let mut status = String::with_capacity(status_core.len() + 24);
        status.push(' ');
        status.push_str(mode_glyph(&signals, mode).render(SignalMode::Glyph));
        status.push(' ');
        status.push_str(&status_core);
        status.push_str("  escriba v");
        status.push_str(env!("CARGO_PKG_VERSION"));
        status.push(' ');
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

        let status_color = chrome_glyph(palette.info);

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
                        load: wgpu::LoadOp::Clear(ground_bg()),
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
///
/// A third tier registers last: [`crate::langs::escriba_local`], the languages
/// escriba serves that the fleet spine does not ship yet (today: blue). Last
/// means an upstream hikari backend for the same language always wins, so a
/// local table retires itself the day hikari grows one — no edit here, and no
/// window where the two disagree.
///
/// Public because the registry IS escriba's language surface: a test that asks
/// "does the editor know this language?" must be able to ask the same object
/// the renderer holds, not a reconstruction of it.
#[must_use]
pub fn build_ecosystem() -> Ecosystem {
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
            covered.push(p.language());
            eco.register(p);
        }
    }
    for p in crate::langs::escriba_local() {
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
                    load: wgpu::LoadOp::Clear(ground_bg()),
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

/// The editor ground as `wgpu::Color`, resolved from the fleet-prescribed
/// theme's `background` role. Gamma-correct: the sRGB token is promoted
/// through `ishou_tokens`' typed sRGB→linear path so it composites
/// correctly on the linear-storage surface.
fn ground_bg() -> wgpu::Color {
    let c = ChromePalette::prescribed().background;
    Srgb::from(c).to_linear().with_alpha(1.0).into()
}

/// ishou `Rgb` → glyphon `Color` (sRGB u8 RGBA, opaque). Theme-agnostic —
/// was `vellum_glyph`, back when the paint path was hardwired to Vellum.
/// Cut `range` wherever a match in `matches` starts or ends inside it.
///
/// Returns `(sub_range, is_match)` pieces that are contiguous, in order, and
/// exactly cover `range` — the property `set_rich_text` depends on. `matches`
/// are byte ranges into the SAME string `range` indexes.
///
/// Splitting the existing syntax partition (rather than building a second one)
/// is what keeps the two colour sources composable: a match inside a string
/// literal recolours only the matched bytes and the literal keeps its colour
/// either side.
fn split_on_matches(
    range: std::ops::Range<usize>,
    matches: &[(usize, usize)],
) -> Vec<(std::ops::Range<usize>, bool)> {
    let mut cuts: Vec<usize> = vec![range.start, range.end];
    for &(a, b) in matches {
        if a > range.start && a < range.end {
            cuts.push(a);
        }
        if b > range.start && b < range.end {
            cuts.push(b);
        }
    }
    if cuts.len() == 2 {
        // No boundary crosses this span — the common case, so avoid the
        // sort/dedup entirely.
        let hit = matches
            .iter()
            .any(|&(a, b)| a <= range.start && b >= range.end);
        return vec![(range, hit)];
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let hit = matches.iter().any(|&(ms, me)| ms <= a && me >= b);
            (a..b, hit)
        })
        .collect()
}

fn chrome_glyph(c: Rgb) -> GlyphColor {
    GlyphColor::rgba(c.r, c.g, c.b, 0xFF)
}

/// hikari `Rgb` (sRGB u8) → glyphon `Color` (opaque) — the syntax-span paint.
fn hl_to_glyph(c: HlRgb) -> GlyphColor {
    GlyphColor::rgba(c.r, c.g, c.b, 0xFF)
}

/// Mode indicator color — used by higher-layer rendering paths that want a
/// glance-readable color. Named by ROLE so the hue follows the active theme:
/// Normal info, Insert success, Visual accent, Command warning.
#[must_use]
pub fn mode_color(mode: Mode) -> Rgb {
    let c = ChromePalette::prescribed();
    match mode {
        Mode::Insert => c.success,
        Mode::Command => c.warning,
        Mode::Visual | Mode::VisualLine => c.accent,
        Mode::Normal => c.info,
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

    // ── search-highlight overlay ──────────────────────────────────────
    //
    // set_rich_text requires a coverage-complete, non-overlapping, sorted
    // partition. Splitting the syntax partition preserves that; these pin it,
    // because a violation shows up as garbled text rather than a panic.

    /// The invariant, asserted directly: pieces are contiguous, ordered, and
    /// exactly cover the input range.
    fn assert_partition(range: std::ops::Range<usize>, out: &[(std::ops::Range<usize>, bool)]) {
        assert!(!out.is_empty(), "a range must yield at least one piece");
        assert_eq!(out[0].0.start, range.start, "starts at the range start");
        assert_eq!(out[out.len() - 1].0.end, range.end, "ends at the range end");
        for w in out.windows(2) {
            assert_eq!(
                w[0].0.end, w[1].0.start,
                "pieces are contiguous, no gap or overlap"
            );
        }
    }

    #[test]
    fn a_span_with_no_match_is_returned_whole() {
        let out = split_on_matches(0..10, &[]);
        assert_eq!(out.len(), 1, "no needless splitting");
        assert!(!out[0].1);
        assert_partition(0..10, &out);
    }

    #[test]
    fn a_match_covering_the_whole_span_marks_it_without_splitting() {
        let out = split_on_matches(4..8, &[(0, 20)]);
        assert_eq!(out.len(), 1);
        assert!(out[0].1, "fully covered span is a match");
        assert_partition(4..8, &out);
    }

    #[test]
    fn a_match_starting_mid_span_splits_it_in_two() {
        // Syntax span 0..10, match 5..10 -> [0..5 plain][5..10 match]
        let out = split_on_matches(0..10, &[(5, 10)]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], (0..5, false));
        assert_eq!(out[1], (5..10, true));
        assert_partition(0..10, &out);
    }

    #[test]
    fn a_match_inside_a_span_splits_it_in_three() {
        // This is the case that matters: a match inside a string literal must
        // recolour only the matched bytes, leaving the literal coloured
        // either side.
        let out = split_on_matches(0..10, &[(3, 6)]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], (0..3, false));
        assert_eq!(out[1], (3..6, true));
        assert_eq!(out[2], (6..10, false));
        assert_partition(0..10, &out);
    }

    #[test]
    fn two_matches_in_one_span_both_split() {
        let out = split_on_matches(0..20, &[(2, 4), (10, 12)]);
        assert_partition(0..20, &out);
        let hits: Vec<_> = out
            .iter()
            .filter(|(_, m)| *m)
            .map(|(r, _)| r.clone())
            .collect();
        assert_eq!(hits, vec![2..4, 10..12]);
    }

    #[test]
    fn a_match_entirely_outside_the_span_changes_nothing() {
        let out = split_on_matches(10..20, &[(0, 5)]);
        assert_eq!(out.len(), 1);
        assert!(!out[0].1);
        assert_partition(10..20, &out);
    }

    #[test]
    fn a_match_touching_the_span_edge_does_not_create_an_empty_piece() {
        // Boundary exactly at the edge must not emit a zero-width run.
        for m in [(0usize, 10usize), (10, 20)] {
            let out = split_on_matches(10..20, &[m]);
            assert_partition(10..20, &out);
            assert!(
                out.iter().all(|(r, _)| r.start < r.end),
                "no empty piece for {m:?}"
            );
        }
    }

    #[test]
    fn adjacent_matches_do_not_produce_duplicate_cuts() {
        // Two matches meeting at 5 must yield one cut there, not two.
        let out = split_on_matches(0..10, &[(0, 5), (5, 10)]);
        assert_partition(0..10, &out);
        assert!(out.iter().all(|(r, _)| r.start < r.end));
        assert!(out.iter().all(|(_, m)| *m), "both halves are matches");
    }
    use super::*;
    use escriba_buffer::BufferSet;

    #[test]
    fn ground_is_the_prescribed_theme_promoted_to_linear() {
        let bg = ground_bg();
        // Was pinned to Vellum's warm parchment (night0 #16140E, r >= g >= b).
        // The prescribed theme is now Nord, whose ground is COOL (b >= r), so
        // the old warmth assertion was theme-specific and had to go. What is
        // actually invariant — and worth asserting — is that the ground is a
        // dark, opaque, gamma-correct promotion of the theme's own
        // background role.
        let want = Srgb::from(ChromePalette::prescribed().background)
            .to_linear()
            .with_alpha(1.0);
        let want: wgpu::Color = want.into();
        assert!((bg.r - want.r).abs() < 1e-6, "r {} != {}", bg.r, want.r);
        assert!((bg.g - want.g).abs() < 1e-6, "g {} != {}", bg.g, want.g);
        assert!((bg.b - want.b).abs() < 1e-6, "b {} != {}", bg.b, want.b);
        assert_eq!(bg.a, 1.0);
        // Dark ground: an editor background must stay well below mid-grey in
        // linear space whatever the theme.
        assert!(
            bg.r < 0.1 && bg.g < 0.1 && bg.b < 0.1,
            "ground is not dark: {bg:?}"
        );
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

    /// Mode pills map to ROLES, not to one theme's hexes. This test used to
    /// pin the four Vellum values (`#94BBB8` …), which is precisely why it
    /// went red the moment the fleet theme moved — a test asserting a
    /// theme's spelling has to be rewritten on every theme change, and is
    /// no evidence the mapping is right. Asserting role identity instead
    /// survives the move AND still catches a mis-wired pill.
    #[test]
    fn mode_colors_are_role_pills() {
        let c = ChromePalette::prescribed();
        assert_eq!(
            mode_color(Mode::Normal).hex(),
            c.info.hex(),
            "Normal = info"
        );
        assert_eq!(
            mode_color(Mode::Insert).hex(),
            c.success.hex(),
            "Insert = success"
        );
        assert_eq!(
            mode_color(Mode::Visual).hex(),
            c.accent.hex(),
            "Visual = accent"
        );
        assert_eq!(
            mode_color(Mode::Command).hex(),
            c.warning.hex(),
            "Command = warning"
        );

        // The four pills must be mutually distinct, or the mode is not
        // glance-readable regardless of which theme is active.
        let mut seen = std::collections::BTreeSet::new();
        for m in [Mode::Normal, Mode::Insert, Mode::Visual, Mode::Command] {
            assert!(
                seen.insert(mode_color(m).hex()),
                "{m:?} duplicates another pill"
            );
        }
    }

    /// Forcing function: the status-line mode glyphs are sourced from the
    /// fleet `EscribaSignals` vocabulary, not hand-picked literals. Pins
    /// the geometric `Glyph`-mode marks so drift in either escriba or
    /// ishou surfaces here.
    #[test]
    fn mode_glyphs_are_fleet_signals() {
        let sig = EscribaSignals::prescribed();
        assert_eq!(
            mode_glyph(&sig, Mode::Normal).render(SignalMode::Glyph),
            "◆"
        );
        assert_eq!(
            mode_glyph(&sig, Mode::Insert).render(SignalMode::Glyph),
            "▸"
        );
        assert_eq!(
            mode_glyph(&sig, Mode::Visual).render(SignalMode::Glyph),
            "▮"
        );
        assert_eq!(
            mode_glyph(&sig, Mode::VisualLine).render(SignalMode::Glyph),
            "▮"
        );
        assert_eq!(
            mode_glyph(&sig, Mode::Command).render(SignalMode::Glyph),
            ":"
        );
    }

    /// Fleet convergence guard: escriba's GPU chrome paints whatever
    /// `ChromePalette::prescribed()` resolves, which is
    /// `FleetTheme::prescribed_default()` BY CONSTRUCTION — so this Guard
    /// cannot be satisfied by a stale hand-written constant.
    ///
    /// It previously hardcoded `FleetTheme::Vellum` to match a paint path
    /// hardwired to `VellumPalette::vellum()`. When the fleet moved its
    /// prescribed theme to PlemeDark (Nord) this went RED — correctly, since
    /// the GPU backend really was painting the wrong theme while the TUI
    /// face and the rest of the fleet (mado, tear, frostmourne, …) moved on.
    /// Smallest real editor state — a scratch buffer. The theming tests
    /// care about the palette, not the buffer, but GpuRenderer owns state.
    fn test_renderer() -> GpuRenderer {
        let mut bufs = escriba_buffer::BufferSet::new();
        let id = bufs.scratch("");
        GpuRenderer::new(Arc::new(Mutex::new(EditorState::new_with_buffer(bufs, id))))
    }

    #[test]
    fn default_theme_is_the_fleet_prescribed_nord() {
        // Nord is the default because the FLEET says so — asserted against
        // FleetTheme::prescribed_default(), never a hand-written "nord",
        // so a fleet re-point cannot leave escriba silently behind.
        let r = test_renderer();
        let want = ChromePalette::for_theme(ishou_tokens::FleetTheme::prescribed_default());
        assert_eq!(r.chrome().hex_tuple(), want.hex_tuple());
    }

    #[test]
    fn set_theme_actually_changes_what_is_painted() {
        // The wiring this exists to prove: before it, every paint site
        // called ChromePalette::prescribed() directly, so (deftheme :preset)
        // resolved to a real FleetTheme that NOTHING consumed. If set_theme
        // ever stops reaching the paint path, this fails.
        let mut r = test_renderer();
        let before = r.chrome().hex_tuple();
        r.set_theme(ishou_tokens::FleetTheme::Vellum);
        let after = r.chrome().hex_tuple();
        assert_ne!(
            before, after,
            "switching to Vellum must change the painted palette"
        );
        assert_eq!(
            after,
            ChromePalette::for_theme(ishou_tokens::FleetTheme::Vellum).hex_tuple()
        );
        // And it is reversible — a theme is a value, not a one-way latch.
        r.set_theme(ishou_tokens::FleetTheme::prescribed_default());
        assert_eq!(r.chrome().hex_tuple(), before);
    }

    #[test]
    fn escriba_gpu_chrome_converges_with_fleet() {
        use ishou_tokens::{FleetTheme, convergence::Guard};
        let chrome_theme = FleetTheme::prescribed_default();
        Guard::for_app("escriba-render")
            .expect_theme(chrome_theme)
            .run();
    }
}
