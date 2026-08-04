//! Ratatui rendering — draws buffer pane + status line each frame.
//!
//! Chrome colors are the **Vellum** fleet theme (warm aged-paper
//! Nord-matte) — every value is a BORN `ishou_tokens::VellumPalette`
//! token, so the TUI chrome matches the rest of the fleet (mado, tear,
//! frostmourne, …) and the GPU backend.

use escriba_core::CursorShape;
use escriba_runtime::EditorState;
use escriba_ui::chrome::ChromePalette;
use ishou_tokens::{EscribaSignals, SignalMode};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout as RLayout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// ishou `Rgb` → ratatui `Color::Rgb`. The single conversion point so
/// every chrome color flows from the BORN Vellum tokens.
/// Theme-agnostic `ishou` color → ratatui color. (Was `vellum()`, back when
/// the paint path was hardwired to one theme.)
fn rgb(c: ishou_tokens::Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// Draw one frame. Call from within `terminal.draw(|f| draw_frame(f, state))`.
pub fn draw_frame(f: &mut Frame<'_>, state: &EditorState) {
    let area = f.area();
    let chunks = RLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    draw_buffer(f, chunks[0], state);
    draw_status_line(f, chunks[1], state);
}

fn draw_buffer(f: &mut Frame<'_>, area: ratatui::layout::Rect, state: &EditorState) {
    let Some(buf) = state.buffers.get(state.active) else {
        f.render_widget(Paragraph::new("<no buffer>").style(error_style()), area);
        return;
    };

    let win = state.layout.active_window();
    let top = win.map_or(0, |w| w.viewport.top_line);
    let left = win.map_or(0, |w| w.viewport.left_column);
    // Visible width minus the gutter ("{:>4} │ " = 7 columns).
    let vis_cols = win.map_or(usize::MAX, |w| w.viewport.visible_columns as usize);
    let visible = area.height.saturating_sub(2).max(1);
    let cursor = state.cursor();
    // The cursor's on-screen shape is derived from the active mode through
    // the one typed `Mode::cursor_shape` function — block in Normal/Command,
    // bar in Insert, underline in Visual. Both backends read it from there,
    // so the shapes can't drift apart.
    let shape = state.modal.mode().cursor_shape();

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(visible as usize);
    for row in 0..visible as u32 {
        let ln = top + row;
        if ln >= buf.line_count() {
            break;
        }
        let Some(line_str) = buf.line(ln) else {
            continue;
        };
        let text = line_str
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_string();
        // Search matches are DOCUMENT char offsets; the renderer paints
        // COLUMNS. Translate once per line via the line's own start offset,
        // so no offset arithmetic leaks into the span builder.
        let line_start = buf
            .position_to_char(escriba_core::Position::new(ln, 0))
            .unwrap_or(0);
        let line_len = text.chars().count();
        let hl: Vec<(usize, usize)> = state
            .search
            .highlights()
            .iter()
            .filter_map(|m| {
                // Clip the match to this line; a multi-line match paints its
                // overlapping part on each line it crosses.
                let s = m.start.saturating_sub(line_start);
                let e = m.end.saturating_sub(line_start);
                (m.end > line_start && m.start < line_start + line_len + 1)
                    .then(|| (s.min(line_len), e.min(line_len)))
            })
            .filter(|(s, e)| e > s)
            .collect();
        lines.push(line_with_gutter(
            ln,
            &text,
            cursor,
            left as usize,
            vis_cols,
            shape,
            &hl,
        ));
    }

    let block = Block::default()
        .borders(Borders::NONE)
        .style(buffer_style());
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render one line with a gutter, sliced horizontally to the visible
/// column window `[left, left + vis_cols)`. Slicing is char-based (not
/// byte-based) so multibyte text stays aligned, and the cursor's on-screen
/// column is computed relative to `left` so the cursor glyph tracks the
/// horizontal scroll.
fn line_with_gutter(
    ln: u32,
    text: &str,
    cursor: escriba_core::Position,
    left: usize,
    vis_cols: usize,
    shape: CursorShape,
    highlights: &[(usize, usize)],
) -> Line<'static> {
    let gutter = format!("{:>4} │ ", ln + 1);
    let mut spans = vec![Span::styled(gutter, muted_style())];

    let chars: Vec<char> = text.chars().collect();
    // The slice of characters actually visible in this window.
    let visible: Vec<char> = chars.iter().copied().skip(left).take(vis_cols).collect();

    // One style slot per visible cell. Painting cell-by-cell and coalescing
    // afterwards is what lets the cursor and any number of search matches
    // overlap on the same line — the previous before/cursor/after split could
    // only ever express ONE styled region, so highlights had nowhere to go.
    let mut cell_styles: Vec<Option<Style>> = vec![None; visible.len()];
    for &(hs, he) in highlights {
        for col in hs..he {
            if col >= left {
                if let Some(slot) = cell_styles.get_mut(col - left) {
                    *slot = Some(search_match_style());
                }
            }
        }
    }

    // The cursor wins over a highlight on its own cell — you must always be
    // able to see where you are, even sitting on a match.
    let cursor_here = (ln == cursor.line && cursor.column as usize >= left)
        .then(|| cursor.column as usize - left);

    if let Some(rel) = cursor_here {
        if rel >= visible.len() {
            push_runs(&mut spans, &visible, &cell_styles);
            spans.extend(cursor_spans(' ', shape));
            return Line::from(spans);
        }
        push_runs(&mut spans, &visible[..rel], &cell_styles[..rel]);
        spans.extend(cursor_spans(visible[rel], shape));
        push_runs(&mut spans, &visible[rel + 1..], &cell_styles[rel + 1..]);
    } else {
        push_runs(&mut spans, &visible, &cell_styles);
    }

    Line::from(spans)
}

/// Emit `chars` as the fewest spans that preserve `styles`, merging adjacent
/// cells that share a style. Without the merge a 200-column line would emit
/// 200 single-char spans every frame.
fn push_runs(spans: &mut Vec<Span<'static>>, chars: &[char], styles: &[Option<Style>]) {
    debug_assert_eq!(chars.len(), styles.len(), "one style slot per cell");
    let mut i = 0;
    while i < chars.len() {
        let style = styles.get(i).copied().flatten();
        let mut j = i + 1;
        while j < chars.len() && styles.get(j).copied().flatten() == style {
            j += 1;
        }
        let run: String = chars[i..j].iter().collect();
        spans.push(match style {
            Some(st) => Span::styled(run, st),
            None => Span::raw(run),
        });
        i = j;
    }
}

/// Render the cell under the cursor in its per-mode [`CursorShape`].
///
/// - [`CursorShape::Block`]: fill the cell (dark glyph on the cursor color)
///   — the Normal/Command "you are here" indicator.
/// - [`CursorShape::Bar`]: a thin vertical bar drawn BEFORE the glyph
///   (Insert mode's between-glyphs caret), the glyph itself left plain.
/// - [`CursorShape::Underline`]: the glyph with an underline modifier
///   (Visual mode), so the highlighted selection stays readable.
fn cursor_spans(under: char, shape: CursorShape) -> Vec<Span<'static>> {
    match shape {
        CursorShape::Block => vec![Span::styled(under.to_string(), cursor_block_style())],
        CursorShape::Bar => vec![
            Span::styled("▏".to_string(), cursor_bar_style()),
            Span::raw(under.to_string()),
        ],
        CursorShape::Underline => vec![Span::styled(under.to_string(), cursor_underline_style())],
    }
}

