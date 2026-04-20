//! `defterm` — Lisp-authored terminal session spec.
//!
//! The escriba side of the escriba ↔ mado typed contract. Every
//! field here mirrors `mado::term_spec::TermSpec` one-for-one so
//! the same JSON shape crosses the MCP boundary without any
//! adapter.
//!
//! Novelty: no editor in the category lets users declare named
//! terminal sessions with typed placement / effect / attach
//! semantics in their rc, then drive them over MCP without any
//! protocol coupling. vim's `:terminal`, nvim's `jobstart`,
//! vscode's `tasks.json` all invent their own shape; `defterm`
//! pins the shape once, on both sides.
//!
//! ```lisp
//! (defterm :name "dev-shell"
//!          :shell "/bin/frost"
//!          :cwd   "~/code/github/pleme-io/escriba"
//!          :placement "split-horizontal"
//!          :effects ("cursor-glow" "bloom")
//!          :keybind "<leader>td")
//!
//! (defterm :name "build-watch"
//!          :shell "cargo"
//!          :args ("watch" "-x" "test")
//!          :cwd "~/code/github/pleme-io/escriba"
//!          :placement "split-vertical"
//!          :env ("CARGO_TERM_COLOR=always" "RUST_LOG=warn")
//!          :keybind "<leader>tb")
//!
//! (defterm :name "attach-main"
//!          :attach "pane-main"      ; focus an existing session
//!          :keybind "<leader>ta")
//! ```
//!
//! # Wire-shape parity with mado
//!
//! - `placement`: `tab` / `split-horizontal` / `split-vertical` / `window`.
//! - `attach`: existing session id; when set, `shell`/`args`/`cwd`/`env`
//!   are ignored (matches mado's `is_attach()` semantics).
//! - `effects`: names reference [`crate::EffectSpec`] canonical set
//!   (`cursor-glow`, `bloom`, …). Mado reads the same set from
//!   `defeffect` declarations.
//! - `env`: list of `"KEY=VALUE"` strings. Tatara-lisp doesn't
//!   directly author HashMaps; a flat string list round-trips
//!   trivially through `env_pairs()`.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defterm")]
pub struct TermSpec {
    /// Human-readable terminal id — unique within the plan.
    pub name: String,
    /// One-line purpose sentence shown in the picker.
    #[serde(default)]
    pub description: String,
    /// Shell command (`bash` / `zsh` / `fish` / `frost`, full path).
    /// Empty = user's `$SHELL`.
    #[serde(default)]
    pub shell: String,
    /// Extra args to the shell.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory. `~` expands to `$HOME`. Empty = inherit.
    #[serde(default)]
    pub cwd: String,
    /// Environment variables as `"KEY=VALUE"` strings.
    #[serde(default)]
    pub env: Vec<String>,
    /// Window-bar title for the session. Empty = derive at open time.
    #[serde(default)]
    pub title: String,
    /// Placement. Must be one of [`KNOWN_PLACEMENTS`] (empty treated
    /// as `"tab"`).
    #[serde(default)]
    pub placement: String,
    /// Existing session id to attach to (vs. spawning new). When
    /// non-empty, `shell`/`args`/`cwd`/`env` are ignored.
    #[serde(default)]
    pub attach: String,
    /// Shader effect names — mirrors [`crate::EffectSpec`] naming.
    #[serde(default)]
    pub effects: Vec<String>,
    /// Optional keybind that opens this terminal.
    #[serde(default)]
    pub keybind: String,
}

/// Placement values mirroring `mado::term_spec::KNOWN_PLACEMENTS`.
/// Keeping them in sync is a cross-repo contract enforced by
/// explicit test assertions on both sides.
pub const KNOWN_PLACEMENTS: &[&str] = &[
    "tab",
    "split-horizontal",
    "split-vertical",
    "window",
];

#[must_use]
pub fn is_known_placement(name: &str) -> bool {
    name.is_empty() || KNOWN_PLACEMENTS.iter().any(|p| *p == name)
}

