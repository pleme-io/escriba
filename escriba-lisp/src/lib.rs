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
mod filetype;
mod formatter;
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
mod theme;

pub use abbrev::AbbrevSpec;
pub use apply::{ApplyReport, apply_plan_to_keymap};
pub use bufferline::BufferLineSpec;
pub use cmd::CmdSpec;
pub use filetype::FiletypeSpec;
pub use formatter::FormatterSpec;
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
pub use theme::{KNOWN_PRESETS, ThemeSpec, is_known_preset};

use std::path::{Path, PathBuf};

pub type LispResult<T> = Result<T, LispError>;

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
    }

    /// Short human-readable summary — useful for startup banners and
    /// the planned `escriba doctor` subcommand.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "keybinds={} cmds={} options={} theme={} hooks={} filetypes={} abbrev={} snippets={} major_modes={} plugins={} highlights={} statusline={} bufferline={} lsp={} formatters={} palettes={} icons={}",
            self.keybinds.len(),
            self.commands.len(),
            self.options.len(),
            if self.theme.is_some() { 1 } else { 0 },
            self.hooks.len(),
            self.filetypes.len(),
            self.abbreviations.len(),
            self.snippets.len(),
            self.major_modes.len(),
            self.plugins.len(),
            self.highlights.len(),
            if self.status_line.is_some() { 1 } else { 0 },
            if self.buffer_line.is_some() { 1 } else { 0 },
            self.lsp_servers.len(),
            self.formatters.len(),
            self.palettes.len(),
            self.icons.len(),
        )
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
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;
    for k in &keybinds {
        validate_mode(&k.mode)?;
    }

    let commands: Vec<CmdSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let options: Vec<OptionSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let themes: Vec<ThemeSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;
    for t in &themes {
        if !t.preset.is_empty() && !is_known_preset(&t.preset) {
            return Err(LispError::UnknownTheme(t.preset.clone()));
        }
    }
    // Last writer wins.
    let theme = themes.into_iter().last();

    let hooks: Vec<HookSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;
    for h in &hooks {
        if !is_known_event(&h.event) {
            return Err(LispError::UnknownHook(h.event.clone()));
        }
    }

    let filetypes: Vec<FiletypeSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let abbreviations: Vec<AbbrevSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let snippets: Vec<SnippetSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let major_modes: Vec<MajorModeSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let plugins: Vec<PluginSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let highlights: Vec<HighlightSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let status_lines: Vec<StatusLineSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;
    // Last writer wins — matches theme semantics.
    let status_line = status_lines.into_iter().last();

    let buffer_lines: Vec<BufferLineSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;
    let buffer_line = buffer_lines.into_iter().last();

    let lsp_servers: Vec<LspServerSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let formatters: Vec<FormatterSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let palettes: Vec<PaletteSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

    let icons: Vec<IconSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| LispError::Parse(e.to_string()))?;

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
