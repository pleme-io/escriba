//! `escriba-command` — command registry + palette.

extern crate self as escriba_command;

pub mod ex;

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
    /// An alias chain that never reaches a body.
    ///
    /// Aliases resolve THROUGH the registry, so `A -> B -> A` would spin
    /// forever. Bounded fuel turns that into a typed report naming the chain
    /// the operator wrote, instead of a hung editor.
    #[error("alias cycle resolving `{0}`")]
    AliasCycle(String),
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
        // ── the write/quit family ────────────────────────────────────────
        //
        // One registered command per DISTINCT BEHAVIOUR; the many spellings
        // an operator can type (`:x`, `:wqa`, `:quita`, …) are the ex
        // grammar's business, not the registry's. See [`ex::VERBS`].
        r.register(Command::native(
            "quit",
            "Exit the editor, unless a buffer is modified",
            erase::<QuitChecked<false>>(),
        ));
        r.register(Command::native(
            "quit!",
            "Exit the editor, discarding unsaved changes",
            erase::<Quit>(),
        ));
        r.register(Command::native(
            "quit-all",
            "Exit the editor, unless any buffer is modified",
            erase::<QuitChecked<true>>(),
        ));
        r.register(Command::native(
            "quit-all!",
            "Exit the editor, discarding every unsaved change",
            erase::<Quit>(),
        ));
        r.register(Command::native(
            "write-quit",
            "Write the active buffer, then exit",
            erase::<WriteQuit>(),
        ));
        r.register(Command::native(
            "write-quit-all",
            "Write every modified buffer, then exit",
            erase::<WriteQuitAll>(),
        ));
        r.register(Command::native(
            "exit-write",
            "Write the active buffer if modified, then exit",
            erase::<ExitWrite>(),
        ));
        r.register(Command::native(
            "buffer.write-all",
            "Write every modified buffer",
            erase::<WriteAll>(),
        ));
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
        r.register(Command::native(
            "picker.buffers",
            "Pick an open buffer",
            erase::<OpenPicker<false>>(),
        ));
        r.register(Command::native(
            "picker.commands",
            "Pick a command",
            erase::<OpenPicker<true>>(),
        ));
        r.register(Command::native(
            "picker.help",
            "Search every keybinding",
            erase::<HelpPicker>(),
        ));
        r.register(Command::native(
            "picker.grep",
            "Search the project for a pattern",
            erase::<GrepPicker>(),
        ));
        r.register(Command::native(
            "picker.files",
            "Pick a file under the working directory",
            erase::<WalkPicker<false>>(),
        ));
        r.register(Command::native(
            "picker.project",
            "Pick a project root",
            erase::<WalkPicker<true>>(),
        ));
        // ── the oil.nvim verbs ───────────────────────────────────────────
        // `files.open` IS the file picker — same capability, the name the
        // catalog binds. Registering it as its own native rather than an
        // alias keeps `--commands` honest about what each name does.
        r.register(Command::native(
            "files.open",
            "Browse files under the working directory",
            erase::<WalkPicker<false>>(),
        ));
        r.register(Command::native(
            "files.open-parent",
            "Browse files from the parent directory",
            erase::<ParentPicker>(),
        ));
        // ── the trouble.nvim verbs ───────────────────────────────────────
        r.register(Command::native(
            "trouble.toggle",
            "Show located findings",
            erase::<FindingsPicker<true>>(),
        ));
        r.register(Command::native(
            "trouble.workspace",
            "Show findings across the workspace",
            erase::<FindingsPicker<true>>(),
        ));
        r.register(Command::native(
            "trouble.document",
            "Show findings in this buffer",
            erase::<FindingsPicker<false>>(),
        ));
        r.register(Command::native(
            "window.split",
            "Split the window horizontally (:sp)",
            erase::<SplitWindow<true>>(),
        ));
        r.register(Command::native(
            "window.vsplit",
            "Split the window vertically (:vsp)",
            erase::<SplitWindow<false>>(),
        ));
        r.register(Command::native(
            "window.close",
            "Close the active window (:close)",
            erase::<CloseWindow>(),
        ));
        r.register(Command::native(
            "pane.left",
            "Focus the window to the left",
            erase::<FocusDir<-1, 0>>(),
        ));
        r.register(Command::native(
            "pane.right",
            "Focus the window to the right",
            erase::<FocusDir<1, 0>>(),
        ));
        r.register(Command::native(
            "pane.up",
            "Focus the window above",
            erase::<FocusDir<0, -1>>(),
        ));
        r.register(Command::native(
            "pane.down",
            "Focus the window below",
            erase::<FocusDir<0, 1>>(),
        ));
        r.register(Command::native(
            "conflict.next",
            "Go to the next merge conflict",
            erase::<ConflictWalk<true>>(),
        ));
        r.register(Command::native(
            "conflict.prev",
            "Go to the previous merge conflict",
            erase::<ConflictWalk<false>>(),
        ));
        r.register(Command::native(
            "conflict.choose-ours",
            "Resolve the conflict keeping ours",
            erase::<ChooseSide<0>>(),
        ));
        r.register(Command::native(
            "conflict.choose-theirs",
            "Resolve the conflict keeping theirs",
            erase::<ChooseSide<1>>(),
        ));
        r.register(Command::native(
            "conflict.choose-both",
            "Resolve the conflict keeping both",
            erase::<ChooseSide<2>>(),
        ));
        r.register(Command::native(
            "todo.next",
            "Go to the next TODO/FIXME marker",
            erase::<TodoWalk<true>>(),
        ));
        r.register(Command::native(
            "todo.prev",
            "Go to the previous TODO/FIXME marker",
            erase::<TodoWalk<false>>(),
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
        self.run_bounded(name, snap, args, ALIAS_FUEL)
    }

    /// One resolution path, bounded.
    ///
    /// An action symbol resolves against the BUILT-IN table first, then
    /// against the registry itself. That second step is the whole point: a
    /// `(defcmd :name "CommentToggle" :action "comment.toggle-line")` alias
    /// used to die as `Unhandled` even though `comment.toggle-line` was a
    /// registered native sitting in the same map — two dispatch tables that
    /// had to agree, and did not. Every one of the 41 catalog aliases was
    /// dead for exactly this reason.
    ///
    /// Resolving through the registry admits cycles, so the chain runs on
    /// fuel and exhaustion is a typed `AliasCycle`.
    fn run_bounded(
        &self,
        name: &str,
        snap: &dyn Snapshot,
        args: &[String],
        fuel: u8,
    ) -> Result<Outcome> {
        let Some(fuel) = fuel.checked_sub(1) else {
            return Err(CommandError::AliasCycle(name.to_string()));
        };
        let cmd = self
            .commands
            .get(name)
            .ok_or_else(|| CommandError::NotFound(name.to_string()))?;
        match &cmd.handler {
            Handler::Native(f) => Ok(f(snap, args)),
            Handler::Action(sym) => match builtin_action(sym) {
                Some(f) => Ok(f(snap, args)),
                // Not a built-in — but the symbol may name a registered
                // command. `sym != name` keeps a self-referential alias from
                // burning the whole budget before reporting.
                None if sym != name && self.commands.contains_key(sym.as_str()) => {
                    self.run_bounded(sym, snap, args, fuel)
                }
                None => Err(CommandError::Unhandled(sym.to_string())),
            },
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

/// How many alias hops a chain may take before it is called a cycle.
///
/// Small on purpose: a legitimate chain is an alias naming a native, which
/// is two hops. Anything deeper is a configuration mistake worth reporting.
const ALIAS_FUEL: u8 = 8;

/// The dotted action symbols escriba implements natively.
///
/// Returns `None` for anything else so the caller can try the registry —
/// this used to return `Err(Unhandled)` directly, which is what made the
/// built-in table the ONLY table and killed every catalog alias.
fn builtin_action(sym: &str) -> Option<CommandFn> {
    Some(match sym {
        "buffer.save" | "buffer.write" => erase::<Save>(),
        "buffer.write-all" => erase::<WriteAll>(),
        "buffer.undo" => erase::<Undo>(),
        "buffer.redo" => erase::<Redo>(),
        "buffer.info" => erase::<Info>(),
        "editor.quit" => erase::<Quit>(),
        "search.clear-highlight" => erase::<Noh>(),
        // The not-yet-implemented namespace. The shipped keybindings that
        // land here are enumerated by `escriba/tests/action_resolution.rs`,
        // which asserts SET EQUALITY — so the count is READ from there rather
        // than restated. It said 85 while the real figure had ratcheted to
        // 78; a duplicated number is a number that rots.
        // Inert and ANNOUNCED; see CommandError::Unhandled.
        _ => return None,
    })
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
/// `picker.buffers` / `picker.commands` — open a picker over a source.
///
/// Reads `caps!()`: the handler does not build the item list. It cannot —
/// a picker needs `&mut` across many keypresses and a handler holds a
/// read-only `Snapshot`. So the slip NAMES the source and the interpreter
/// populates it, which keeps the one-writer seam intact while the widget
/// still gets the mutable state it needs.
struct OpenPicker<const COMMANDS: bool>;
impl<const COMMANDS: bool> Native for OpenPicker<COMMANDS> {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::OpenPicker(if COMMANDS {
            escriba_madoguchi::PickerSource::Commands
        } else {
            escriba_madoguchi::PickerSource::Buffers
        })])
    }
}

/// `picker.help` — the searchable keymap.
struct HelpPicker;
impl Native for HelpPicker {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::OpenPicker(
            escriba_madoguchi::PickerSource::Help,
        )])
    }
}

