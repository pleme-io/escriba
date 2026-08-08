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

/// The highlight ecosystem, built ONCE per thread.
///
/// `build_ecosystem` constructs tree-sitter hosts; doing that per frame would
/// make every keystroke pay for grammar registration. The GPU face caches it
/// on the renderer struct — this face has no such struct, its `draw_frame`
/// takes `&EditorState`, so the cache lives here.
fn ecosystem() -> &'static hikari_core::Ecosystem {
    use std::sync::OnceLock;
    static ECO: OnceLock<hikari_core::Ecosystem> = OnceLock::new();
    ECO.get_or_init(escriba_ts::build_ecosystem)
}

/// Per-line syntax colouring for the visible window: for each row, the
/// `(start_col, end_col, colour)` runs in CHARACTER columns.
///
/// Highlighted over the whole visible slice rather than line by line, exactly
/// as the GPU face does. Per-line highlighting is easier and wrong: a block
/// comment or a multi-line string only reads correctly when the highlighter
/// sees the lines together, and the two faces disagreeing about that is the
/// drift this repo keeps paying for.
fn syntax_runs(
    lines: &[String],
    path: &str,
    theme: &escriba_ui::syntax::ChromeSyntax,
) -> Vec<Vec<(usize, usize, ishou_tokens::Rgb)>> {
    use hikari_core::Theme as _;
    let mut text = String::new();
    let mut starts = Vec::with_capacity(lines.len());
    for l in lines {
        starts.push(text.len());
        text.push_str(l);
        text.push('\n');
    }
    let mut out = vec![Vec::new(); lines.len()];
    let hl = ecosystem().highlighter_for_path(path);
    for span in hl.highlight(&text) {
        let r = span.span.range();
        let c = theme.color(span.class);
        let rgb = ishou_tokens::Rgb::new(c.r, c.g, c.b);
        // Which row does this span start on, and where within it?
        let Some(row) = starts.iter().rposition(|s| *s <= r.start) else {
            continue;
        };
        let Some(line) = lines.get(row) else { continue };
        let base = starts[row];
        // BYTE offsets from the highlighter, CHARACTER columns on screen —
        // the conversion every multibyte line depends on.
        let to_col = |byte: usize| line[..byte.min(line.len())].chars().count();
        let s_col = to_col(r.start.saturating_sub(base));
        let e_col = to_col(r.end.saturating_sub(base).min(line.len()));
        if e_col > s_col {
            out[row].push((s_col, e_col, rgb));
        }
    }
    out
}

/// How many buffer lines a terminal of `total_height` rows actually shows.
///
/// ONE definition, because two is what went wrong. The ratatui face never
/// wrote `viewport.visible_lines`, on the reasoning that "ratatui auto-picks
/// up the new size on the next draw" — true of PAINTING and false of the
/// model. `scroll_to_contain` kept using the constructor's default of 40, so
/// in any terminal shorter than that the editor believed the cursor was
/// visible while it had scrolled off the screen. A rendered-frame test now
/// pins it (`tests/viewport_frame.rs`).
///
/// The arithmetic must match `draw_frame`'s split (one row for the status
/// line) and `draw_buffer`'s own reservation, which is why it lives here
/// rather than being spelled again in the run loop.
#[must_use]
pub fn viewport_rows(total_height: u16) -> u16 {
    // -1 status line (the layout split), -2 draw_buffer's own reservation.
    total_height.saturating_sub(3).max(1)
}

/// Point `state`'s viewport at a terminal of this size.
///
/// The ratatui peer of the GPU face's `RenderCallback::resize`. Both faces
/// have to tell the runtime how much they can show, or the scroll-to-contain
/// invariant is computed against a window that does not exist.
pub fn sync_viewport(state: &mut EditorState, width: u16, height: u16) {
    let rows = u32::from(viewport_rows(height));
    // The FULL terminal width, NOT minus the gutter. `draw_buffer` subtracts
    // the gutter itself (it has to — the width depends on the buffer's line
    // count), and the GPU face splits the same way: `resize` stores the whole
    // grid, `render` reserves the gutter. Subtracting here too would take it
    // twice and clip every line short by a gutter's worth of text.
    let cols = u32::from(width);
    for w in &mut state.layout.windows {
        w.viewport.visible_lines = rows;
        w.viewport.visible_columns = cols.max(1);
    }
    // A resize moves the WINDOW, not the cursor, so nothing else re-runs
    // scroll-to-contain. Without this the cursor sits off-screen after a
    // shrink until the operator happens to move it.
    state.refollow_cursor();
}