fn draw_status_line(f: &mut Frame<'_>, area: ratatui::layout::Rect, state: &EditorState) {
    let mode = state.modal.mode().as_str();
    let pos = format!("{}:{}", state.cursor().line + 1, state.cursor().column + 1);
    let path = state
        .buffers
        .get(state.active)
        .and_then(|b| b.path.clone())
        .map_or("scratch".to_string(), |p| p.display().to_string());
    let modified = state.buffers.get(state.active).is_some_and(|b| b.modified);
    // Status glyphs are the BORN fleet vocabulary (`ishou_tokens::EscribaSignals`),
    // not hand-picked literals. Single-width `Glyph` mode keeps the
    // status-line column alignment-safe.
    let sig = EscribaSignals::prescribed();
    let modified_indicator = if modified {
        format!(" {}", sig.modified.render(SignalMode::Glyph))
    } else {
        String::new()
    };

    // Mode pill = fleet mode glyph + escriba's canonical uppercase label.
    let mode_glyph = mode_signal(&sig, state.modal.mode()).render(SignalMode::Glyph);
    let mode_span = Span::styled(
        format!(" {mode_glyph} {mode} "),
        mode_style_for(state.modal.mode()),
    );
    let path_span = Span::styled(format!(" {path}{modified_indicator} "), status_style());
    let minibuffer = if state.modal.mode() == escriba_core::Mode::Command {
        {
            // Command mode hosts BOTH the ex-line and the search prompt (vim's
            // cmdline does the same), so the prefix must report which prompt is
            // actually open — a hardcoded ':' rendered a `/foo` search as `:foo`.
            //
            // The prefix used to be decided here, by a second copy of that
            // reasoning. It now comes from `EditorState::status_model()`, the same
            // model the GPU face renders, so the two faces cannot drift — which
            // they had: the GPU face drew no prompt at all.
            let model = state.status_model();
            let mut line = String::from(" ");
            model.render_prompt_into(&mut line);
            Span::styled(line, cmd_style())
        }
    } else {
        Span::raw("")
    };
    let pos_span = Span::styled(format!(" {pos} "), status_style());

    // `[3/17]`. Both halves were already computed by the engine and both were
    // discarded; the denominator is what turns "press n until it looks right"
    // into a decision — `[1/1]` says a rename is safe, `[1/240]` says narrow
    // the pattern first. Silent when there is nothing to count.
    let count = state.status_model().count;
    let count_span = if count.is_idle() {
        Span::raw("")
    } else {
        let mut c = String::from(" ");
        count.render_into(&mut c);
        c.push(' ');
        Span::styled(c, status_style())
    };

    // Layout: [mode] [path+modified] … (flex) … [count] [minibuffer] [pos]
    let available = usize::from(area.width);
    let left = format!("{}{}", mode_span.content, path_span.content,);
    let right = format!(
        "{}{}{}",
        count_span.content, minibuffer.content, pos_span.content
    );
    let pad = available.saturating_sub(left.chars().count() + right.chars().count());

    let line = Line::from(vec![
        mode_span,
        path_span,
        Span::raw(" ".repeat(pad)),
        count_span,
        minibuffer,
        pos_span,
    ]);
    f.render_widget(Paragraph::new(line).style(status_style()), area);
}

