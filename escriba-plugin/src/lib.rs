//! `escriba-plugin` — caixa-native plugin model for escriba.
//!
//! **An escriba plugin IS a caixa.** A plugin is a directory (a git
//! repo with a `caixa.lisp` at its root — the pleme-io git-as-registry
//! model) whose *escriba entry* is tatara-lisp declaring the plugin's
//! keybinds / commands / options / highlights via `escriba-lisp`
//! def-forms (and, later, imperative setup via `escriba-vm`). The
//! user's rc DECLARES a plugin; escriba RESOLVES it to a plugin
//! directory, LOADS its entry, and ACTIVATES it (applies the entry's
//! def-forms to live `EditorState`) either eagerly or when an
//! activation trigger fires.
//!
//! **Lineage-safe by design.** escriba reads the plugin's tatara-lisp
//! with its OWN (`pleme-io/tatara-lisp`) parser; it deliberately does
//! NOT depend on `caixa-core` (which is on the other tatara-lisp
//! lineage) — that would re-introduce a two-lineage conflict. Plugin
//! install/resolve uses the caixa git model (`feira` / git) out of
//! band; this crate is the *consumer* side: discovery, load, and
//! trigger-gated activation.

use std::path::{Path, PathBuf};

use escriba_config::PluginDecl;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod forge;
pub use forge::{CaixaArtifacts, ForgeError, emit_flake_nix, forge_plugin, write_plugin_caixa};

/// When a plugin's setup is applied to the editor — the lazy.nvim
/// activation model, typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivationTrigger {
    /// Apply at startup (eager).
    Startup,
    /// Apply when a buffer of this filetype is opened.
    FileType(String),
    /// Apply when this editor event fires (`BufWritePost`, …).
    Event(String),
    /// Apply when this command is first invoked (lazy command load).
    Command(String),
}

impl ActivationTrigger {
    /// Parse an `:ativar-em` entry. Accepts `"Startup"` / `"eager"`, or
    /// a `"<Kind>: <arg>"` shape: `"FileType: lisp"`, `"Event:
    /// BufWritePost"`, `"Command: Paredit"` (`ft`/`cmd` aliases too).
    /// Returns `None` for an unrecognized shape so the caller can warn.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim();
        if t.eq_ignore_ascii_case("startup") || t.eq_ignore_ascii_case("eager") {
            return Some(Self::Startup);
        }
        let (head, rest) = t.split_once(':')?;
        let arg = rest.trim().to_string();
        if arg.is_empty() {
            return None;
        }
        match head.trim().to_ascii_lowercase().as_str() {
            "filetype" | "ft" => Some(Self::FileType(arg)),
            "event" => Some(Self::Event(arg)),
            "command" | "cmd" => Some(Self::Command(arg)),
            _ => None,
        }
    }
}

/// The conventional relative paths (priority order) where a plugin
/// caixa keeps its escriba entry. First existing wins.
pub const ENTRY_CANDIDATES: &[&str] =
    &["escriba/plugin.lisp", "escriba.lisp", "plugin/escriba.lisp"];

/// A loaded plugin caixa: identity + the tatara-lisp entry source +
/// parsed activation triggers + on-disk root.
#[derive(Debug, Clone)]
pub struct PluginCaixa {
    pub name: String,
    pub version: String,
    /// The tatara-lisp source of the plugin's escriba entry — applied
    /// to `EditorState` (via `escriba-lisp`) on activation.
    pub entry_src: String,
    pub triggers: Vec<ActivationTrigger>,
    pub root: PathBuf,
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin `{caixa}`: no escriba entry found under {root} (looked for {candidates:?})")]
    EntryNotFound {
        caixa: String,
        root: String,
        candidates: Vec<String>,
    },
    #[error("plugin `{caixa}`: io error reading {path}: {source}")]
    Io {
        caixa: String,
        path: String,
        source: std::io::Error,
    },
}

impl PluginCaixa {
    /// Load a plugin from its installed caixa directory. `ativar_em`
    /// entries that don't parse are skipped (the caller can surface a
    /// warning); a plugin with no parseable triggers is eager. The
    /// entry tatara-lisp is read from the first existing
    /// [`ENTRY_CANDIDATES`] path under `root`.
    pub fn load(
        name: &str,
        version: &str,
        ativar_em: &[String],
        root: &Path,
    ) -> Result<Self, PluginError> {
        for cand in ENTRY_CANDIDATES {
            let p = root.join(cand);
            if p.exists() {
                let entry_src = std::fs::read_to_string(&p).map_err(|e| PluginError::Io {
                    caixa: name.to_string(),
                    path: p.display().to_string(),
                    source: e,
                })?;
                let triggers = ativar_em
                    .iter()
                    .filter_map(|s| ActivationTrigger::parse(s))
                    .collect();
                return Ok(Self {
                    name: name.to_string(),
                    version: version.to_string(),
                    entry_src,
                    triggers,
                    root: root.to_path_buf(),
                });
            }
        }
        Err(PluginError::EntryNotFound {
            caixa: name.to_string(),
            root: root.display().to_string(),
            candidates: ENTRY_CANDIDATES.iter().map(|s| (*s).to_string()).collect(),
        })
    }