/// `picker.grep` — matches for a pattern across the project.
///
/// The pattern is the command's ARGUMENT (`:picker.grep fn main`). Declining
/// with no argument rather than opening an empty picker: an overlay with
/// nothing in it and no way to say why is worse than a message.
struct GrepPicker;
impl Native for GrepPicker {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, args: &[String]) -> Outcome {
        let pattern = args.join(" ");
        if pattern.is_empty() {
            return Outcome::declined("grep: give a pattern — `:picker.grep <pattern>`");
        }
        Outcome::did(vec![Negai::GrepProject { pattern }])
    }
}

/// `picker.files` / `picker.project` — the bounded-walk sources.
struct WalkPicker<const PROJECT: bool>;
impl<const PROJECT: bool> Native for WalkPicker<PROJECT> {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::OpenPicker(if PROJECT {
            escriba_madoguchi::PickerSource::Project
        } else {
            escriba_madoguchi::PickerSource::Files
        })])
    }
}

/// `files.open-parent` — the same bounded walk, rooted one level up.
///
/// oil.nvim's `-`: browse from where the current file lives, then upward.
/// The root is resolved from the working directory rather than the buffer
/// because a scratch buffer has no directory, and "browse from nowhere" has
/// no sensible answer — falling back to `.` is the honest one.
struct ParentPicker;
impl Native for ParentPicker {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        let root = std::env::current_dir()
            .ok()
            .and_then(|d| d.parent().map(std::path::Path::to_path_buf))
            .unwrap_or_else(|| std::path::PathBuf::from(".."));
        Outcome::did(vec![Negai::OpenPicker(
            escriba_madoguchi::PickerSource::FilesUnder(root),
        )])
    }
}