// ─── Styles — Vellum (warm aged-paper Nord-matte) ───────────────────────
//
// Every chrome color resolves through `escriba_ui::chrome::ChromePalette`
// — the one theme seam, shared with the GPU backend so the two faces cannot
// drift apart. Colors are named by ROLE (text / surface / cursor / error),
// never by a theme's own token spelling, which is what lets the theme change
// without touching a single call site here.
//
// `ChromePalette::prescribed()` is cheap (plain struct construction from
// ishou role bindings); the per-call cost is negligible at the
// once-per-frame cadence these helpers run at.
//
// NOTE: these read the FLEET-PRESCRIBED theme, not a per-buffer
// `(deftheme :preset …)`. Threading the operator's chosen theme down to the
// paint path is the remaining half of the theming work — the seam now exists
// (`ChromePalette::for_theme`), but nothing calls it with a config value yet.

fn buffer_style() -> Style {
    let c = ChromePalette::prescribed();
    Style::default().fg(rgb(c.text)).bg(rgb(c.background))
}

fn muted_style() -> Style {
    let c = ChromePalette::prescribed();
    Style::default().fg(rgb(c.text_dim)) // comment / gutter
}

/// Block cursor (Normal / Command) — dark glyph filled onto the cursor
/// color, the "you are here" cell.
fn cursor_block_style() -> Style {
    let c = ChromePalette::prescribed();
    Style::default()
        .fg(rgb(c.background)) // ground-colored text on the cursor
        .bg(rgb(c.cursor))
        .add_modifier(Modifier::BOLD)
}