    /// Load from a declarative [`PluginDecl`] (`defplugin :caixa …
    /// :versao … :ativar-em …`) rooted at `root` (typically
    /// `<plugins_dir>/<caixa>`).
    pub fn from_decl(decl: &PluginDecl, root: &Path) -> Result<Self, PluginError> {
        Self::load(&decl.caixa, &decl.versao, &decl.ativar_em, root)
    }

    /// Eager iff there are no triggers, or an explicit [`Startup`]
    /// trigger is present.
    ///
    /// [`Startup`]: ActivationTrigger::Startup
    #[must_use]
    pub fn is_eager(&self) -> bool {
        self.triggers.is_empty() || self.triggers.contains(&ActivationTrigger::Startup)
    }

    /// Does an opened `filetype` match any `FileType` trigger?
    #[must_use]
    pub fn matches_filetype(&self, filetype: &str) -> bool {
        self.triggers
            .iter()
            .any(|t| matches!(t, ActivationTrigger::FileType(f) if f == filetype))
    }

    /// Does `event` match any `Event` trigger?
    #[must_use]
    pub fn matches_event(&self, event: &str) -> bool {
        self.triggers
            .iter()
            .any(|t| matches!(t, ActivationTrigger::Event(e) if e == event))
    }

    /// Does `command` match any `Command` trigger?
    #[must_use]
    pub fn matches_command(&self, command: &str) -> bool {
        self.triggers
            .iter()
            .any(|t| matches!(t, ActivationTrigger::Command(c) if c == command))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_trigger_shapes() {
        assert_eq!(
            ActivationTrigger::parse("Startup"),
            Some(ActivationTrigger::Startup)
        );
        assert_eq!(
            ActivationTrigger::parse("eager"),
            Some(ActivationTrigger::Startup)
        );
        assert_eq!(
            ActivationTrigger::parse("FileType: lisp"),
            Some(ActivationTrigger::FileType("lisp".into())),
        );
        assert_eq!(
            ActivationTrigger::parse("ft: rust"),
            Some(ActivationTrigger::FileType("rust".into())),
        );
        assert_eq!(
            ActivationTrigger::parse("Event: BufWritePost"),
            Some(ActivationTrigger::Event("BufWritePost".into())),
        );
        assert_eq!(
            ActivationTrigger::parse("Command: Paredit"),
            Some(ActivationTrigger::Command("Paredit".into())),
        );
        // Unrecognized shapes.
        assert_eq!(ActivationTrigger::parse("Nonsense: x"), None);
        assert_eq!(ActivationTrigger::parse("FileType:"), None);
    }

    /// Write a plugin caixa skeleton (caixa.lisp + entry) under a fresh
    /// temp dir and return its root.
    fn scratch_plugin(slug: &str, entry_rel: &str, entry_src: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("escriba-plugin-test-{slug}"));
        let _ = std::fs::remove_dir_all(&root);
        if let Some(parent) = root.join(entry_rel).parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            root.join("caixa.lisp"),
            format!("(defcaixa :nome \"{slug}\" :versao \"0.1.0\" :kind Biblioteca)\n"),
        )
        .unwrap();
        std::fs::write(root.join(entry_rel), entry_src).unwrap();
        root
    }

    #[test]
    fn load_reads_entry_and_parses_triggers() {
        let root = scratch_plugin(
            "paredit",
            "escriba/plugin.lisp",
            r#"(defkeybind :mode "normal" :key "<A-f>" :action "forward-sexp")"#,
        );
        let p = PluginCaixa::load(
            "escriba-paredit",
            "^0.1",
            &["FileType: lisp".to_string()],
            &root,
        )
        .expect("plugin loads");
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(p.name, "escriba-paredit");
        assert!(p.entry_src.contains("defkeybind"));
        assert_eq!(p.triggers, vec![ActivationTrigger::FileType("lisp".into())]);
        assert!(!p.is_eager(), "a FileType-triggered plugin is lazy");
        assert!(p.matches_filetype("lisp"));
        assert!(!p.matches_filetype("rust"));
    }

    #[test]
    fn no_triggers_is_eager() {
        let root = scratch_plugin(
            "eagerplug",
            "escriba.lisp",
            "(defoption :name \"x\" :value \"1\")",
        );
        let p = PluginCaixa::load("eagerplug", "0.1", &[], &root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(p.is_eager());
    }

    #[test]
    fn missing_entry_errors_with_candidates() {
        let root = std::env::temp_dir().join("escriba-plugin-test-noentry");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("caixa.lisp"), "(defcaixa :nome \"x\")").unwrap();
        let err = PluginCaixa::load("x", "0.1", &[], &root).unwrap_err();
        let _ = std::fs::remove_dir_all(&root);
        assert!(matches!(err, PluginError::EntryNotFound { .. }));
    }

    #[test]
    fn from_decl_bridges_plugindecl() {
        let root = scratch_plugin(
            "fromdecl",
            "escriba/plugin.lisp",
            r#"(defcmd :name "hi" :action "editor.noop")"#,
        );
        let decl = PluginDecl {
            caixa: "fromdecl".into(),
            versao: "^0.2".into(),
            ativar_em: vec!["Command: Hi".into()],
        };
        let p = PluginCaixa::from_decl(&decl, &root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(p.version, "^0.2");
        assert!(p.matches_command("Hi"));
    }
}
