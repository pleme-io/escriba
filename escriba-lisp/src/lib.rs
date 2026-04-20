//! Tatara-Lisp ↔ escriba bridge.
//!
//! Mirrors [`frost-lisp`](https://github.com/pleme-io/frost/tree/main/crates/frost-lisp)
//! for the editor domain. The pleme-io Rust + Lisp pattern:
//!
//! - **Rust** owns the types, invariants, and execution.
//! - **Lisp** owns the authoring surface — users declare what they
//!   want the editor to be, and the runtime resolves it.
//!
//! This crate parses a Lisp source and returns an [`ApplyPlan`] —
//! a typed bundle of every declaration found. The escriba binary
//! then applies the plan to live state (keymap, command registry,
//! options, hooks). Keeping the plan as data (vs. mutating live
//! state directly) means the bridge stays trivially testable and
//! composable: load several files, merge plans, apply once.
//!
//! # Supported forms
//!
//! | Keyword       | Spec type                                    |
//! |---------------|----------------------------------------------|
//! | `defkeybind`  | [`KeybindSpec`]                              |
//! | `defcmd`      | [`CmdSpec`]                                  |
//! | `defoption`   | [`OptionSpec`]                               |
//! | `deftheme`    | [`ThemeSpec`]                                |
//! | `defhook`     | [`HookSpec`]                                 |
//! | `defft`       | [`FiletypeSpec`]                             |
//! | `defabbrev`   | [`AbbrevSpec`]                               |
//! | `defsnippet`  | [`SnippetSpec`]                              |
//! | `defmode`     | [`MajorModeSpec`]                            |
//! | `defplugin`   | [`PluginSpec`]                               |
//! | `defhighlight`| [`HighlightSpec`]                            |
//! | `defstatusline`| [`StatusLineSpec`]                          |
//! | `defbufferline`| [`BufferLineSpec`]                          |
//! | `deflsp`      | [`LspServerSpec`]                            |
//! | `defformatter`| [`FormatterSpec`]                            |
//! | `defpalette`  | [`PaletteSpec`]                              |
//! | `deficon`     | [`IconSpec`]                                 |
//! | `defdap`      | [`DapAdapterSpec`]                           |
//! | `defgate`     | [`GateSpec`] — convergence pre/post-condition on an editor event |
//! | `deftextobject`| [`TextObjectSpec`] — tree-sitter text object bound to vim `i`/`a` grammar |
//! | `defworkflow` | [`WorkflowSpec`] — named DAG of gates + actions (editor-layer workflow) |
//! | `defsession`  | [`SessionSpec`] — named workspace layout (absorbs vim :mksession / vscode workspaces) |
//! | `defeffect`   | [`EffectSpec`] — ghostty-style GPU shader effect (cursor glow / bloom / scanlines) |
//! | `defterm`     | [`TermSpec`] — terminal session spec (wire-compatible with `mado::term_spec::TermSpec`) |
//! | `defmark`     | [`MarkSpec`] — named marks with jump/anchor/glance kind semantics |
//! | `deftask`     | [`TaskSpec`] — single shell task with filetype + cwd + env scope (absorbs vscode tasks.json / nvim asynctasks) |
//! | `defschedule` | [`ScheduleSpec`] — typed declarative triggers (cron / interval / idle / startup) for commands + workflows (invention — no editor ships this typed) |
//! | `defkmacro`   | [`KmacroSpec`] — declarative keyboard macro (vim `q`-register / emacs `kmacro.el` as rc-authored typed spec) |
//! | `defattest`   | [`AttestSpec`] — content-addressed rc integrity attestation (BLAKE3-128 of `ApplyPlan::summary()`; invention — pleme-io convergence-computing at the editor layer) |
//! | `defruler`    | [`RulerSpec`] — declarative column rulers / visual guides (absorbs vim `colorcolumn`, vscode `editor.rulers`, jetbrains hard-wrap margin) |
//! | `defmcp`      | [`McpToolSpec`] — declarative MCP-tool binding (invention — no editor ships typed cross-process MCP import) |
//!
//! # Extending
//!
//! Add a new module next to the others with a `#[derive(DeriveTataraDomain)]`
//! struct plus `#[tatara(keyword = "…")]`, re-export it from [`lib`], and
//! add a line in [`apply_source`] that pulls it out via
//! `tatara_lisp::compile_typed`.

mod abbrev;
mod apply;
mod attest;
mod bufferline;
mod cmd;
mod dap;
mod effect;
mod filetype;
mod formatter;
mod gate;
mod hash;
mod highlight;
mod hook;
mod icon;
mod keybind;
mod kmacro;
mod lsp;
mod mark;
mod mcp;
mod mode;
mod mode_spec;
mod option;
mod palette;
mod plugin;
mod ruler;
mod schedule;
mod session;
mod snippet;
mod statusline;
mod task;
mod term;
mod textobject;
mod theme;
mod workflow;

pub use abbrev::AbbrevSpec;
pub use apply::{
    ApplyReport, GrammarApplyReport, apply_plan_to_grammar_extensions, apply_plan_to_keymap,
};
pub use attest::{
    AttestResult, AttestSpec, KNOWN_KINDS as ATTEST_KINDS,
    KNOWN_SEVERITIES as ATTEST_SEVERITIES, compute_summary_hash,
    is_known_kind as is_known_attest_kind,
    is_known_severity as is_known_attest_severity,
};
pub use bufferline::BufferLineSpec;
pub use cmd::CmdSpec;
pub use dap::{DapAdapterSpec, KNOWN_ADAPTERS, is_known_adapter};
pub use effect::{
    CANONICAL_EFFECTS, EffectSpec, KNOWN_KINDS as EFFECT_KINDS,
    is_canonical_effect, is_known_kind as is_known_effect_kind,
};
pub use filetype::FiletypeSpec;
pub use formatter::FormatterSpec;
pub use gate::{
    GateMode, GateSpec, KNOWN_ACTIONS as GATE_ACTIONS, KNOWN_SEVERITIES as GATE_SEVERITIES,
    KNOWN_SOURCES as GATE_SOURCES, is_known_action as is_gate_action,
    is_known_severity as is_gate_severity, is_known_source as is_gate_source,
};
pub use hash::{compute_blake3_128_hex, is_blake3_128_hex};
pub use highlight::{CANONICAL_GROUPS, HighlightSpec, is_canonical_group};
pub use hook::{HookSpec, KNOWN_EVENTS, is_known_event};
pub use icon::IconSpec;
pub use keybind::KeybindSpec;
pub use kmacro::{
    KNOWN_MODES as KMACRO_MODES, KmacroSpec,
    is_known_mode as is_known_kmacro_mode,
};
pub use lsp::{KNOWN_SERVERS, LspServerSpec, is_known_server};
pub use mark::{KNOWN_KINDS as MARK_KINDS, MarkSpec, is_known_kind as is_known_mark_kind};
pub use mcp::McpToolSpec;
pub use mode::{KNOWN_MODES, is_known_mode};
pub use mode_spec::MajorModeSpec;
pub use option::OptionSpec;
pub use palette::PaletteSpec;
pub use plugin::{KNOWN_CATEGORIES, PluginSpec, is_known_category};
pub use ruler::{KNOWN_STYLES as RULER_STYLES, RulerSpec, is_known_style as is_known_ruler_style};
pub use schedule::{Dispatch as ScheduleDispatch, ScheduleSpec, Trigger as ScheduleTrigger};
pub use session::{KNOWN_LAYOUTS as SESSION_LAYOUTS, SessionSpec, is_known_layout};
pub use snippet::{Resolution as SnippetResolution, SnippetSpec};
pub use statusline::{KNOWN_SEGMENTS, StatusLineSpec, StatusSegment, is_known_segment};
pub use task::TaskSpec;
pub use term::{
    KNOWN_PLACEMENTS as TERM_PLACEMENTS, TermSpec,
    is_known_placement as is_known_term_placement,
};
pub use textobject::{
    CANONICAL_NAMES as TEXTOBJECT_CANONICAL_NAMES,
    KNOWN_SCOPES as TEXTOBJECT_SCOPES, TextObjectSpec,
    is_canonical_short as is_canonical_textobject_short,
    is_known_scope as is_known_textobject_scope,
};
pub use theme::{KNOWN_PRESETS, ThemeSpec, is_known_preset};
pub use workflow::{
    KNOWN_FAILURE_MODES as WORKFLOW_FAILURE_MODES, KNOWN_STEP_KINDS as WORKFLOW_STEP_KINDS,
    WorkflowSpec, is_known_failure_mode as is_workflow_failure_mode,
    is_known_step_kind as is_workflow_step_kind,
};

use std::path::{Path, PathBuf};

pub type LispResult<T> = Result<T, LispError>;

/// Compile every `T`-keyword form in `src` into a `Vec<T>`. Wraps
/// `tatara_lisp::compile_typed` to map its parse errors onto
/// [`LispError::Parse`] — so `apply_source` reads as a series of
/// calls to this helper rather than 20 copies of the same dance.
fn compile<T: tatara_lisp::TataraDomain>(src: &str) -> LispResult<Vec<T>> {
    tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))
}

/// Compile `T`-keyword forms and validate each via `validate` before
/// returning. Short-circuits on first validation failure. Collapses
/// the `compile + for-loop-check` pattern that repeated across every
/// validated def-form in `apply_source`.
///
/// The validator is an `FnMut` so closures can capture mutable state
/// (e.g., dedup sets) if a future validator needs them. For now all
/// callers pass `Fn`-like closures.
fn compile_validated<T, V>(src: &str, mut validate: V) -> LispResult<Vec<T>>
where
    T: tatara_lisp::TataraDomain,
    V: FnMut(&T) -> LispResult<()>,
{
    let specs: Vec<T> = compile(src)?;
    for spec in &specs {
        validate(spec)?;
    }
    Ok(specs)
}

