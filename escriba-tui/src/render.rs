//! Ratatui rendering — draws buffer pane + status line each frame.

use escriba_runtime::EditorState;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout as RLayout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

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
    let modified_indicator = if modified { " ●" } else { "" };

    let mode_span = Span::styled(format!(" {mode} "), mode_style_for(state.modal.mode));
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

// ─── Styles — Nord-inspired ─────────────────────────────────────────────

fn buffer_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0xD8, 0xDE, 0xE9)) // Nord4
        .bg(Color::Rgb(0x2E, 0x34, 0x40)) // Nord0
}

fn muted_style() -> Style {
    Style::default().fg(Color::Rgb(0x4C, 0x56, 0x6A)) // Nord3
}

fn cursor_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0x2E, 0x34, 0x40)) // Nord0 on
        .bg(Color::Rgb(0x88, 0xC0, 0xD0)) // Nord8
        .add_modifier(Modifier::BOLD)
}

fn status_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0xE5, 0xE9, 0xF0)) // Nord5
        .bg(Color::Rgb(0x3B, 0x42, 0x52)) // Nord1
}

fn cmd_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0xEB, 0xCB, 0x8B)) // Nord13 (yellow — hint)
        .bg(Color::Rgb(0x3B, 0x42, 0x52))
        .add_modifier(Modifier::BOLD)
}

fn error_style() -> Style {
    Style::default()
        .fg(Color::Rgb(0xBF, 0x61, 0x6A)) // Nord11
        .bg(Color::Rgb(0x2E, 0x34, 0x40))
}

fn mode_style_for(mode: escriba_core::Mode) -> Style {
    let (fg, bg) = match mode {
        escriba_core::Mode::Normal => (
            (0x2E, 0x34, 0x40), // Nord0 on
            (0x88, 0xC0, 0xD0), // Nord8 (frost accent)
        ),
        escriba_core::Mode::Insert => (
            (0x2E, 0x34, 0x40),
            (0xA3, 0xBE, 0x8C), // Nord14 (green)
        ),
        escriba_core::Mode::Visual | escriba_core::Mode::VisualLine => (
            (0x2E, 0x34, 0x40),
            (0xB4, 0x8E, 0xAD), // Nord15 (purple)
        ),
        escriba_core::Mode::Command => (
            (0x2E, 0x34, 0x40),
            (0xEB, 0xCB, 0x8B), // Nord13 (yellow)
        ),
    };
    Style::default()
        .fg(Color::Rgb(fg.0, fg.1, fg.2))
        .bg(Color::Rgb(bg.0, bg.1, bg.2))
        .add_modifier(Modifier::BOLD)
}
