//! `escriba-command` — command registry + palette.

extern crate self as escriba_command;

use std::collections::HashMap;

use escriba_core::BufferId;
use escriba_madoguchi::cap::{Buffers, Cursor, Syntax};
use escriba_madoguchi::{BufferView, Native, Negai, Outcome, Snapshot, View, caps, erase};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("command not found: {0}")]
    NotFound(String),
    /// A registered command whose action symbol nothing implements yet.
    ///
    /// Distinct from [`NotFound`](Self::NotFound), and the distinction is the
    /// point: `NotFound` means the operator typed a name that does not exist,
    /// while `Unhandled` means the editor ADVERTISED a binding — it is in
    /// `--commands`, it is in the keymap, `--list-rc` counts it — and then did
    /// nothing. The second is the more misleading of the two and used to be
    /// the silent one.
    #[error("action `{0}` is declared but not implemented yet")]
    Unhandled(String),
    #[error("command failed: {0}")]
    Failed(String),
    // NOTE: there was a `Buffer(#[from] BufferError)` variant here. The M2
    // port made it dead: a command no longer performs I/O, so it cannot
    // produce a buffer error. Save/undo/redo failures now surface from the
    // interpreter, which is the thing that actually touches the filesystem.
}

pub type Result<T> = std::result::Result<T, CommandError>;

/// A command body.
///
/// Reads through the counter, returns slips. There is no `&mut` in this
/// signature, which is the point: a command cannot reach editor state, so it
/// cannot corrupt it. It replaces `fn(&mut EditContext, &[String])`, whose
/// `&mut BufferSet` was simultaneously too much power and too little reach —
/// the runtime still had to special-case `:noh` because `EditContext` could
/// not see `SearchState`.
pub type CommandFn = fn(&dyn Snapshot, &[String]) -> Outcome;

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
            erase::<Save>(),
        ));
        r.register(Command::native("quit", "Exit the editor", erase::<Quit>()));
        // Named for the ACTION SYMBOLS the shipped keybindings use, so
        // `<leader>bn` resolves instead of reporting "declared but not
        // implemented yet". These are the first three entries to leave the
        // INERT inventory in escriba/tests/action_resolution.rs.
        r.register(Command::native(
            "buffer.next",
            "Go to the next buffer",
            erase::<BufferNext>(),
        ));
        r.register(Command::native(
            "buffer.prev",
            "Go to the previous buffer",
            erase::<BufferPrev>(),
        ));
        r.register(Command::native(
            "buffer.delete",
            "Close the active buffer",
            erase::<BufferDelete>(),
        ));
        for name in ["comment.toggle-line", "comment.toggle-block"] {
            r.register(Command::native(
                name,
                "Toggle the comment on the current line",
                erase::<CommentToggle>(),
            ));
        }
        for alias in ["noh", "nohl", "nohlsearch"] {
            r.register(Command::action(
                alias,
                "Stop highlighting matches, keep the pattern",
                "search.clear-highlight",
            ));
        }
        r.register(Command::native(
            "undo",
            "Undo the last change",
            erase::<Undo>(),
        ));
        r.register(Command::native(
            "redo",
            "Redo the last undone change",
            erase::<Redo>(),
        ));
        r.register(Command::native(
            "buffer-info",
            "Print the active buffer summary",
            erase::<Info>(),
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

    /// Dispatch `name`.
    ///
    /// `Err` means the registry could not dispatch at all — Phase 0's two
    /// failures, kept distinct because they mean different things to the
    /// operator. `Ok(outcome)` means a body ran and reported for itself.
    pub fn run(&self, name: &str, snap: &dyn Snapshot, args: &[String]) -> Result<Outcome> {
        let cmd = self
            .commands
            .get(name)
            .ok_or_else(|| CommandError::NotFound(name.to_string()))?;
        match &cmd.handler {
            Handler::Native(f) => Ok(f(snap, args)),
            Handler::Action(sym) => run_action(sym, snap, args),
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

/// Resolve a dotted action symbol to a built-in body.
fn run_action(sym: &str, snap: &dyn Snapshot, args: &[String]) -> Result<Outcome> {
    match sym {
        "buffer.save" | "buffer.write" => Ok(erase::<Save>()(snap, args)),
        "buffer.write-all" => Ok(erase::<WriteAll>()(snap, args)),
        "buffer.undo" => Ok(erase::<Undo>()(snap, args)),
        "buffer.redo" => Ok(erase::<Redo>()(snap, args)),
        "buffer.info" => Ok(erase::<Info>()(snap, args)),
        "editor.quit" => Ok(erase::<Quit>()(snap, args)),
        "search.clear-highlight" => Ok(erase::<Noh>()(snap, args)),
        // The not-yet-implemented namespace — 85 shipped keybindings land
        // here (escriba/tests/action_resolution.rs pins the inventory).
        // Inert and ANNOUNCED; see CommandError::Unhandled.
        _ => Err(CommandError::Unhandled(sym.to_string())),
    }
}

/// The active buffer, or the outcome to return when there isn't one.
///
/// "No buffer" is a DECLINE, not a failure: it is a legitimate state (boot,
/// every `--no-defaults` run) and the operator did nothing wrong.
fn active_or_decline(b: &escriba_madoguchi::snapshot::Buffers<'_>) -> Result2<BufferId> {
    b.active()
        .map(BufferView::id)
        .ok_or_else(|| Outcome::declined("no active buffer"))
}

type Result2<T> = std::result::Result<T, Outcome>;

/// Save every modified, path-backed buffer.
///
/// Best-effort BY CONSTRUCTION: one slip per buffer, applied independently,
/// so one buffer's permission error cannot abort the rest. Scratch buffers
/// have no path and are skipped.
struct WriteAll;
impl Native for WriteAll {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _args: &[String]) -> Outcome {
        let b = v.buffers();
        let slips: Vec<Negai> = b
            .ids()
            .into_iter()
            .filter(|id| {
                b.get(*id)
                    .is_some_and(|x| x.is_modified() && x.path().is_some())
            })
            .map(|buffer| Negai::Save { buffer })
            .collect();
        if slips.is_empty() {
            return Outcome::declined("no modified files");
        }
        Outcome::did(slips)
    }
}

struct Save;
impl Native for Save {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        match active_or_decline(&v.buffers()) {
            Ok(buffer) => Outcome::did(vec![Negai::Save { buffer }]),
            Err(o) => o,
        }
    }
}

struct Undo;
impl Native for Undo {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        match active_or_decline(&v.buffers()) {
            Ok(buffer) => Outcome::did(vec![Negai::Undo { buffer }]),
            Err(o) => o,
        }
    }
}

struct Redo;
impl Native for Redo {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        match active_or_decline(&v.buffers()) {
            Ok(buffer) => Outcome::did(vec![Negai::Redo { buffer }]),
            Err(o) => o,
        }
    }
}