#[derive(Debug, thiserror::Error)]
pub enum LispError {
    #[error("io error reading rc file {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("tatara-lisp parse error: {0}")]
    Parse(String),
    #[error("unknown hook event: {0} (valid: {valid})", valid = KNOWN_EVENTS.join(", "))]
    UnknownHook(String),
    #[error("unknown theme preset: {0} (valid: {valid})", valid = KNOWN_PRESETS.join(", "))]
    UnknownTheme(String),
    #[error("unknown mode name: {0} (valid: {valid})", valid = KNOWN_MODES.join(", "))]
    UnknownMode(String),
    #[error("unknown gate action: {0} (valid: {valid})", valid = GATE_ACTIONS.join(", "))]
    UnknownGateAction(String),
    #[error(
        "invalid gate shape on `{0}` — exactly one of `:command` / `:source` must be set"
    )]
    InvalidGateShape(String),
    #[error(
        "unknown gate severity: {0} (valid: {valid})",
        valid = GATE_SEVERITIES.join(", ")
    )]
    UnknownGateSeverity(String),
    #[error(
        "unknown text-object scope: {0} (valid: {valid})",
        valid = TEXTOBJECT_SCOPES.join(", ")
    )]
    UnknownTextObjectScope(String),
    #[error(
        "unknown workflow on-failure mode: {0} (valid: {valid})",
        valid = WORKFLOW_FAILURE_MODES.join(", ")
    )]
    UnknownWorkflowFailureMode(String),
    #[error("defeffect `{0}` has :kind \"custom\" but no :shader path")]
    MalformedCustomEffect(String),
    #[error(
        "unknown defterm :placement `{0}` (valid: {valid})",
        valid = TERM_PLACEMENTS.join(", ")
    )]
    UnknownTermPlacement(String),
    #[error(
        "unknown defmark :kind `{0}` (valid: {valid})",
        valid = MARK_KINDS.join(", ")
    )]
    UnknownMarkKind(String),
    #[error(
        "defsnippet `{0}` must set exactly one of `:body` / `:hash` — got neither or both"
    )]
    InvalidSnippetShape(String),
    #[error(
        "defsnippet :hash `{0}` is not a 32-char lowercase BLAKE3-128 hex token"
    )]
    MalformedSnippetHash(String),
    #[error("deftask missing `:name` — each task spec needs a non-empty id")]
    EmptyTaskName,
    #[error("deftask `{0}` has empty `:command` — shell task needs a binary to exec")]
    EmptyTaskCommand(String),
    #[error(
        "defschedule `{0}` has an ill-formed trigger — set exactly one of \
        `:cron` / `:interval-seconds` / `:idle-seconds` / `:at-startup`, \
        or none of them (manual-only via `:keybind`)"
    )]
    InvalidScheduleTrigger(String),
    #[error(
        "defschedule `{0}` has an ill-formed dispatch — set exactly one of \
        `:command` / `:workflow` / `:action`"
    )]
    InvalidScheduleDispatch(String),
    #[error(
        "defschedule `{0}` :cron expression is malformed — expected five \
        whitespace-separated fields (minute hour day month day-of-week)"
    )]
    MalformedScheduleCron(String),
    #[error("defkmacro `{0}` has empty `:keys` — macro needs a non-empty key sequence")]
    EmptyKmacroKeys(String),
    #[error(
        "defkmacro `{name}` has unknown `:mode` `{mode}` (valid: {valid})",
        valid = KNOWN_MODES.join(", ")
    )]
    UnknownKmacroMode { name: String, mode: String },
    #[error(
        "defkmacro `{0}` has malformed `:register` — expected a single a-z / A-Z / 0-9 char"
    )]
    MalformedKmacroRegister(String),
    #[error(
        "defattest `{name}` has unknown `:kind` `{kind}` (valid: {valid})",
        valid = ATTEST_KINDS.join(", ")
    )]
    UnknownAttestKind { name: String, kind: String },
    #[error(
        "defattest `{name}` has unknown `:severity` `{severity}` (valid: {valid})",
        valid = ATTEST_SEVERITIES.join(", ")
    )]
    UnknownAttestSeverity { name: String, severity: String },
    #[error(
        "defattest `{0}` :counts-hash is malformed — expected 32 lowercase \
        BLAKE3-128 hex chars, or empty for an unpinned attestation"
    )]
    MalformedAttestHash(String),
    #[error("defruler has empty `:columns` — specify at least one column position")]
    EmptyRulerColumns,
    #[error("defruler has a zero column position — columns are 1-based")]
    ZeroRulerColumn,
    #[error(
        "defruler has unknown `:style` `{0}` (valid: {valid})",
        valid = RULER_STYLES.join(", ")
    )]
    UnknownRulerStyle(String),
    #[error(
        "defruler `:color` `{0}` is malformed — expected `#rrggbb` or `#rrggbbaa`"
    )]
    MalformedRulerColor(String),
    #[error("defmcp `{0}` has empty `:server` — required MCP server alias")]
    EmptyMcpServer(String),
    #[error("defmcp `{0}` has empty `:tool` — required remote tool name")]
    EmptyMcpTool(String),
    #[error(
        "defmcp `{name}` has malformed `:on-result` `{value}` \
        — expected empty, or one of {prefixes}",
        prefixes = mcp::McpToolSpec::ON_RESULT_PREFIXES.join(", ")
    )]
    MalformedMcpOnResult { name: String, value: String },
}

/// Everything a Lisp rc file can declare, in one typed bundle.
///
/// Consumers iterate the vectors and apply them to live editor
/// state (keymap, command registry, options, etc.). Each vector is
/// append-only relative to the input source order — last writer
/// wins is the consumer's policy, not this crate's.
#[derive(Debug, Clone, Default)]
pub struct ApplyPlan {
    pub keybinds: Vec<KeybindSpec>,
    pub commands: Vec<CmdSpec>,
    pub options: Vec<OptionSpec>,
    pub theme: Option<ThemeSpec>,
    pub hooks: Vec<HookSpec>,
    pub filetypes: Vec<FiletypeSpec>,
    pub abbreviations: Vec<AbbrevSpec>,
    pub snippets: Vec<SnippetSpec>,
    pub major_modes: Vec<MajorModeSpec>,
    pub plugins: Vec<PluginSpec>,
    pub highlights: Vec<HighlightSpec>,
    pub status_line: Option<StatusLineSpec>,
    pub buffer_line: Option<BufferLineSpec>,
    pub lsp_servers: Vec<LspServerSpec>,
    pub formatters: Vec<FormatterSpec>,
    pub palettes: Vec<PaletteSpec>,
    pub icons: Vec<IconSpec>,
    pub dap_adapters: Vec<DapAdapterSpec>,
    pub gates: Vec<GateSpec>,
    pub text_objects: Vec<TextObjectSpec>,
    pub workflows: Vec<WorkflowSpec>,
    pub sessions: Vec<SessionSpec>,
    pub effects: Vec<EffectSpec>,
    pub terms: Vec<TermSpec>,
    pub marks: Vec<MarkSpec>,
    pub tasks: Vec<TaskSpec>,
    pub schedules: Vec<ScheduleSpec>,
    pub kmacros: Vec<KmacroSpec>,
    pub attests: Vec<AttestSpec>,
    pub rulers: Vec<RulerSpec>,
    pub mcp_tools: Vec<McpToolSpec>,
}

impl ApplyPlan {
    /// Merge `other` into `self`. Vectors concatenate in input order;
    /// `theme` is overwritten (last-writer-wins — matches frost-lisp's
    /// `deftheme` semantics).
    pub fn merge(&mut self, other: ApplyPlan) {
        self.keybinds.extend(other.keybinds);
        self.commands.extend(other.commands);
        self.options.extend(other.options);
        if other.theme.is_some() {
            self.theme = other.theme;
        }
        self.hooks.extend(other.hooks);
        self.filetypes.extend(other.filetypes);
        self.abbreviations.extend(other.abbreviations);
        self.snippets.extend(other.snippets);
        self.major_modes.extend(other.major_modes);
        self.plugins.extend(other.plugins);
        self.highlights.extend(other.highlights);
        if other.status_line.is_some() {
            self.status_line = other.status_line;
        }
        if other.buffer_line.is_some() {
            self.buffer_line = other.buffer_line;
        }
        self.lsp_servers.extend(other.lsp_servers);
        self.formatters.extend(other.formatters);
        self.palettes.extend(other.palettes);
        self.icons.extend(other.icons);
        self.dap_adapters.extend(other.dap_adapters);
        self.gates.extend(other.gates);
        self.text_objects.extend(other.text_objects);
        self.workflows.extend(other.workflows);
        self.sessions.extend(other.sessions);
        self.effects.extend(other.effects);
        self.terms.extend(other.terms);
        self.marks.extend(other.marks);
        self.tasks.extend(other.tasks);
        self.schedules.extend(other.schedules);
        self.kmacros.extend(other.kmacros);
        self.attests.extend(other.attests);
        self.rulers.extend(other.rulers);
        self.mcp_tools.extend(other.mcp_tools);
    }

    /// Pairs of `(label, count)` — the single source of truth the
    /// summary + startup banner + planned `escriba doctor` all read
    /// from. Adding a def-form = one new entry here + a glyph in
    /// [`form_glyph`]; nothing else.
    #[must_use]
    pub fn counts(&self) -> Vec<(&'static str, usize)> {
        vec![
            ("keybinds", self.keybinds.len()),
            ("cmds", self.commands.len()),
            ("options", self.options.len()),
            ("theme", usize::from(self.theme.is_some())),
            ("hooks", self.hooks.len()),
            ("filetypes", self.filetypes.len()),
            ("abbrev", self.abbreviations.len()),
            ("snippets", self.snippets.len()),
            ("major_modes", self.major_modes.len()),
            ("plugins", self.plugins.len()),
            ("highlights", self.highlights.len()),
            ("statusline", usize::from(self.status_line.is_some())),
            ("bufferline", usize::from(self.buffer_line.is_some())),
            ("lsp", self.lsp_servers.len()),
            ("formatters", self.formatters.len()),
            ("palettes", self.palettes.len()),
            ("icons", self.icons.len()),
            ("dap", self.dap_adapters.len()),
            ("gates", self.gates.len()),
            ("textobjects", self.text_objects.len()),
            ("workflows", self.workflows.len()),
            ("sessions", self.sessions.len()),
            ("effects", self.effects.len()),
            ("terms", self.terms.len()),
            ("marks", self.marks.len()),
            ("tasks", self.tasks.len()),
            ("schedules", self.schedules.len()),
            ("kmacros", self.kmacros.len()),
            ("attests", self.attests.len()),
            ("rulers", self.rulers.len()),
            ("mcp_tools", self.mcp_tools.len()),
        ]
    }

