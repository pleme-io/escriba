//! Live-apply layer — resolve an [`ApplyPlan`] into runtime editor
//! state.
//!
//! Parsing the rc into a plan is the decoupled first pass; this
//! module is the second pass that actually mutates a [`Keymap`], the
//! [`CommandRegistry`], and the editor option store (more sinks land
//! per wave).
//!
//! Kept separate from the parse layer so tests can build a plan in
//! memory without touching live runtime types.
//!
//! # Key-string grammar
//!
//! Mirrors the zsh / vim / helix convention users already know. Single
//! keys and multi-key SEQUENCES (`<leader>ff`, `gg`, `<C-w>h`) share
//! one tokenizer ([`parse_key_sequence`]); `<leader>` / `<localleader>`
//! resolve to the keymap's configured leader.
//!
//! | Input         | Resolves to         |
//! |---------------|---------------------|
//! | `"a"`         | `Key::Char('a')`    |
//! | `"<Esc>"`     | `Key::Esc`          |
//! | `"<CR>"` / `"<Enter>"` | `Key::Enter` |
//! | `"<Tab>"`     | `Key::Tab`          |
//! | `"<BS>"` / `"<Backspace>"` | `Key::Backspace` |
//! | `"<Left>"` / `"<Right>"` / `"<Up>"` / `"<Down>"` | directional |
//! | `"<Home>"` / `"<End>"` | `Key::Home` / `Key::End` |
//! | `"<PageUp>"` / `"<PageDown>"` | paging |
//! | `"<C-r>"`     | `Key::Ctrl('r')`    |
//! | `"<A-f>"` / `"<M-f>"` | `Key::Alt('f')` |
//! | `"<leader>ff"` | sequence `[leader, f, f]` |
//!
//! # Action-string grammar
//!
//! A curated set of well-known action names resolves directly into
//! [`Action`] variants (so the rc can bind `"move-left"` without
//! caring about the enum shape). Unknown action names fall back to
//! [`Action::Command`] — the command dispatcher resolves them at
//! runtime against the command registry.

use escriba_command::{Command, CommandRegistry};
use escriba_core::{Action, CommentString, Filetype, FiletypeTable, Mode, Motion, SearchDirection};
use escriba_keymap::{Key, Keymap};
use escriba_ui::splash::{Splash, SplashEntry};

use crate::{ApplyPlan, KeybindSpec, LispError, LispResult};

/// Report of how many grammar extensions the apply pass registered
/// and how many were skipped because the grammar wasn't known.
#[derive(Debug, Clone, Default)]
pub struct GrammarApplyReport {
    /// Count of (language, ext) pairs successfully registered.
    pub extensions_registered: u32,
    /// Count of pairs skipped because the language wasn't known.
    pub extensions_skipped_unknown_language: u32,
    /// Languages whose `defmode` asked for registration but isn't
    /// yet registered in `escriba-ts`. Informational — the apply
    /// never fails; unknown grammars degrade to plain-text display.
    pub unknown_languages: Vec<String>,
}

/// Wire every `(defmode :name X :tree-sitter Y :extensions (…))`
/// declaration into a grammar registry. The registry is
/// `escriba-ts::GrammarRegistry`, but this function stays decoupled
/// via two callbacks — so `escriba-lisp` doesn't need to depend on
/// the tree-sitter runtime crate.
///
/// - `is_known_grammar(lang)` — does the registry have a `Grammar`
///   for this name? Typically `|n| registry.get(n).is_some()`.
/// - `add_extension(lang, ext)` — register the extension. Typically
///   `|l, e| { registry.add_extension(l, e); }`.
///
/// Returns a [`GrammarApplyReport`] so callers can surface the
/// result in `--list-rc` output.
pub fn apply_plan_to_grammar_extensions<F, G>(
    plan: &ApplyPlan,
    mut is_known_grammar: F,
    mut add_extension: G,
) -> GrammarApplyReport
where
    F: FnMut(&str) -> bool,
    G: FnMut(&str, &str),
{
    let mut report = GrammarApplyReport::default();
    for mode in &plan.major_modes {
        if mode.tree_sitter.is_empty() {
            continue;
        }
        if !is_known_grammar(&mode.tree_sitter) {
            report.extensions_skipped_unknown_language += mode.extensions.len() as u32;
            if !report
                .unknown_languages
                .iter()
                .any(|l| l == &mode.tree_sitter)
            {
                report.unknown_languages.push(mode.tree_sitter.clone());
            }
            continue;
        }
        for ext in &mode.extensions {
            add_extension(&mode.tree_sitter, ext);
            report.extensions_registered += 1;
        }
    }
    report
}

/// Summary of what an `apply_plan_to_*` pass did. Shaped like frost
/// doctor's report — "applied vs. skipped (and why)" — so the
/// escriba binary can surface it in `--list-rc` output.
#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    /// Count of keybindings successfully written.
    pub keybinds_applied: u32,
    /// Keybindings whose action string didn't match a known action
    /// — these fall through to [`Action::Command`] and will resolve
    /// at dispatch time (not an error, but worth surfacing).
    pub keybinds_deferred_to_commands: u32,
    /// Multi-key SEQUENCE bindings applied (`gh`, `<leader>ff`, …).
    /// These are bound into the keymap's sequence table and resolved
    /// by the runtime's pending-stroke loop — a subset of
    /// `keybinds_applied`, surfaced separately so the doctor can show
    /// how much of the leader-driven surface is live.
    pub keybinds_sequences: u32,
    /// Human-readable warnings for anything we had to skip entirely
    /// (unknown keys, malformed single-key sequences). Never a
    /// fatal error — partial application is preferable to a broken
    /// editor on one typo.
    pub warnings: Vec<String>,
}

impl ApplyReport {
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "keybinds={} deferred_cmd={} sequences={} warnings={}",
            self.keybinds_applied,
            self.keybinds_deferred_to_commands,
            self.keybinds_sequences,
            self.warnings.len(),
        )
    }
}