/// Report the active buffer's shape.
///
/// This used to `eprintln!`. From a TUI holding the alternate screen that
/// writes straight through the ratatui frame and corrupts it — a latent bug
/// the port removed for free, because a command's only way to say something
/// is now `Negai::Message`, which lands on the status line.
struct Info;
impl Native for Info {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        let b = v.buffers();
        let Some(buf) = b.active() else {
            return Outcome::declined("no active buffer");
        };
        let mut m = String::with_capacity(48);
        m.push_str("buffer ");
        m.push_str(&buf.id().0.to_string());
        m.push_str(" — ");
        m.push_str(&buf.line_count().to_string());
        m.push_str(" line(s)");
        if buf.is_modified() {
            m.push_str(" [modified]");
        }
        Outcome::did(vec![Negai::Message(m)])
    }
}

/// Quit reads NOTHING.
///
/// Worth pausing on: under the old `EditContext` this function was handed
/// `&mut BufferSet` and `&mut ModalState` in order to set one bool. Its
/// capability set is now literally empty, and the type system enforces that
/// — `caps!()` proves no membership, so every accessor on its view is
/// unbuildable.
/// `buffer.next` / `buffer.prev` — walk the buffer list.
struct BufferNext;
impl Native for BufferNext {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::CycleBuffer { forward: true }])
    }
}

struct BufferPrev;
impl Native for BufferPrev {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::CycleBuffer { forward: false }])
    }
}

/// `buffer.delete` — close the active buffer.
///
/// Reads `Buffers` only to name WHICH buffer; whether a modified buffer may
/// close, and what becomes active afterwards, are the interpreter's policy.
struct BufferDelete;
impl Native for BufferDelete {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        match active_or_decline(&v.buffers()) {
            Ok(buffer) => Outcome::did(vec![Negai::CloseBuffer(buffer)]),
            Err(o) => o,
        }
    }
}

