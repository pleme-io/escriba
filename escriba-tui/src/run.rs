//! The interactive TUI event loop — owns the terminal, drives ticks.

use std::io::{Write, stdout};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, execute};
use escriba_runtime::EditorState;
use madori::AppEvent;
use madori::event::{KeyEvent as MadoriKey, Modifiers as MadoriMods};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::keys::translate_crossterm_key;
use crate::render::draw_frame;

/// Enter raw mode + alt screen, loop until `EditorState::quit_requested`.
pub fn run(mut state: EditorState) -> Result<()> {
    let mut out = stdout();
    out.execute(EnterAlternateScreen)
        .context("claiming alt screen")?;
    enable_raw_mode().context("enabling raw mode")?;
    let result = inner_loop(&mut state);
    // Always restore the terminal, even on error.
    let _ = disable_raw_mode();
    let _ = execute!(out, LeaveAlternateScreen);
    result
}

fn inner_loop(state: &mut EditorState) -> Result<()> {
    let mut terminal =
        Terminal::new(CrosstermBackend::new(stdout())).context("opening ratatui terminal")?;

    // First draw before waiting for input.
    terminal.draw(|f| draw_frame(f, state))?;

    loop {
        if state.quit_requested {
            return Ok(());
        }
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(ke) if ke.kind == KeyEventKind::Press => {
                    // Translate into escriba's madori-shaped AppEvent so the
                    // runtime can use the same tick() pipeline the GPU path does.
                    if let Some(_key) = translate_crossterm_key(&ke) {
                        let app_event = crossterm_to_app_event(&ke);
                        state.tick(&app_event);
                    }
                }
                Event::Resize(_, _) => {
                    // ratatui auto-picks up the new size on the next draw.
                }
                Event::FocusGained | Event::FocusLost | Event::Mouse(_) | Event::Paste(_) => {
                    // Phase 2 routes.
                }
                _ => {}
            }
        }
        terminal.draw(|f| draw_frame(f, state))?;
    }
}

/// Convert a crossterm key event into the madori-shaped AppEvent the
/// runtime expects. This keeps a single `EditorState::tick` path across
/// GPU (madori), text (snapshot), and TUI (crossterm) backends.
fn crossterm_to_app_event(ke: &crossterm::event::KeyEvent) -> AppEvent {
    use crossterm::event::KeyCode as CkKey;
    use madori::event::KeyCode as MdKey;
    let mods = MadoriMods {
        shift: ke.modifiers.contains(crossterm::event::KeyModifiers::SHIFT),
        ctrl: ke
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL),
        alt: ke.modifiers.contains(crossterm::event::KeyModifiers::ALT),
        meta: ke.modifiers.contains(crossterm::event::KeyModifiers::SUPER),
    };
    let code = match ke.code {
        CkKey::Enter => MdKey::Enter,
        CkKey::Esc => MdKey::Escape,
        CkKey::Backspace => MdKey::Backspace,
        CkKey::Tab => MdKey::Tab,
        CkKey::Up => MdKey::Up,
        CkKey::Down => MdKey::Down,
        CkKey::Left => MdKey::Left,
        CkKey::Right => MdKey::Right,
        CkKey::Home => MdKey::Home,
        CkKey::End => MdKey::End,
        CkKey::PageUp => MdKey::PageUp,
        CkKey::PageDown => MdKey::PageDown,
        CkKey::Delete => MdKey::Delete,
        CkKey::Char(' ') => MdKey::Space,
        CkKey::Char(c) => MdKey::Char(c),
        _ => MdKey::Unknown,
    };
    AppEvent::Key(MadoriKey {
        key: code,
        pressed: true,
        modifiers: mods,
        text: None,
    })
}
