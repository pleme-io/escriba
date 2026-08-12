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
// coverage-complete HlClass span partition. HlClass→Rgb resolves through
// `escriba_ui::syntax::ChromeSyntax`, NOT hikari's hardcoded `NordTheme`:
// this face used to hold one by value, so picking Vellum recoloured the frame
// and left the code Nord.
use escriba_ui::syntax::ChromeSyntax;
use hikari_core::{Ecosystem, Rgb as HlRgb, Theme};

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
    /// The refresh generation of the currently-cached text buffer — the seal
    /// (`theory/ESCRIBA.md` §Refresh-Seal). When `EditorState::edit_gen()`
    /// still equals this, the cached shaped buffer is reused verbatim: no
    /// re-highlight, no re-shape. Init `u64::MAX` so the first frame always
    /// paints.
    last_gen: EditGen,
    /// The shaped main-text glyphon buffer, cached across frames while the
    /// generation is unchanged. `None` before the first paint.
    cached_text: Option<Buffer>,
    /// The shaped gutter and the pixel width it occupies, cached under the
    /// SAME generation as `cached_text`. One gate for both, so a frame can
    /// never show this scroll position's line numbers beside the previous
    /// one's text.
    ///
    /// The width travels WITH the buffer rather than being recomputed at
    /// draw time. It depends on the buffer's line count, so a frame that
    /// reuses a cached gutter must offset its text by the width that gutter
    /// was actually shaped at — recomputing from a line count that has since
    /// changed is exactly how text lands on top of line numbers for one
    /// frame after a file grows.
    cached_gutter: Option<(Buffer, f32)>,
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
            last_gen: EditGen(u64::MAX),
            cached_text: None,
            cached_gutter: None,
            highlighter: None,
        }
    }

    /// Point the editor at a theme — the wiring that makes
    /// `(deftheme :preset …)` real.
    ///
    /// Writes THROUGH to the shared `EditorState`, which is the single
    /// owner of the theme. A renderer-local copy would be a second answer
    /// to "what colour is this editor", and the TUI face would not see it.
    pub fn set_theme(&mut self, theme: ishou_tokens::FleetTheme) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_theme(theme);
    }

    /// The palette currently painted with — read from the editor.
    #[must_use]
    pub fn chrome(&self) -> ChromePalette {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .chrome()
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

/// The visible text, and everything that indexes INTO it.
///
/// One struct rather than a tuple because there are now two independent
/// overlays keyed by byte offset into `text`, and both are built in the SAME
/// pass that builds it — which is the property that matters. `text` is a
/// reconstructed string (rows trimmed, char-sliced to the horizontal window,
/// `\n`-joined), so an offset computed against anything else — the document,
/// the previous frame — indexes the wrong characters. Carrying them together
/// is what makes computing them apart impossible to do by accident.
struct TextFrame {
    /// The visible rows, joined. Every offset below indexes this.
    text: String,
    /// The buffer's path, which is what resolves hikari's language.
    path: String,
    /// Search-match byte ranges.
    matches: Vec<(usize, usize)>,
    /// Language-server token byte ranges and what the server says they are.
    /// Empty when no server answered, when the answer was about another
    /// buffer, or when the operator has typed since — all three read the same
    /// to the painter, which then uses hikari's lexer alone.
    lsp: Vec<(usize, usize, hikari_core::HlClass)>,
}

/// The server-declared class covering byte `at`, if any.
///
/// Linear on purpose: `lsp` holds one screen's worth of tokens, and
/// [`split_on_matches`] beside it already scans its own list the same way.
/// Making this a binary search would add an ordering precondition to a list
/// whose ordering is not this function's to guarantee.
fn class_at(
    at: usize,
    lsp: &[(usize, usize, hikari_core::HlClass)],
) -> Option<hikari_core::HlClass> {
    lsp.iter()
        .find(|&&(a, b, _)| a <= at && at < b)
        .map(|&(_, _, c)| c)
}

/// What one piece of the final partition is painted as.
///
/// A search match is NOT a `HlClass` and must not be modelled as one: it is a
/// transient UI affordance that outranks meaning, so folding it into the
/// syntax vocabulary would let a theme rebinding change what "you are looking
/// at a hit" looks like, and let a lexer class accidentally claim it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    /// Meaning — from the language server when it claimed this span, from
    /// hikari's lexer otherwise.
    Class(hikari_core::HlClass),
    /// A live search hit. Wins over everything underneath it.
    SearchMatch,
}