/// `comment.toggle-line` / `comment.toggle-block` — the first commands to
/// need TWO capabilities, and the first consumer of `:commentstring`.
///
/// Toggle, not comment: if the line is already commented it is uncommented.
/// A one-way "comment" verb makes the same keystroke mean two things
/// depending on state, which is how you end up with `//// x`.
struct CommentToggle;
impl Native for CommentToggle {
    type Reads = caps!(Buffers, Cursor, Syntax);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        let Some(ft) = v.syntax().filetype() else {
            return Outcome::declined("no filetype for this buffer");
        };
        let Some(comment) = ft.comment.as_ref() else {
            let mut m = String::from("no comment syntax for ");
            m.push_str(&ft.name);
            return Outcome::declined(m);
        };
        let b = v.buffers();
        let Some(buf) = b.active() else {
            return Outcome::declined("no active buffer");
        };
        let line_no = v.cursor().position().line;
        let Some(line) = buf.line(line_no) else {
            return Outcome::declined("cursor past the end of the buffer");
        };
        // An empty line has nothing to comment, and commenting it would
        // leave a bare marker the next toggle cannot recognise as content.
        if line.trim().is_empty() {
            return Outcome::declined("nothing on this line");
        }

        // Indentation is preserved: a comment marker inserted before the
        // indent would destroy the alignment the code is relying on.
        let indent_len = line.len() - line.trim_start().len();
        let (indent, body) = line.split_at(indent_len);
        let toggled = match comment.strip(body) {
            Some(uncommented) => uncommented.to_string(),
            None => comment.wrap(body),
        };
        let mut text = String::with_capacity(indent.len() + toggled.len());
        text.push_str(indent);
        text.push_str(&toggled);

        Outcome::did(vec![Negai::Edit {
            buffer: buf.id(),
            edit: escriba_core::Edit {
                range: escriba_core::Range::new(
                    escriba_core::Position::new(line_no, 0),
                    escriba_core::Position::new(
                        line_no,
                        u32::try_from(line.chars().count()).unwrap_or(u32::MAX),
                    ),
                ),
                kind: escriba_core::EditKind::Replace { text },
            },
        }])
    }
}

struct Quit;
impl Native for Quit {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::Quit])
    }
}