/// Apply every [`KeybindSpec`] in `plan` to `keymap`. Multi-key
/// sequences like `"gh"` or `"<leader>ff"` bind into the keymap's
/// sequence table (resolved by the runtime's pending-stroke loop) and
/// are tallied in `keybinds_sequences`; single keys bind into the flat
/// table. Truly malformed bindings (unknown mode, unterminated `<…>`,
/// unknown key token) surface as individual warnings — partial apply
/// beats a dead editor on one typo.
pub fn apply_plan_to_keymap(plan: &ApplyPlan, keymap: &mut Keymap) -> ApplyReport {
    let mut report = ApplyReport::default();
    for spec in &plan.keybinds {
        match apply_keybind(spec, keymap) {
            Ok(applied) => {
                report.keybinds_applied += 1;
                if applied.deferred_to_command {
                    report.keybinds_deferred_to_commands += 1;
                }
                if applied.is_sequence {
                    report.keybinds_sequences += 1;
                }
            }
            Err(warning) => {
                report.warnings.push(warning);
            }
        }
    }
    report
}

/// How a successfully-applied keybind was classified. `is_sequence`
/// is true for multi-key bindings (`gh`, `<leader>ff`);
/// `deferred_to_command` is true when the action wasn't a curated
/// typed action and will resolve against the command registry at
/// dispatch. The two are independent — a leader binding to a picker
/// command is both.
struct KeybindApplied {
    is_sequence: bool,
    deferred_to_command: bool,
}

/// Apply a single keybind. Resolves the key string into a full key
/// SEQUENCE (`<leader>ff` → three keys, `h` → one) and binds it —
/// single keys into the flat table, multi-key sequences into the
/// sequence table the runtime's pending-stroke loop resolves. Returns
/// a classification, or `Err(warning)` when the spec is malformed
/// (unknown mode, unterminated `<`, unknown key token).
fn apply_keybind(spec: &KeybindSpec, keymap: &mut Keymap) -> Result<KeybindApplied, String> {
    let mode =
        parse_mode(&spec.mode).map_err(|e| format!("defkeybind :mode {:?} — {e}", spec.mode))?;
    let leader = keymap.leader().clone();
    let keys = parse_key_sequence(&spec.key, &leader)
        .map_err(|e| format!("defkeybind :key {:?} — {e}", spec.key))?;
    let (action, deferred) = resolve_action(&spec.action);
    let description = if spec.description.is_empty() {
        spec.action.clone()
    } else {
        spec.description.clone()
    };
    let is_sequence = keys.len() > 1;
    keymap.bind_sequence(mode, keys, action, description);
    Ok(KeybindApplied {
        is_sequence,
        deferred_to_command: deferred,
    })
}

/// Tokenize a key string into `<…>` and bare-char tokens, parsing
/// each into a [`Key`]. `<leader>ff` → `[leader, f, f]`; `gg` →
/// `[g, g]`; `<C-w>h` → `[Ctrl('w'), h]`. `<leader>`/`<localleader>`
/// resolve to `leader`. Errors on an unterminated `<…>`, an unknown
/// token, or an empty string.
fn parse_key_sequence(s: &str, leader: &Key) -> Result<Vec<Key>, String> {
    let mut keys = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        let token: String = if c == '<' {
            let mut t = String::new();
            for ch in chars.by_ref() {
                t.push(ch);
                if ch == '>' {
                    break;
                }
            }
            if !t.ends_with('>') {
                return Err(format!("unterminated `<` in key `{s}`"));
            }
            t
        } else {
            chars.next();
            c.to_string()
        };
        let key = parse_key_token(&token, leader)
            .ok_or_else(|| format!("unknown key token `{token}` in `{s}`"))?;
        keys.push(key);
    }
    if keys.is_empty() {
        return Err(format!("empty key string `{s}`"));
    }
    Ok(keys)
}

/// Parse a SINGLE key token — a bare char or a `<…>` form — resolving
/// `<leader>`/`<localleader>` against `leader`. Returns `None` for an
/// unknown token so [`parse_key_sequence`] can surface the offending
/// piece.
fn parse_key_token(tok: &str, leader: &Key) -> Option<Key> {
    if let Some(inner) = tok.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
        let lower = inner.to_ascii_lowercase();
        if lower == "leader" || lower == "localleader" {
            return Some(leader.clone());
        }
        return parse_bracket_key(inner);
    }
    let mut chars = tok.chars();
    let c = chars.next()?;
    if chars.next().is_none() {
        Some(Key::Char(c))
    } else {
        None
    }
}

/// Parse a leader-key VALUE — the `:value` of
/// `(defoption :name "mapleader" :value …)` — into a [`Key`]. Accepts a
/// bracket token (`<space>`, `<Tab>`, `<C-x>`) or a bare char (`,`,
/// `\`). Unlike a keybind token it never resolves `<leader>` (a
/// leader-of-a-leader is nonsense). Returns `None` if the value isn't a
/// single key, so the caller can keep the default leader and warn.
///
/// Public because the binary reads the `mapleader` option from the
/// applied option store and calls [`Keymap::set_leader`] BEFORE
/// [`apply_plan_to_keymap`] — `<leader>` is resolved at bind time, so
/// the leader must be set first.
#[must_use]
pub fn parse_leader_key(value: &str) -> Option<Key> {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower == "<leader>" || lower == "<localleader>" {
        return None;
    }
    // The dummy leader is irrelevant here — parse_key_token only
    // consults it for the `<leader>`/`<localleader>` tokens we just
    // rejected.
    parse_key_token(trimmed, &Key::Char(','))
}

/// Parse a mode name from [`KeybindSpec`].
fn parse_mode(name: &str) -> LispResult<Mode> {
    match name {
        "normal" => Ok(Mode::Normal),
        "insert" => Ok(Mode::Insert),
        "visual" => Ok(Mode::Visual),
        "visual-line" => Ok(Mode::VisualLine),
        "command" => Ok(Mode::Command),
        _ => Err(LispError::UnknownMode(name.to_string())),
    }
}

