//! Ratatui rendering — draws buffer pane + status line each frame.
//!
//! Chrome colors are the **Vellum** fleet theme (warm aged-paper
//! Nord-matte) — every value is a BORN `ishou_tokens::VellumPalette`
//! token, so the TUI chrome matches the rest of the fleet (mado, tear,
//! frostmourne, …) and the GPU backend.

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
    let visible = area.height.saturating_sub(2).max(1);
    let cursor = state.cursor;

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
        lines.push(line_with_gutter(ln, &text, cursor));
    }

    let block = Block::default()
        .borders(Borders::NONE)
        .style(buffer_style());
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn line_with_gutter(ln: u32, text: &str, cursor: escriba_core::Position) -> Line<'static> {
    let gutter = format!("{:>4} │ ", ln + 1);
    let mut spans = vec![Span::styled(gutter, muted_style())];

    if ln == cursor.line {
        let col = cursor.column as usize;
        let chars: Vec<char> = text.chars().collect();
        if col >= chars.len() {
            spans.push(Span::raw(text.to_string()));
            spans.push(Span::styled(" ".to_string(), cursor_style()));
        } else {
            let before: String = chars[..col].iter().collect();
            let under: String = chars[col].to_string();
            let after: String = chars[col + 1..].iter().collect();
            spans.push(Span::raw(before));
            spans.push(Span::styled(under, cursor_style()));
            spans.push(Span::raw(after));
        }
    } else {
        spans.push(Span::raw(text.to_string()));
    }

    Line::from(spans)
}

fn draw_status_line(f: &mut Frame<'_>, area: ratatui::layout::Rect, state: &EditorState) {
    let mode = state.modal.mode.as_str();
    let pos = format!("{}:{}", state.cursor.line + 1, state.cursor.column + 1);
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
    let mode_glyph = mode_signal(&sig, state.modal.mode).render(SignalMode::Glyph);
    let mode_span = Span::styled(
        format!(" {mode_glyph} {mode} "),
        mode_style_for(state.modal.mode),
    );
    let path_span = Span::styled(format!(" {path}{modified_indicator} "), status_style());
    let minibuffer = if state.modal.mode == escriba_core::Mode::Command {
        Span::styled(format!(" :{}", state.modal.minibuffer), cmd_style())
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

fn cursor_style() -> Style {
    let p = VellumPalette::vellum();
    Style::default()
        .fg(vellum(p.night0)) // #16140E — dark text on cursor
        .bg(vellum(p.green_bright)) // #ADD7A3 — cursor (= ishou surfaces.cursor)
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
}