    /// Short human-readable summary — useful for startup banners and
    /// the planned `escriba doctor` subcommand. Derived from
    /// [`counts()`](ApplyPlan::counts) so the two never drift.
    #[must_use]
    pub fn summary(&self) -> String {
        self.counts()
            .iter()
            .map(|(name, n)| format!("{name}={n}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// BLAKE3-128 hex hash of [`Self::content_summary`] — the
    /// content-address of the plan's shape, excluding the attest
    /// count itself so adding a `defattest` doesn't modify the
    /// value it's attesting (a self-reference problem). `defattest
    /// :counts-hash "…"` pins an expected value;
    /// [`Self::evaluate_attests`] compares the live plan against
    /// each declared attestation.
    ///
    /// Same hash shape as [`SnippetSpec::hash`] and mado's clipboard
    /// store — one token format across the stack.
    #[must_use]
    pub fn summary_hash(&self) -> String {
        compute_summary_hash(&self.content_summary())
    }

    /// Summary shape with the `attests` count removed. Used as the
    /// hashable projection for [`Self::summary_hash`]. Keeps the
    /// attestation layer transparent to its own check: a single
    /// `defattest` added after a plan-stable hash won't invalidate
    /// that hash.
    #[must_use]
    pub fn content_summary(&self) -> String {
        self.counts()
            .iter()
            .filter(|(name, _)| *name != "attests")
            .map(|(name, n)| format!("{name}={n}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Run every [`AttestSpec`] this plan declares against the plan's
    /// own live summary hash. Returns `(&AttestSpec, AttestResult)`
    /// pairs so callers can render a per-attestation report.
    ///
    /// The evaluation is deterministic and side-effect-free — the
    /// runtime calls this after `apply_source` succeeds and escalates
    /// based on each spec's [`effective_severity`](AttestSpec::effective_severity).
    #[must_use]
    pub fn evaluate_attests(&self) -> Vec<(&AttestSpec, AttestResult)> {
        let actual = self.summary_hash();
        self.attests
            .iter()
            .map(|spec| (spec, spec.evaluate(&actual)))
            .collect()
    }
}

/// Parse a Lisp source string into an [`ApplyPlan`].
///
/// Validates `defhook :event` against the known-event list and
/// `deftheme :preset` against the known-preset list. Unknown keybind
/// modes are rejected; unknown actions are accepted (forward-compat
/// with new commands registered by plugins).
pub fn apply_source(src: &str) -> LispResult<ApplyPlan> {
    let keybinds: Vec<KeybindSpec> =
        compile(src)?;
    for k in &keybinds {
        validate_mode(&k.mode)?;
    }

    let commands: Vec<CmdSpec> =
        compile(src)?;

    let options: Vec<OptionSpec> =
        compile(src)?;

    let themes: Vec<ThemeSpec> = compile_validated(src, |t: &ThemeSpec| {
        if !t.preset.is_empty() && !is_known_preset(&t.preset) {
            return Err(LispError::UnknownTheme(t.preset.clone()));
        }
        Ok(())
    })?;
    // Last writer wins.
    let theme = themes.into_iter().last();

    let hooks: Vec<HookSpec> = compile_validated(src, |h: &HookSpec| {
        if !is_known_event(&h.event) {
            return Err(LispError::UnknownHook(h.event.clone()));
        }
        Ok(())
    })?;

    let filetypes: Vec<FiletypeSpec> =
        compile(src)?;

    let abbreviations: Vec<AbbrevSpec> =
        compile(src)?;

    let snippets: Vec<SnippetSpec> = compile_validated(src, |s: &SnippetSpec| {
        match s.resolution() {
            SnippetResolution::Inline | SnippetResolution::Hashed => {}
            SnippetResolution::Invalid => {
                return Err(LispError::InvalidSnippetShape(s.trigger.clone()));
            }
        }
        // If `:hash` is set, it must look like a BLAKE3-128 hex
        // token. Catches typos (`:hash "af42"`) at parse time so
        // users see the error in `--list-rc` instead of at expand.
        if s.resolution() == SnippetResolution::Hashed && !s.has_valid_hash_format() {
            return Err(LispError::MalformedSnippetHash(s.hash.clone()));
        }
        Ok(())
    })?;

    let major_modes: Vec<MajorModeSpec> =
        compile(src)?;

    let plugins: Vec<PluginSpec> =
        compile(src)?;

    let highlights: Vec<HighlightSpec> =
        compile(src)?;

    let status_lines: Vec<StatusLineSpec> =
        compile(src)?;
    // Last writer wins — matches theme semantics.
    let status_line = status_lines.into_iter().last();

    let buffer_lines: Vec<BufferLineSpec> =
        compile(src)?;
    let buffer_line = buffer_lines.into_iter().last();

    let lsp_servers: Vec<LspServerSpec> =
        compile(src)?;

    let formatters: Vec<FormatterSpec> =
        compile(src)?;

    let palettes: Vec<PaletteSpec> =
        compile(src)?;

    let icons: Vec<IconSpec> =
        compile(src)?;

    let dap_adapters: Vec<DapAdapterSpec> =
        compile(src)?;

    // Strict validation on gates — ill-formed specs (unknown action /
    // neither command nor source / both set) fail fast so users
    // learn about the mistake at apply time, not at dispatch.
    let gates: Vec<GateSpec> = compile_validated(src, |g: &GateSpec| {
        if !gate::is_known_action(&g.action) {
            return Err(LispError::UnknownGateAction(g.action.clone()));
        }
        if g.mode() == gate::GateMode::Invalid {
            return Err(LispError::InvalidGateShape(g.name.clone()));
        }
        if !g.severity.is_empty() && !gate::is_known_severity(&g.severity) {
            return Err(LispError::UnknownGateSeverity(g.severity.clone()));
        }
        Ok(())
    })?;

    let text_objects: Vec<TextObjectSpec> = compile_validated(src, |t: &TextObjectSpec| {
        if !textobject::is_known_scope(&t.scope) {
            return Err(LispError::UnknownTextObjectScope(t.scope.clone()));
        }
        Ok(())
    })?;

    let workflows: Vec<WorkflowSpec> = compile_validated(src, |w: &WorkflowSpec| {
        if !workflow::is_known_failure_mode(&w.on_failure) {
            return Err(LispError::UnknownWorkflowFailureMode(w.on_failure.clone()));
        }
        Ok(())
    })?;

    // Layout is advisory — unknowns pass through, no error.
    let sessions: Vec<SessionSpec> = compile(src)?;

    let effects: Vec<EffectSpec> = compile_validated(src, |e: &EffectSpec| {
        if e.is_malformed_custom() {
            return Err(LispError::MalformedCustomEffect(e.name.clone()));
        }
        Ok(())
    })?;

    let terms: Vec<TermSpec> = compile_validated(src, |t: &TermSpec| {
        if !term::is_known_placement(&t.placement) {
            return Err(LispError::UnknownTermPlacement(t.placement.clone()));
        }
        Ok(())
    })?;

    let marks: Vec<MarkSpec> = compile_validated(src, |m: &MarkSpec| {
        if !mark::is_known_kind(&m.kind) {
            return Err(LispError::UnknownMarkKind(m.kind.clone()));
        }
        Ok(())
    })?;

    let tasks: Vec<TaskSpec> = compile_validated(src, |t: &TaskSpec| {
        if t.name.is_empty() {
            return Err(LispError::EmptyTaskName);
        }
        if t.command.is_empty() {
            return Err(LispError::EmptyTaskCommand(t.name.clone()));
        }
        Ok(())
    })?;

    let schedules: Vec<ScheduleSpec> = compile_validated(src, |s: &ScheduleSpec| {
        if s.trigger() == schedule::Trigger::Invalid {
            return Err(LispError::InvalidScheduleTrigger(s.name.clone()));
        }
        if s.dispatch() == schedule::Dispatch::Invalid {
            return Err(LispError::InvalidScheduleDispatch(s.name.clone()));
        }
        if s.trigger() == schedule::Trigger::Cron && !s.has_well_shaped_cron() {
            return Err(LispError::MalformedScheduleCron(s.name.clone()));
        }
        Ok(())
    })?;

    let kmacros: Vec<KmacroSpec> = compile_validated(src, |m: &KmacroSpec| {
        if m.keys.is_empty() {
            return Err(LispError::EmptyKmacroKeys(m.name.clone()));
        }
        if !m.mode.is_empty() && !kmacro::is_known_mode(&m.mode) {
            return Err(LispError::UnknownKmacroMode {
                name: m.name.clone(),
                mode: m.mode.clone(),
            });
        }
        if !m.has_valid_register() {
            return Err(LispError::MalformedKmacroRegister(m.name.clone()));
        }
        Ok(())
    })?;

    let rulers: Vec<RulerSpec> = compile_validated(src, |r: &RulerSpec| {
        if r.columns.is_empty() {
            return Err(LispError::EmptyRulerColumns);
        }
        if !r.all_columns_positive() {
            return Err(LispError::ZeroRulerColumn);
        }
        if !r.style.is_empty() && !ruler::is_known_style(&r.style) {
            return Err(LispError::UnknownRulerStyle(r.style.clone()));
        }
        if !r.has_valid_color_format() {
            return Err(LispError::MalformedRulerColor(r.color.clone()));
        }
        Ok(())
    })?;

    let mcp_tools: Vec<McpToolSpec> = compile_validated(src, |m: &McpToolSpec| {
        if m.server.is_empty() {
            return Err(LispError::EmptyMcpServer(m.name.clone()));
        }
        if m.tool.is_empty() {
            return Err(LispError::EmptyMcpTool(m.name.clone()));
        }
        if !m.has_valid_on_result() {
            return Err(LispError::MalformedMcpOnResult {
                name: m.name.clone(),
                value: m.on_result.clone(),
            });
        }
        Ok(())
    })?;

    let attests: Vec<AttestSpec> = compile_validated(src, |a: &AttestSpec| {
        if !a.kind.is_empty() && !attest::is_known_kind(&a.kind) {
            return Err(LispError::UnknownAttestKind {
                name: a.id.clone(),
                kind: a.kind.clone(),
            });
        }
        if !a.severity.is_empty() && !attest::is_known_severity(&a.severity) {
            return Err(LispError::UnknownAttestSeverity {
                name: a.id.clone(),
                severity: a.severity.clone(),
            });
        }
        // Empty hash is legal (stub attestation); otherwise strict hex.
        if !a.is_empty_hash() && !a.has_valid_hash_format() {
            return Err(LispError::MalformedAttestHash(a.id.clone()));
        }
        Ok(())
    })?;

    Ok(ApplyPlan {
        keybinds,
        commands,
        options,
        theme,
        hooks,
        filetypes,
        abbreviations,
        snippets,
        major_modes,
        plugins,
        highlights,
        status_line,
        buffer_line,
        lsp_servers,
        formatters,
        palettes,
        icons,
        dap_adapters,
        gates,
        text_objects,
        workflows,
        sessions,
        effects,
        terms,
        marks,
        tasks,
        schedules,
        kmacros,
        attests,
        rulers,
        mcp_tools,
    })
}

/// Load and parse the rc file at `path`.
pub fn load_rc(path: &Path) -> LispResult<ApplyPlan> {
    let src = std::fs::read_to_string(path).map_err(|e| LispError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    apply_source(&src)
}

/// Resolve the default rc path — `$ESCRIBARC` if set, else
/// `$XDG_CONFIG_HOME/escriba/rc.lisp`, else `$HOME/.escribarc.lisp`.
/// Matches the shape frost uses for `$FROSTRC`.
#[must_use]
pub fn default_rc_path() -> PathBuf {
    if let Ok(p) = std::env::var("ESCRIBARC") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("escriba").join("rc.lisp");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".escribarc.lisp");
    }
    PathBuf::from(".escribarc.lisp")
}

/// `(label, glyph)` pairs for every def-form in [`ApplyPlan::counts`].
///
/// The single source of truth for "every known def-form label" — the
/// binary's `--list-rc` banner, the planned `escriba doctor`, and any
/// MCP schema builder derive their label set from this table rather
/// than from a parallel literal. [`form_glyph`] is a linear search
/// over this constant; [`form_labels`] walks it as an iterator.
///
/// Order matches [`ApplyPlan::counts`] — consumers that render a
/// fixed-order banner can zip the two without sorting.
pub const FORM_GLYPHS: &[(&str, &str)] = &[
    ("keybinds",    "⌨️ "),
    ("cmds",        "⚡"),
    ("options",     "⚙️ "),
    ("theme",       "🎨"),
    ("hooks",       "🪝"),
    ("filetypes",   "📄"),
    ("abbrev",      "✏️ "),
    ("snippets",    "✂️ "),
    ("major_modes", "🎭"),
    ("plugins",     "🧩"),
    ("highlights",  "🌈"),
    ("statusline",  "📊"),
    ("bufferline",  "📑"),
    ("lsp",         "💡"),
    ("formatters",  "📐"),
    ("palettes",    "🖌️ "),
    ("icons",       "🏷️ "),
    ("dap",         "🐛"),
    ("gates",       "🛡️ "),
    ("textobjects", "🎯"),
    ("workflows",   "🧵"),
    ("sessions",    "🗂️ "),
    ("effects",     "✨"),
    ("terms",       "🪟"),
    ("marks",       "📌"),
    ("tasks",       "🏃"),
    ("schedules",   "⏰"),
    ("kmacros",     "🎬"),
    ("attests",     "🔏"),
    ("rulers",      "📏"),
    ("mcp_tools",   "🔌"),
];

/// `(category, glyph)` pairs for plugin `:category` strings — see
/// [`PluginSpec`](crate::PluginSpec) and [`KNOWN_CATEGORIES`]. Same
/// contract as [`FORM_GLYPHS`].
pub const CATEGORY_GLYPHS: &[(&str, &str)] = &[
    ("common",      "📦"),
    ("completion",  "🔤"),
    ("formatting",  "📐"),
    ("keybindings", "⌨️ "),
    ("lsp",         "💡"),
    ("telescope",   "🔭"),
    ("theming",     "🎨"),
    ("tmux",        "⫽ "),
    ("treesitter",  "🌳"),
    ("files",       "📁"),
    ("git",         "🌿"),
    ("ai",          "🤖"),
];

/// Canonical glyph for a def-form label. Labels come from
/// [`ApplyPlan::counts`]; the binary walks `counts()` and looks up
/// glyphs via this function so the `--list-rc` banner stays in sync
/// with the spec surface. Unknown labels fall back to the bullet
/// glyph `•`.
///
/// Lives in `escriba-lisp` (not the binary) so every consumer — the
/// binary, the planned `escriba doctor`, any future TUI / GPU surface
/// — picks up new glyphs automatically when a def-form lands.
#[must_use]
pub fn form_glyph(label: &str) -> &'static str {
    lookup_glyph(FORM_GLYPHS, label, "•")
}

/// Canonical glyph for a plugin category, matching the `:category`
/// strings in [`PluginSpec`](crate::PluginSpec). Same contract as
/// [`form_glyph`] — the single source of truth is [`CATEGORY_GLYPHS`].
#[must_use]
pub fn category_glyph(cat: &str) -> &'static str {
    lookup_glyph(CATEGORY_GLYPHS, cat, "✦")
}

/// Labels of every known def-form in [`FORM_GLYPHS`] order.
///
/// Lets tests + the binary + MCP schema builders enumerate forms
/// without duplicating the label list. Adding a new def-form =
/// one new entry in [`FORM_GLYPHS`]; this iterator picks it up
/// automatically.
pub fn form_labels() -> impl Iterator<Item = &'static str> {
    FORM_GLYPHS.iter().map(|(k, _)| *k)
}

/// Labels of every canonical plugin category in [`CATEGORY_GLYPHS`] order.
pub fn category_labels() -> impl Iterator<Item = &'static str> {
    CATEGORY_GLYPHS.iter().map(|(k, _)| *k)
}

/// Shared linear-search for the two glyph tables. Consumers should
/// call [`form_glyph`] / [`category_glyph`] — this helper only
/// exists to share the match→fallback shape.
fn lookup_glyph(
    table: &'static [(&'static str, &'static str)],
    key: &str,
    fallback: &'static str,
) -> &'static str {
    table
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(fallback)
}