fn parse_bracket_key(inner: &str) -> Option<Key> {
    // Named keys (case-insensitive for the common ones).
    let lower = inner.to_ascii_lowercase();
    if let Some(k) = named_key(&lower) {
        return Some(k);
    }

    // `<F1>`..`<F20>`. Declarable before this and never delivered, because
    // the input layer discarded `KeyCode::F(_)` at the door.
    if let Some(n) = lower.strip_prefix('f').and_then(|d| d.parse::<u8>().ok()) {
        if (1..=20).contains(&n) {
            return Some(Key::F(n));
        }
    }

    // Modifier+char form: `C-r`, `A-f`, `M-f`, `S-h`. Dash-separated.
    // The operand is a single char OR a named key that denotes a
    // printable char (`<C-Space>` -> Ctrl(' ')) — see `modifier_operand`.
    // Each shorthand DECLINES rather than returning `None` outright, so a
    // form it cannot express (`<C-S-p>` — two modifiers) falls through to
    // `rich_chord` instead of dying here. Returning early was why multi-
    // modifier chords parsed as nothing at all.
    if let Some(rest) = inner
        .strip_prefix("C-")
        .or_else(|| inner.strip_prefix("c-"))
    {
        if let Some(c) = modifier_operand(rest) {
            return Some(Key::Ctrl(c));
        }
    }
    if let Some(rest) = inner
        .strip_prefix("A-")
        .or_else(|| inner.strip_prefix("a-"))
        .or_else(|| inner.strip_prefix("M-"))
        .or_else(|| inner.strip_prefix("m-"))
    {
        if let Some(c) = modifier_operand(rest) {
            return Some(Key::Alt(c));
        }
    }
    // Shift has no `Key` variant of its own, and needs none: the input
    // layer already delivers a shifted letter as its uppercase char, so
    // `<S-h>` IS `H` (the vim convention this catalog is written in).
    // Modelling it as a distinct modifier would make `<S-h>` and a typed
    // `H` two different bindings for one physical keystroke.
    if let Some(rest) = inner
        .strip_prefix("S-")
        .or_else(|| inner.strip_prefix("s-"))
    {
        if let Some(c) = modifier_operand(rest) {
            return Some(Key::Char(c.to_ascii_uppercase()));
        }
    }

    // Anything the single-modifier shorthands cannot say: two or more
    // modifiers (`<C-S-p>`), or `Super`/`Cmd` (`<D-p>`). These were simply
    // unparseable — an author could write them and get silence.
    rich_chord(inner)
}

/// A chord with more than one modifier, or with `Super`/`Cmd`.
///
/// Reached only after the single-modifier forms decline, so it never changes
/// how `<C-r>` or `<S-h>` parse. Builds `awase::Hotkey` directly — escriba
/// widens by consuming the fleet vocabulary rather than growing a parallel
/// spelling of it.
fn rich_chord(inner: &str) -> Option<Key> {
    use awase::Modifiers as M;
    let mut mods = M::NONE;
    let mut rest = inner;
    // Greedy: strip every modifier prefix present, in any order.
    loop {
        let lower = rest.to_ascii_lowercase();
        let (bit, len) = if lower.starts_with("c-") {
            (M::CTRL, 2)
        } else if lower.starts_with("a-") || lower.starts_with("m-") {
            (M::ALT, 2)
        } else if lower.starts_with("s-") {
            (M::SHIFT, 2)
        } else if lower.starts_with("d-") {
            (M::CMD, 2)
        } else if lower.starts_with("cmd-") {
            (M::CMD, 4)
        } else if lower.starts_with("super-") {
            (M::CMD, 6)
        } else {
            break;
        };
        mods = mods | bit;
        rest = &rest[len..];
    }
    if mods == M::NONE {
        return None;
    }
    let key = awase::Key::from_name(&rest.to_ascii_lowercase())?;
    Some(Key::Chord(awase::Hotkey::new(mods, key)))
}

/// The named-key table, shared by the bare `<Esc>` form and by the
/// modifier forms via [`modifier_operand`].
fn named_key(lower: &str) -> Option<Key> {
    let named: &[(&str, Key)] = &[
        ("esc", Key::Esc),
        ("escape", Key::Esc),
        ("cr", Key::Enter),
        ("enter", Key::Enter),
        ("return", Key::Enter),
        ("tab", Key::Tab),
        ("bs", Key::Backspace),
        ("backspace", Key::Backspace),
        ("left", Key::Left),
        ("right", Key::Right),
        ("up", Key::Up),
        ("down", Key::Down),
        ("home", Key::Home),
        ("end", Key::End),
        ("pageup", Key::PageUp),
        ("pagedown", Key::PageDown),
        // Space is a first-class leader/binding token in the nvim
        // ecosystem (`<space>` / `<Space>` / `<spc>`). The input layer
        // produces `Key::Char(' ')` for the spacebar, so the keymap
        // side must resolve the same.
        ("space", Key::Char(' ')),
        ("spc", Key::Char(' ')),
    ];
    for (n, k) in named {
        if lower == *n {
            return Some(k.clone());
        }
    }
    None
}

/// Resolve the char a modifier applies to. Accepts a bare single char
/// (`C-r` -> 'r') or a named key that denotes a printable char
/// (`C-Space` -> ' '), which is what made `<C-Space>` — shipped in
/// escriba's own `escriba-cmp` catalog plugin — fail to parse: the
/// operand was required to be exactly one char, so the 5-char `Space`
/// was rejected and the binding was dropped with a startup warning.
/// Named keys with no printable char (`<C-Esc>`) stay unrepresentable,
/// since `Key::Ctrl` holds a `char`.
fn modifier_operand(s: &str) -> Option<char> {
    if let Some(c) = single_char(s) {
        return Some(c);
    }
    match named_key(&s.to_ascii_lowercase()) {
        Some(Key::Char(c)) => Some(c),
        _ => None,
    }
}

fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let c = chars.next()?;
    if chars.next().is_none() {
        Some(c)
    } else {
        None
    }
}

