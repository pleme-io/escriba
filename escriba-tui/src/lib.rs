//! `escriba-tui` — ratatui terminal renderer.
//!
//! Third render backend alongside the GPU window (garasu) and the headless
//! text snapshot. Runs inside any TTY (ghostty, alacritty, iTerm, kitty,
//! ssh -t, tmux, CI with a pty). Same modal editing, same command palette,
//! same tatara-lisp config — different presentation layer.
//!
//! Architecture:
//!   1. `run()` enters raw mode, claims the alternate screen.
//!   2. A `poll_events` loop reads crossterm key events, translates each
//!      into an `escriba_runtime` tick, marks the UI dirty.
//!   3. On every dirty tick, ratatui redraws the frame — buffer pane +
//!      status line, with Nord-inspired styling.
//!   4. On quit (Ctrl-C, `:q`, Esc on the top-level), raw mode is released
//!      and the alternate screen restored.
//!
//! Same `EditorState` powers all three backends, so behavior is identical;
//! only the rendering target differs.

extern crate self as escriba_tui;

pub mod keys;
pub mod render;
pub mod run;

pub use keys::translate_crossterm_key;
pub use render::draw_frame;
pub use run::run;