/// Draw one frame. Call from within `terminal.draw(|f| draw_frame(f, state))`.
pub fn draw_frame(f: &mut Frame<'_>, state: &EditorState) {
    let area = f.area();
    let chunks = RLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    // The operator's theme, resolved once per frame and handed to every
    // painter. Read from the EDITOR, not from the fleet default — that is
    // what makes `(deftheme :preset …)` reach the screen.
    let chrome = state.chrome();

    // The start screen replaces the buffer pane rather than overlaying it:
    // there is nothing behind it worth showing (escriba only raises it on
    // an empty scratch buffer), and an overlay would have to reason about
    // what it is covering.
    match state.splash() {
        Some(splash) => draw_splash(f, chunks[0], splash, &chrome),
        None => draw_buffer(f, chunks[0], state, &chrome),
    }
    draw_status_line(f, chunks[1], state, &chrome);
    // The picker floats OVER the pane — painted last so it occludes, and
    // outside the splash/buffer match because it is not an alternative to
    // either. This is the first real overlay; the start screen replaces its
    // pane rather than floating, which is why it could never have proven
    // occlusion.
    if let Some(p) = state.picker() {
        draw_picker(f, chunks[0], p, &chrome);
    }
}

/// Paint the start screen.
///
/// All the layout arithmetic lives in `escriba_ui::splash`; this walks the
/// rows it hands back and colors each span by ROLE. That is the whole reason
/// the model exists — the GPU and text faces run the same two loops over the
/// same rows, so the three faces cannot lay the screen out three ways.
fn draw_splash(
    f: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    splash: &escriba_ui::splash::Splash,
    chrome: &ChromePalette,
) {
    let ground = Style::default()
        .fg(rgb(chrome.text))
        .bg(rgb(chrome.background));
    f.render_widget(Block::default().borders(Borders::NONE).style(ground), area);

    for row in splash.rows(area.width, area.height) {
        let spans: Vec<Span<'static>> = row
            .spans
            .iter()
            .map(|s| {
                Span::styled(
                    s.text.clone(),
                    ground.fg(rgb(s.role.color(chrome))).add_modifier(
                        // The wordmark and the menu keys carry the weight;
                        // everything else stays quiet so they can.
                        if matches!(
                            s.role,
                            escriba_ui::splash::SplashRole::Art
                                | escriba_ui::splash::SplashRole::MenuKey
                        ) {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        },
                    ),
                )
            })
            .collect();
        let line_area = ratatui::layout::Rect {
            x: area.x + row.col,
            y: area.y + row.row,
            width: area.width.saturating_sub(row.col),
            height: 1,
        };
        f.render_widget(Paragraph::new(Line::from(spans)).style(ground), line_area);
    }
}