/// Bar cursor (Insert) — the thin vertical caret drawn between glyphs,
/// colored in the cursor accent.
fn cursor_bar_style() -> Style {
    let c = ChromePalette::prescribed();
    Style::default()
        .fg(rgb(c.cursor))
        .add_modifier(Modifier::BOLD)
}

/// Underline cursor (Visual) — the glyph kept legible with an underline in
/// the cursor accent.
/// Style for a search match under `hlsearch`.
///
/// Reversed against the `warning` role rather than a literal colour: it reads
/// as "look here" without colliding with `cursor` (which must stay
/// distinguishable when the cursor sits ON a match) or with `error`. Sourced
/// from ChromePalette so it follows the fleet theme like every other style
/// here — a hardcoded hex would be the one span that ignores the theme.
fn search_match_style() -> Style {
    let c = ChromePalette::prescribed();
    Style::default().fg(rgb(c.background)).bg(rgb(c.warning))
}

fn cursor_underline_style() -> Style {
    let c = ChromePalette::prescribed();
    Style::default()
        .fg(rgb(c.cursor))
        .add_modifier(Modifier::UNDERLINED)
        .add_modifier(Modifier::BOLD)
}

fn status_style() -> Style {
    let c = ChromePalette::prescribed();
    // Was a raw `Color::Rgb(0xCD, 0xC7, 0xB6)` literal ("statusline_fg,
    // Vellum extra") — the one genuinely hardcoded color in this file, and
    // dead weight the moment the theme moved. It is now the `text` role.
    Style::default().fg(rgb(c.text)).bg(rgb(c.surface))
}

fn cmd_style() -> Style {
    let c = ChromePalette::prescribed();
    Style::default()
        .fg(rgb(c.warning))
        .bg(rgb(c.surface))
        .add_modifier(Modifier::BOLD)
}

fn error_style() -> Style {
    let c = ChromePalette::prescribed();
    Style::default().fg(rgb(c.error)).bg(rgb(c.background))
}

/// Map an editor [`Mode`](escriba_core::Mode) to its fleet
/// [`Signal`](ishou_tokens::Signal) from [`EscribaSignals`].
///
/// `VisualLine` shares `mode_visual` with `Visual` — the fleet signal
/// set has one visual signal, matching how [`mode_style_for`] groups the
/// two under one pill color.
fn mode_signal(sig: &EscribaSignals, mode: escriba_core::Mode) -> &ishou_tokens::Signal {
    match mode {
        escriba_core::Mode::Normal => &sig.mode_normal,
        escriba_core::Mode::Insert => &sig.mode_insert,
        escriba_core::Mode::Visual | escriba_core::Mode::VisualLine => &sig.mode_visual,
        escriba_core::Mode::Command => &sig.mode_command,
    }
}

