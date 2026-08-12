//! The interactive TUI event loop — owns the terminal, drives ticks.

use std::io::stdout;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::cursor::SetCursorStyle;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, execute};
use escriba_runtime::EditorState;
use madori::AppEvent;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::keys::crossterm_key_event;
use crate::render::draw_frame;

/// Enter raw mode + alt screen, loop until `EditorState::quit_requested`.
pub fn run(mut state: EditorState) -> Result<()> {
    let mut out = stdout();
    out.execute(EnterAlternateScreen)
        .context("claiming alt screen")?;
    enable_raw_mode().context("enabling raw mode")?;
    // Shape the terminal's own cursor as a bar, ONCE.
    //
    // The terminal cursor is only ever made visible in the two places a bar is
    // the right shape: Insert mode (parked by `draw_pane`) and an open prompt
    // (parked by `draw_status_line`). Normal and Visual paint their own block /
    // underline into the cell and leave the terminal cursor hidden, so there is
    // no per-frame shape to track — a `SetCursorStyle` per draw would be a
    // write to the tty on every keystroke to say something that never changes.
    //
    // Best-effort: a terminal that does not implement DECSCUSR ignores it and
    // shows its default cursor, which is still a real cursor in the right cell.
    // That is why this is not `?` — failing to launch the editor over a
    // shape hint would be the worse outcome.
    let _ = out.execute(SetCursorStyle::SteadyBar);
    let result = inner_loop(&mut state);
    // Always restore the terminal, even on error. The cursor style is part of
    // that: leaving a bar behind would edit the operator's SHELL prompt.
    let _ = execute!(out, SetCursorStyle::DefaultUserShape);
    let _ = disable_raw_mode();
    let _ = execute!(out, LeaveAlternateScreen);
    result
}

fn inner_loop(state: &mut EditorState) -> Result<()> {
    let mut terminal =
        Terminal::new(CrosstermBackend::new(stdout())).context("opening ratatui terminal")?;

    // Tell the runtime how much this terminal can show BEFORE the first
    // draw, so `scroll_to_contain` is computed against the real window
    // rather than the constructor's 40-line default.
    let size = terminal.size()?;
    crate::render::sync_viewport(state, size.width, size.height);
    terminal.draw(|f| draw_frame(f, state))?;

    loop {
        if state.quit_requested {
            return Ok(());
        }
        // Courier replies first, before any input is read.
        //
        // Not hung off the input path on purpose: `event::poll` returning
        // false — every quiet moment, which is most of a scan — skips the
        // whole match arm below, so a drain in there would only run when
        // the operator happened to be typing.
        state.deliver();
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(ke) if ke.kind == KeyEventKind::Press => {
                    // Translate into escriba's madori-shaped AppEvent so the
                    // runtime can use the same tick() pipeline the GPU path
                    // does. No pre-gate: `tick` already drops what it cannot
                    // translate, and the gate that used to stand here read a
                    // SECOND table which disagreed with this one about
                    // `Delete` and the F-keys.
                    state.tick(&AppEvent::Key(crossterm_key_event(&ke)));
                }
                Event::Resize(w, h) => {
                    // ratatui picks up the new size for PAINTING on its own.
                    // The viewport MODEL does not follow, and that is the
                    // difference that let the cursor scroll off screen: the
                    // editor kept believing it could show 40 lines.
                    crate::render::sync_viewport(state, w, h);
                }
                Event::FocusGained | Event::FocusLost | Event::Mouse(_) | Event::Paste(_) => {
                    // Phase 2 routes.
                }
                _ => {}
            }
        }
        let size = terminal.size()?;
        crate::render::sync_viewport(state, size.width, size.height);
        terminal.draw(|f| draw_frame(f, state))?;
    }
}