/// Resolve an action string into an [`Action`]. Known strings map to
/// typed variants; everything else becomes a [`Action::Command`]
/// handoff (the registry resolves it at dispatch time).
///
/// Returns `(action, deferred)` — `deferred` marks the command-registry
/// fallback, so a caller can report "this will resolve later" rather
/// than pretending it resolved now.
///
/// Public because `:action` is not a keybind-only field: `defsplash`
/// entries carry one too, and TWO resolvers would mean `"quit"` could
/// come to mean two things.
#[must_use]
pub fn resolve_action(name: &str) -> (Action, bool) {
    // Keep the table alphabetised per family to make additions obvious.
    match name {
        // ── Mode transitions ───────────────────────────────────────
        "insert" | "enter-insert" => (Action::ChangeMode(Mode::Insert), false),
        "normal" | "enter-normal" => (Action::ChangeMode(Mode::Normal), false),
        "visual" | "enter-visual" => (Action::ChangeMode(Mode::Visual), false),
        "visual-line" | "enter-visual-line" => (Action::ChangeMode(Mode::VisualLine), false),
        "command" | "enter-command" => (Action::ChangeMode(Mode::Command), false),

        // ── Text motions ───────────────────────────────────────────
        "move-left" => (Action::Move(Motion::Left), false),
        "move-right" => (Action::Move(Motion::Right), false),
        "move-up" => (Action::Move(Motion::Up), false),
        "move-down" => (Action::Move(Motion::Down), false),
        "word-next-start" => (Action::Move(Motion::WordStartNext), false),
        "word-next-end" => (Action::Move(Motion::WordEndNext), false),
        "word-prev-start" => (Action::Move(Motion::WordStartPrev), false),
        "line-start" => (Action::Move(Motion::LineStart), false),
        "line-first-non-blank" => (Action::Move(Motion::LineFirstNonBlank), false),
        "line-end" => (Action::Move(Motion::LineEnd), false),
        "doc-start" => (Action::Move(Motion::DocStart), false),
        "doc-end" => (Action::Move(Motion::DocEnd), false),
        "page-up" => (Action::Move(Motion::PageUp), false),
        "page-down" => (Action::Move(Motion::PageDown), false),
        "half-page-up" => (Action::Move(Motion::HalfPageUp), false),
        "half-page-down" => (Action::Move(Motion::HalfPageDown), false),

        // ── Structural Lisp motions (paredit) ──────────────────────
        "forward-sexp" => (Action::Move(Motion::ForwardSexp), false),
        "backward-sexp" => (Action::Move(Motion::BackwardSexp), false),
        "up-list" => (Action::Move(Motion::UpList), false),
        "down-list" => (Action::Move(Motion::DownList), false),
        "beginning-of-defun" => (Action::Move(Motion::BeginningOfDefun), false),
        "end-of-defun" => (Action::Move(Motion::EndOfDefun), false),
        "beginning-of-sexp" => (Action::Move(Motion::BeginningOfSexp), false),
        "end-of-sexp" => (Action::Move(Motion::EndOfSexp), false),

        // ── Search ─────────────────────────────────────────────────
        "search-forward" => (Action::SearchOpen(SearchDirection::Forward), false),
        "search-backward" => (Action::SearchOpen(SearchDirection::Backward), false),
        "search-next" => (Action::SearchRepeat { reverse: false }, false),
        "search-prev" => (Action::SearchRepeat { reverse: true }, false),

        // ── Erasing ────────────────────────────────────────────────
        // One action per direction, not one per target: the runtime routes
        // each to the search prompt, the ex line, or the buffer by which is
        // open. An rc that binds `<C-h>` to "backspace" therefore gets the
        // right thing in all three without saying which.
        "backspace" => (Action::Backspace, false),
        "delete-forward" => (Action::DeleteForward, false),
        "delete-word-before" => (Action::DeleteWordBefore, false),
        "delete-to-line-start" => (Action::DeleteToLineStart, false),

        // ── Editor-wide actions ────────────────────────────────────
        "undo" => (Action::Undo, false),
        "redo" => (Action::Redo, false),
        "save" => (Action::Save, false),
        "quit" => (Action::Quit, false),
        "submit-command" => (Action::SubmitCommand, false),

        // ── Fallback: treat as command-registry lookup ─────────────
        other => (
            Action::Command {
                name: other.to_string(),
                args: Vec::new(),
            },
            true,
        ),
    }
}

/// Report of how an [`apply_plan_to_commands`] pass touched the
/// [`CommandRegistry`]. Shaped like [`ApplyReport`] / frost doctor's
/// "applied vs. skipped" so the binary surfaces it in `--list-rc`.
#[derive(Debug, Clone, Default)]
pub struct CommandApplyReport {
    /// New command names registered (didn't previously exist).
    pub registered: u32,
    /// Existing command names overridden by a `defcmd` (last-writer
    /// wins — user/rc commands intentionally shadow built-ins).
    pub overridden: u32,
    /// Names that overrode a command, surfaced so a user can see when
    /// their `defcmd` replaced a built-in (rename if unintended).
    pub overridden_names: Vec<String>,
}

impl CommandApplyReport {
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "commands_registered={} overridden={}",
            self.registered, self.overridden,
        )
    }
}

/// Apply every [`CmdSpec`](crate::CmdSpec) in `plan` to `registry`.
///
/// Each `(defcmd :name … :description … :action "<dotted>")` becomes
/// a [`Command`] with an [`escriba_command::Handler::Action`] handler
/// — a genuinely invokable command whose dotted action symbol the
/// registry resolves at run time. This is the keystone that connects
/// the parse layer to live state: before this, a `defkeybind :action
/// "write-all"` deferred to a command name that was never registered,
/// so it dead-ended at the five built-ins. Now `defcmd` populates the
/// registry the deferred keybinds dispatch into.
///
/// Last-writer-wins: a `defcmd` whose `:name` matches an existing
/// command (built-in or earlier `defcmd`) overrides it — this is how
/// a user rebinds `save` to a richer behavior. The override is tallied
/// so `--list-rc` can show it.
pub fn apply_plan_to_commands(
    plan: &ApplyPlan,
    registry: &mut CommandRegistry,
) -> CommandApplyReport {
    let mut report = CommandApplyReport::default();
    for spec in &plan.commands {
        if registry.contains(&spec.name) {
            report.overridden += 1;
            report.overridden_names.push(spec.name.clone());
        } else {
            report.registered += 1;
        }
        registry.register(Command::action(
            spec.name.clone(),
            spec.description.clone(),
            spec.action.clone(),
        ));
    }
    report
}

/// Report of how an [`apply_plan_to_options`] pass touched the option
/// store.
#[derive(Debug, Clone, Default)]
pub struct OptionApplyReport {
    /// Options newly set (name didn't previously exist).
    pub set: u32,
    /// Options whose value overrode an existing entry.
    pub overridden: u32,
}

impl OptionApplyReport {
    #[must_use]
    pub fn summary(&self) -> String {
        format!("options_set={} overridden={}", self.set, self.overridden)
    }
}

