//! Ratatui rendering — draws buffer pane + status line each frame.
//!
//! Chrome colors are the **Vellum** fleet theme (warm aged-paper
//! Nord-matte) — every value is a BORN `ishou_tokens::VellumPalette`
//! token, so the TUI chrome matches the rest of the fleet (mado, tear,
//! frostmourne, …) and the GPU backend.

use escriba_core::CursorShape;
use escriba_runtime::EditorState;
use ishou_tokens::{EscribaSignals, SignalMode, VellumPalette};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout as RLayout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// ishou `Rgb` → ratatui `Color::Rgb`. The single conversion point so
/// every chrome color flows from the BORN Vellum tokens.
fn vellum(rgb: ishou_tokens::Rgb) -> Color {
    Color::Rgb(rgb.r, rgb.g, rgb.b)
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
        lines.push(line_with_gutter(
            ln,
            &text,
            cursor,
            left as usize,
            vis_cols,
            shape,
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
) -> Line<'static> {
    let gutter = format!("{:>4} │ ", ln + 1);
    let mut spans = vec![Span::styled(gutter, muted_style())];

    let chars: Vec<char> = text.chars().collect();
    // The slice of characters actually visible in this window.
    let visible: Vec<char> = chars.iter().copied().skip(left).take(vis_cols).collect();

    if ln == cursor.line && cursor.column as usize >= left {
        // Cursor column relative to the horizontal scroll.
        let rel = cursor.column as usize - left;
        if rel >= visible.len() {
            // Cursor at/after the end of the visible text — render the
            // shape over a blank trailing cell.
            spans.push(Span::raw(visible.iter().collect::<String>()));
            spans.extend(cursor_spans(' ', shape));
        } else {
            let before: String = visible[..rel].iter().collect();
            let under = visible[rel];
            let after: String = visible[rel + 1..].iter().collect();
            spans.push(Span::raw(before));
            spans.extend(cursor_spans(under, shape));
            spans.push(Span::raw(after));
        }
    } else {
        spans.push(Span::raw(visible.iter().collect::<String>()));
    }

    Line::from(spans)
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
        Span::styled(format!(" :{}", state.modal.minibuffer()), cmd_style())
    } else {
        Span::raw("")
    };
    let pos_span = Span::styled(format!(" {pos} "), status_style());

    // Layout: [mode] [path+modified] … (flex) … [minibuffer] [pos]
    let available = usize::from(area.width);
    let left = format!("{}{}", mode_span.content, path_span.content,);
    let right = format!("{}{}", minibuffer.content, pos_span.content);
    let pad = available.saturating_sub(left.chars().count() + right.chars().count());

    let line = Line::from(vec![
        mode_span,
        path_span,
        Span::raw(" ".repeat(pad)),
        minibuffer,
        pos_span,
    ]);
    f.render_widget(Paragraph::new(line).style(status_style()), area);
}

// ─── Styles — Vellum (warm aged-paper Nord-matte) ───────────────────────
//
// Every chrome color is a BORN `ishou_tokens::VellumPalette` token.
// `VellumPalette::vellum()` is cheap (plain struct construction); the
// per-call cost is negligible at the once-per-frame cadence these
// helpers run at.

fn buffer_style() -> Style {
    let p = VellumPalette::vellum();
    Style::default()
        .fg(vellum(p.snow1)) // #E2DBC8 — fg
        .bg(vellum(p.night0)) // #16140E — bg
}

fn muted_style() -> Style {
    let p = VellumPalette::vellum();
    Style::default().fg(vellum(p.shadow1)) // #90897B — comment/gutter
}

/// Block cursor (Normal / Command) — dark glyph filled onto the cursor
/// color, the "you are here" cell.
fn cursor_block_style() -> Style {
    let p = VellumPalette::vellum();
    Style::default()
        .fg(vellum(p.night0)) // #16140E — dark text on cursor
        .bg(vellum(p.green_bright)) // #ADD7A3 — cursor (= ishou surfaces.cursor)
        .add_modifier(Modifier::BOLD)
}

