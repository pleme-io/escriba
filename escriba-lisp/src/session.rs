//! `defsession` — Lisp-authored named workspace layouts.
//!
//! Absorbs vim `:mksession`, vscode workspaces, emacs
//! `desktop-save-mode`, and jetbrains "scope" concepts into one
//! typed declarative form. A session bundles:
//!
//! - a **name** — the session picker's label.
//! - a set of **buffers** — files to open on activation.
//! - a **layout** — how those buffers arrange
//!   (`single` / `horizontal` / `vertical` / `grid-2x2`).
//! - an optional **cwd** — working directory the editor chdirs to.
//! - an optional **env** map — per-session env vars.
//! - optional **on-enter** / **on-leave** — action/workflow refs run
//!   at transition time.
//!
//! ```lisp
//! (defsession :name "escriba-dev"
//!             :description "Working on the escriba authoring bridge"
//!             :buffers ("escriba-lisp/src/lib.rs"
//!                       "escriba-lisp/src/apply.rs"
//!                       "escriba/configs/blnvim-defaults.lisp")
//!             :layout "horizontal"
//!             :cwd "~/code/github/pleme-io/escriba"
//!             :on-enter ("workflow:save-and-test"))
//!
//! (defsession :name "blog"
//!             :buffers ("content/posts/latest.md")
//!             :layout "single"
//!             :cwd "~/blog")
//! ```
//!
//! # Novelty
//!
//! Every editor has session support; none expose it as a typed,
//! validated, rc-composed declarative primitive. vim `:mksession`
//! serializes state; vscode workspaces are JSON blobs; emacs
//! `desktop-save` writes elisp. `defsession` is *intent* —
//! "this is what 'escriba-dev' means" — not a point-in-time snapshot.
//! Renames survive reboots; machine-specific paths stay out; the rc
//! tracks it in git along with the rest of the config.
//!
//! Planned runtime: a picker (escriba-picker, Wave 2) lists sessions,
//! activation chdirs + opens buffers + arranges layout + runs on-enter
//! workflows, deactivation triggers on-leave. Layout strings beyond the
//! built-in four are plugin-extensible.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defsession")]
pub struct SessionSpec {
    /// Human-readable session id — unique within the plan. Appears
    /// in the session picker; referenced from `session:NAME`
    /// step strings in workflows.
    pub name: String,
    /// One-line purpose sentence shown in the picker.
    #[serde(default)]
    pub description: String,
    /// Buffers to open on activation. Paths may be absolute, or
    /// relative to `:cwd` (or process cwd if `:cwd` is empty). `~`
    /// expands to `$HOME`.
    #[serde(default)]
    pub buffers: Vec<String>,
    /// Split layout identifier. Canonical values in
    /// [`KNOWN_LAYOUTS`]; unknown values get advisory flagging but
    /// pass through (plugins add custom layouts at runtime).
    #[serde(default)]
    pub layout: String,
    /// Working directory the editor chdirs to on activation.
    /// `~` expands to `$HOME`. Empty = stay in the process cwd.
    #[serde(default)]
    pub cwd: String,
    /// Optional keybinding that activates this session. Format
    /// matches [`crate::KeybindSpec`] `:key`.
    #[serde(default)]
    pub keybind: String,
    /// Action / workflow refs to run on activation. Each string
    /// uses the same grammar as [`crate::WorkflowSpec`] steps
    /// (`action:…`, `workflow:…`, `shell:…`, `cmd:…`).
    #[serde(default)]
    pub on_enter: Vec<String>,
    /// Action / workflow refs to run on deactivation.
    #[serde(default)]
    pub on_leave: Vec<String>,
}

/// Canonical layout identifiers the runtime guarantees. Plugins
/// register more at runtime — unknown values warn but still apply.
pub const KNOWN_LAYOUTS: &[&str] = &[
    "single",       // one buffer, no splits.
    "horizontal",   // buffers stacked top→bottom.
    "vertical",     // buffers side-by-side left→right.
    "grid-2x2",     // 2×2 grid (first 4 buffers).
    "grid-3x3",     // 3×3 grid (first 9 buffers).
    "main-side",    // one big left pane, remaining stacked right.
    "tabs",         // each buffer is a tab, no splits.
];

#[must_use]
pub fn is_known_layout(name: &str) -> bool {
    name.is_empty() || KNOWN_LAYOUTS.iter().any(|l| *l == name)
}

impl SessionSpec {
    /// Whether the session has at least one buffer to open.
    /// Empty-buffer sessions are legal (they just chdir + run
    /// on-enter); useful for "open my dev env" scripts where
    /// the actions decide what buffers to open.
    #[must_use]
    pub fn has_buffers(&self) -> bool {
        !self.buffers.is_empty()
    }

    /// Best-effort integer for how many visible panes the layout
    /// implies. `single`/`tabs` = 1, `horizontal`/`vertical` =
    /// `buffers.len()`, `grid-2x2` = 4, `grid-3x3` = 9, unknown
    /// = `buffers.len()` (fallback to one pane per buffer).
    #[must_use]
    pub fn pane_count(&self) -> usize {
        match self.layout.as_str() {
            "single" | "tabs" => 1,
            "grid-2x2" => 4,
            "grid-3x3" => 9,
            _ => self.buffers.len().max(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_layout_accepts_empty_and_canonicals() {
        assert!(is_known_layout(""));
        assert!(is_known_layout("single"));
        assert!(is_known_layout("horizontal"));
        assert!(is_known_layout("grid-2x2"));
        assert!(!is_known_layout("zigzag"));
    }

    #[test]
    fn pane_count_handles_canonical_layouts() {
        let mut s = SessionSpec {
            name: "x".into(),
            layout: "single".into(),
            buffers: vec!["a".into(), "b".into(), "c".into()],
            ..Default::default()
        };
        assert_eq!(s.pane_count(), 1);

        s.layout = "horizontal".into();
        assert_eq!(s.pane_count(), 3);

        s.layout = "grid-2x2".into();
        assert_eq!(s.pane_count(), 4);

        s.layout = "grid-3x3".into();
        assert_eq!(s.pane_count(), 9);

        s.layout = "tabs".into();
        assert_eq!(s.pane_count(), 1);

        // Unknown layout falls back to buffers.len().
        s.layout = "custom-plugin-layout".into();
        assert_eq!(s.pane_count(), 3);
    }

    #[test]
    fn has_buffers_checks_emptiness() {
        let a = SessionSpec {
            name: "x".into(),
            buffers: vec!["a".into()],
            ..Default::default()
        };
        let b = SessionSpec {
            name: "y".into(),
            ..Default::default()
        };
        assert!(a.has_buffers());
        assert!(!b.has_buffers());
    }
}

impl Default for SessionSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            buffers: Vec::new(),
            layout: String::new(),
            cwd: String::new(),
            keybind: String::new(),
            on_enter: Vec::new(),
            on_leave: Vec::new(),
        }
    }
}