fn validate_mode(mode: &str) -> LispResult<()> {
    if is_known_mode(mode) {
        Ok(())
    } else {
        Err(LispError::UnknownMode(mode.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_yields_empty_plan() {
        let plan = apply_source("").unwrap();
        assert_eq!(plan.keybinds.len(), 0);
        assert_eq!(plan.commands.len(), 0);
        assert_eq!(plan.options.len(), 0);
        assert!(plan.theme.is_none());
        assert_eq!(plan.hooks.len(), 0);
    }

    #[test]
    fn parses_keybinds() {
        let plan = apply_source(
            r#"
            (defkeybind :mode "normal" :key "gh" :action "goto-home")
            (defkeybind :mode "insert" :key "jk" :action "escape")
            "#,
        )
        .unwrap();
        assert_eq!(plan.keybinds.len(), 2);
        assert_eq!(plan.keybinds[0].mode, "normal");
        assert_eq!(plan.keybinds[0].key, "gh");
        assert_eq!(plan.keybinds[1].action, "escape");
    }

    #[test]
    fn rejects_unknown_mode() {
        let err = apply_source(r#"(defkeybind :mode "bogus" :key "g" :action "x")"#)
            .expect_err("unknown mode should error");
        assert!(matches!(err, LispError::UnknownMode(_)));
    }

    #[test]
    fn parses_options_and_theme() {
        let plan = apply_source(
            r#"
            (defoption :name "number"   :value "true")
            (defoption :name "tabstop"  :value "4")
            (deftheme :preset "nord")
            "#,
        )
        .unwrap();
        assert_eq!(plan.options.len(), 2);
        assert_eq!(plan.options[0].name, "number");
        assert_eq!(plan.options[1].value, "4");
        assert_eq!(plan.theme.as_ref().unwrap().preset, "nord");
    }

    #[test]
    fn rejects_unknown_theme_preset() {
        let err = apply_source(r#"(deftheme :preset "laser-unicorn")"#)
            .expect_err("unknown preset should error");
        assert!(matches!(err, LispError::UnknownTheme(_)));
    }

    #[test]
    fn theme_last_writer_wins() {
        let plan = apply_source(
            r#"
            (deftheme :preset "nord")
            (deftheme :preset "gruvbox-dark")
            "#,
        )
        .unwrap();
        assert_eq!(plan.theme.unwrap().preset, "gruvbox-dark");
    }

    #[test]
    fn parses_hooks() {
        let plan = apply_source(
            r#"
            (defhook :event "BufWritePost" :command "run-formatter")
            (defhook :event "ModeChanged"  :to "insert" :command "highlight")
            "#,
        )
        .unwrap();
        assert_eq!(plan.hooks.len(), 2);
        assert_eq!(plan.hooks[1].to, "insert");
    }

    #[test]
    fn rejects_unknown_hook_event() {
        let err = apply_source(
            r#"(defhook :event "UserFired" :command "zap")"#,
        )
        .expect_err("unknown event should error");
        assert!(matches!(err, LispError::UnknownHook(_)));
    }

    #[test]
    fn parses_filetypes_abbrev_snippets_cmd() {
        let plan = apply_source(
            r#"
            (defcmd :name "write-all" :description "write every buffer" :action "buffer.write-all")
            (defft :ext "rs" :mode "rust")
            (defabbrev :trigger "teh" :expansion "the")
            (defsnippet :trigger "fn" :body "fn ${1:name}(${2}) -> ${3} { ${0} }")
            "#,
        )
        .unwrap();
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].name, "write-all");
        assert_eq!(plan.filetypes.len(), 1);
        assert_eq!(plan.filetypes[0].ext, "rs");
        assert_eq!(plan.abbreviations.len(), 1);
        assert_eq!(plan.abbreviations[0].expansion, "the");
        assert_eq!(plan.snippets.len(), 1);
        assert_eq!(plan.snippets[0].trigger, "fn");
    }

    #[test]
    fn plan_merge_concatenates_vectors_and_overwrites_theme() {
        let mut a = apply_source(
            r#"
            (defkeybind :mode "normal" :key "gh" :action "home")
            (deftheme :preset "nord")
            "#,
        )
        .unwrap();
        let b = apply_source(
            r#"
            (defkeybind :mode "normal" :key "gl" :action "end")
            (deftheme :preset "gruvbox-dark")
            "#,
        )
        .unwrap();
        a.merge(b);
        assert_eq!(a.keybinds.len(), 2);
        assert_eq!(a.theme.unwrap().preset, "gruvbox-dark");
    }

    #[test]
    fn summary_shape_is_useful() {
        let plan = apply_source(
            r#"
            (defkeybind :mode "normal" :key "g" :action "x")
            (defcmd :name "c" :action "x")
            (deftheme :preset "nord")
            (defhook :event "BufEnter" :command "c")
            "#,
        )
        .unwrap();
        let s = plan.summary();
        assert!(s.contains("keybinds=1"));
        assert!(s.contains("cmds=1"));
        assert!(s.contains("theme=1"));
        assert!(s.contains("hooks=1"));
    }

    #[test]
    fn form_glyph_defined_for_every_count_label() {
        // Contract test: every label `counts()` emits must resolve to
        // a non-bullet glyph. Catches the "added a def-form but
        // forgot to extend form_glyph" regression at the typed edge.
        let plan = apply_source("").unwrap();
        for (label, _) in plan.counts() {
            let g = form_glyph(label);
            assert_ne!(
                g, "•",
                "form_glyph missing entry for def-form label {label:?}",
            );
        }
    }

    #[test]
    fn category_glyph_defined_for_canonical_categories() {
        // KNOWN_CATEGORIES (from plugin.rs) and the category_glyph
        // table must stay aligned. An unknown category falls back
        // to `✦`; anything in KNOWN_CATEGORIES should resolve to a
        // real glyph.
        for cat in KNOWN_CATEGORIES {
            let g = category_glyph(cat);
            assert_ne!(
                g, "✦",
                "category_glyph missing entry for canonical category {cat:?}",
            );
        }
    }

    #[test]
    fn compile_validated_short_circuits_on_first_failure() {
        // Validator errors should bubble up immediately — the helper
        // must not keep walking after the first failed spec (side-effect
        // validators can depend on this).
        let src = r#"
            (defkeybind :mode "normal" :key "a" :action "x")
            (defkeybind :mode "normal" :key "b" :action "x")
            (defkeybind :mode "normal" :key "c" :action "x")
        "#;
        let mut seen = 0usize;
        let err = compile_validated(src, |_: &KeybindSpec| -> LispResult<()> {
            seen += 1;
            if seen >= 2 {
                Err(LispError::UnknownMode("stop-here".into()))
            } else {
                Ok(())
            }
        })
        .expect_err("validator error should propagate");
        assert!(matches!(err, LispError::UnknownMode(_)));
        // Walked exactly 2 — the one that succeeded + the one that
        // errored. Not 3.
        assert_eq!(seen, 2);
    }

    #[test]
    fn counts_is_the_single_source_of_truth() {
        // counts() and summary() must never drift — summary is
        // literally counts formatted. Prove it by reconstructing
        // the string from counts() and comparing.
        let plan = apply_source(
            r#"
            (defkeybind :mode "normal" :key "g" :action "x")
            (deftheme :preset "nord")
            (defgate :name "g1" :on-event "BufWritePre" :command "ok" :action "warn")
            "#,
        )
        .unwrap();

        let from_counts: String = plan
            .counts()
            .iter()
            .map(|(n, c)| format!("{n}={c}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(plan.summary(), from_counts);

        // Every label registered in FORM_GLYPHS appears in counts()
        // — adding a new spec means one entry in each, and nothing
        // else. No third hardcoded list needs updating.
        let names: Vec<&str> = plan.counts().iter().map(|(n, _)| *n).collect();
        for required in form_labels() {
            assert!(
                names.contains(&required),
                "counts() missing required def-form: {required}",
            );
        }
    }

    #[test]
    fn counts_order_matches_form_glyphs_order() {
        // Banner-stability contract: consumers that zip counts()
        // against FORM_GLYPHS (for fixed-order rendering) should
        // never see the two tables drift.
        let plan = apply_source("").unwrap();
        let counts_labels: Vec<&str> = plan.counts().iter().map(|(n, _)| *n).collect();
        let glyph_labels: Vec<&str> = form_labels().collect();
        assert_eq!(
            counts_labels, glyph_labels,
            "counts() order diverged from FORM_GLYPHS order",
        );
    }

    #[test]
    fn form_glyphs_has_no_duplicate_labels() {
        // Linear search short-circuits on the first hit, so a dupe
        // would silently mask later entries. Catch that here.
        let mut seen = std::collections::HashSet::new();
        for (label, _) in FORM_GLYPHS {
            assert!(
                seen.insert(*label),
                "duplicate label in FORM_GLYPHS: {label}",
            );
        }
    }

    #[test]
    fn category_glyphs_has_no_duplicate_labels() {
        let mut seen = std::collections::HashSet::new();
        for (cat, _) in CATEGORY_GLYPHS {
            assert!(
                seen.insert(*cat),
                "duplicate category in CATEGORY_GLYPHS: {cat}",
            );
        }
    }

    #[test]
    fn unknown_labels_fall_back_to_sentinel_glyphs() {
        // Contract: unknown form label → "•", unknown category → "✦".
        // Callers (binary banners) key off this to decide whether a
        // label is recognized.
        assert_eq!(form_glyph("nonexistent-form"), "•");
        assert_eq!(form_glyph(""), "•");
        assert_eq!(category_glyph("nonexistent-category"), "✦");
        assert_eq!(category_glyph(""), "✦");
    }

    #[test]
    fn merge_is_total_on_every_def_form() {
        // The merge() implementation must touch every field — an
        // orphaned field would silently drop on merge. Prove it by
        // merging two plans with one item per form and checking the
        // combined plan's counts for each.
        let mut a = apply_source(
            r##"
            (defkeybind :mode "normal" :key "a" :action "x")
            (defcmd     :name "a-cmd"  :action "x")
            (defoption  :name "a-opt"  :value "1")
            (deftheme   :preset "nord")
            (defhook    :event "BufEnter" :command "a")
            (defft      :ext "a" :mode "plain")
            (defabbrev  :trigger "ax" :expansion "ay")
            (defsnippet :trigger "a"  :body "${1}")
            (defmode    :name "a-lang" :extensions ("a"))
            (defplugin  :name "a-plug" :category "common")
            (defhighlight :group "Normal" :fg "#ff0000")
            (defstatusline :left ((:segment "mode")))
            (defbufferline :separator "|")
            (deflsp     :name "a-ls"  :command "a-ls" :filetypes ("a-lang"))
            (defformatter :filetype "a-lang" :command "a-fmt")
            (defpalette :name "a-pal" :base00 "#000000")
            (deficon    :filetype "a-lang" :glyph "∷")
            (defdap     :name "a-dap" :command "a-dap" :filetypes ("a-lang"))
            (defgate    :name "a-gate" :on-event "BufWritePre" :command "echo" :action "warn")
            (deftextobject :name "f" :scope "outer" :query "(x) @f")
            (defworkflow :name "a-wf" :steps ("gate:a-gate"))
            (defsession  :name "a-sess" :buffers ("a.rs"))
            (defeffect   :name "a-fx" :kind "cursor" :enable #t :intensity 0.5)
            (defterm     :name "a-term" :shell "/bin/frost" :placement "tab")
            (defmark     :name "'A" :file "~/README.md" :line 1 :kind "jump")
            (deftask     :name "a-task" :command "ls")
            (defschedule :name "a-sched" :interval-seconds 60 :command "save")
            (defkmacro   :name "a-macro" :keys "iHello<Esc>")
            (defattest   :id "a-attest" :counts-hash "af42c0d18e9b3f4aa18b7c3ef1de93a4")
            (defruler    :columns (80))
            (defmcp      :name "a-mcp" :server "mado" :tool "status")
            "##,
        )
        .unwrap();
        let b = a.clone();
        a.merge(b);
        // Each form should have doubled (except last-writer-wins
        // singletons: theme, statusline, bufferline which stay 1).
        for (name, count) in a.counts() {
            let expected = match name {
                "theme" | "statusline" | "bufferline" => 1,
                _ => 2,
            };
            assert_eq!(count, expected, "{name} did not double on self-merge");
        }
    }

    #[test]
    fn default_rc_path_is_nonempty() {
        assert!(!default_rc_path().as_os_str().is_empty());
    }

    #[test]
    fn parses_major_modes() {
        let plan = apply_source(
            r#"
            (defmode :name "rust"
                     :extensions ("rs")
                     :tree-sitter "rust"
                     :commentstring "// %s"
                     :indent 4)
            (defmode :name "lisp"
                     :extensions ("lisp" "cl" "el")
                     :tree-sitter "commonlisp"
                     :commentstring ";; %s"
                     :indent 2
                     :structural-lisp #t)
            "#,
        )
        .unwrap();
        assert_eq!(plan.major_modes.len(), 2);
        assert_eq!(plan.major_modes[0].name, "rust");
        assert_eq!(plan.major_modes[0].extensions, vec!["rs"]);
        assert_eq!(plan.major_modes[0].tree_sitter, "rust");
        assert_eq!(plan.major_modes[0].indent, 4);
        assert!(!plan.major_modes[0].structural_lisp);

        assert_eq!(plan.major_modes[1].name, "lisp");
        assert_eq!(
            plan.major_modes[1].extensions,
            vec!["lisp", "cl", "el"]
        );
        assert!(plan.major_modes[1].structural_lisp);
    }

    #[test]
    fn parses_plugins_with_lazy_triggers() {
        let plan = apply_source(
            r#"
            (defplugin :name "trouble"
                       :description "Diagnostic list UI"
                       :category "lsp"
                       :on-event "LspAttach"
                       :lazy #t)
            (defplugin :name "oil"
                       :category "files"
                       :on-command "Oil"
                       :keybinds ("<leader>e")
                       :lazy #t)
            (defplugin :name "nord"
                       :category "theming"
                       :priority 1000)
            "#,
        )
        .unwrap();
        assert_eq!(plan.plugins.len(), 3);
        assert_eq!(plan.plugins[0].name, "trouble");
        assert_eq!(plan.plugins[0].on_event, "LspAttach");
        assert!(plan.plugins[0].lazy);
        assert_eq!(plan.plugins[1].keybinds, vec!["<leader>e"]);
        assert_eq!(plan.plugins[2].priority, 1000);
    }

    #[test]
    fn parses_highlights_with_attrs_and_links() {
        let plan = apply_source(
            r##"
            (defhighlight :group "Function" :fg "#88c0d0" :bold #t)
            (defhighlight :group "Comment"  :fg "#4c566a" :italic #t)
            (defhighlight :group "@function.call" :link "Function")
            (defhighlight :group "DiagnosticError" :fg "#bf616a" :bg "#2e3440" :bold #t :undercurl #t)
            "##,
        )
        .unwrap();
        assert_eq!(plan.highlights.len(), 4);
        assert_eq!(plan.highlights[0].group, "Function");
        assert_eq!(plan.highlights[0].fg, "#88c0d0");
        assert!(plan.highlights[0].bold);
        assert!(plan.highlights[1].italic);
        assert!(plan.highlights[2].is_link());
        assert_eq!(plan.highlights[2].link, "Function");
        assert!(plan.highlights[3].has_attrs());
        assert!(plan.highlights[3].undercurl);
    }

    #[test]
    fn parses_status_line_with_three_alignment_slots() {
        let plan = apply_source(
            r#"
            (defstatusline
              :left ((:segment "mode")
                     (:segment "branch" :prefix "  "))
              :center ((:segment "file" :highlight "StatusLineFile"))
              :right ((:segment "diagnostics")
                      (:segment "time" :format "%H:%M")))
            "#,
        )
        .unwrap();
        let sl = plan.status_line.expect("defstatusline should produce a spec");
        assert_eq!(sl.left.len(), 2);
        assert_eq!(sl.center.len(), 1);
        assert_eq!(sl.right.len(), 2);
        assert_eq!(sl.segment_count(), 5);
        assert_eq!(sl.left[0].segment, "mode");
        assert_eq!(sl.left[1].prefix, "  ");
        assert_eq!(sl.right[1].format, "%H:%M");
    }

    #[test]
    fn status_line_last_writer_wins() {
        let plan = apply_source(
            r#"
            (defstatusline :left ((:segment "mode")))
            (defstatusline :right ((:segment "time")))
            "#,
        )
        .unwrap();
        let sl = plan.status_line.unwrap();
        // Last writer wins entirely — left is empty because the
        // second form replaced the first, not merged into it.
        assert!(sl.left.is_empty());
        assert_eq!(sl.right.len(), 1);
    }

    #[test]
    fn parses_lsp_servers() {
        let plan = apply_source(
            r#"
            (deflsp :name "rust-analyzer"
                    :command "rust-analyzer"
                    :filetypes ("rust")
                    :root-markers ("Cargo.toml" "rust-project.json"))
            (deflsp :name "typescript"
                    :command "typescript-language-server"
                    :args ("--stdio")
                    :filetypes ("typescript" "javascript")
                    :manual-only #t)
            "#,
        )
        .unwrap();
        assert_eq!(plan.lsp_servers.len(), 2);
        assert_eq!(plan.lsp_servers[0].name, "rust-analyzer");
        assert_eq!(plan.lsp_servers[0].filetypes, vec!["rust"]);
        assert_eq!(
            plan.lsp_servers[0].root_markers,
            vec!["Cargo.toml", "rust-project.json"]
        );
        assert_eq!(plan.lsp_servers[1].args, vec!["--stdio"]);
        // Default polarity: `manual_only` is false → auto-attach on.
        assert!(!plan.lsp_servers[0].manual_only);
        assert!(plan.lsp_servers[1].manual_only);
    }

    #[test]
    fn parses_formatters_and_honours_defaults() {
        let plan = apply_source(
            r#"
            (defformatter :filetype "rust"    :command "rustfmt")
            (defformatter :filetype "python"  :command "ruff"
                          :args ("format" "-"))
            (defformatter :filetype "lua"     :command "stylua"
                          :manual-only #t)
            "#,
        )
        .unwrap();
        assert_eq!(plan.formatters.len(), 3);
        assert_eq!(plan.formatters[0].filetype, "rust");
        // Default polarity: `manual_only` false → format-on-save on.
        assert!(!plan.formatters[0].manual_only);
        assert_eq!(plan.formatters[1].args, vec!["format", "-"]);
        assert!(plan.formatters[2].manual_only);
    }

    #[test]
    fn parses_base16_palette() {
        let plan = apply_source(
            r##"
            (defpalette :name "gruvbox-dark-soft"
                        :base00 "#32302f" :base01 "#3c3836"
                        :base05 "#d5c4a1"
                        :base08 "#fb4934" :base0b "#b8bb26"
                        :base0d "#83a598")
            "##,
        )
        .unwrap();
        assert_eq!(plan.palettes.len(), 1);
        let p = &plan.palettes[0];
        assert_eq!(p.name, "gruvbox-dark-soft");
        assert_eq!(p.base00, "#32302f");
        assert_eq!(p.base05, "#d5c4a1");
        assert_eq!(p.base08, "#fb4934");
        assert_eq!(p.base0b, "#b8bb26");
        assert_eq!(p.base0d, "#83a598");
        // Unspecified fields empty.
        assert!(p.base07.is_empty());
    }

    #[test]
    fn parses_icons_with_both_binding_styles() {
        let plan = apply_source(
            r##"
            (deficon :filetype "rust"   :glyph "" :fg "#dea584")
            (deficon :filetype "python" :glyph "" :fg "#ffbc03")
            (deficon :pattern  ".envrc" :glyph "" :fg "#89e051")
            "##,
        )
        .unwrap();
        assert_eq!(plan.icons.len(), 3);
        assert_eq!(plan.icons[0].filetype, "rust");
        assert!(!plan.icons[0].is_pattern());
        assert!(plan.icons[2].is_pattern());
        assert_eq!(plan.icons[2].pattern, ".envrc");
    }

    #[test]
    fn hook_event_vocabulary_includes_nvim_canon() {
        // The expanded KNOWN_EVENTS table must now contain the
        // events blnvim configs reach for most often.
        assert!(is_known_event("BufReadPost"));
        assert!(is_known_event("LspAttach"));
        assert!(is_known_event("InsertLeave"));
        assert!(is_known_event("TextYankPost"));
        assert!(is_known_event("CursorHold"));
        assert!(is_known_event("ColorScheme"));
        assert!(is_known_event("TermOpen"));
        assert!(is_known_event("CmdlineEnter"));
        assert!(is_known_event("FileType"));
        // Unknown values stay rejected.
        assert!(!is_known_event("BufGalactus"));
    }

    #[test]
    fn parses_hash_referenced_snippet() {
        let plan = apply_source(
            r#"
            (defsnippet :trigger "deploy"
                        :hash "af42c0d18e9b3f4aa18b7c3ef1de93a4"
                        :description "Team deploy command — pasted from mado")
            "#,
        )
        .unwrap();
        assert_eq!(plan.snippets.len(), 1);
        assert_eq!(plan.snippets[0].resolution(), SnippetResolution::Hashed);
        assert!(plan.snippets[0].has_valid_hash_format());
        assert!(plan.snippets[0].body.is_empty());
    }

    #[test]
    fn snippet_without_body_or_hash_rejected() {
        let err = apply_source(r#"(defsnippet :trigger "oops")"#)
            .expect_err("must have body or hash");
        assert!(matches!(err, LispError::InvalidSnippetShape(_)));
    }

    #[test]
    fn snippet_with_both_body_and_hash_rejected() {
        let err = apply_source(
            r#"(defsnippet :trigger "both"
                           :body "x"
                           :hash "af42c0d18e9b3f4aa18b7c3ef1de93a4")"#,
        )
        .expect_err("exactly one of body/hash");
        assert!(matches!(err, LispError::InvalidSnippetShape(_)));
    }

    #[test]
    fn snippet_with_malformed_hash_rejected() {
        let err = apply_source(
            r#"(defsnippet :trigger "x" :hash "af42")"#,
        )
        .expect_err("hash must be 32-char hex");
        assert!(matches!(err, LispError::MalformedSnippetHash(_)));

        let err = apply_source(
            r#"(defsnippet :trigger "x" :hash "AF42C0D18E9B3F4AA18B7C3EF1DE93A4")"#,
        )
        .expect_err("hash must be lowercase");
        assert!(matches!(err, LispError::MalformedSnippetHash(_)));
    }

    #[test]
    fn parses_marks_with_three_kinds() {
        let plan = apply_source(
            r#"
            (defmark :name "'C"
                     :file "~/.config/escriba/rc.lisp"
                     :line 1
                     :kind "jump")
            (defmark :name "bug-notes"
                     :file "~/notes/bugs.md"
                     :line 42 :column 7
                     :kind "anchor"
                     :description "Flaky tests")
            (defmark :name "'S"
                     :file "~/README.md"
                     :kind "glance")
            "#,
        )
        .unwrap();
        assert_eq!(plan.marks.len(), 3);
        assert_eq!(plan.marks[0].effective_kind(), "jump");
        assert_eq!(plan.marks[1].kind, "anchor");
        assert_eq!(plan.marks[1].column, 7);
        assert_eq!(plan.marks[2].kind, "glance");
        assert!(plan.marks[0].is_vim_single_letter());
        assert!(!plan.marks[1].is_vim_single_letter());
    }

    #[test]
    fn mark_with_unknown_kind_rejected() {
        let err = apply_source(
            r#"(defmark :name "'X" :kind "teleport")"#,
        )
        .expect_err("unknown kind should error");
        assert!(matches!(err, LispError::UnknownMarkKind(_)));
    }

    #[test]
    fn mark_empty_kind_resolves_to_jump() {
        let plan = apply_source(
            r#"(defmark :name "'X" :file "~/a.txt")"#,
        )
        .unwrap();
        assert_eq!(plan.marks[0].effective_kind(), "jump");
    }

    #[test]
    fn parses_terms_with_wire_compatible_fields() {
        let plan = apply_source(
            r#"
            (defterm :name "dev"
                     :shell "/bin/frost"
                     :cwd "~/code"
                     :placement "split-horizontal"
                     :env ("RUST_LOG=info" "CARGO_TERM_COLOR=always")
                     :effects ("cursor-glow" "bloom")
                     :keybind "<leader>td")
            (defterm :name "attach"
                     :attach "pane-42"
                     :keybind "<leader>ta")
            "#,
        )
        .unwrap();
        assert_eq!(plan.terms.len(), 2);
        assert_eq!(plan.terms[0].shell, "/bin/frost");
        assert_eq!(plan.terms[0].placement, "split-horizontal");
        assert_eq!(plan.terms[0].effects, vec!["cursor-glow", "bloom"]);
        assert_eq!(plan.terms[0].env_pairs().len(), 2);
        assert!(plan.terms[1].is_attach());
    }

    #[test]
    fn term_with_unknown_placement_rejected() {
        let err = apply_source(
            r#"(defterm :name "x" :placement "zigzag")"#,
        )
        .expect_err("unknown placement should error");
        assert!(matches!(err, LispError::UnknownTermPlacement(_)));
    }

    #[test]
    fn term_mcp_payload_round_trips_through_json() {
        // Cross-repo contract test: `TermSpec::to_mcp_value()` must
        // produce exactly the shape mado's spawn_term tool accepts.
        // This asserts the field names — if either side renames,
        // the test fires.
        let plan = apply_source(
            r#"(defterm :name "x" :shell "zsh" :placement "window"
                        :env ("FOO=bar"))"#,
        )
        .unwrap();
        let payload = plan.terms[0].to_mcp_value();
        for key in ["shell", "args", "cwd", "env", "title", "placement", "attach", "effects"] {
            assert!(
                payload.get(key).is_some(),
                "mado-contract field `{key}` missing from MCP payload",
            );
        }
    }

    #[test]
    fn parses_sessions_with_buffers_and_on_enter() {
        let plan = apply_source(
            r#"
            (defsession :name "escriba-dev"
                        :description "escriba-lisp day"
                        :buffers ("escriba-lisp/src/lib.rs"
                                  "escriba-lisp/src/apply.rs")
                        :layout "horizontal"
                        :cwd "~/code/github/pleme-io/escriba"
                        :on-enter ("workflow:save-and-test"))
            (defsession :name "blog"
                        :buffers ("posts/latest.md")
                        :layout "single")
            "#,
        )
        .unwrap();
        assert_eq!(plan.sessions.len(), 2);
        assert_eq!(plan.sessions[0].name, "escriba-dev");
        assert_eq!(plan.sessions[0].pane_count(), 2);
        assert!(plan.sessions[0].has_buffers());
        assert_eq!(plan.sessions[1].layout, "single");
        assert_eq!(plan.sessions[1].pane_count(), 1);
    }

    #[test]
    fn parses_effects_with_canonical_names() {
        let plan = apply_source(
            r##"
            (defeffect :name "cursor-glow"
                       :kind "cursor"
                       :enable #t
                       :intensity 0.6
                       :radius 1.8
                       :color "#88c0d0")
            (defeffect :name "bloom"
                       :kind "screen"
                       :enable #t
                       :intensity 0.25
                       :threshold 0.75)
            (defeffect :name "scanlines"
                       :kind "screen"
                       :enable #f
                       :intensity 0.15)
            "##,
        )
        .unwrap();
        assert_eq!(plan.effects.len(), 3);
        assert_eq!(plan.effects[0].name, "cursor-glow");
        assert!(plan.effects[0].enable);
        assert_eq!(plan.effects[0].intensity, 0.6);
        assert_eq!(plan.effects[1].threshold, 0.75);
        assert!(!plan.effects[2].enable);
    }

    #[test]
    fn effect_custom_without_shader_rejected() {
        let err = apply_source(
            r##"(defeffect :name "mine" :kind "custom" :enable #t)"##,
        )
        .expect_err("custom w/o shader should error");
        assert!(matches!(err, LispError::MalformedCustomEffect(_)));
    }

    #[test]
    fn parses_workflows_with_step_grammar() {
        let plan = apply_source(
            r#"
            (defworkflow :name "ship-rust"
                         :description "Format, test, push"
                         :steps ("gate:rust-format-drift"
                                 "shell:cargo test"
                                 "action:git.push")
                         :on-failure "abort"
                         :keybind "<leader>ws")
            (defworkflow :name "chore"
                         :steps ("cmd:write-all"
                                 "workflow:ship-rust")
                         :on-failure "prompt")
            "#,
        )
        .unwrap();
        assert_eq!(plan.workflows.len(), 2);
        assert_eq!(plan.workflows[0].name, "ship-rust");
        assert_eq!(plan.workflows[0].steps.len(), 3);
        assert_eq!(plan.workflows[0].on_failure, "abort");
        assert_eq!(plan.workflows[0].keybind, "<leader>ws");
        assert_eq!(
            plan.workflows[0].step_kinds(),
            vec!["gate", "shell", "action"]
        );
        assert_eq!(
            plan.workflows[1].step_kinds(),
            vec!["cmd", "workflow"]
        );
        assert!(plan.workflows[0].all_steps_known());
    }

    #[test]
    fn workflow_with_unknown_failure_mode_rejected() {
        let err = apply_source(
            r#"(defworkflow :name "x" :steps () :on-failure "explode")"#,
        )
        .expect_err("unknown failure mode should error");
        assert!(matches!(err, LispError::UnknownWorkflowFailureMode(_)));
    }

    #[test]
    fn workflow_on_failure_empty_is_default_abort() {
        let plan = apply_source(
            r#"(defworkflow :name "x" :steps ("gate:g"))"#,
        )
        .unwrap();
        assert_eq!(plan.workflows.len(), 1);
        assert_eq!(plan.workflows[0].on_failure, "");
    }

    #[test]
    fn parses_textobjects_with_both_scopes() {
        let plan = apply_source(
            r#"
            (deftextobject :name "f"
                           :scope "outer"
                           :filetype "rust"
                           :query "(function_item) @function.outer")
            (deftextobject :name "f"
                           :scope "inner"
                           :filetype "rust"
                           :query "(function_item body: (block) @function.inner)")
            (deftextobject :name "c"
                           :scope "outer"
                           :filetype "python"
                           :query "(class_definition) @class.outer")
            "#,
        )
        .unwrap();
        assert_eq!(plan.text_objects.len(), 3);
        assert_eq!(plan.text_objects[0].name, "f");
        assert_eq!(plan.text_objects[0].scope, "outer");
        assert_eq!(plan.text_objects[1].scope, "inner");
        assert_eq!(plan.text_objects[2].filetype, "python");
        assert!(plan.text_objects[0].query.contains("function_item"));
    }

    #[test]
    fn textobject_with_unknown_scope_rejected() {
        let err = apply_source(
            r#"(deftextobject :name "f" :scope "around" :query "x")"#,
        )
        .expect_err("unknown scope should error");
        assert!(matches!(err, LispError::UnknownTextObjectScope(_)));
    }

    #[test]
    fn parses_gates_with_both_modes() {
        let plan = apply_source(
            r#"
            (defgate :name "rust-format"
                     :on-event "BufWritePre"
                     :filetype "rust"
                     :command "rustfmt --check $FILE"
                     :action "auto-fix"
                     :auto-fix "rustfmt $FILE")
            (defgate :name "lsp-clean"
                     :on-event "BufWritePost"
                     :source "lsp.diagnostics"
                     :severity "error"
                     :action "warn")
            "#,
        )
        .unwrap();
        assert_eq!(plan.gates.len(), 2);
        assert_eq!(plan.gates[0].mode(), GateMode::Command);
        assert_eq!(plan.gates[1].mode(), GateMode::Source);
        assert_eq!(plan.gates[0].action, "auto-fix");
        assert_eq!(plan.gates[0].auto_fix, "rustfmt $FILE");
        assert_eq!(plan.gates[1].severity, "error");
    }

    #[test]
    fn gate_with_unknown_action_rejected() {
        let err = apply_source(
            r#"(defgate :name "x" :on-event "BufWritePre" :command "echo" :action "panic")"#,
        )
        .expect_err("unknown action should error");
        assert!(matches!(err, LispError::UnknownGateAction(_)));
    }

    #[test]
    fn gate_with_neither_command_nor_source_rejected() {
        let err = apply_source(
            r#"(defgate :name "x" :on-event "BufWritePre" :action "reject")"#,
        )
        .expect_err("neither command nor source is invalid");
        assert!(matches!(err, LispError::InvalidGateShape(_)));
    }

    #[test]
    fn gate_with_both_command_and_source_rejected() {
        let err = apply_source(
            r#"(defgate :name "x" :on-event "BufWritePre"
                        :command "echo" :source "lsp.diagnostics" :action "warn")"#,
        )
        .expect_err("both command and source is invalid");
        assert!(matches!(err, LispError::InvalidGateShape(_)));
    }

    #[test]
    fn gate_with_unknown_severity_rejected() {
        let err = apply_source(
            r#"(defgate :name "x" :on-event "BufWritePost"
                        :source "lsp.diagnostics" :severity "yelling"
                        :action "warn")"#,
        )
        .expect_err("unknown severity should error");
        assert!(matches!(err, LispError::UnknownGateSeverity(_)));
    }

    #[test]
    fn parses_dap_adapters() {
        let plan = apply_source(
            r#"
            (defdap :name "lldb"
                    :command "lldb-dap"
                    :filetypes ("rust" "c" "cpp"))
            (defdap :name "delve"
                    :command "dlv"
                    :args ("dap" "-l" "127.0.0.1:38697")
                    :filetypes ("go")
                    :port 38697)
            "#,
        )
        .unwrap();
        assert_eq!(plan.dap_adapters.len(), 2);
        assert_eq!(plan.dap_adapters[0].name, "lldb");
        assert_eq!(plan.dap_adapters[0].filetypes, vec!["rust", "c", "cpp"]);
        assert_eq!(plan.dap_adapters[0].port, 0);
        assert_eq!(plan.dap_adapters[1].port, 38697);
        assert_eq!(plan.dap_adapters[1].args, vec!["dap", "-l", "127.0.0.1:38697"]);
    }

    #[test]
    fn parses_buffer_line() {
        let plan = apply_source(
            r#"
            (defbufferline :separator "|"
                           :modified-indicator "●"
                           :show-diagnostics #t
                           :max-name-length 20)
            "#,
        )
        .unwrap();
        let bl = plan.buffer_line.unwrap();
        assert_eq!(bl.separator, "|");
        assert_eq!(bl.modified_indicator, "●");
        assert!(bl.show_diagnostics);
        assert_eq!(bl.max_name_length, 20);
        // Default polarity: no_icons false → icons shown; no_click
        // false → clicks focus the tab.
        assert!(!bl.no_icons);
        assert!(!bl.no_click);
    }

    #[test]
    fn summary_includes_major_modes_count() {
        let plan = apply_source(
            r#"
            (defmode :name "rust" :extensions ("rs"))
            (defmode :name "py"   :extensions ("py"))
            (defmode :name "lisp" :extensions ("lisp"))
            "#,
        )
        .unwrap();
        assert!(plan.summary().contains("major_modes=3"));
    }

    #[test]
    fn parses_tasks_with_filetype_and_env() {
        let plan = apply_source(
            r#"
            (deftask :name "cargo-test"
                     :description "cargo test --workspace"
                     :command "cargo"
                     :args ("test" "--workspace")
                     :filetype "rust"
                     :keybind "<leader>rt")
            (deftask :name "fleet-rebuild"
                     :command "nix"
                     :args ("run" ".#rebuild")
                     :cwd "~/code/github/pleme-io/nix"
                     :env ("RUST_LOG=warn")
                     :background #t
                     :keybind "<leader>rR"
                     :timeout-ms 300000)
            "#,
        )
        .unwrap();
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].name, "cargo-test");
        assert_eq!(plan.tasks[0].args, vec!["test", "--workspace"]);
        assert_eq!(plan.tasks[0].filetype, "rust");
        assert_eq!(plan.tasks[0].display_command(), "cargo test --workspace");
        assert!(plan.tasks[1].background);
        assert_eq!(plan.tasks[1].env_pairs().len(), 1);
        assert_eq!(plan.tasks[1].timeout_ms, 300_000);
    }

    #[test]
    fn task_with_empty_command_rejected() {
        let err = apply_source(
            r#"(deftask :name "oops")"#,
        )
        .expect_err("empty command should error");
        assert!(matches!(err, LispError::EmptyTaskCommand(_)));
    }

    #[test]
    fn task_with_empty_name_rejected() {
        // Tatara-lisp enforces `:name` at parse time, so the empty-name
        // path only triggers when the user writes `:name ""` explicitly.
        let err = apply_source(
            r#"(deftask :name "" :command "ls")"#,
        )
        .expect_err("empty name should error");
        assert!(matches!(err, LispError::EmptyTaskName));
    }

    #[test]
    fn task_without_name_at_all_is_parse_error() {
        // `:name` is a required key — tatara-lisp rejects the form
        // before our validator runs.
        let err = apply_source(
            r#"(deftask :command "ls")"#,
        )
        .expect_err("missing :name should parse-error");
        assert!(matches!(err, LispError::Parse(_)));
    }

    // ── defschedule — typed declarative triggers ────────────────────────────

    #[test]
    fn parses_schedules_across_every_trigger_kind() {
        let plan = apply_source(
            r#"
            (defschedule :name "hourly-pull"
                         :description "top-of-hour git pull"
                         :cron "0 * * * *"
                         :command "git.pull")
            (defschedule :name "refresh-diag"
                         :interval-seconds 300
                         :workflow "diagnostics-refresh")
            (defschedule :name "autosave"
                         :idle-seconds 30
                         :command "save-all")
            (defschedule :name "banner"
                         :at-startup #t
                         :action "picker.banner")
            (defschedule :name "kick-refresh"
                         :workflow "diagnostics-refresh"
                         :keybind "<leader>dr")
            "#,
        )
        .unwrap();
        assert_eq!(plan.schedules.len(), 5);
        assert_eq!(plan.schedules[0].trigger(), ScheduleTrigger::Cron);
        assert_eq!(plan.schedules[1].trigger(), ScheduleTrigger::Interval);
        assert_eq!(plan.schedules[2].trigger(), ScheduleTrigger::Idle);
        assert_eq!(plan.schedules[3].trigger(), ScheduleTrigger::Startup);
        // Last one is manual — no auto trigger, just a keybind.
        assert_eq!(plan.schedules[4].trigger(), ScheduleTrigger::Manual);
        assert!(!plan.schedules[4].is_automatic());
        assert_eq!(plan.schedules[0].trigger_label(), "cron:0 * * * *");
        assert_eq!(plan.schedules[1].trigger_label(), "interval:300s");
        assert_eq!(plan.schedules[3].trigger_label(), "startup");
    }

    #[test]
    fn schedule_with_two_triggers_rejected() {
        let err = apply_source(
            r#"(defschedule :name "x"
                            :cron "0 * * * *"
                            :interval-seconds 60
                            :command "save")"#,
        )
        .expect_err("multiple triggers should error");
        assert!(matches!(err, LispError::InvalidScheduleTrigger(_)));
    }

    #[test]
    fn schedule_with_no_dispatch_rejected() {
        let err = apply_source(
            r#"(defschedule :name "x" :interval-seconds 60)"#,
        )
        .expect_err("missing dispatch should error");
        assert!(matches!(err, LispError::InvalidScheduleDispatch(_)));
    }

    #[test]
    fn schedule_with_multiple_dispatch_rejected() {
        let err = apply_source(
            r#"(defschedule :name "x"
                            :interval-seconds 60
                            :command "a"
                            :workflow "b")"#,
        )
        .expect_err("multiple dispatch targets should error");
        assert!(matches!(err, LispError::InvalidScheduleDispatch(_)));
    }

    #[test]
    fn schedule_with_malformed_cron_rejected() {
        let err = apply_source(
            r#"(defschedule :name "x" :cron "garbage" :command "save")"#,
        )
        .expect_err("malformed cron should error");
        assert!(matches!(err, LispError::MalformedScheduleCron(_)));
    }

    #[test]
    fn manual_only_schedule_requires_no_trigger() {
        // The manual-only shape (no trigger fields set, just :keybind
        // plus a dispatch) is explicitly valid — used for wiring a
        // dispatch target the user kicks on demand.
        let plan = apply_source(
            r#"(defschedule :name "kick"
                            :command "format-buffer"
                            :keybind "<leader>kf")"#,
        )
        .unwrap();
        assert_eq!(plan.schedules.len(), 1);
        assert_eq!(plan.schedules[0].trigger(), ScheduleTrigger::Manual);
        assert_eq!(plan.schedules[0].dispatch(), ScheduleDispatch::Command);
    }

    // ── defkmacro — declarative keyboard macros ─────────────────────────────

    #[test]
    fn parses_kmacros_with_full_field_set() {
        let plan = apply_source(
            r#"
            (defkmacro :name "header-comment"
                       :description "wrap line in C-style block comment"
                       :keys "I/* <Esc>A */<Esc>"
                       :mode "normal"
                       :filetype "c"
                       :keybind "<leader>mh")
            (defkmacro :name "insert-date"
                       :keys ":put =strftime('%Y-%m-%d')<CR>"
                       :mode "normal")
            (defkmacro :name "reg-a"
                       :keys "vip>"
                       :register "a")
            "#,
        )
        .unwrap();
        assert_eq!(plan.kmacros.len(), 3);
        assert_eq!(plan.kmacros[0].name, "header-comment");
        assert_eq!(plan.kmacros[0].filetype, "c");
        // "I/* <Esc>A */<Esc>" → two <Esc> named-key tokens.
        assert_eq!(plan.kmacros[0].named_key_count(), 2);
        assert_eq!(plan.kmacros[2].register, "a");
    }

    #[test]
    fn kmacro_with_empty_keys_rejected() {
        let err = apply_source(r#"(defkmacro :name "oops" :keys "")"#)
            .expect_err("empty keys should error");
        assert!(matches!(err, LispError::EmptyKmacroKeys(_)));
    }

    #[test]
    fn kmacro_without_keys_at_all_is_parse_error() {
        let err = apply_source(r#"(defkmacro :name "no-keys")"#)
            .expect_err("missing :keys should error");
        // tatara-lisp handles missing fields; our validator handles
        // set-but-empty. The missing case falls through as an empty
        // string (default), so our validator fires.
        assert!(matches!(err, LispError::EmptyKmacroKeys(_)));
    }

    #[test]
    fn kmacro_with_unknown_mode_rejected() {
        let err = apply_source(
            r#"(defkmacro :name "m" :keys "x" :mode "superman")"#,
        )
        .expect_err("unknown mode should error");
        assert!(matches!(err, LispError::UnknownKmacroMode { .. }));
    }

    #[test]
    fn kmacro_with_multichar_register_rejected() {
        let err = apply_source(
            r#"(defkmacro :name "m" :keys "x" :register "aa")"#,
        )
        .expect_err("multi-char register should error");
        assert!(matches!(err, LispError::MalformedKmacroRegister(_)));
    }

    #[test]
    fn kmacro_with_symbol_register_rejected() {
        let err = apply_source(
            r#"(defkmacro :name "m" :keys "x" :register "!")"#,
        )
        .expect_err("symbol register should error");
        assert!(matches!(err, LispError::MalformedKmacroRegister(_)));
    }

    #[test]
    fn kmacro_mode_vocabulary_is_the_keybind_mode_vocabulary() {
        // After the mode.rs extraction, `kmacro::KNOWN_MODES` and the
        // canonical `KNOWN_MODES` must be the same `&'static [&str]`
        // — not just equal in content. Pin identity here so a future
        // accidental reintroduction of a parallel const gets caught.
        assert!(std::ptr::eq(
            KMACRO_MODES.as_ptr(),
            KNOWN_MODES.as_ptr(),
        ));
        assert_eq!(KMACRO_MODES, KNOWN_MODES);
    }

    #[test]
    fn kmacro_with_empty_mode_is_valid() {
        // Empty mode = "replay in whatever mode is current". Nothing
        // should complain.
        let plan = apply_source(
            r#"(defkmacro :name "m" :keys "x")"#,
        )
        .unwrap();
        assert_eq!(plan.kmacros.len(), 1);
        assert_eq!(plan.kmacros[0].mode, "");
    }

    // ── defattest — content-addressed rc integrity attestations ─────────────

    #[test]
    fn summary_hash_is_stable_across_equivalent_plans() {
        // Two plans built from the same Lisp must produce the same
        // hash — that's the whole content-addressing contract.
        let a = apply_source(
            r#"
            (defkeybind :mode "normal" :key "g" :action "x")
            (deftheme :preset "nord")
            "#,
        )
        .unwrap();
        let b = apply_source(
            r#"
            (defkeybind :mode "normal" :key "g" :action "x")
            (deftheme :preset "nord")
            "#,
        )
        .unwrap();
        assert_eq!(a.summary_hash(), b.summary_hash());
        assert_eq!(a.summary_hash().len(), 32);
    }

    #[test]
    fn summary_hash_diverges_when_plan_shape_changes() {
        let a = apply_source(r#"(defkeybind :mode "normal" :key "g" :action "x")"#).unwrap();
        let b = apply_source(r#"(defkeybind :mode "normal" :key "g" :action "x")
                                 (defkeybind :mode "insert" :key "jk" :action "escape")"#)
            .unwrap();
        assert_ne!(a.summary_hash(), b.summary_hash());
    }

    #[test]
    fn parses_attests_across_kinds_and_severities() {
        let plan = apply_source(
            r#"
            (defattest :id "v1-baseline"
                       :description "team baseline"
                       :counts-hash "af42c0d18e9b3f4aa18b7c3ef1de93a4"
                       :kind "pin"
                       :severity "error")
            (defattest :id "min-core"
                       :counts-hash "903b11ef41d09e4be9c2b7aea0f65e2f"
                       :kind "min"
                       :severity "warn")
            (defattest :id "stub-todo")
            "#,
        )
        .unwrap();
        assert_eq!(plan.attests.len(), 3);
        assert_eq!(plan.attests[0].effective_kind(), "pin");
        assert_eq!(plan.attests[0].effective_severity(), "error");
        assert_eq!(plan.attests[1].effective_kind(), "min");
        assert_eq!(plan.attests[1].effective_severity(), "warn");
        // Stub (empty hash) defaults: pin + error.
        assert_eq!(plan.attests[2].effective_kind(), "pin");
        assert_eq!(plan.attests[2].effective_severity(), "error");
        assert!(plan.attests[2].is_empty_hash());
    }

    #[test]
    fn attest_unknown_kind_rejected() {
        let err = apply_source(
            r#"(defattest :id "x" :kind "strict")"#,
        )
        .expect_err("unknown kind should error");
        assert!(matches!(err, LispError::UnknownAttestKind { .. }));
    }

    #[test]
    fn attest_unknown_severity_rejected() {
        let err = apply_source(
            r#"(defattest :id "x" :severity "critical")"#,
        )
        .expect_err("unknown severity should error");
        assert!(matches!(err, LispError::UnknownAttestSeverity { .. }));
    }

    #[test]
    fn attest_malformed_hash_rejected() {
        // Uppercase rejected — matches the defsnippet :hash rule.
        let err = apply_source(
            r#"(defattest :id "x" :counts-hash "AF42C0D18E9B3F4AA18B7C3EF1DE93A4")"#,
        )
        .expect_err("uppercase hash should error");
        assert!(matches!(err, LispError::MalformedAttestHash(_)));

        // Wrong length.
        let err = apply_source(
            r#"(defattest :id "x" :counts-hash "af42")"#,
        )
        .expect_err("short hash should error");
        assert!(matches!(err, LispError::MalformedAttestHash(_)));
    }

    #[test]
    fn evaluate_attests_reports_ok_for_matching_hash() {
        // Compute a plan, then re-apply the same plan with a
        // defattest pinning its summary hash. The evaluation should
        // resolve to Ok.
        let base = apply_source(r#"(deftheme :preset "nord")"#).unwrap();
        let expected_hash = base.summary_hash();
        let src = format!(
            r#"(deftheme :preset "nord")
               (defattest :id "pin-nord" :counts-hash "{expected_hash}")"#,
        );
        let plan = apply_source(&src).unwrap();
        let results = plan.evaluate_attests();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, AttestResult::Ok);
    }

    #[test]
    fn evaluate_attests_reports_drift_for_stale_hash() {
        let src = format!(
            r#"(deftheme :preset "nord")
               (defattest :id "stale"
                          :counts-hash "00000000000000000000000000000000"
                          :severity "warn")"#
        );
        let plan = apply_source(&src).unwrap();
        let results = plan.evaluate_attests();
        assert_eq!(results.len(), 1);
        match &results[0].1 {
            AttestResult::Drift { expected, actual } => {
                assert_eq!(expected, "00000000000000000000000000000000");
                assert_ne!(actual, expected);
            }
            other => panic!("expected Drift, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_attests_skips_unpinned_stubs() {
        let plan = apply_source(
            r#"
            (deftheme :preset "nord")
            (defattest :id "todo")
            "#,
        )
        .unwrap();
        let results = plan.evaluate_attests();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, AttestResult::Skipped);
    }

    #[test]
    fn compute_summary_hash_matches_plan_content_summary() {
        // The free function and the plan method must agree when
        // hashing the same string. The hashable projection is
        // `content_summary()`, not `summary()` (the latter includes
        // `attests=N`, which would self-reference).
        let plan = apply_source(r#"(deftheme :preset "nord")"#).unwrap();
        assert_eq!(
            plan.summary_hash(),
            compute_summary_hash(&plan.content_summary()),
        );
    }

    #[test]
    fn content_summary_omits_attests_but_summary_keeps_it() {
        let plan = apply_source(
            r#"
            (deftheme :preset "nord")
            (defattest :id "x")
            (defattest :id "y")
            "#,
        )
        .unwrap();
        assert!(plan.summary().contains("attests=2"));
        assert!(!plan.content_summary().contains("attests"));
    }

    #[test]
    fn summary_hash_is_stable_under_added_attestations() {
        // The whole point of excluding attests from the hash: a
        // user can add `defattest` entries without invalidating
        // their own hash pin.
        let a = apply_source(r#"(deftheme :preset "nord")"#).unwrap();
        let b = apply_source(
            r#"
            (deftheme :preset "nord")
            (defattest :id "a")
            (defattest :id "b")
            "#,
        )
        .unwrap();
        assert_eq!(a.summary_hash(), b.summary_hash());
    }

    // ── defruler — column rulers / visual guides ────────────────────────────

    #[test]
    fn parses_rulers_with_multiple_columns_and_styles() {
        let plan = apply_source(
            r##"
            (defruler :columns (80 120)
                      :style "soft"
                      :color "#4c566a")
            (defruler :columns (100)
                      :filetype "rust"
                      :style "hard"
                      :color "#bf616a"
                      :description "rust-wide line cap")
            (defruler :columns (4 8 12 16 20)
                      :filetype "python"
                      :style "dim")
            "##,
        )
        .unwrap();
        assert_eq!(plan.rulers.len(), 3);
        assert_eq!(plan.rulers[0].columns, vec![80, 120]);
        assert_eq!(plan.rulers[0].effective_style(), "soft");
        assert_eq!(plan.rulers[1].filetype, "rust");
        assert_eq!(plan.rulers[1].effective_style(), "hard");
        assert_eq!(plan.rulers[2].columns.len(), 5);
    }

    #[test]
    fn ruler_with_empty_columns_rejected() {
        let err = apply_source(r#"(defruler :columns ())"#)
            .expect_err("empty columns should error");
        assert!(matches!(err, LispError::EmptyRulerColumns));
    }

    #[test]
    fn ruler_with_zero_column_rejected() {
        // Columns are 1-based; 0 is meaningless.
        let err = apply_source(r#"(defruler :columns (80 0 120))"#)
            .expect_err("zero column should error");
        assert!(matches!(err, LispError::ZeroRulerColumn));
    }

    #[test]
    fn ruler_with_unknown_style_rejected() {
        let err = apply_source(
            r#"(defruler :columns (80) :style "bold")"#,
        )
        .expect_err("unknown style should error");
        assert!(matches!(err, LispError::UnknownRulerStyle(_)));
    }

    #[test]
    fn ruler_with_malformed_color_rejected() {
        // Named colors aren't accepted — users must use hex.
        let err = apply_source(
            r#"(defruler :columns (80) :color "blue")"#,
        )
        .expect_err("named color should error");
        assert!(matches!(err, LispError::MalformedRulerColor(_)));

        // Short-hex rejected (we require #rrggbb / #rrggbbaa).
        let err = apply_source(
            r##"(defruler :columns (80) :color "#fff")"##,
        )
        .expect_err("short hex should error");
        assert!(matches!(err, LispError::MalformedRulerColor(_)));
    }

    #[test]
    fn blake3_hash_vocab_is_one_predicate_across_specs() {
        // defsnippet :hash and defattest :counts-hash both route
        // structural validation through `crate::hash::is_blake3_128_hex`
        // — a uppercase-hex string must fail both the same way, and
        // a valid lowercase must pass both. Proves the single-
        // predicate contract end-to-end through apply_source.
        let valid = "af42c0d18e9b3f4aa18b7c3ef1de93a4";
        let uppercase = "AF42C0D18E9B3F4AA18B7C3EF1DE93A4";

        // Both specs accept the valid lowercase.
        apply_source(&format!(
            r#"(defsnippet :trigger "x" :hash "{valid}")"#
        ))
        .expect("valid lowercase hash should parse for defsnippet");
        apply_source(&format!(
            r#"(defattest :id "x" :counts-hash "{valid}")"#
        ))
        .expect("valid lowercase hash should parse for defattest");

        // Both specs reject the uppercase form.
        assert!(matches!(
            apply_source(&format!(
                r#"(defsnippet :trigger "x" :hash "{uppercase}")"#
            )),
            Err(LispError::MalformedSnippetHash(_)),
        ));
        assert!(matches!(
            apply_source(&format!(
                r#"(defattest :id "x" :counts-hash "{uppercase}")"#
            )),
            Err(LispError::MalformedAttestHash(_)),
        ));

        // The shared predicate agrees with both edges.
        assert!(is_blake3_128_hex(valid));
        assert!(!is_blake3_128_hex(uppercase));
    }

    #[test]
    fn compute_blake3_128_hex_is_the_shared_compute_function() {
        // compute_summary_hash is a thin wrapper over
        // compute_blake3_128_hex — the two must agree byte-for-byte
        // for every input. Pinning this guarantees mado's clipboard
        // store + defsnippet + defattest can all derive the same
        // token from the same bytes.
        let s = "keybinds=10 cmds=5 theme=1";
        assert_eq!(
            compute_blake3_128_hex(s.as_bytes()),
            compute_summary_hash(s),
        );
    }

    #[test]
    fn ruler_with_empty_color_uses_theme_default() {
        // Empty color means "fall back to theme" — not a malformed
        // value. Parse should succeed.
        let plan = apply_source(
            r#"(defruler :columns (80) :style "soft")"#,
        )
        .unwrap();
        assert_eq!(plan.rulers.len(), 1);
        assert!(plan.rulers[0].color.is_empty());
    }

    // ── defmcp — declarative MCP-tool bindings ──────────────────────────────

    #[test]
    fn parses_mcp_tools_with_full_field_set() {
        let plan = apply_source(
            r#"
            (defmcp :name "mado.clipboard.get"
                    :description "fetch a clipboard payload by BLAKE3 hash"
                    :server "mado"
                    :tool "clipboard_get"
                    :keybind "<leader>mcg"
                    :on-result "action:insert-at-cursor")
            (defmcp :name "mado.prompt.list"
                    :server "mado"
                    :tool "prompt_marks_list"
                    :keybind "<leader>mpp")
            (defmcp :name "curupira.react.tree"
                    :server "curupira"
                    :tool "react_get_component_tree"
                    :background #t)
            "#,
        )
        .unwrap();
        assert_eq!(plan.mcp_tools.len(), 3);
        assert_eq!(plan.mcp_tools[0].qualified_id(), "mado.clipboard_get");
        assert_eq!(plan.mcp_tools[1].keybind, "<leader>mpp");
        assert!(plan.mcp_tools[2].background);
        // on-result may be empty (discard).
        assert!(plan.mcp_tools[1].on_result.is_empty());
    }

    #[test]
    fn mcp_with_empty_server_rejected() {
        let err = apply_source(
            r#"(defmcp :name "x" :tool "clipboard_get")"#,
        )
        .expect_err("empty server should error");
        assert!(matches!(err, LispError::EmptyMcpServer(_)));
    }

    #[test]
    fn mcp_with_empty_tool_rejected() {
        let err = apply_source(
            r#"(defmcp :name "x" :server "mado")"#,
        )
        .expect_err("empty tool should error");
        assert!(matches!(err, LispError::EmptyMcpTool(_)));
    }

    #[test]
    fn mcp_with_malformed_on_result_rejected() {
        let err = apply_source(
            r#"(defmcp :name "x"
                      :server "mado"
                      :tool "clipboard_get"
                      :on-result "notify:bell")"#,
        )
        .expect_err("unknown on-result prefix should error");
        assert!(matches!(err, LispError::MalformedMcpOnResult { .. }));
    }

    #[test]
    fn mcp_accepts_each_on_result_prefix() {
        let plan = apply_source(
            r#"
            (defmcp :name "a" :server "s" :tool "t" :on-result "action:save")
            (defmcp :name "b" :server "s" :tool "t" :on-result "command:buffer.write-all")
            (defmcp :name "c" :server "s" :tool "t" :on-result "workflow:ship-rust")
            (defmcp :name "d" :server "s" :tool "t")
            "#,
        )
        .unwrap();
        assert_eq!(plan.mcp_tools.len(), 4);
        for spec in &plan.mcp_tools {
            assert!(spec.has_valid_on_result());
        }
    }
}