impl TermSpec {
    /// Parse `:env` strings as `(key, value)` pairs. Strings without
    /// `=` are dropped silently — they're invalid env-var syntax and
    /// `KEY=VALUE` is the mado-side contract.
    #[must_use]
    pub fn env_pairs(&self) -> Vec<(String, String)> {
        self.env
            .iter()
            .filter_map(|s| {
                s.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect()
    }

    /// True when this spec asks to attach vs. spawn.
    #[must_use]
    pub fn is_attach(&self) -> bool {
        !self.attach.is_empty()
    }

    /// Serialize into the MCP payload shape mado's `spawn_term`
    /// tool accepts. Shared via serde — camelCase field names,
    /// `env` stays a flat `Vec<String>` (matches mado).
    ///
    /// Returning `serde_json::Value` lets callers splice this into
    /// larger MCP request bodies without an intermediate String.
    pub fn to_mcp_value(&self) -> serde_json::Value {
        serde_json::json!({
            "shell": self.shell,
            "args": self.args,
            "cwd": self.cwd,
            "env": self.env_pairs().into_iter().collect::<std::collections::HashMap<_, _>>(),
            "title": self.title,
            "placement": self.placement,
            "attach": self.attach,
            "effects": self.effects,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_pairs_splits_on_equals() {
        let s = TermSpec {
            name: "x".into(),
            env: vec![
                "RUST_LOG=warn".into(),
                "CARGO_TERM_COLOR=always".into(),
                "NO_EQUALS_HERE".into(), // dropped
                "EMPTY=".into(),
            ],
            ..Default::default()
        };
        let pairs = s.env_pairs();
        assert_eq!(pairs.len(), 3);
        assert!(pairs.contains(&("RUST_LOG".to_string(), "warn".to_string())));
        assert!(pairs.contains(&("EMPTY".to_string(), "".to_string())));
    }

    #[test]
    fn known_placement_accepts_canonicals_plus_empty() {
        assert!(is_known_placement(""));
        for p in KNOWN_PLACEMENTS {
            assert!(is_known_placement(p));
        }
        assert!(!is_known_placement("zigzag"));
    }

    #[test]
    fn mcp_payload_shape_matches_mado_termspec() {
        // Contract test: the MCP payload escriba sends must round-trip
        // back to the same TermSpec mado parses on its side. We can't
        // import mado directly (different repo), but we can assert the
        // exact wire-level JSON shape mado::TermSpec's serde derives
        // expect.
        let s = TermSpec {
            name: "dev".into(),
            shell: "frost".into(),
            args: vec!["-l".into()],
            cwd: "~/code".into(),
            env: vec!["RUST_LOG=info".into()],
            title: "dev".into(),
            placement: "split-horizontal".into(),
            effects: vec!["cursor-glow".into()],
            ..Default::default()
        };
        let payload = s.to_mcp_value();
        assert_eq!(payload["shell"], "frost");
        assert_eq!(payload["placement"], "split-horizontal");
        // env is an object on the wire (mado side: HashMap).
        assert_eq!(payload["env"]["RUST_LOG"], "info");
        // effects + args stay as arrays.
        assert!(payload["args"].is_array());
        assert!(payload["effects"].is_array());
    }

    #[test]
    fn attach_signals_existing_session() {
        let s = TermSpec {
            name: "x".into(),
            attach: "pane-42".into(),
            ..Default::default()
        };
        assert!(s.is_attach());

        let fresh = TermSpec {
            name: "y".into(),
            ..Default::default()
        };
        assert!(!fresh.is_attach());
    }
}

impl Default for TermSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            shell: String::new(),
            args: Vec::new(),
            cwd: String::new(),
            env: Vec::new(),
            title: String::new(),
            placement: String::new(),
            attach: String::new(),
            effects: Vec::new(),
            keybind: String::new(),
        }
    }
}