/// `trouble.*` — a view over the result registry.
///
/// `WORKSPACE` is the whole difference between `trouble.document` and
/// `trouble.workspace`; `trouble.toggle` is the workspace view because that
/// is what an operator means by "show me the problems".
struct FindingsPicker<const WORKSPACE: bool>;
impl<const WORKSPACE: bool> Native for FindingsPicker<WORKSPACE> {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::OpenPicker(
            escriba_madoguchi::PickerSource::Findings {
                workspace: WORKSPACE,
            },
        )])
    }
}

/// `:sp` / `:vsp` — split the active window.
///
/// Reads `caps!()`: splitting is layout, not buffer content. The handler
/// names the axis and the interpreter does it, which is the same shape the
/// picker uses and for the same reason — a handler holds a read-only
/// `Snapshot`.
struct SplitWindow<const STACKED: bool>;
impl<const STACKED: bool> Native for SplitWindow<STACKED> {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::SplitWindow { stacked: STACKED }])
    }
}

/// `:close` / `<C-w>c`.
struct CloseWindow;
impl Native for CloseWindow {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::CloseWindow])
    }
}

/// `pane.{left,right,up,down}` — `<C-w>hjkl`.
///
/// The direction is two const params rather than an enum so one impl covers
/// all four without a runtime match, and so a wrong direction is a wrong
/// TYPE at the registration site rather than a wrong argument.
struct FocusDir<const DX: i8, const DY: i8>;
impl<const DX: i8, const DY: i8> Native for FocusDir<DX, DY> {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::FocusDir { dx: DX, dy: DY }])
    }
}

