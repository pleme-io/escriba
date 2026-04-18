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
}

pub type CommandFn = fn(&mut EditContext<'_>, &[String]) -> Result<()>;

#[derive(Debug, Clone)]
pub struct Command {
    pub name: &'static str,
    pub description: &'static str,
    pub handler: CommandFn,
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
        r.register(Command {
            name: "save",
            description: "Write the active buffer to disk",
            handler: cmd_save,
        });
        r.register(Command {
            name: "quit",
            description: "Exit the editor",
            handler: cmd_quit,
        });
        r.register(Command {
            name: "undo",
            description: "Undo the last change",
            handler: cmd_undo,
        });
        r.register(Command {
            name: "redo",
            description: "Redo the last undone change",
            handler: cmd_redo,
        });
        r.register(Command {
            name: "buffer-info",
            description: "Print the active buffer summary",
            handler: cmd_buffer_info,
        });
        r
    }

    pub fn register(&mut self, command: Command) {
        self.commands.insert(command.name.to_string(), command);
    }

    pub fn run(&self, name: &str, ctx: &mut EditContext<'_>, args: &[String]) -> Result<()> {
        let cmd = self
            .commands
            .get(name)
            .ok_or_else(|| CommandError::NotFound(name.to_string()))?;
        (cmd.handler)(ctx, args)
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
    // Quit is signaled via state.minibuffer — phase 1 uses a sentinel, phase 2
    // graduates to a proper Result enum with QuitRequested(code).
    ctx.state.minibuffer.push_str("__quit__");
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
        let mut ctx = EditContext {
            buffers: &mut bufs,
            active: None,
            state: &mut state,
        };
        let err = r.run("nope", &mut ctx, &[]).unwrap_err();
        assert!(matches!(err, CommandError::NotFound(_)));
    }
}