/// Apply every `(defoption :name … :value …)` in `plan` to the option
/// store. Last-writer-wins: a later `defoption` (or a runtime
/// `(set-option …)` effect, or a user rc layered over the defaults)
/// overrides an earlier value.
///
/// Crucially this writes the SAME `HashMap` that the tatara-lisp
/// `set-option` effect writes (`escriba_vm::HostEffect::SetOption`) —
/// the declarative (def-form) and imperative (live Lisp) tiers converge
/// on one option source of truth rather than two parallel stores.
pub fn apply_plan_to_options(
    plan: &ApplyPlan,
    options: &mut std::collections::HashMap<String, String>,
) -> OptionApplyReport {
    let mut report = OptionApplyReport::default();
    for opt in &plan.options {
        if options
            .insert(opt.name.clone(), opt.value.clone())
            .is_some()
        {
            report.overridden += 1;
        } else {
            report.set += 1;
        }
    }
    report
}

/// Apply every `(defmode …)` to the live filetype table.
///
/// `:commentstring` has parsed and validated since Wave 1 and reached
/// nothing. This is the consumer. A mode whose commentstring has no `%s`
/// gets `comment: None` rather than a guess — the malformed string is
/// dropped, the rest of the mode survives, and the comment verbs decline
/// instead of producing nonsense.
pub fn apply_plan_to_filetypes(plan: &ApplyPlan, table: &mut FiletypeTable) -> usize {
    let mut n = 0;
    for m in &plan.major_modes {
        if m.name.is_empty() {
            continue;
        }
        table.insert(Filetype {
            name: m.name.clone(),
            extensions: m.extensions.clone(),
            comment: CommentString::parse(&m.commentstring),
        });
        n += 1;
    }
    n
}