/// Bar cursor (Insert) — the thin vertical caret drawn between glyphs,
/// colored in the cursor accent.
fn cursor_bar_style() -> Style {
    let p = VellumPalette::vellum();
    Style::default()
        .fg(vellum(p.green_bright)) // #ADD7A3 — cursor accent for the bar
        .add_modifier(Modifier::BOLD)
}

/// Underline cursor (Visual) — the glyph kept legible with an underline in
/// the cursor accent.
fn cursor_underline_style() -> Style {
    let p = VellumPalette::vellum();
    Style::default()
        .fg(vellum(p.green_bright)) // #ADD7A3 — cursor accent underline
        .add_modifier(Modifier::UNDERLINED)
        .add_modifier(Modifier::BOLD)
}

fn status_style() -> Style {
    let p = VellumPalette::vellum();
    Style::default()
        .fg(Color::Rgb(0xCD, 0xC7, 0xB6)) // statusline_fg (Vellum extra)
        .bg(vellum(p.night1)) // #1F1C15 — statusline_bg
}

fn cmd_style() -> Style {
    let p = VellumPalette::vellum();
    Style::default()
        .fg(vellum(p.first_light)) // #D7C489 — yellow hint
        .bg(vellum(p.night1)) // #1F1C15
        .add_modifier(Modifier::BOLD)
}

fn error_style() -> Style {
    let p = VellumPalette::vellum();
    Style::default()
        .fg(vellum(p.aurora_red)) // #C9837B — red
        .bg(vellum(p.night0)) // #16140E
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
    let p = VellumPalette::vellum();
    // Mode pills — all with dark (#16140E night0) text per the Vellum
    // spec: Normal cyan, Insert green, Visual purple, Command yellow.
    let bg = match mode {
        escriba_core::Mode::Normal => p.ice_cyan, // #94BBB8
        escriba_core::Mode::Insert => p.aurora_green, // #A9BB8C
        escriba_core::Mode::Visual | escriba_core::Mode::VisualLine => p.solar_magenta, // #B8A1B9
        escriba_core::Mode::Command => p.first_light, // #D7C489
    };
    Style::default()
        .fg(vellum(p.night0)) // #16140E — dark text on every pill
        .bg(vellum(bg))
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_core::Mode;

    /// Forcing function: the status-line mode glyphs are sourced from the
    /// fleet `EscribaSignals` vocabulary, not hand-picked literals.
    #[test]
    fn mode_glyphs_are_fleet_signals() {
        let sig = EscribaSignals::prescribed();
        assert_eq!(mode_signal(&sig, Mode::Normal).render(SignalMode::Glyph), "◆");
        assert_eq!(mode_signal(&sig, Mode::Insert).render(SignalMode::Glyph), "▸");
        assert_eq!(mode_signal(&sig, Mode::Visual).render(SignalMode::Glyph), "▮");
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
        assert_eq!(block[0].style.bg, Some(vellum(VellumPalette::vellum().green_bright)));

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

    /// Fleet convergence guard: escriba's TUI chrome paints through
    /// `VellumPalette::vellum()` — the fleet-prescribed `FleetTheme::Vellum`,
    /// the same theme its GPU backend uses. This pins that convergence so a
    /// drift in the fleet baseline (e.g. `FleetDefaults::prescribed().theme`
    /// moving off Vellum) surfaces here rather than silently leaving the TUI
    /// chrome out of step with the GPU backend and the rest of the fleet.
    /// Uses the shared `ishou_tokens::convergence::Guard` harness.
    #[test]
    fn escriba_tui_chrome_converges_with_fleet() {
        use ishou_tokens::{FleetTheme, convergence::Guard};
        // Vellum by construction — every chrome helper above reads
        // `VellumPalette::vellum()`. The Guard asserts that equals the
        // fleet prescribed theme.
        let chrome_theme = FleetTheme::Vellum;
        Guard::for_app("escriba-tui")
            .expect_theme(chrome_theme)
            .run();
    }
}
