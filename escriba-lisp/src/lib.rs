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
//!
//! # Extending
//!
//! Add a new module next to the others with a `#[derive(DeriveTataraDomain)]`
//! struct plus `#[tatara(keyword = "…")]`, re-export it from [`lib`], and
//! add a line in [`apply_source`] that pulls it out via
//! `tatara_lisp::compile_typed`.

mod abbrev;
mod cmd;
mod filetype;
mod hook;
mod keybind;
mod option;
mod snippet;
mod theme;

pub use abbrev::AbbrevSpec;
pub use cmd::CmdSpec;
pub use filetype::FiletypeSpec;
pub use hook::{HookSpec, KNOWN_EVENTS, is_known_event};
pub use keybind::KeybindSpec;
pub use option::OptionSpec;
pub use snippet::SnippetSpec;
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
    }

    /// Short human-readable summary — useful for startup banners and
    /// the planned `escriba doctor` subcommand.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "keybinds={} cmds={} options={} theme={} hooks={} filetypes={} abbrev={} snippets={}",
            self.keybinds.len(),
            self.commands.len(),
            self.options.len(),
            if self.theme.is_some() { 1 } else { 0 },
            self.hooks.len(),
            self.filetypes.len(),
            self.abbreviations.len(),
            self.snippets.len(),
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

    Ok(ApplyPlan {
        keybinds,
        commands,
        options,
        theme,
        hooks,
        filetypes,
        abbreviations,
        snippets,
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
}