/// Paint the picker as a centred floating panel.
fn draw_picker(
    f: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    picker: &escriba_ui::picker::Picker,
    chrome: &ChromePalette,
) {
    // Centred, and bounded so it never exceeds its pane — a surface that can
    // be drawn outside its container is a panic waiting for a small terminal.
    let w = area.width.saturating_mul(3) / 4;
    let h = (picker.visible_count() as u16 + 3)
        .min(area.height.saturating_sub(2))
        .max(3);
    let panel = ratatui::layout::Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w.min(area.width),
        height: h.min(area.height),
    };

    let ground = Style::default()
        .fg(rgb(chrome.text))
        .bg(rgb(chrome.surface));
    f.render_widget(ratatui::widgets::Clear, panel);

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(panel.height as usize);
    let mut title = String::from(" ");
    title.push_str(picker.source().title());
    title.push_str("  ");
    title.push_str(picker.query());
    lines.push(Line::from(Span::styled(
        title,
        ground.fg(rgb(chrome.accent)).add_modifier(Modifier::BOLD),
    )));
    for (label, selected) in picker.rows() {
        let mut row = String::with_capacity(label.len() + 2);
        row.push_str(if selected { "> " } else { "  " });
        row.push_str(&label);
        lines.push(Line::from(Span::styled(
            row,
            if selected {
                ground.fg(rgb(chrome.background)).bg(rgb(chrome.accent))
            } else {
                ground
            },
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(rgb(chrome.accent)))
        .style(ground);
    f.render_widget(Paragraph::new(lines).block(block), panel);
}

fn draw_buffer(
    f: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    state: &EditorState,
    chrome: &ChromePalette,
) {
    let Some(buf) = state.buffers.get(state.active) else {
        f.render_widget(
            Paragraph::new("<no buffer>").style(error_style(chrome)),
            area,
        );
        return;
    };

    let win = state.layout.active_window();
    let top = win.map_or(0, |w| w.viewport.top_line);
    let left = win.map_or(0, |w| w.viewport.left_column);
    // The gutter's width derives from the buffer, so every line of THIS
    // buffer agrees and the text column cannot move while scrolling. The old
    // comment here claimed a fixed 7 columns; it was never 7 (the mark cell
    // made it 8) and it was never fixed (a 10 000-line file needs 9).
    let line_count = buf.line_count();
    let gutter_cols = escriba_ui::gutter::gutter_width(line_count);
    let vis_cols = win.map_or(usize::MAX, |w| {
        (w.viewport.visible_columns as usize).saturating_sub(gutter_cols)
    });
    let visible = area.height.saturating_sub(2).max(1);
    let cursor = state.cursor();
    // The cursor's on-screen shape is derived from the active mode through
    // the one typed `Mode::cursor_shape` function — block in Normal/Command,
    // bar in Insert, underline in Visual. Both backends read it from there,
    // so the shapes can't drift apart.
    let shape = state.modal.mode().cursor_shape();

    // The visible slice, gathered BEFORE painting so the highlighter sees the
    // rows together — a block comment or multi-line string only reads right
    // that way.
    let visible_text: Vec<String> = (0..visible as u32)
        .map_while(|row| {
            let ln = top + row;
            (ln < buf.line_count()).then(|| {
                buf.line(ln)
                    .unwrap_or_default()
                    .trim_end_matches('\n')
                    .trim_end_matches('\r')
                    .to_string()
            })
        })
        .collect();
    let path = buf
        .path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let syntax = syntax_runs(
        &visible_text,
        &path,
        &escriba_ui::syntax::ChromeSyntax::new(*chrome),
    );

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
        // The worst finding on this line, if any — one cell, always, so a
        // diagnostic arriving does not shift every line sideways.
        let mark = state
            .results
            .worst_on_line(&state.world(), state.active, ln);
        lines.push(line_with_gutter(
            chrome,
            mark,
            ln,
            line_count,
            syntax.get(row as usize).map_or(&[][..], Vec::as_slice),
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
        .style(buffer_style(chrome));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render one line with a gutter, sliced horizontally to the visible
/// column window `[left, left + vis_cols)`. Slicing is char-based (not
/// byte-based) so multibyte text stays aligned, and the cursor's on-screen
/// column is computed relative to `left` so the cursor glyph tracks the
/// horizontal scroll.
fn line_with_gutter(
    chrome: &ChromePalette,
    mark: Option<escriba_shirube::Severity>,
    ln: u32,
    // The buffer's total line count — the gutter's width derives from it, so
    // every line of one buffer agrees. Passed in rather than read here so a
    // test can render a line without standing up an `EditorState`.
    line_count: u32,
    // Syntax colouring for THIS line, in character columns.
    syntax: &[(usize, usize, ishou_tokens::Rgb)],
    text: &str,
    cursor: escriba_core::Position,
    left: usize,
    vis_cols: usize,
    shape: CursorShape,
    highlights: &[(usize, usize)],
) -> Line<'static> {
    // The gutter is COMPOSED by `escriba_ui::gutter`, not here. This face's
    // only job is to turn each cell's role into a ratatui `Style` — which is
    // what makes the GPU face able to paint the identical gutter by answering
    // the same question in its own colours.
    let mut spans: Vec<Span<'static>> = escriba_ui::gutter::gutter_cells(ln, mark, line_count)
        .into_iter()
        .map(|c| {
            let style = match c.role {
                escriba_ui::gutter::GutterRole::Mark(sev) => {
                    Style::default().fg(rgb(escriba_ui::chrome::severity_color(chrome, sev)))
                }
                _ => muted_style(chrome),
            };
            Span::styled(c.text, style)
        })
        .collect();

    let chars: Vec<char> = text.chars().collect();
    // The slice of characters actually visible in this window.
    let visible: Vec<char> = chars.iter().copied().skip(left).take(vis_cols).collect();

    // One style slot per visible cell. Painting cell-by-cell and coalescing
    // afterwards is what lets the cursor and any number of search matches
    // overlap on the same line — the previous before/cursor/after split could
    // only ever express ONE styled region, so highlights had nowhere to go.
    let mut cell_styles: Vec<Option<Style>> = vec![None; visible.len()];
    // Syntax FIRST, so a search match paints over it. The precedence is
    // deliberate and reads bottom-up at the call sites below: syntax, then
    // search, then the cursor — each one is a more urgent thing to see than
    // the one under it.
    for &(ss, se, colour) in syntax {
        for col in ss..se {
            if col >= left {
                if let Some(slot) = cell_styles.get_mut(col - left) {
                    *slot = Some(Style::default().fg(rgb(colour)));
                }
            }
        }
    }
    for &(hs, he) in highlights {
        for col in hs..he {
            if col >= left {
                if let Some(slot) = cell_styles.get_mut(col - left) {
                    *slot = Some(search_match_style(chrome));
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
            spans.extend(cursor_spans(chrome, ' ', shape));
            return Line::from(spans);
        }
        push_runs(&mut spans, &visible[..rel], &cell_styles[..rel]);
        spans.extend(cursor_spans(chrome, visible[rel], shape));
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
fn cursor_spans(c: &ChromePalette, under: char, shape: CursorShape) -> Vec<Span<'static>> {
    match shape {
        CursorShape::Block => vec![Span::styled(under.to_string(), cursor_block_style(c))],
        CursorShape::Bar => vec![
            Span::styled("▏".to_string(), cursor_bar_style(c)),
            Span::raw(under.to_string()),
        ],
        CursorShape::Underline => vec![Span::styled(under.to_string(), cursor_underline_style(c))],
    }
}

fn draw_status_line(
    f: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    state: &EditorState,
    chrome: &ChromePalette,
) {
    // ONE model, read once. The pill and the prompt both derive from it, so
    // they cannot describe two different states of the same editor.
    let model = state.status_model();
    let pos = format!("{}:{}", state.cursor().line + 1, state.cursor().column + 1);
    // Status glyphs are the BORN fleet vocabulary (`ishou_tokens::EscribaSignals`),
    // not hand-picked literals. Single-width `Glyph` mode keeps the
    // status-line column alignment-safe.
    let sig = EscribaSignals::prescribed();

    // The pill leads with the OPEN PROMPT'S sigil when there is one, and the
    // mode glyph otherwise. Search reuses `Mode::Command` (vim's `/` IS the
    // command line), so painting the raw mode drew `: COMMAND` for a search
    // — a status line character-for-character identical to the one `:`
    // produces. That is how a fully working search reads as "pressing `/`
    // put me in `:` mode": the editor was right and its report was wrong.
    let mut pill = String::with_capacity(16);
    pill.push(' ');
    match model.pill_sigil() {
        Some(sigil) => pill.push(sigil),
        None => pill.push_str(mode_signal(&sig, state.modal.mode()).render(SignalMode::Glyph)),
    }
    pill.push(' ');
    pill.push_str(model.mode_label());
    pill.push(' ');
    let mode_span = Span::styled(pill, pill_style_for(chrome, &model, state.modal.mode()));

    // vim puts the command line bottom-LEFT, where the eye already is. This
    // prompt used to render at the far RIGHT, wedged between the match count
    // and the cursor position — `/foo` was on screen and nobody saw it. When
    // a prompt is open it takes the slot the path occupies, the way vim's
    // cmdline covers the status text.
    let context_span = if model.pill_sigil().is_some() {
        let mut line = String::from(" ");
        model.render_prompt_into(&mut line);
        line.push(' ');
        Span::styled(line, cmd_style(chrome))
    } else {
        let path = state
            .buffers
            .get(state.active)
            .and_then(|b| b.path.clone())
            .map_or("scratch".to_string(), |p| p.display().to_string());
        let modified = state.buffers.get(state.active).is_some_and(|b| b.modified);
        let modified_indicator = if modified {
            format!(" {}", sig.modified.render(SignalMode::Glyph))
        } else {
            String::new()
        };
        Span::styled(
            format!(" {path}{modified_indicator} "),
            status_style(chrome),
        )
    };
    let pos_span = Span::styled(format!(" {pos} "), status_style(chrome));

    // `[3/17]`. Both halves were already computed by the engine and both were
    // discarded; the denominator is what turns "press n until it looks right"
    // into a decision — `[1/1]` says a rename is safe, `[1/240]` says narrow
    // the pattern first. Silent when there is nothing to count.
    let count = model.count;
    let count_span = if count.is_idle() {
        Span::raw("")
    } else {
        let mut c = String::from(" ");
        count.render_into(&mut c);
        c.push(' ');
        Span::styled(c, status_style(chrome))
    };

    // Layout: [pill] [prompt-or-path] … (flex) … [count] [pos]
    let available = usize::from(area.width);
    let left = mode_span.content.chars().count() + context_span.content.chars().count();
    let right = count_span.content.chars().count() + pos_span.content.chars().count();
    let pad = available.saturating_sub(left + right);

    let line = Line::from(vec![
        mode_span,
        context_span,
        Span::raw(" ".repeat(pad)),
        count_span,
        pos_span,
    ]);
    f.render_widget(Paragraph::new(line).style(status_style(chrome)), area);
}

// ─── Styles — Vellum (warm aged-paper Nord-matte) ───────────────────────
//
// Every chrome color resolves through `escriba_ui::chrome::ChromePalette`
// — the one theme seam, shared with the GPU backend so the two faces cannot
// drift apart. Colors are named by ROLE (text / surface / cursor / error),
// never by a theme's own token spelling, which is what lets the theme change
// without touching a single call site here.
//
// Each helper takes the LIVE palette rather than reading
// `ChromePalette::prescribed()` for itself. That parameter is the whole
// theming fix: while these read the prescribed value directly, an operator
// could author `(deftheme :preset "vellum")`, watch it parse, validate and
// resolve to a real `FleetTheme` — and see the editor paint Nord anyway,
// because nothing downstream consumed it. A palette that arrives as an
// argument cannot be ignored.

fn buffer_style(c: &ChromePalette) -> Style {
    Style::default().fg(rgb(c.text)).bg(rgb(c.background))
}

fn muted_style(c: &ChromePalette) -> Style {
    Style::default().fg(rgb(c.text_dim)) // comment / gutter
}

/// Block cursor (Normal / Command) — dark glyph filled onto the cursor
/// color, the "you are here" cell.
fn cursor_block_style(c: &ChromePalette) -> Style {
    Style::default()
        .fg(rgb(c.background)) // ground-colored text on the cursor
        .bg(rgb(c.cursor))
        .add_modifier(Modifier::BOLD)
}

/// Bar cursor (Insert) — the thin vertical caret drawn between glyphs,
/// colored in the cursor accent.
fn cursor_bar_style(c: &ChromePalette) -> Style {
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
fn search_match_style(c: &ChromePalette) -> Style {
    Style::default().fg(rgb(c.background)).bg(rgb(c.warning))
}

fn cursor_underline_style(c: &ChromePalette) -> Style {
    Style::default()
        .fg(rgb(c.cursor))
        .add_modifier(Modifier::UNDERLINED)
        .add_modifier(Modifier::BOLD)
}

fn status_style(c: &ChromePalette) -> Style {
    // Was a raw `Color::Rgb(0xCD, 0xC7, 0xB6)` literal ("statusline_fg,
    // Vellum extra") — the one genuinely hardcoded color in this file, and
    // dead weight the moment the theme moved. It is now the `text` role.
    Style::default().fg(rgb(c.text)).bg(rgb(c.surface))
}

fn cmd_style(c: &ChromePalette) -> Style {
    Style::default()
        .fg(rgb(c.warning))
        .bg(rgb(c.surface))
        .add_modifier(Modifier::BOLD)
}

fn error_style(c: &ChromePalette) -> Style {
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

fn mode_style_for(c: &ChromePalette, mode: escriba_core::Mode) -> Style {
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

/// The pill's style, chosen from the status MODEL rather than the raw mode.
///
/// A search and an ex-command share `Mode::Command`, so [`mode_style_for`]
/// alone paints them identically — same colour, and (before this) the same
/// `: COMMAND` text. The search gets the accent field so the two prompts are
/// distinguishable at a glance, not only by reading the label.
fn pill_style_for(
    c: &ChromePalette,
    model: &escriba_runtime::StatusModel<'_>,
    mode: escriba_core::Mode,
) -> Style {
    if model.prompt.is_search() {
        return Style::default()
            .fg(rgb(c.background))
            .bg(rgb(c.accent))
            .add_modifier(Modifier::BOLD);
    }
    mode_style_for(c, mode)
}

#[cfg(test)]
mod tests {

    // ── search highlight rendering ────────────────────────────────────

    /// The palette the render tests paint with. A FIXED theme, not the
    /// live one: these assert LAYOUT and span structure, and pinning them
    /// to whatever the fleet currently prescribes would make them rewrite
    /// themselves on every theme move (which is exactly what happened to
    /// the mode-pill test before it started asserting roles).
    fn chrome() -> ChromePalette {
        ChromePalette::prescribed()
    }

    fn styles_of(spans: &[Span<'static>]) -> Vec<(String, bool)> {
        // (text, is-highlighted) — comparing against the exact Style would
        // pin the palette, which is a theming concern, not a layout one.
        spans
            .iter()
            .map(|sp| {
                (
                    sp.content.to_string(),
                    sp.style.bg == search_match_style(&chrome()).bg,
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
            *slot = Some(search_match_style(&chrome()));
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
            &chrome(),
            None,
            0,
            64,
            &[],
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
            &chrome(),
            None,
            0,
            64,
            &[],
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
            &chrome(),
            None,
            0,
            64,
            &[],
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
            &chrome(),
            None,
            0,
            64,
            &[],
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
            &chrome(),
            None,
            0,
            64,
            &[],
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
        let block = cursor_spans(&chrome(), 'a', CursorShape::Block);
        assert_eq!(block.len(), 1);
        assert_eq!(block[0].content, "a");
        // The cursor ROLE, not a theme's own token — this assertion used to
        // name `VellumPalette::vellum().green_bright`, which pinned the test
        // to one theme and would have had to change on every theme move.
        assert_eq!(block[0].style.bg, Some(rgb(chrome().cursor)));

        // Bar: a thin caret span BEFORE the (unstyled) glyph.
        let bar = cursor_spans(&chrome(), 'a', CursorShape::Bar);
        assert_eq!(bar.len(), 2);
        assert_eq!(bar[0].content, "▏");
        assert_eq!(bar[1].content, "a");
        assert_eq!(bar[1].style.bg, None, "bar leaves the glyph cell unfilled");

        // Underline: one glyph span carrying the UNDERLINED modifier.
        let under = cursor_spans(&chrome(), 'a', CursorShape::Underline);
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
        assert_eq!(buffer_style(&c).bg, Some(rgb(c.background)));
        assert_eq!(buffer_style(&c).fg, Some(rgb(c.text)));
    }
}
