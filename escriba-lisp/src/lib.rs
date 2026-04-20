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
//!
//! # Extending
//!
//! Add a new module next to the others with a `#[derive(DeriveTataraDomain)]`
//! struct plus `#[tatara(keyword = "…")]`, re-export it from [`lib`], and
//! add a line in [`apply_source`] that pulls it out via
//! `tatara_lisp::compile_typed`.

mod abbrev;
mod apply;
mod bufferline;
mod cmd;
mod dap;
mod filetype;
mod formatter;
mod gate;
mod highlight;
mod hook;
mod icon;
mod keybind;
mod lsp;
mod mode_spec;
mod option;
mod palette;
mod plugin;
mod snippet;
mod statusline;
mod textobject;
mod theme;
mod workflow;

pub use abbrev::AbbrevSpec;
pub use apply::{
    ApplyReport, GrammarApplyReport, apply_plan_to_grammar_extensions, apply_plan_to_keymap,
};
pub use bufferline::BufferLineSpec;
pub use cmd::CmdSpec;
pub use dap::{DapAdapterSpec, KNOWN_ADAPTERS, is_known_adapter};
pub use filetype::FiletypeSpec;
pub use formatter::FormatterSpec;
pub use gate::{
    GateMode, GateSpec, KNOWN_ACTIONS as GATE_ACTIONS, KNOWN_SEVERITIES as GATE_SEVERITIES,
    KNOWN_SOURCES as GATE_SOURCES, is_known_action as is_gate_action,
    is_known_severity as is_gate_severity, is_known_source as is_gate_source,
};
pub use highlight::{CANONICAL_GROUPS, HighlightSpec, is_canonical_group};
pub use hook::{HookSpec, KNOWN_EVENTS, is_known_event};
pub use icon::IconSpec;
pub use keybind::KeybindSpec;
pub use lsp::{KNOWN_SERVERS, LspServerSpec, is_known_server};
pub use mode_spec::MajorModeSpec;
pub use option::OptionSpec;
pub use palette::PaletteSpec;
pub use plugin::{KNOWN_CATEGORIES, PluginSpec, is_known_category};
pub use snippet::SnippetSpec;
pub use statusline::{KNOWN_SEGMENTS, StatusLineSpec, StatusSegment, is_known_segment};
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
    #[error("unknown mode name: {0} (valid: normal, insert, visual, visual-line, command)")]
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
    }

    /// Pairs of `(label, count)` — the single source of truth the
    /// summary + startup banner + planned `escriba doctor` all read
    /// from. Adding a def-form = one new entry here, instead of
    /// touching a 20-arg format string.
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

    let themes: Vec<ThemeSpec> =
        compile(src)?;
    for t in &themes {
        if !t.preset.is_empty() && !is_known_preset(&t.preset) {
            return Err(LispError::UnknownTheme(t.preset.clone()));
        }
    }
    // Last writer wins.
    let theme = themes.into_iter().last();

    let hooks: Vec<HookSpec> =
        compile(src)?;
    for h in &hooks {
        if !is_known_event(&h.event) {
            return Err(LispError::UnknownHook(h.event.clone()));
        }
    }

    let filetypes: Vec<FiletypeSpec> =
        compile(src)?;

    let abbreviations: Vec<AbbrevSpec> =
        compile(src)?;

    let snippets: Vec<SnippetSpec> =
        compile(src)?;

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

    let gates: Vec<GateSpec> =
        compile(src)?;
    // Strict validation on gates — ill-formed specs (unknown action /
    // neither command nor source / both set) fail fast so users
    // learn about the mistake at apply time, not at dispatch.
    for g in &gates {
        if !gate::is_known_action(&g.action) {
            return Err(LispError::UnknownGateAction(g.action.clone()));
        }
        if g.mode() == gate::GateMode::Invalid {
            return Err(LispError::InvalidGateShape(g.name.clone()));
        }
        if !g.severity.is_empty() && !gate::is_known_severity(&g.severity) {
            return Err(LispError::UnknownGateSeverity(g.severity.clone()));
        }
    }

    let text_objects: Vec<TextObjectSpec> =
        compile(src)?;
    for t in &text_objects {
        if !textobject::is_known_scope(&t.scope) {
            return Err(LispError::UnknownTextObjectScope(t.scope.clone()));
        }
    }

    let workflows: Vec<WorkflowSpec> = compile(src)?;
    for w in &workflows {
        if !workflow::is_known_failure_mode(&w.on_failure) {
            return Err(LispError::UnknownWorkflowFailureMode(w.on_failure.clone()));
        }
    }

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

fn validate_mode(mode: &str) -> LispResult<()> {
    match mode {
        "normal" | "insert" | "visual" | "visual-line" | "command" => Ok(()),
        _ => Err(LispError::UnknownMode(mode.to_string())),
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

        // Every known def-form appears in counts() — adding a new
        // spec without extending counts() would be a regression.
        let names: Vec<&str> = plan.counts().iter().map(|(n, _)| *n).collect();
        for required in &[
            "keybinds",
            "cmds",
            "options",
            "theme",
            "hooks",
            "filetypes",
            "abbrev",
            "snippets",
            "major_modes",
            "plugins",
            "highlights",
            "statusline",
            "bufferline",
            "lsp",
            "formatters",
            "palettes",
            "icons",
            "dap",
            "gates",
            "textobjects",
            "workflows",
        ] {
            assert!(
                names.contains(required),
                "counts() missing required def-form: {required}",
            );
        }
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
}