/// `:noh` — the command that proves the seam, and it also reads nothing.
///
/// It lived as a hard-coded branch inside `EditorState::run_command`,
/// bypassing the registry entirely, because the old `EditContext` exposed
/// buffers and modal state and could not reach `SearchState`. It is now an
/// ordinary command asking for an ordinary slip, and it turns out not to
/// need a view at all — it does not READ the search, it asks to change it.
struct Noh;
impl Native for Noh {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::ClearSearchHighlight])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_core::BufferId;
    use escriba_madoguchi::{FakeBuffer, FakeSnapshot, Verdict};

    /// A snapshot holding one dirty, path-backed buffer.
    fn dirty_file() -> FakeSnapshot {
        let mut s = FakeSnapshot::default();
        s.buffers = vec![FakeBuffer::new(1, "dirty").at("/tmp/x.txt").dirty()];
        s.active = Some(BufferId(1));
        s
    }

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
        // Phase 0's first failure: a name nobody registered. Still an Err,
        // because the runtime tells a typo apart from an unbuilt capability.
        let r = CommandRegistry::new();
        let err = r.run("nope", &FakeSnapshot::default(), &[]).unwrap_err();
        assert!(matches!(err, CommandError::NotFound(_)));
    }

    #[test]
    fn a_command_asks_rather_than_acts() {
        // The whole point of the port. `write-all` used to reach into
        // `&mut BufferSet` and call `.save()`. It now RETURNS a request per
        // modified path-backed buffer and touches nothing — which is also
        // why it is best-effort by construction: the interpreter applies
        // each slip independently, so one permission error cannot abort the
        // rest.
        let mut r = CommandRegistry::new();
        r.register(Command::action(
            "w-all",
            "Write every modified buffer",
            "buffer.write-all",
        ));
        let out = r
            .run("w-all", &dirty_file(), &[])
            .expect("registered command dispatches");
        assert_eq!(
            out.slips,
            vec![Negai::Save {
                buffer: BufferId(1)
            }]
        );
        assert_eq!(out.verdict, Verdict::Did);
    }

    #[test]
    fn nothing_to_save_declines_rather_than_claiming_success() {
        // Three verdicts, not two. A scratch buffer has no path, so there is
        // genuinely nothing to write — and saying "Did" would be the same
        // silent lie Phase 0 removed.
        let mut r = CommandRegistry::new();
        r.register(Command::action("w-all", "Write all", "buffer.write-all"));
        let out = r
            .run("w-all", &FakeSnapshot::with_buffer("scratch"), &[])
            .expect("dispatches");
        assert!(out.slips.is_empty());
        assert_eq!(out.verdict, Verdict::Declined("no modified files".into()));
    }

    #[test]
    fn no_active_buffer_declines_rather_than_failing() {
        // Boot, and every `--no-defaults` run, reach commands with no
        // buffer. The operator did nothing wrong, so it is not an error.
        let mut r = CommandRegistry::new();
        r.register(Command::action("w", "Save", "buffer.save"));
        let out = r
            .run("w", &FakeSnapshot::default(), &[])
            .expect("dispatches");
        assert_eq!(out.verdict, Verdict::Declined("no active buffer".into()));
        assert!(out.slips.is_empty(), "a decline asks for nothing");
    }

    #[test]
    fn unknown_action_symbol_is_reported_not_silent() {
        // This test used to assert the DEFECT — it called `.expect()` on the
        // Ok, pinning `_ => Ok(())`, under which a dead keybinding and a
        // working one were indistinguishable at every layer.
        //
        // Inert is still correct: `picker.files` genuinely has not landed.
        // SILENT was never correct. It must be `Unhandled`, not `NotFound`:
        // the command IS registered, which is what made the silence
        // misleading in the first place.
        let mut r = CommandRegistry::new();
        r.register(Command::action("pick", "Pick a file", "picker.files"));
        let err = r
            .run("pick", &FakeSnapshot::default(), &[])
            .expect_err("an unimplemented action must report, not report success");
        assert!(
            matches!(&err, CommandError::Unhandled(s) if s == "picker.files"),
            "expected Unhandled(picker.files), got {err:?}",
        );
        assert!(r.contains("pick"), "the command survives its own failure");
    }

    #[test]
    fn action_naming_a_command_is_inert_not_recursive() {
        // `:action` takes action SYMBOLS, not command names: `run_action`
        // resolves dotted symbols and does NOT recurse into the registry.
        // Recursion would let a handler reach anything by naming it, which
        // is the ceiling madoguchi exists to remove.
        //
        // What changed with the port: the non-recursion is now REPORTED
        // rather than looking like a successful save.
        let mut r = CommandRegistry::new();
        r.register(Command::action("alias", "aliases save by name", "save"));
        let err = r
            .run("alias", &dirty_file(), &[])
            .expect_err("a command-name alias resolves nothing, and says so");
        assert!(
            matches!(&err, CommandError::Unhandled(s) if s == "save"),
            "expected Unhandled(save), got {err:?}",
        );
    }

    #[test]
    fn quit_is_a_request_not_a_flag_poke() {
        // Was `*ctx.quit_requested = true` — a command reaching into a
        // borrowed flag. Quit is now a request like any other, and the
        // interpreter decides, because the interpreter is the thing that
        // knows about unsaved buffers.
        let mut r = CommandRegistry::new();
        r.register(Command::action("bye", "Quit", "editor.quit"));
        let out = r
            .run("bye", &FakeSnapshot::default(), &[])
            .expect("dispatches");
        assert_eq!(out.slips, vec![Negai::Quit]);
    }

    #[test]
    fn buffer_info_speaks_through_a_slip_not_stderr() {
        // It used to `eprintln!`, which from a TUI holding the alternate
        // screen writes straight through the ratatui frame and corrupts it.
        // A command's only way to say anything is now Negai::Message.
        let mut r = CommandRegistry::new();
        r.register(Command::action("info", "Buffer info", "buffer.info"));
        let out = r.run("info", &dirty_file(), &[]).expect("dispatches");
        let Some(Negai::Message(m)) = out.slips.first() else {
            panic!("expected a Message slip, got {:?}", out.slips);
        };
        assert!(m.contains("buffer 1"), "{m}");
        assert!(m.contains("[modified]"), "{m}");
    }
}