/// `]x` / `[x` — walk merge conflicts.
///
/// Identical in shape to `TodoWalk`, and that is the claim being tested: a
/// conflict is a located finding, so navigating one is the SAME walk. If
/// this needed anything TodoWalk did not, the shirube model would be wrong.
struct ConflictWalk<const FORWARD: bool>;
impl<const FORWARD: bool> Native for ConflictWalk<FORWARD> {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        let b = v.buffers();
        let Some(buf) = b.active() else {
            return Outcome::declined("no active buffer");
        };
        let findings = escriba_shirube::conflict::findings(buf.id(), &buf.text());
        if findings.is_empty() {
            return Outcome::declined("no merge conflicts in this buffer");
        }
        Outcome::did(vec![
            Negai::PublishFindings {
                list: "conflict".to_string(),
                findings,
            },
            Negai::WalkList {
                list: "conflict".to_string(),
                forward: FORWARD,
            },
        ])
    }
}

/// `conflict.choose-{ours,theirs,both}` — resolve the conflict at the cursor.
///
/// Reads `caps!(Buffers, Cursor)`: it needs the text to find the region and
/// the cursor to know WHICH region. The edit it emits replaces whole lines,
/// because there is no such thing as keeping half of "ours".
struct ChooseSide<const SIDE: u8>;
impl<const SIDE: u8> Native for ChooseSide<SIDE> {
    type Reads = caps!(Buffers, Cursor);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        use escriba_shirube::conflict::{Side, at, resolution};
        let b = v.buffers();
        let Some(buf) = b.active() else {
            return Outcome::declined("no active buffer");
        };
        let text = buf.text();
        let line = v.cursor().position().line;
        let Some(c) = at(&text, line) else {
            // Declined, not failed: standing outside a conflict is an
            // ordinary place to be, and vim says nothing there either.
            return Outcome::declined("not inside a merge conflict");
        };
        let side = match SIDE {
            0 => Side::Ours,
            1 => Side::Theirs,
            _ => Side::Both,
        };
        let (from, to) = c.lines();
        Outcome::did(vec![
            Negai::Edit {
                buffer: buf.id(),
                edit: escriba_core::Edit {
                    range: escriba_core::Range::new(
                        escriba_core::Position::new(from, 0),
                        escriba_core::Position::new(to, 0),
                    ),
                    kind: escriba_core::EditKind::Replace {
                        text: resolution(&text, c, side),
                    },
                },
            },
            // Land on the resolved text rather than wherever the deleted
            // markers left the cursor.
            Negai::SetCursor {
                buffer: buf.id(),
                to: escriba_core::Position::new(from, 0),
            },
        ])
    }
}

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

/// `todo.next` / `todo.prev` — walk the marker list.
///
/// Scans on every invocation rather than relying on a cached list. The scan
/// is pure text and costs nothing at keyboard cadence, and re-scanning means
/// the list is always fresh — the freshness machinery then guards the window
/// BETWEEN a publish and a walk, which is where a stale list would otherwise
/// slip through.
///
/// This is also the shape every later producer takes: the command COMPUTES
/// (it has the text through `Buffers`) and asks the interpreter to publish.
/// Nothing here touches the registry.
struct TodoWalk<const FORWARD: bool>;
impl<const FORWARD: bool> Native for TodoWalk<FORWARD> {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        let b = v.buffers();
        let Some(buf) = b.active() else {
            return Outcome::declined("no active buffer");
        };
        let findings = escriba_shirube::scan_markers(buf.id(), &buf.text());
        if findings.is_empty() {
            return Outcome::declined("no TODO markers in this buffer");
        }
        Outcome::did(vec![
            Negai::PublishFindings {
                list: "todo".to_string(),
                findings,
            },
            Negai::WalkList {
                list: "todo".to_string(),
                forward: FORWARD,
            },
        ])
    }
}

/// `:q!` / `:qa!` — leave, whatever the buffers say.
///
/// Reads nothing, on purpose. The force form's whole meaning is "do not
/// consult the thing that would stop you", and a handler that cannot see the
/// modified flag cannot be talked into honouring it.
struct Quit;
impl Native for Quit {
    type Reads = caps!();
    fn run(_v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        Outcome::did(vec![Negai::Quit])
    }
}

