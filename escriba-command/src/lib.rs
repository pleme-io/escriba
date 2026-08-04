//! `escriba-command` — command registry + palette.

extern crate self as escriba_command;

use std::collections::HashMap;

use escriba_buffer::BufferSet;
use escriba_core::BufferId;
use escriba_mode::ModalState;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("command not found: {0}")]
    NotFound(String),
    #[error("command failed: {0}")]
    Failed(String),
    #[error("buffer: {0}")]
    Buffer(#[from] escriba_buffer::BufferError),
}

pub type Result<T> = std::result::Result<T, CommandError>;

pub struct EditContext<'a> {
    pub buffers: &'a mut BufferSet,
    pub active: Option<BufferId>,
    pub state: &'a mut ModalState,
    /// Typed quit signal. A command that wants to exit the editor sets
    /// this to `true`; the runtime reads it after `run`. This replaces the
    /// old stringly-typed `__quit__` sentinel that was smuggled through the
    /// command minibuffer — a channel that only existed because `minibuffer`
    /// used to be a mode-independent scratch `String`. With the typed
    /// modal sum, the minibuffer exists only in Command mode, so quit is now
    /// a proper typed flag, not a buffer hack.
    pub quit_requested: &'a mut bool,
}

pub type CommandFn = fn(&mut EditContext<'_>, &[String]) -> Result<()>;

/// How a command executes when invoked.
///
/// - [`Handler::Native`] wraps a compiled-in Rust `fn` — the
///   built-in command set (`save`, `quit`, …).
/// - [`Handler::Action`] carries a dotted action symbol
///   (e.g. `"buffer.write-all"`, `"picker.files"`) authored via a
///   Tatara-Lisp `(defcmd …)` form and resolved at run time by
///   [`run_action`]. This is what lets `defcmd` register a real,
///   invokable command without a compiled handler.
///
/// A future `Lisp(Thunk)` variant will carry a `tatara-lisp-eval`
/// closure for fully-programmable commands — the imperative tier of
/// the two-tier programmability model. Keeping the handler an enum
/// (not a bare `fn`) is what makes that extension a one-variant add.
#[derive(Debug, Clone)]
pub enum Handler {
    /// Compiled-in Rust handler.
    Native(CommandFn),
    /// Dotted action symbol resolved at run time (Lisp `defcmd`).
    Action(String),
}

#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub handler: Handler,
}