/// Lower `(defsplash …)` into the [`Splash`] the renderers paint.
///
/// Returns `None` when the plan declares no splash, or declares one
/// with `:disabled #t` — a retired screen, per MODULARIZE-DON'T-DELETE,
/// keeps its declaration and loses only its effect.
///
/// Entry actions resolve through [`resolve_action`], the same table
/// `defkeybind` uses, so a menu entry and a key bound to the same
/// action string dispatch identically.
///
/// `facts` is filled by the caller (the binary), not here: the footer
/// states what THIS build actually is — version, plugin count, theme —
/// and none of that is knowable from a parsed plan alone.
#[must_use]
pub fn apply_plan_to_splash(plan: &ApplyPlan) -> Option<Splash> {
    let spec = plan.splash.as_ref().filter(|s| s.is_enabled())?;
    Some(Splash {
        art: spec.art.clone(),
        tagline: spec.tagline.clone(),
        entries: spec
            .resolved_entries()
            .map(|(key, e)| SplashEntry {
                key,
                label: e.label.clone(),
                action: resolve_action(&e.action).0,
            })
            .collect(),
        facts: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply_source;

    // ── defsplash → Splash ────────────────────────────────────────

    const SPLASH_RC: &str = r#"
        (defsplash :tagline "a modal editor"
                   :art ("  ___" " |___|")
                   :entries ((:key "e" :label "start editing" :action "normal")
                             (:key "/" :label "search"        :action "search-forward")
                             (:key "f" :label "find file"     :action "picker.files")
                             (:key "q" :label "quit"          :action "quit")))
    "#;

    #[test]
    fn splash_lowers_art_tagline_and_entries() {
        let splash = apply_plan_to_splash(&apply_source(SPLASH_RC).unwrap())
            .expect("an enabled defsplash yields a Splash");
        assert_eq!(splash.art.len(), 2);
        assert_eq!(splash.tagline, "a modal editor");
        assert_eq!(splash.entries.len(), 4);
        assert_eq!(splash.entries[0].key, 'e');
        assert_eq!(splash.entries[0].label, "start editing");
    }

    #[test]
    fn splash_entry_actions_use_the_keybind_resolver() {
        let splash = apply_plan_to_splash(&apply_source(SPLASH_RC).unwrap()).unwrap();
        // Typed variants where the table knows the name…
        assert_eq!(splash.entry_for('q').unwrap().action, Action::Quit);
        assert_eq!(
            splash.entry_for('e').unwrap().action,
            Action::ChangeMode(Mode::Normal),
        );
        assert_eq!(
            splash.entry_for('/').unwrap().action,
            Action::SearchOpen(SearchDirection::Forward),
        );
        // …and the SAME command-registry handoff a keybind gets when it
        // doesn't. An entry naming a plugin command is legal and defers.
        assert_eq!(
            splash.entry_for('f').unwrap().action,
            Action::Command {
                name: "picker.files".to_string(),
                args: Vec::new(),
            },
        );
    }

    #[test]
    fn a_disabled_splash_is_declared_but_inert() {
        let plan = apply_source(r#"(defsplash :disabled #t :tagline "hidden")"#).unwrap();
        assert!(plan.splash.is_some(), "the declaration survives");
        assert!(
            apply_plan_to_splash(&plan).is_none(),
            "…but produces no screen",
        );
    }

    #[test]
    fn no_defsplash_means_no_splash() {
        assert!(apply_plan_to_splash(&ApplyPlan::default()).is_none());
    }

    #[test]
    fn search_action_strings_resolve_to_typed_variants() {
        // These were missing from the table, which meant `:action
        // "search-forward"` silently became a command-registry lookup
        // for a command that does not exist.
        for (name, expected) in [
            (
                "search-forward",
                Action::SearchOpen(SearchDirection::Forward),
            ),
            (
                "search-backward",
                Action::SearchOpen(SearchDirection::Backward),
            ),
            ("search-next", Action::SearchRepeat { reverse: false }),
            ("search-prev", Action::SearchRepeat { reverse: true }),
        ] {
            let (action, deferred) = resolve_action(name);
            assert_eq!(action, expected, "{name}");
            assert!(!deferred, "{name} must not defer to the command registry");
        }
    }

    #[test]
    fn parse_single_char_token() {
        let l = Key::Char(',');
        assert_eq!(parse_key_token("a", &l), Some(Key::Char('a')));
        assert_eq!(parse_key_token("G", &l), Some(Key::Char('G')));
        assert_eq!(parse_key_token("0", &l), Some(Key::Char('0')));
    }

    #[test]
    fn parse_named_bracket_tokens() {
        let l = Key::Char(',');
        assert_eq!(parse_key_token("<Esc>", &l), Some(Key::Esc));
        assert_eq!(parse_key_token("<CR>", &l), Some(Key::Enter));
        assert_eq!(parse_key_token("<Enter>", &l), Some(Key::Enter));
        assert_eq!(parse_key_token("<Tab>", &l), Some(Key::Tab));
        assert_eq!(parse_key_token("<Backspace>", &l), Some(Key::Backspace));
        assert_eq!(parse_key_token("<BS>", &l), Some(Key::Backspace));
        assert_eq!(parse_key_token("<Left>", &l), Some(Key::Left));
        assert_eq!(parse_key_token("<PageUp>", &l), Some(Key::PageUp));
    }

    #[test]
    fn parse_modifier_bracket_tokens() {
        let l = Key::Char(',');
        assert_eq!(parse_key_token("<C-r>", &l), Some(Key::Ctrl('r')));
        assert_eq!(parse_key_token("<c-r>", &l), Some(Key::Ctrl('r')));
        assert_eq!(parse_key_token("<A-f>", &l), Some(Key::Alt('f')));
        assert_eq!(parse_key_token("<M-f>", &l), Some(Key::Alt('f')));
    }

    #[test]
    fn parse_leader_token_resolves_to_leader() {
        // `<leader>` / `<localleader>` resolve to the keymap's leader.
        assert_eq!(
            parse_key_token("<leader>", &Key::Char(',')),
            Some(Key::Char(','))
        );
        assert_eq!(
            parse_key_token("<Leader>", &Key::Char(' ')),
            Some(Key::Char(' '))
        );
        assert_eq!(
            parse_key_token("<localleader>", &Key::Char('\\')),
            Some(Key::Char('\\'))
        );
    }

    #[test]
    fn parse_token_rejects_multichar_and_unknown_bracket() {
        let l = Key::Char(',');
        // A bare multi-char token isn't a single key.
        assert_eq!(parse_key_token("gh", &l), None);
        // Unknown bracket name.
        assert_eq!(parse_key_token("<Galactus>", &l), None);
    }

    #[test]
    fn parse_key_sequence_tokenizes_leader_and_brackets() {
        let comma = Key::Char(',');
        assert_eq!(
            parse_key_sequence("<leader>ff", &comma).unwrap(),
            vec![Key::Char(','), Key::Char('f'), Key::Char('f')],
        );
        assert_eq!(
            parse_key_sequence("gg", &comma).unwrap(),
            vec![Key::Char('g'), Key::Char('g')],
        );
        assert_eq!(
            parse_key_sequence("<C-w>h", &comma).unwrap(),
            vec![Key::Ctrl('w'), Key::Char('h')],
        );
        assert_eq!(
            parse_key_sequence("x", &comma).unwrap(),
            vec![Key::Char('x')]
        );
        // Malformed: unterminated bracket + unknown token.
        assert!(parse_key_sequence("<leader", &comma).is_err());
        assert!(parse_key_sequence("<Nope>", &comma).is_err());
    }

    #[test]
    fn space_token_parses_in_tokens_and_sequences() {
        let comma = Key::Char(',');
        assert_eq!(parse_key_token("<space>", &comma), Some(Key::Char(' ')));
        assert_eq!(parse_key_token("<Space>", &comma), Some(Key::Char(' ')));
        assert_eq!(parse_key_token("<spc>", &comma), Some(Key::Char(' ')));
        assert_eq!(
            parse_key_sequence("<space>w", &comma).unwrap(),
            vec![Key::Char(' '), Key::Char('w')],
        );
    }

    /// Regression: escriba's OWN bundled catalog shipped three bindings
    /// its parser rejected, so every default boot logged three warnings
    /// and silently dropped the bindings —
    /// `<C-Space>` (escriba-cmp: trigger completion) and `<S-h>`/`<S-l>`
    /// (escriba-bufferline: prev/next buffer). Two independent gaps: a
    /// modifier operand had to be exactly one char (so `Space` failed),
    /// and `S-` was not a recognised modifier at all.
    #[test]
    fn modifier_accepts_named_operand_and_shift() {
        let comma = Key::Char(',');
        // `<C-Space>` — named operand behind a modifier.
        assert_eq!(parse_key_token("<C-Space>", &comma), Some(Key::Ctrl(' ')));
        assert_eq!(parse_key_token("<c-space>", &comma), Some(Key::Ctrl(' ')));
        assert_eq!(parse_key_token("<A-Space>", &comma), Some(Key::Alt(' ')));
        // `<S-h>` / `<S-l>` — shift folds to the uppercase char, which is
        // what the input layer delivers for a shifted letter.
        assert_eq!(parse_key_token("<S-h>", &comma), Some(Key::Char('H')));
        assert_eq!(parse_key_token("<S-l>", &comma), Some(Key::Char('L')));
        assert_eq!(parse_key_token("<s-h>", &comma), Some(Key::Char('H')));
        // Plain modifier forms keep working.
        assert_eq!(parse_key_token("<C-w>", &comma), Some(Key::Ctrl('w')));
        // `<C-Esc>` USED to be unrepresentable, and this test documented why:
        // `Key::Ctrl` holds a `char`, so a named operand had nothing to hold.
        // `Key::Chord` carries an `awase::Hotkey`, so it holds fine now — the
        // limitation is gone, and asserting `None` would be pinning it back.
        match parse_key_token("<C-Esc>", &comma) {
            Some(Key::Chord(h)) => {
                assert!(h.modifiers.contains(awase::Modifiers::CTRL));
                assert_eq!(h.key, awase::Key::Escape);
            }
            other => panic!("`<C-Esc>` is representable now, got {other:?}"),
        }
        // And genuine nonsense still errors rather than binding silently.
        assert_eq!(parse_key_token("<C-NotAKey>", &comma), None);
    }

    #[test]
    fn parse_leader_key_handles_space_comma_and_rejects_recursion() {
        assert_eq!(parse_leader_key("<space>"), Some(Key::Char(' ')));
        assert_eq!(parse_leader_key("<Space>"), Some(Key::Char(' ')));
        assert_eq!(parse_leader_key(","), Some(Key::Char(',')));
        assert_eq!(parse_leader_key("\\"), Some(Key::Char('\\')));
        assert_eq!(parse_leader_key("<C-a>"), Some(Key::Ctrl('a')));
        // leader-of-leader is nonsense; multichar isn't a single key.
        assert_eq!(parse_leader_key("<leader>"), None);
        assert_eq!(parse_leader_key("nope-multichar"), None);
    }

    #[test]
    fn leader_sequence_resolves_against_configured_leader() {
        // set_leader changes which prefix `<leader>` resolves to —
        // proving the mapleader-config path the binary wires. With a
        // space leader, `<leader>ff` binds under [space, f, f] and NOT
        // under the comma default.
        let mut km = Keymap::new();
        km.set_leader(Key::Char(' '));
        let plan =
            apply_source(r#"(defkeybind :mode "normal" :key "<leader>ff" :action "picker.files")"#)
                .unwrap();
        apply_plan_to_keymap(&plan, &mut km);
        let space_seq = vec![Key::Char(' '), Key::Char('f'), Key::Char('f')];
        assert!(
            km.lookup_sequence(Mode::Normal, &space_seq).is_some(),
            "<leader>ff should bind under the configured space leader",
        );
        let comma_seq = vec![Key::Char(','), Key::Char('f'), Key::Char('f')];
        assert!(
            km.lookup_sequence(Mode::Normal, &comma_seq).is_none(),
            "the comma default must NOT capture <leader> once leader is space",
        );
    }

    #[test]
    fn resolve_known_action_returns_typed_variant() {
        let (a, deferred) = resolve_action("move-left");
        assert_eq!(a, Action::Move(Motion::Left));
        assert!(!deferred);
    }

    #[test]
    fn resolve_unknown_action_falls_back_to_command() {
        let (a, deferred) = resolve_action("goto-home");
        assert!(deferred);
        match a {
            Action::Command { name, .. } => assert_eq!(name, "goto-home"),
            other => panic!("expected Action::Command, got {other:?}"),
        }
    }

    #[test]
    fn apply_populates_keymap_with_typed_action() {
        let plan = apply_source(
            r#"
            (defkeybind :mode "normal" :key "h" :action "move-left")
            (defkeybind :mode "insert" :key "<Esc>" :action "normal")
            "#,
        )
        .unwrap();
        let mut km = Keymap::new();
        let report = apply_plan_to_keymap(&plan, &mut km);
        assert_eq!(report.keybinds_applied, 2);
        assert_eq!(report.keybinds_deferred_to_commands, 0);
        assert!(report.warnings.is_empty());

        let b = km.lookup(Mode::Normal, &Key::Char('h')).unwrap();
        assert_eq!(b.action, Action::Move(Motion::Left));
    }

    #[test]
    fn apply_defers_unknown_action_to_command_registry() {
        let plan =
            apply_source(r#"(defkeybind :mode "normal" :key "g" :action "goto-home")"#).unwrap();
        let mut km = Keymap::new();
        let report = apply_plan_to_keymap(&plan, &mut km);
        assert_eq!(report.keybinds_applied, 1);
        assert_eq!(report.keybinds_deferred_to_commands, 1);
    }

    #[test]
    fn apply_binds_multi_key_sequences_into_sequence_table() {
        // Multi-key bindings (`gh`, `<leader>ff`) now BIND into the
        // keymap's sequence table — the runtime's pending-stroke loop
        // resolves them. `<leader>` resolves to the keymap's leader
        // (`,` by default — blnvim parity).
        let plan = apply_source(
            r#"
            (defkeybind :mode "normal" :key "gh" :action "doc-start")
            (defkeybind :mode "normal" :key "<leader>ff" :action "picker.files")
            (defkeybind :mode "normal" :key "<leader>fg" :action "picker.grep")
            (defkeybind :mode "normal" :key "<Leader>fb" :action "picker.buffers")
            "#,
        )
        .unwrap();
        let mut km = Keymap::new();
        let report = apply_plan_to_keymap(&plan, &mut km);
        assert_eq!(report.keybinds_applied, 4);
        assert_eq!(report.keybinds_sequences, 4);
        // The three picker binds defer to the command registry; gh's
        // `doc-start` is a curated typed action.
        assert_eq!(report.keybinds_deferred_to_commands, 3);
        assert!(report.warnings.is_empty());

        // `<leader>ff` resolves to its command via the sequence table.
        let seq = vec![Key::Char(','), Key::Char('f'), Key::Char('f')];
        let b = km
            .lookup_sequence(Mode::Normal, &seq)
            .expect("<leader>ff should be bound");
        assert!(matches!(&b.action, Action::Command { name, .. } if name == "picker.files"));

        // `gh` binds as a 2-key sequence to a typed motion.
        let gh = vec![Key::Char('g'), Key::Char('h')];
        assert_eq!(
            km.lookup_sequence(Mode::Normal, &gh).unwrap().action,
            Action::Move(Motion::DocStart),
        );
    }

    #[test]
    fn apply_still_warns_on_truly_unrecognised_keys() {
        let plan = apply_source(r#"(defkeybind :mode "normal" :key "<Galactus>" :action "home")"#)
            .unwrap();
        let mut km = Keymap::new();
        let report = apply_plan_to_keymap(&plan, &mut km);
        assert_eq!(report.keybinds_applied, 0);
        assert_eq!(report.keybinds_sequences, 0);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn apply_commands_registers_defcmd_into_registry() {
        let plan = apply_source(
            r#"
            (defcmd :name "write-all" :description "Write every modified buffer" :action "buffer.write-all")
            (defcmd :name "pick-files" :description "Pick a file" :action "picker.files")
            "#,
        )
        .unwrap();
        let mut registry = CommandRegistry::default_set();
        let before = registry.len();
        let report = apply_plan_to_commands(&plan, &mut registry);
        assert_eq!(report.registered, 2);
        assert_eq!(report.overridden, 0);
        assert!(registry.contains("write-all"));
        assert!(registry.contains("pick-files"));
        assert_eq!(registry.len(), before + 2);
    }

    #[test]
    fn apply_commands_defcmd_overrides_builtin_last_writer_wins() {
        // A `defcmd :name "save"` shadows the built-in `save` — the
        // mechanism by which a user upgrades a built-in to a richer
        // Lisp-authored behavior. The override is tallied, not silent.
        let plan = apply_source(
            r#"(defcmd :name "save" :description "Save, formatted" :action "buffer.write")"#,
        )
        .unwrap();
        let mut registry = CommandRegistry::default_set();
        let report = apply_plan_to_commands(&plan, &mut registry);
        assert_eq!(report.registered, 0);
        assert_eq!(report.overridden, 1);
        assert_eq!(report.overridden_names, vec!["save".to_string()]);
    }

    #[test]
    fn apply_commands_empty_plan_is_noop() {
        let plan = apply_source("").unwrap();
        let mut registry = CommandRegistry::default_set();
        let before = registry.len();
        let report = apply_plan_to_commands(&plan, &mut registry);
        assert_eq!(report.registered, 0);
        assert_eq!(report.overridden, 0);
        assert_eq!(registry.len(), before);
    }

    #[test]
    fn apply_options_sets_and_overrides() {
        let plan = apply_source(
            r#"
            (defoption :name "number" :value "true")
            (defoption :name "tabstop" :value "4")
            (defoption :name "number" :value "false")
            "#,
        )
        .unwrap();
        let mut opts = std::collections::HashMap::new();
        let report = apply_plan_to_options(&plan, &mut opts);
        // 3 specs: number(set), tabstop(set), number(override).
        assert_eq!(report.set, 2);
        assert_eq!(report.overridden, 1);
        // Last writer wins.
        assert_eq!(opts.get("number").map(String::as_str), Some("false"));
        assert_eq!(opts.get("tabstop").map(String::as_str), Some("4"));
    }

    #[test]
    fn grammar_apply_registers_known_and_reports_unknown() {
        let plan = apply_source(
            r#"
            (defmode :name "rust" :tree-sitter "rust" :extensions ("rs" "rs.in"))
            (defmode :name "nix"  :tree-sitter "nix"  :extensions ("nix"))
            (defmode :name "plain" :extensions ("txt"))
            "#,
        )
        .unwrap();

        // Stub registry: only "rust" is known.
        let known_langs = ["rust"];
        let mut registered: Vec<(String, String)> = Vec::new();
        let report = apply_plan_to_grammar_extensions(
            &plan,
            |name| known_langs.iter().any(|k| *k == name),
            |lang, ext| registered.push((lang.to_string(), ext.to_string())),
        );

        // rust: 2 exts registered. nix: 1 ext skipped. plain: no ts lang, skipped silently.
        assert_eq!(report.extensions_registered, 2);
        assert_eq!(report.extensions_skipped_unknown_language, 1);
        assert_eq!(report.unknown_languages, vec!["nix".to_string()]);
        assert_eq!(
            registered,
            vec![
                ("rust".to_string(), "rs".to_string()),
                ("rust".to_string(), "rs.in".to_string()),
            ]
        );
    }

    #[test]
    fn grammar_apply_tolerates_defmode_without_tree_sitter() {
        // A `defmode` that omits `:tree-sitter` (e.g. plain-text
        // languages) should be skipped silently — no warning, no error.
        let plan = apply_source(r#"(defmode :name "plain" :extensions ("txt" "log"))"#).unwrap();
        let report = apply_plan_to_grammar_extensions(
            &plan,
            |_| true,
            |_, _| panic!("should not register anything"),
        );
        assert_eq!(report.extensions_registered, 0);
        assert!(report.unknown_languages.is_empty());
    }

    #[test]
    fn apply_overrides_default_vim_binding() {
        // A user's `(defkeybind :mode "normal" :key "h" :action "move-right")`
        // should overwrite the default vim binding.
        let plan =
            apply_source(r#"(defkeybind :mode "normal" :key "h" :action "move-right")"#).unwrap();
        let mut km = Keymap::default_vim();
        let before = km
            .lookup(Mode::Normal, &Key::Char('h'))
            .cloned()
            .expect("default vim should bind h");
        assert_eq!(before.action, Action::Move(Motion::Left));

        apply_plan_to_keymap(&plan, &mut km);
        let after = km.lookup(Mode::Normal, &Key::Char('h')).unwrap();
        assert_eq!(after.action, Action::Move(Motion::Right));
    }
}

#[cfg(test)]
mod widened_vocabulary {
    use super::*;

    fn comma() -> Key {
        Key::Char(',')
    }

    #[test]
    fn function_keys_parse_and_are_no_longer_discarded() {
        // `<F5>` was declarable and undeliverable: the input layer dropped
        // `KeyCode::F(_)` at the door, so the binding existed and the key
        // never arrived.
        assert_eq!(parse_key_token("<F5>", &comma()), Some(Key::F(5)));
        assert_eq!(parse_key_token("<f12>", &comma()), Some(Key::F(12)));
        assert_eq!(parse_key_token("<F21>", &comma()), None, "past the range");
    }

    #[test]
    fn multi_modifier_chords_parse() {
        // `Ctrl(char)` folds ONE modifier into the key, so `<C-S-p>` was
        // unparseable — an author could write it and get silence.
        let k = parse_key_token("<C-S-p>", &comma()).expect("parses");
        match k {
            Key::Chord(h) => {
                assert!(h.modifiers.contains(awase::Modifiers::CTRL));
                assert!(h.modifiers.contains(awase::Modifiers::SHIFT));
                assert_eq!(h.key, awase::Key::P);
            }
            other => panic!("expected a chord, got {other:?}"),
        }
    }

    #[test]
    fn super_parses_in_every_spelling() {
        for tok in ["<D-p>", "<Cmd-p>", "<super-p>"] {
            match parse_key_token(tok, &comma()) {
                Some(Key::Chord(h)) => {
                    assert!(h.modifiers.contains(awase::Modifiers::CMD), "{tok}");
                }
                other => panic!("{tok} should be a Super chord, got {other:?}"),
            }
        }
    }

    #[test]
    fn the_single_modifier_shorthands_are_unchanged() {
        // The widening must not move `<C-r>` or `<A-f>` — 431 existing
        // references depend on those staying the shorthand variants.
        assert_eq!(parse_key_token("<C-r>", &comma()), Some(Key::Ctrl('r')));
        assert_eq!(parse_key_token("<A-f>", &comma()), Some(Key::Alt('f')));
        assert_eq!(parse_key_token("<M-f>", &comma()), Some(Key::Alt('f')));
    }

    #[test]
    fn shift_alone_still_folds_to_the_uppercase_char() {
        // Deliberate and documented: the input layer delivers a shifted
        // letter as its uppercase char, so `<S-h>` IS `H`. Modelling it as a
        // modifier would make one physical keystroke two bindings.
        assert_eq!(parse_key_token("<S-h>", &comma()), Some(Key::Char('H')));
    }
}