/// Cut one hikari span into painted pieces: the LSP overlay recolours it, the
/// search overlay then wins over whatever is underneath.
///
/// **Extracted rather than left inline**, per this face's standing rule: logic
/// that can be WRONG belongs outside `render()`, which needs a live wgpu
/// device and cannot run under `cargo test`. A mis-composed partition here
/// renders perfectly — glyphon shapes whatever runs it is handed — so the only
/// place the check can live is a test of this function.
///
/// The result is contiguous, in order, and exactly covers `span`. That is not
/// incidental: [`set_rich_text`](glyphon::Buffer::set_rich_text) is fed the
/// concatenation of these across every span and a gap or an overlap garbles
/// the text rather than failing. Two `split_on_matches` passes are what
/// preserve it — splitting a coverage-complete partition yields another one,
/// so composing the passes cannot lose the property, where a bespoke three-way
/// splitter would be a second chance to lose it.
///
/// `lsp` is `(start, end, class)` byte ranges; `matches` is `(start, end)`.
/// Both index the same string `span` does.
#[must_use]
pub fn paint_pieces(
    span: std::ops::Range<usize>,
    lexer: hikari_core::HlClass,
    lsp: &[(usize, usize, hikari_core::HlClass)],
    matches: &[(usize, usize)],
) -> Vec<(std::ops::Range<usize>, Paint)> {
    let lsp_bounds: Vec<(usize, usize)> = lsp.iter().map(|&(a, b, _)| (a, b)).collect();
    split_on_matches(span, &lsp_bounds)
        .into_iter()
        .flat_map(|(piece, is_token)| {
            // The server's word for this piece if it claimed one, hikari's
            // otherwise. A token type escriba has no class for never reaches
            // here — it was dropped at the decode — so the lexer's answer
            // survives rather than being overwritten with a guess.
            let class = if is_token {
                class_at(piece.start, lsp).unwrap_or(lexer)
            } else {
                lexer
            };
            split_on_matches(piece, matches)
                .into_iter()
                .map(move |(r, is_match)| {
                    (
                        r,
                        if is_match {
                            Paint::SearchMatch
                        } else {
                            Paint::Class(class)
                        },
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

impl RenderCallback for GpuRenderer {
    fn render(&mut self, ctx: &mut RenderContext<'_>) {
        // ── 1. Read state under lock. The visible text is built ONLY when a
        //    rebuild is due (the refresh-generation gate): an idle frame reads
        //    just mode/cursor for the status line and reuses the cached shaped
        //    buffer below — zero re-highlight, zero re-shape. `rebuild_input`
        //    is `Some(TextFrame)` exactly when the generation moved.
        let (rebuild_input, gutter_rows, splash_chunks, mode, status_core, cur_gen, palette) = {
            let s = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(buf) = s.buffers.get(s.active) else {
                return clear_frame(ctx);
            };
            let cur_gen = s.edit_gen();
            let rebuild = cur_gen != self.last_gen || self.cached_text.is_none();
            // The start screen replaces the buffer text entirely, so when
            // one is up the (expensive) highlight+slice pass below is not
            // merely wasted, it is wrong — it would paint the scratch
            // buffer underneath. Laid out in CELLS, from the same estimate
            // `resize` uses, so the screen centres on what is really there.
            let splash_chunks = (rebuild)
                .then(|| s.splash())
                .flatten()
                .map(|sp| {
                    let grid = cell_grid(ctx.width, ctx.height, self.font_size, self.line_height);
                    sp.screen_chunks(grid.cols, grid.rows)
                })
                .filter(|c| !c.is_empty());
            let rebuild = rebuild && splash_chunks.is_none();
            // The rendered text and both overlays that index it — see
            // [`TextFrame`] for why they travel together.
            let rebuild_input: Option<TextFrame> = if rebuild {
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
                // What the language server said, for THIS buffer, at THIS
                // revision — the accessor answers empty for any other case,
                // so there is nothing to re-check here. Columns are `char`s
                // within a document line (the conversion from LSP's UTF-16
                // happened at the boundary), which is the same scale
                // `left_column` counts in.
                let lsp_spans = s.semantic_spans(s.active);
                let mut lsp_bytes: Vec<(usize, usize, hikari_core::HlClass)> = Vec::new();
                // A cursor into `lsp_spans`, advanced monotonically as the
                // rows do. Sound because LSP's delta encoding carries UNSIGNED
                // line deltas, so a decoded token list cannot go backwards in
                // line order — the sortedness is structural, not a promise
                // some server might break.
                let mut si = 0usize;
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
                        let seg_byte0 = out.len();
                        let seg_chars = sliced.chars().count();
                        // char index -> byte index within this segment. Built
                        // ONCE and shared by both overlays: it used to live
                        // inside the search branch, and a second copy for the
                        // token overlay is exactly how two overlays start
                        // disagreeing about where a character is.
                        let bytes: Vec<usize> = if hl.is_empty() && lsp_spans.is_empty() {
                            Vec::new()
                        } else {
                            sliced
                                .char_indices()
                                .map(|(b, _)| b)
                                .chain(std::iter::once(sliced.len()))
                                .collect()
                        };
                        if !hl.is_empty() {
                            // Document char span this rendered segment covers.
                            let doc0 = buf
                                .position_to_char(escriba_core::Position::new(ln, 0))
                                .unwrap_or(0)
                                + left_column;
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
                        // The token overlay, clipped to the horizontal window
                        // the same way the search overlay is. A token whose
                        // start is scrolled off the left keeps its visible
                        // tail coloured rather than vanishing.
                        while si < lsp_spans.len() && lsp_spans[si].line < ln {
                            si += 1;
                        }
                        let mut sj = si;
                        while sj < lsp_spans.len() && lsp_spans[sj].line == ln {
                            let sp = &lsp_spans[sj];
                            sj += 1;
                            let a = (sp.start_char as usize).max(left_column);
                            let b = (sp.start_char as usize + sp.len_chars as usize)
                                .min(left_column + seg_chars);
                            if a < b {
                                lsp_bytes.push((
                                    seg_byte0 + bytes[a - left_column],
                                    seg_byte0 + bytes[b - left_column],
                                    sp.class,
                                ));
                            }
                        }
                        out.push_str(&sliced);
                        out.push('\n');
                    }
                }
                Some(TextFrame {
                    text: out,
                    path,
                    matches: match_bytes,
                    lsp: lsp_bytes,
                })
            } else {
                None
            };
            // The gutter's rows, gathered under the SAME lock and the same
            // rebuild gate as the text they sit beside. Computing them in a
            // second pass would let the two disagree about which lines are on
            // screen — a mark one row off its finding is worse than no mark.
            #[allow(clippy::type_complexity)]
            let gutter_rows: Option<(
                u32,
                Vec<(u32, Option<escriba_shirube::Severity>)>,
            )> = rebuild_input.is_some().then(|| {
                let win = s.layout.active_window().cloned();
                let top_line = win.as_ref().map_or(0, |w| w.viewport.top_line);
                let visible_lines = win
                    .as_ref()
                    .map_or(40, |w| w.viewport.visible_lines.max(20));
                let world = s.world();
                let rows = (0..visible_lines)
                    .map(|row| top_line + row)
                    .take_while(|ln| *ln < buf.line_count())
                    .map(|ln| (ln, s.results.worst_on_line(&world, s.active, ln)))
                    .collect();
                (buf.line_count(), rows)
            });
            (
                rebuild_input,
                gutter_rows,
                splash_chunks,
                s.modal.mode(),
                s.status_model().render(),
                cur_gen,
                s.chrome(),
            )
        };

        // ── 2. Rebuild the shaped main-text buffer ONLY on a generation
        //    change; otherwise reuse the cached one. This is the seal
        //    (theory/ESCRIBA.md §Refresh-Seal): highlight + set_rich_text +
        //    shape — the frame's dominant cost — run once per edit, never
        //    per vsync.
        let fg = chrome_glyph(palette.text);
        let width = ctx.width as f32;
        let height = ctx.height as f32 - self.line_height; // reserve bottom row for status
        if let Some(chunks) = splash_chunks {
            // The start screen: same laid-out stream the ANSI face
            // consumes, roles turned into glyphon attrs instead of SGR.
            let base = Attrs::new().family(Family::Monospace);
            let mut buffer = Buffer::new(&mut ctx.text.font_system, self.metrics);
            buffer.set_size(&mut ctx.text.font_system, Some(width), Some(height));
            let runs: Vec<(&str, Attrs)> = splash_runs(&chunks, &palette)
                .into_iter()
                .map(|(text, color)| (text, base.clone().color(color)))
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
            // The start screen has no gutter — it is not a view of a file.
            // Dropping the cached one matters: without this, dismissing a file
            // and returning to the splash would leave the last file's line
            // numbers painted down its left edge.
            self.cached_gutter = None;
            self.last_gen = cur_gen;
        } else if let Some(TextFrame {
            text,
            path,
            matches: match_bytes,
            lsp: lsp_bytes,
        }) = rebuild_input
        {
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
            let search_color = chrome_glyph(palette.warning);
            // The code's colours come from the SAME palette as the chrome's,
            // so a `(deftheme :preset …)` recolours both together. Built here
            // rather than held on the renderer: a stored copy would be one
            // more thing to remember to update on a theme change, and the
            // last one that was stored is exactly why code stayed Nord.
            let syntax_theme = ChromeSyntax::new(palette);
            // The LSP overlay is a SECOND cut of the same kind, applied before
            // search so search still wins the pixel. All of that composition
            // lives in `paint_pieces` — pure, and therefore testable, which is
            // the only place a mis-composed partition can be caught: glyphon
            // shapes whatever runs it is handed and renders a wrong one
            // perfectly.
            let runs: Vec<(&str, Attrs)> = spans
                .iter()
                .flat_map(|sp| {
                    paint_pieces(sp.span.range(), sp.class, &lsp_bytes, &match_bytes)
                        .into_iter()
                        .filter_map(|(r, paint)| {
                            text.get(r).map(|slice| {
                                (
                                    slice,
                                    match paint {
                                        Paint::SearchMatch => base.clone().color(search_color),
                                        Paint::Class(c) => {
                                            base.clone().color(hl_to_glyph(syntax_theme.color(c)))
                                        }
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
        // ── 2b. The gutter, shaped as its OWN glyphon buffer.
        //
        // Separate rather than prefixed into the text, and this is the load-
        // bearing reason: the syntax spans and the search-match ranges are
        // BYTE offsets into `out`. Prefixing each line with `"  12 │ "` would
        // shift every one of those offsets, so the highlighter would paint the
        // wrong spans and search would box the wrong characters. Two areas
        // keeps one coordinate system per buffer.
        if let Some((line_count, rows)) = gutter_rows {
            let base = Attrs::new().family(Family::Monospace);
            let muted = chrome_glyph(palette.text_dim);
            let gutter_w = gutter_px(self.font_size, line_count);
            let mut gutter_buf = Buffer::new(&mut ctx.text.font_system, self.metrics);
            gutter_buf.set_size(&mut ctx.text.font_system, Some(gutter_w), Some(height));
            // Owned strings first: `set_rich_text` borrows its slices, so the
            // runs cannot reference temporaries created inside the same call.
            let mut owned: Vec<(String, GlyphColor)> = Vec::with_capacity(rows.len() * 5);
            for (ln, mark) in &rows {
                for cell in escriba_ui::gutter::gutter_cells(*ln, *mark, line_count) {
                    let color = match cell.role {
                        escriba_ui::gutter::GutterRole::Mark(sev) => {
                            chrome_glyph(escriba_ui::chrome::severity_color(&palette, sev))
                        }
                        _ => muted,
                    };
                    owned.push((cell.text, color));
                }
                owned.push(("\n".to_string(), muted));
            }
            let runs: Vec<(&str, Attrs)> = owned
                .iter()
                .map(|(t, c)| (t.as_str(), base.clone().color(*c)))
                .collect();
            gutter_buf.set_rich_text(
                &mut ctx.text.font_system,
                runs,
                &base,
                Shaping::Advanced,
                None,
            );
            gutter_buf.shape_until_scroll(&mut ctx.text.font_system, false);
            self.cached_gutter = Some((gutter_buf, gutter_w));
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

        // The text starts AFTER the gutter when there is one, and at the left
        // margin when there is not (the start screen). Deriving the offset
        // from `cached_gutter` rather than from a flag keeps the two from
        // disagreeing — an indented text column with no gutter beside it would
        // just look like a broken margin.
        let text_left = 8.0 + self.cached_gutter.as_ref().map_or(0.0, |(_, w)| *w);
        let mut text_areas = vec![
            TextArea {
                buffer,
                left: text_left,
                top: 8.0,
                scale: 1.0,
                bounds: TextBounds {
                    left: text_left as i32,
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
        if let Some((g, gutter_w)) = self.cached_gutter.as_ref() {
            text_areas.push(TextArea {
                buffer: g,
                left: 8.0,
                top: 8.0,
                scale: 1.0,
                // Bounded to its own columns. Without this a line number
                // wider than the field would spill into the text column and
                // overprint the first characters of the file.
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: (8.0 + gutter_w) as i32,
                    bottom: (height as i32).max(0),
                },
                default_color: chrome_glyph(palette.text_dim),
                custom_glyphs: &[],
            });
        }

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
                        load: wgpu::LoadOp::Clear(ground_bg(&palette)),
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
            // The SAME grid the start screen is laid out on — one estimate,
            // one status-row reservation, one place to fix either.
            let grid = cell_grid(width, height, self.font_size, self.line_height);
            for w in s.layout.windows_mut() {
                w.viewport.visible_lines = u32::from(grid.rows);
                // The full grid, NOT minus the gutter. The gutter's width
                // depends on the buffer's line count, which `resize` has no
                // business knowing; the subtraction happens in `render`,
                // where the buffer is in scope. Reserving a guessed width
                // here would be wrong for every file but one.
                w.viewport.visible_columns = u32::from(grid.cols);
            }
        }
    }
}

/// The highlight registry — re-exported from `escriba-ts`, where it now
/// lives. It was defined HERE, which put escriba's language knowledge behind
/// a GPU dependency; the re-export keeps this face's call sites and its tests
/// unchanged while the runtime can now reach the same registry without wgpu.
pub use escriba_ts::build_ecosystem;

/// Pair each start-screen chunk with the colour its ROLE resolves to under
/// `palette` — the GPU face's half of the role→paint mapping, extracted so
/// it can be tested without a device.
///
/// This is the piece of the splash path that can be wrong in a way glyphon
/// would not notice: a mis-mapped role paints the menu keys as body text and
/// renders perfectly. The plumbing either side (buffer sizing, shaping) is
/// upstream's contract; this is ours.
///
/// Borrows from `chunks`, so the returned slices concatenate to exactly the
/// screen — the coverage-complete partition `set_rich_text` requires.
///
/// Public so `tests/gpu_logic.rs` can assert on the REAL mapping rather
/// than on a reconstruction of it; a test that rebuilt this from
/// `screen_chunks` would pass even if the renderer stopped calling it.
#[must_use]
pub fn splash_runs<'a>(
    chunks: &'a [escriba_ui::splash::SplashSpan],
    palette: &ChromePalette,
) -> Vec<(&'a str, GlyphColor)> {
    chunks
        .iter()
        .map(|c| (c.text.as_str(), chrome_glyph(c.role.color(palette))))
        .collect()
}

/// The gutter's width in PIXELS for a buffer of `line_count` lines.
///
/// Uses the same `MONO_ADVANCE_RATIO` estimate `cell_grid` does — so the
/// gutter and the text agree about how wide a column is, and the text starts
/// exactly where the gutter stops. The column count comes from
/// `escriba_ui::gutter::gutter_width`, never restated here: the number of
/// columns this face RESERVES and the number the shared model PAINTS have to
/// be the same number, and a second definition is how they stop being.
#[must_use]
pub fn gutter_px(font_size: f32, line_count: u32) -> f32 {
    (font_size * MONO_ADVANCE_RATIO).max(1.0) * escriba_ui::gutter::gutter_width(line_count) as f32
}

/// The character grid a pixel surface maps to.
///
/// Both the viewport (how many buffer lines and columns fit) and the start
/// screen (what canvas to centre on) need this, and they used to compute it
/// separately: `resize` divided height by line-height and subtracted a row,
/// `render` subtracted a line-height and then divided. Same intent, two
/// spellings, two places to get the status-row reservation wrong.
///
/// Pure and total — no GPU, no state — which is what makes the one piece of
/// arithmetic in the GPU face that can actually be WRONG testable without a
/// device. The `0.6` is glyphon's monospace advance ratio for
/// `Family::Monospace`: an estimate, and the honest reason the start screen
/// centres approximately rather than exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellGrid {
    pub cols: u16,
    pub rows: u16,
}

/// Advance-to-font-size ratio for glyphon's monospace face.
const MONO_ADVANCE_RATIO: f32 = 0.6;

#[must_use]
pub fn cell_grid(width_px: u32, height_px: u32, font_size: f32, line_height: f32) -> CellGrid {
    let cell_w = (font_size * MONO_ADVANCE_RATIO).max(1.0);
    let cell_h = line_height.max(1.0);
    let cols = (width_px as f32 / cell_w).floor().max(1.0);
    // One row is reserved for the status line, which is drawn as its own
    // text area below the main pane. Reserved ONCE, here, so no caller can
    // forget it or subtract it twice.
    let rows = (height_px as f32 / cell_h).floor().max(2.0) - 1.0;
    CellGrid {
        cols: cols.min(f32::from(u16::MAX)) as u16,
        rows: rows.min(f32::from(u16::MAX)) as u16,
    }
}

/// Utility — clear the frame to the ground colour. Used on error paths.
///
/// This one legitimately paints the FLEET-PRESCRIBED ground rather than the
/// operator's: it runs when the editor state could not be read (no active
/// buffer, a failed glyphon prepare), which is exactly when the operator's
/// theme is unknowable. A dark frame in the default theme beats a panic or
/// an undefined surface.
fn clear_frame(ctx: &mut RenderContext<'_>) {
    let palette = ChromePalette::prescribed();
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
                    load: wgpu::LoadOp::Clear(ground_bg(&palette)),
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
fn ground_bg(c: &ChromePalette) -> wgpu::Color {
    Srgb::from(c.background).to_linear().with_alpha(1.0).into()
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
pub fn mode_color(c: &ChromePalette, mode: Mode) -> Rgb {
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
        let bg = ground_bg(&ChromePalette::prescribed());
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
        let n = mode_color(&ChromePalette::prescribed(), Mode::Normal);
        let i = mode_color(&ChromePalette::prescribed(), Mode::Insert);
        let v = mode_color(&ChromePalette::prescribed(), Mode::Visual);
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
            mode_color(&ChromePalette::prescribed(), Mode::Normal).hex(),
            c.info.hex(),
            "Normal = info"
        );
        assert_eq!(
            mode_color(&ChromePalette::prescribed(), Mode::Insert).hex(),
            c.success.hex(),
            "Insert = success"
        );
        assert_eq!(
            mode_color(&ChromePalette::prescribed(), Mode::Visual).hex(),
            c.accent.hex(),
            "Visual = accent"
        );
        assert_eq!(
            mode_color(&ChromePalette::prescribed(), Mode::Command).hex(),
            c.warning.hex(),
            "Command = warning"
        );

        // The four pills must be mutually distinct, or the mode is not
        // glance-readable regardless of which theme is active.
        let mut seen = std::collections::BTreeSet::new();
        for m in [Mode::Normal, Mode::Insert, Mode::Visual, Mode::Command] {
            assert!(
                seen.insert(mode_color(&ChromePalette::prescribed(), m).hex()),
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