impl Command {
    /// A built-in command backed by a compiled-in Rust `fn`.
    pub fn native(
        name: impl Into<String>,
        description: impl Into<String>,
        handler: CommandFn,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            handler: Handler::Native(handler),
        }
    }

    /// A Lisp-authored command whose behavior is a dotted action
    /// symbol resolved at run time. Mirrors `(defcmd :name … :action
    /// "buffer.write-all")`.
    pub fn action(
        name: impl Into<String>,
        description: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            handler: Handler::Action(action.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CommandSpec {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub args: Vec<CommandArgSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CommandArgSpec {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct CommandRegistry {
    commands: HashMap<String, Command>,
}

impl CommandRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn default_set() -> Self {
        let mut r = Self::new();
        r.register(Command::native(
            "save",
            "Write the active buffer to disk",
            cmd_save,
        ));
        r.register(Command::native("quit", "Exit the editor", cmd_quit));
        r.register(Command::native("undo", "Undo the last change", cmd_undo));
        r.register(Command::native(
            "redo",
            "Redo the last undone change",
            cmd_redo,
        ));
        r.register(Command::native(
            "buffer-info",
            "Print the active buffer summary",
            cmd_buffer_info,
        ));
        r
    }

    pub fn register(&mut self, command: Command) {
        self.commands.insert(command.name.clone(), command);
    }

    /// Is `name` registered? Lets the apply layer report
    /// override-vs-new without exposing the inner map.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    /// Number of registered commands.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// True when no commands are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub fn run(&self, name: &str, ctx: &mut EditContext<'_>, args: &[String]) -> Result<()> {
        let cmd = self
            .commands
            .get(name)
            .ok_or_else(|| CommandError::NotFound(name.to_string()))?;
        match &cmd.handler {
            Handler::Native(f) => f(ctx, args),
            Handler::Action(sym) => run_action(sym, ctx, args),
        }
    }

    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.commands.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    #[must_use]
    pub fn specs(&self) -> Vec<CommandSpec> {
        let mut out: Vec<CommandSpec> = self
            .commands
            .values()
            .map(|c| CommandSpec {
                name: c.name.to_string(),
                description: c.description.to_string(),
                args: Vec::new(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

/// Resolve a dotted action symbol — the `:action` of a Lisp
/// `(defcmd …)` form — to a built-in behavior.
///
/// Canonical `buffer.*` / `editor.*` symbols map onto the same
/// primitives the native commands use, so a Lisp-authored command is
/// genuinely invokable (not a stub). Symbols that have no built-in
/// yet (`picker.*`, `telescope.*`, plugin-provided actions) are an
/// inert `Ok(())` — the command stays registered and invokable, it
/// just does nothing until its wave lands. Crucially this NEVER
/// errors or panics, so a deferred keybind whose action resolves to
/// an unimplemented symbol degrades to a no-op instead of a crash.
fn run_action(sym: &str, ctx: &mut EditContext<'_>, args: &[String]) -> Result<()> {
    match sym {
        "buffer.save" | "buffer.write" => cmd_save(ctx, args),
        "buffer.write-all" => cmd_write_all(ctx, args),
        "buffer.undo" => cmd_undo(ctx, args),
        "buffer.redo" => cmd_redo(ctx, args),
        "buffer.info" => cmd_buffer_info(ctx, args),
        "editor.quit" => cmd_quit(ctx, args),
        // Not-yet-implemented action namespace. Registered + invokable
        // but inert until the relevant wave (pickers, plugin actions,
        // tatara-lisp thunks) wires it. Inert, never fatal.
        _ => Ok(()),
    }
}

/// Save every modified, path-backed buffer. Best-effort: a single
/// buffer's save failure (e.g. a permission error) must not abort the
/// rest, and scratch buffers (no path) are skipped rather than
/// surfacing [`BufferError::NoPath`].
fn cmd_write_all(ctx: &mut EditContext<'_>, _args: &[String]) -> Result<()> {
    for id in ctx.buffers.ids() {
        if let Some(buf) = ctx.buffers.get_mut(id) {
            if buf.modified && buf.path.is_some() {
                let _ = buf.save();
            }
        }
    }
    Ok(())
}

fn cmd_save(ctx: &mut EditContext<'_>, _args: &[String]) -> Result<()> {
    let id = ctx
        .active
        .ok_or_else(|| CommandError::Failed("no active buffer".into()))?;
    let buf = ctx
        .buffers
        .get_mut(id)
        .ok_or_else(|| CommandError::Failed("active buffer gone".into()))?;
    buf.save()?;
    Ok(())
}

fn cmd_quit(ctx: &mut EditContext<'_>, _: &[String]) -> Result<()> {
    // Quit is a typed signal on the edit context — no string sentinel,
    // no mode-specific scratch buffer. Phase 2 graduates this to a proper
    // Result enum carrying `QuitRequested(code)`.
    *ctx.quit_requested = true;
    Ok(())
}

fn cmd_undo(ctx: &mut EditContext<'_>, _: &[String]) -> Result<()> {
    let id = ctx
        .active
        .ok_or_else(|| CommandError::Failed("no active buffer".into()))?;
    ctx.buffers
        .get_mut(id)
        .ok_or_else(|| CommandError::Failed("gone".into()))?
        .undo()?;
    Ok(())
}

fn cmd_redo(ctx: &mut EditContext<'_>, _: &[String]) -> Result<()> {
    let id = ctx
        .active
        .ok_or_else(|| CommandError::Failed("no active buffer".into()))?;
    ctx.buffers
        .get_mut(id)
        .ok_or_else(|| CommandError::Failed("gone".into()))?
        .redo()?;
    Ok(())
}

fn cmd_buffer_info(ctx: &mut EditContext<'_>, _: &[String]) -> Result<()> {
    let id = ctx
        .active
        .ok_or_else(|| CommandError::Failed("no active buffer".into()))?;
    let buf = ctx
        .buffers
        .get(id)
        .ok_or_else(|| CommandError::Failed("gone".into()))?;
    eprintln!(
        "buffer {} — {} line(s), {} char(s){}",
        id,
        buf.line_count(),
        buf.char_count(),
        if buf.modified { " [modified]" } else { "" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_set_is_populated() {
        let r = CommandRegistry::default_set();
        let names = r.names();
        assert!(names.contains(&"save"));
        assert!(names.contains(&"quit"));
    }

    #[test]
    fn specs_are_sorted() {
        let r = CommandRegistry::default_set();
        let specs = r.specs();
        assert!(specs.windows(2).all(|w| w[0].name <= w[1].name));
    }

    #[test]
    fn not_found_errors() {
        let r = CommandRegistry::new();
        let mut bufs = BufferSet::new();
        let mut state = ModalState::new();
        let mut quit = false;
        let mut ctx = EditContext {
            buffers: &mut bufs,
            active: None,
            state: &mut state,
            quit_requested: &mut quit,
        };
        let err = r.run("nope", &mut ctx, &[]).unwrap_err();
        assert!(matches!(err, CommandError::NotFound(_)));
    }

    #[test]
    fn action_command_registers_and_is_invokable() {
        // A Lisp-authored `(defcmd :name "w-all" :action
        // "buffer.write-all")` registers an `Action` handler. It must
        // resolve (not NotFound) and run without error over a scratch
        // buffer (write-all skips path-less buffers).
        let mut r = CommandRegistry::new();
        r.register(Command::action(
            "w-all",
            "Write every modified buffer",
            "buffer.write-all",
        ));
        assert!(r.contains("w-all"));

        let mut bufs = BufferSet::new();
        let id = bufs.scratch("dirty");
        bufs.get_mut(id).unwrap().modified = true;
        let mut state = ModalState::new();
        let mut quit = false;
        let mut ctx = EditContext {
            buffers: &mut bufs,
            active: Some(id),
            state: &mut state,
            quit_requested: &mut quit,
        };
        // No path → write-all is a no-op, never NoPath-errors.
        r.run("w-all", &mut ctx, &[]).expect("action command runs");
    }

    #[test]
    fn unknown_action_symbol_is_inert_not_fatal() {
        // An action symbol with no built-in (a future picker/plugin
        // action) is registered + invokable but does nothing — it must
        // never error, so a deferred keybind can't dead-end loudly.
        let mut r = CommandRegistry::new();
        r.register(Command::action("pick", "Pick a file", "picker.files"));
        let mut bufs = BufferSet::new();
        let mut state = ModalState::new();
        let mut quit = false;
        let mut ctx = EditContext {
            buffers: &mut bufs,
            active: None,
            state: &mut state,
            quit_requested: &mut quit,
        };
        r.run("pick", &mut ctx, &[])
            .expect("unknown action is inert");
    }

    #[test]
    fn action_naming_a_command_is_inert_not_recursive() {
        // A `defcmd :action` that names another COMMAND ("save") rather
        // than a dotted action symbol ("buffer.save") is INERT —
        // run_action only resolves dotted symbols and does NOT recurse
        // into the registry. This pins the boundary: `:action` takes
        // action SYMBOLS, not command names. (If aliasing-by-name is
        // ever wanted, run_action must take registry access and this
        // test will flip.)
        let mut r = CommandRegistry::new();
        r.register(Command::action("alias", "aliases save by name", "save"));
        let mut bufs = BufferSet::new();
        let id = bufs.scratch("dirty");
        bufs.get_mut(id).unwrap().modified = true;
        let mut state = ModalState::new();
        let mut quit = false;
        {
            let mut ctx = EditContext {
                buffers: &mut bufs,
                active: Some(id),
                state: &mut state,
                quit_requested: &mut quit,
            };
            r.run("alias", &mut ctx, &[]).expect("alias runs inertly");
        }
        assert!(
            bufs.get(id).unwrap().modified,
            "command-name alias must be inert — save did not fire (no recursion)",
        );
    }

    #[test]
    fn action_quit_sets_quit_flag() {
        // `editor.quit` must route to the same typed quit signal the
        // native quit command uses, so a Lisp-defined quit alias behaves
        // identically to the built-in.
        let mut r = CommandRegistry::new();
        r.register(Command::action("bye", "Quit", "editor.quit"));
        let mut bufs = BufferSet::new();
        let mut state = ModalState::new();
        let mut quit = false;
        let mut ctx = EditContext {
            buffers: &mut bufs,
            active: None,
            state: &mut state,
            quit_requested: &mut quit,
        };
        r.run("bye", &mut ctx, &[]).expect("quit action runs");
        assert!(*ctx.quit_requested, "editor.quit sets the typed quit flag");
    }
}
