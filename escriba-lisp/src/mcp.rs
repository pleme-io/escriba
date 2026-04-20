//! `defmcp` — declarative MCP-tool bindings.
//!
//! **Invention.** No editor in the category ships typed declarative
//! MCP bindings. vscode's extensions are JavaScript activation
//! callbacks, zed's LSP client is Rust-compiled, neovim's `cmp` /
//! LSP wires are plugin-specific, jetbrains' actions are XML in a
//! packaged plugin. Escriba lets users declare "the MCP tool
//! `<server>.<tool>` becomes an editor command" in one line of
//! Lisp. The pleme-io fleet (mado, anvil, cursor, curupira, kurage,
//! shinryu, umbra) all speak MCP — every one of their tools is one
//! `defmcp` form away from an escriba palette entry, keybind, or
//! workflow step.
//!
//! ```lisp
//! ;; Paste mado's last BLAKE3-addressed clipboard entry.
//! (defmcp :name "mado.clipboard.get"
//!         :description "fetch a clipboard payload by BLAKE3 hash from mado"
//!         :server "mado"
//!         :tool "clipboard_get"
//!         :keybind "<leader>mcg"
//!         :on-result "action:insert-at-cursor")
//!
//! ;; List past prompts across every mado session — picker surface.
//! (defmcp :name "mado.prompt.list"
//!         :server "mado"
//!         :tool "prompt_marks_list"
//!         :keybind "<leader>mpp")
//!
//! ;; Launch a curupira browser diagnostic from the editor.
//! (defmcp :name "curupira.react.tree"
//!         :description "React component tree for the attached Chrome"
//!         :server "curupira"
//!         :tool "react_get_component_tree"
//!         :keybind "<leader>crt"
//!         :background #t)
//! ```
//!
//! ## Contract
//!
//! - `:name` — unique id in the command palette. Required.
//! - `:server` — MCP server alias the runtime resolves to a transport
//!   endpoint (stdio / socket / http). Required non-empty.
//! - `:tool` — tool name the remote server advertises. Required
//!   non-empty. The apply layer does not validate existence against
//!   any live server — that's a runtime-time concern.
//! - `:description` — picker hint.
//! - `:input-schema` — optional JSON-schema fragment describing the
//!   tool's argument shape. Informational today; a future tick will
//!   parse this for tab-completion + argument prompting. Empty =
//!   "no input arguments advertised".
//! - `:filetype` — optional scope (empty = global).
//! - `:keybind` — optional one-shot invocation hotkey.
//! - `:on-result` — optional action to dispatch with the tool's
//!   return value. Grammar: `action:<name>` / `command:<name>` /
//!   `workflow:<name>`. Empty = "discard result".
//! - `:background` — when true, run without blocking the focused
//!   pane (equivalent to spawning the call in a task queue); the
//!   result action fires via notification on completion. Default:
//!   false.
//!
//! ## Why typed
//!
//! The wire contract (name, server, tool) is three fields of text;
//! free-form strings anywhere in the stack mean typos land at the
//! end of a cross-process round-trip. Lifting the contract into a
//! `#[derive(DeriveTataraDomain)]` spec gives content-addressable
//! identity, picker-surface enumeration, and BLAKE3-128-stable
//! `defattest` pins for teams that want to lock the MCP surface
//! across workstations.

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defmcp")]
pub struct McpToolSpec {
    /// Command-palette id — unique within the plan.
    pub name: String,
    /// One-line description shown in the picker.
    #[serde(default)]
    pub description: String,
    /// MCP server alias — resolved by the runtime against the active
    /// MCP config registry (`~/.config/escriba/mcp.json` or the
    /// shikumi equivalent).
    #[serde(default)]
    pub server: String,
    /// Tool name advertised by the remote server.
    #[serde(default)]
    pub tool: String,
    /// Optional JSON-schema fragment for the tool's input. Empty
    /// means "no arguments".
    #[serde(default)]
    pub input_schema: String,
    /// Filetype scope — empty means global.
    #[serde(default)]
    pub filetype: String,
    /// Optional one-shot keybind.
    #[serde(default)]
    pub keybind: String,
    /// Action to dispatch with the tool's return value. Grammar
    /// shared with [`WorkflowSpec`](crate::WorkflowSpec): `action:…`
    /// / `command:…` / `workflow:…`. Empty = discard.
    #[serde(default)]
    pub on_result: String,
    /// When true, run without blocking the editor; dispatch
    /// `:on-result` via notification on completion.
    #[serde(default)]
    pub background: bool,
}

impl McpToolSpec {
    /// `<server>.<tool>` — the canonical global id used in logs,
    /// attestations, and the picker. Same rendering the runtime uses
    /// when dispatching the call so the user sees one consistent
    /// identifier.
    #[must_use]
    pub fn qualified_id(&self) -> String {
        format!("{}.{}", self.server, self.tool)
    }

    /// Structural check on `:on-result` — accepts an empty string
    /// (discard) OR a prefix grammar (`action:` / `command:` /
    /// `workflow:`). Prefix drift would mean a result gets silently
    /// dropped because the dispatcher doesn't match the string.
    #[must_use]
    pub fn has_valid_on_result(&self) -> bool {
        if self.on_result.is_empty() {
            return true;
        }
        self.on_result.starts_with("action:")
            || self.on_result.starts_with("command:")
            || self.on_result.starts_with("workflow:")
    }

    /// Valid known on-result prefix tokens — the same three surfaces
    /// [`WorkflowSpec`](crate::WorkflowSpec) step-kinds accept. Kept
    /// as a `const` so error messages can enumerate the vocabulary.
    pub const ON_RESULT_PREFIXES: &'static [&'static str] =
        &["action:", "command:", "workflow:"];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_id_renders_server_dot_tool() {
        let s = McpToolSpec {
            name: "x".into(),
            server: "mado".into(),
            tool: "clipboard_get".into(),
            ..Default::default()
        };
        assert_eq!(s.qualified_id(), "mado.clipboard_get");
    }

    #[test]
    fn on_result_empty_is_valid_means_discard() {
        let s = McpToolSpec {
            name: "x".into(),
            ..Default::default()
        };
        assert!(s.has_valid_on_result());
        assert!(s.on_result.is_empty());
    }

    #[test]
    fn on_result_accepts_each_prefix_kind() {
        for prefix in ["action:save", "command:buffer.write-all", "workflow:ship-rust"] {
            let s = McpToolSpec {
                name: "x".into(),
                on_result: prefix.into(),
                ..Default::default()
            };
            assert!(s.has_valid_on_result(), "should accept {prefix:?}");
        }
    }

    #[test]
    fn on_result_rejects_unknown_prefix() {
        for bad in ["notify:x", "inline:x", "lol", "just-text"] {
            let s = McpToolSpec {
                name: "x".into(),
                on_result: bad.into(),
                ..Default::default()
            };
            assert!(!s.has_valid_on_result(), "should reject {bad:?}");
        }
    }

    #[test]
    fn on_result_prefixes_const_exposes_exactly_three() {
        // Pin the vocabulary — adding a fourth dispatch kind here is
        // a conscious edit that needs the runtime to grow a new arm.
        assert_eq!(
            McpToolSpec::ON_RESULT_PREFIXES,
            &["action:", "command:", "workflow:"],
        );
    }
}