/// vim's E37, worded as vim words it — the `(add ! to override)` tail is the
/// half that matters, because it names the key that gets the operator out.
fn no_write_since_last_change(what: &str) -> Outcome {
    let mut m = String::with_capacity(64);
    m.push_str("E37: No write since last change in ");
    m.push_str(what);
    m.push_str(" (add ! to override)");
    Outcome::declined(m)
}

/// How a buffer is named in a message: its path, or `[No Name]` as vim calls
/// an unsaved scratch.
fn buffer_label(b: &dyn BufferView) -> String {
    b.path()
        .map_or_else(|| "[No Name]".to_string(), |p| p.display().to_string())
}

/// `:q` — leave, unless that would drop an unsaved change.
///
/// The refusal is what makes the bang mean something. Without it `:q` and
/// `:q!` are the same key sequence with different lengths, and the editor
/// discards an afternoon's typing without a word — which is the one failure
/// mode a modal editor's users are trained to expect protection from.
struct QuitChecked<const ALL: bool>;
impl<const ALL: bool> Native for QuitChecked<ALL> {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        let b = v.buffers();
        // `:q` asks about the buffer in front of the operator; `:qa` asks
        // about every one, because it is the whole editor it is closing.
        let blocker = if ALL {
            b.ids()
                .into_iter()
                .filter_map(|id| b.get(id))
                .find(|x| x.is_modified())
                .map(buffer_label)
        } else {
            b.active().filter(|x| x.is_modified()).map(buffer_label)
        };
        match blocker {
            Some(what) => no_write_since_last_change(&what),
            None => Outcome::did(vec![Negai::Quit]),
        }
    }
}

/// `:wq` — write the active buffer, then leave.
///
/// Writes unconditionally, as vim does: `:wq` is how you touch a file's mtime
/// on purpose. `:x` is the other one. A buffer with no path cannot be written
/// anywhere, so it declines rather than quitting and silently losing the
/// text — `:wq` on a scratch buffer promised a write.
struct WriteQuit;
impl Native for WriteQuit {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        let b = v.buffers();
        let Some(buf) = b.active() else {
            return Outcome::declined("no active buffer");
        };
        if buf.path().is_none() {
            return Outcome::declined("E32: No file name");
        }
        Outcome::did(vec![Negai::Save { buffer: buf.id() }, Negai::Quit])
    }
}

/// `:x` / `:exit` — write only if modified, then leave.
///
/// The difference from `:wq` is one `is_modified` and it is not cosmetic: a
/// pointless write bumps the mtime, and something is always watching the
/// mtime.
struct ExitWrite;
impl Native for ExitWrite {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        let b = v.buffers();
        let Some(buf) = b.active() else {
            return Outcome::declined("no active buffer");
        };
        if !buf.is_modified() {
            return Outcome::did(vec![Negai::Quit]);
        }
        if buf.path().is_none() {
            return Outcome::declined("E32: No file name");
        }
        Outcome::did(vec![Negai::Save { buffer: buf.id() }, Negai::Quit])
    }
}

/// `:wqa` / `:xa` — write every modified, path-backed buffer, then leave.
///
/// Shares [`WriteAll`]'s rule about scratch buffers rather than restating it:
/// one slip per writable buffer, and a buffer with nowhere to go is skipped.
/// It does NOT decline on an unwritable buffer the way `:wq` does — `:wqa` is
/// a request to leave, and refusing the whole thing over one scratch buffer
/// would leave the operator with no way out but `:qa!`, which discards the
/// buffers `:wqa` was about to save.
struct WriteQuitAll;
impl Native for WriteQuitAll {
    type Reads = caps!(Buffers);
    fn run(v: &View<'_, Self::Reads>, _: &[String]) -> Outcome {
        let b = v.buffers();
        let mut slips: Vec<Negai> = b
            .ids()
            .into_iter()
            .filter(|id| {
                b.get(*id)
                    .is_some_and(|x| x.is_modified() && x.path().is_some())
            })
            .map(|buffer| Negai::Save { buffer })
            .collect();
        slips.push(Negai::Quit);
        Outcome::did(slips)
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