fn mode_style_for(mode: escriba_core::Mode) -> Style {
    let c = ChromePalette::prescribed();
    // Mode pills — ground-colored text on a role-colored field:
    // Normal info, Insert success, Visual accent, Command warning. Naming
    // the ROLE rather than the hue is what keeps these correct across
    // themes: on Nord `info` is frost blue, on Vellum it was ice cyan, and
    // neither call site has to know.
    let bg = match mode {
        escriba_core::Mode::Normal => c.info,
        escriba_core::Mode::Insert => c.success,
        escriba_core::Mode::Visual | escriba_core::Mode::VisualLine => c.accent,
        escriba_core::Mode::Command => c.warning,
    };
    Style::default()
        .fg(rgb(c.background))
        .bg(rgb(bg))
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {

    // ── search highlight rendering ────────────────────────────────────

    fn styles_of(spans: &[Span<'static>]) -> Vec<(String, bool)> {
        // (text, is-highlighted) — comparing against the exact Style would
        // pin the palette, which is a theming concern, not a layout one.
        spans
            .iter()
            .map(|sp| {
                (
                    sp.content.to_string(),
                    sp.style.bg == search_match_style().bg,
                )
            })
            .collect()
    }

    #[test]
    fn push_runs_merges_adjacent_cells_of_equal_style() {
        // A 200-column line must not emit 200 spans per frame.
        let chars: Vec<char> = "aaaabbbb".chars().collect();
        let mut styles = vec![None; 8];
        for slot in styles.iter_mut().take(4) {
            *slot = Some(search_match_style());
        }
        let mut spans = vec![];
        push_runs(&mut spans, &chars, &styles);
        assert_eq!(spans.len(), 2, "one span per run, not per char");
        assert_eq!(spans[0].content, "aaaa");
        assert_eq!(spans[1].content, "bbbb");
    }

    #[test]
    fn push_runs_on_empty_input_emits_nothing() {
        let mut spans = vec![];
        push_runs(&mut spans, &[], &[]);
        assert!(spans.is_empty());
    }

    #[test]
    fn a_match_is_painted_and_the_rest_is_not() {
        // "hello world", match on "world" (cols 6..11), cursor elsewhere.
        let line = line_with_gutter(
            0,
            "hello world",
            escriba_core::Position::new(9, 0), // cursor on another line
            0,
            80,
            CursorShape::Block,
            &[(6, 11)],
        );
        let painted: Vec<String> = styles_of(&line.spans)
            .into_iter()
            .filter(|(_, hl)| *hl)
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            painted,
            vec!["world".to_string()],
            "exactly the match is lit"
        );
    }

    #[test]
    fn two_matches_on_one_line_are_both_painted() {
        // The old before/cursor/after split could express only ONE styled
        // region — this is the case it structurally could not render.
        let line = line_with_gutter(
            0,
            "foo bar foo",
            escriba_core::Position::new(9, 0),
            0,
            80,
            CursorShape::Block,
            &[(0, 3), (8, 11)],
        );
        let painted: Vec<String> = styles_of(&line.spans)
            .into_iter()
            .filter(|(_, hl)| *hl)
            .map(|(t, _)| t)
            .collect();
        assert_eq!(painted, vec!["foo".to_string(), "foo".to_string()]);
    }

    #[test]
    fn the_cursor_stays_visible_when_sitting_on_a_match() {
        // A highlight must never swallow the cursor cell, or you lose your
        // place the moment you land on a match — which is always, after `n`.
        let line = line_with_gutter(
            0,
            "foo bar",
            escriba_core::Position::new(0, 1),
            0,
            80,
            CursorShape::Block,
            &[(0, 3)],
        );
        let texts: Vec<String> = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            texts.contains(&"o".to_string()),
            "cursor cell rendered alone: {texts:?}"
        );
    }

    #[test]
    fn highlights_respect_horizontal_scroll() {
        // Scrolled right by 4: the match at cols 6..11 must shift left by 4.
        let line = line_with_gutter(
            0,
            "hello world",
            escriba_core::Position::new(9, 0),
            4,
            80,
            CursorShape::Block,
            &[(6, 11)],
        );
        let painted: Vec<String> = styles_of(&line.spans)
            .into_iter()
            .filter(|(_, hl)| *hl)
            .map(|(t, _)| t)
            .collect();
        assert_eq!(
            painted,
            vec!["world".to_string()],
            "still exactly the match"
        );
    }

    #[test]
    fn no_highlights_renders_a_plain_line() {
        let line = line_with_gutter(
            0,
            "hello world",
            escriba_core::Position::new(9, 0),
            0,
            80,
            CursorShape::Block,
            &[],
        );
        assert!(
            styles_of(&line.spans).iter().all(|(_, hl)| !hl),
            "nothing lit"
        );
    }
    use super::*;
    use escriba_core::Mode;

    /// Forcing function: the status-line mode glyphs are sourced from the
    /// fleet `EscribaSignals` vocabulary, not hand-picked literals.
    #[test]
    fn mode_glyphs_are_fleet_signals() {
        let sig = EscribaSignals::prescribed();
        assert_eq!(
            mode_signal(&sig, Mode::Normal).render(SignalMode::Glyph),
            "◆"
        );
        assert_eq!(
            mode_signal(&sig, Mode::Insert).render(SignalMode::Glyph),
            "▸"
        );
        assert_eq!(
            mode_signal(&sig, Mode::Visual).render(SignalMode::Glyph),
            "▮"
        );
        assert_eq!(
            mode_signal(&sig, Mode::VisualLine).render(SignalMode::Glyph),
            "▮"
        );
        assert_eq!(
            mode_signal(&sig, Mode::Command).render(SignalMode::Glyph),
            ":"
        );
    }

    /// The modified indicator is the fleet `modified` glyph (`●`), not a
    /// hand-picked literal.
    #[test]
    fn modified_indicator_is_fleet_signal() {
        let sig = EscribaSignals::prescribed();
        assert_eq!(sig.modified.render(SignalMode::Glyph), "●");
    }

    /// The cursor is rendered in its per-mode shape: a block fills the
    /// cell (Normal), a bar precedes the glyph (Insert), an underline marks
    /// the glyph (Visual). The shape is selected by `Mode::cursor_shape`.
    #[test]
    fn cursor_spans_render_per_mode_shape() {
        // Block: a single span styled with the cursor BG (block fill).
        let block = cursor_spans('a', CursorShape::Block);
        assert_eq!(block.len(), 1);
        assert_eq!(block[0].content, "a");
        // The cursor ROLE, not a theme's own token — this assertion used to
        // name `VellumPalette::vellum().green_bright`, which pinned the test
        // to one theme and would have had to change on every theme move.
        assert_eq!(
            block[0].style.bg,
            Some(rgb(ChromePalette::prescribed().cursor))
        );

        // Bar: a thin caret span BEFORE the (unstyled) glyph.
        let bar = cursor_spans('a', CursorShape::Bar);
        assert_eq!(bar.len(), 2);
        assert_eq!(bar[0].content, "▏");
        assert_eq!(bar[1].content, "a");
        assert_eq!(bar[1].style.bg, None, "bar leaves the glyph cell unfilled");

        // Underline: one glyph span carrying the UNDERLINED modifier.
        let under = cursor_spans('a', CursorShape::Underline);
        assert_eq!(under.len(), 1);
        assert!(under[0].style.add_modifier.contains(Modifier::UNDERLINED));
    }

    /// End-to-end: the shape the buffer pane uses is derived from the live
    /// modal mode through the one typed `Mode::cursor_shape` mapping.
    #[test]
    fn buffer_shape_follows_modal_mode() {
        use escriba_core::Mode;
        assert_eq!(Mode::Normal.cursor_shape(), CursorShape::Block);
        assert_eq!(Mode::Insert.cursor_shape(), CursorShape::Bar);
        assert_eq!(Mode::Visual.cursor_shape(), CursorShape::Underline);
    }

    /// Fleet convergence guard: escriba's TUI chrome paints whatever
    /// `ChromePalette::prescribed()` resolves, which is
    /// `FleetTheme::prescribed_default()` BY CONSTRUCTION — so this Guard
    /// can no longer be satisfied by a stale hand-written constant.
    ///
    /// It previously hardcoded `FleetTheme::Vellum` here to match a paint
    /// path hardwired to `VellumPalette::vellum()`. When the fleet moved its
    /// prescribed theme to PlemeDark (Nord), that made the test RED —
    /// correctly: escriba really was painting the wrong theme. Asserting the
    /// resolved value instead of a literal is what stops that class of drift
    /// from needing a human to notice it twice.
    #[test]
    fn escriba_tui_chrome_converges_with_fleet() {
        use ishou_tokens::{FleetTheme, convergence::Guard};
        let chrome_theme = FleetTheme::prescribed_default();
        Guard::for_app("escriba-tui")
            .expect_theme(chrome_theme)
            .run();
    }

    /// The chrome helpers must actually paint the fleet theme — not merely
    /// agree with it in the assertion above. Pins the buffer ground to the
    /// prescribed chrome's background so a renderer that silently kept a
    /// different palette would fail here even if the Guard passed.
    #[test]
    fn buffer_ground_is_the_prescribed_chrome() {
        let c = ChromePalette::prescribed();
        assert_eq!(buffer_style().bg, Some(rgb(c.background)));
        assert_eq!(buffer_style().fg, Some(rgb(c.text)));
    }
}
