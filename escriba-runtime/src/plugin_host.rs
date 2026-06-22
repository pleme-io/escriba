//! `PluginHost` — runtime lazy activation for USER plugin caixas.
//!
//! The bundled default plugin catalog is applied eagerly at boot (the
//! binary merges it into the default plan). This host serves the OTHER
//! case: a user's lazy plugins installed in the plugins dir, declared
//! with `(defplugin :caixa … :ativar-em ("Command: Foo"))`. Such a
//! plugin's escriba entry is NOT applied until its trigger fires — the
//! lazy.nvim model, typed.
//!
//! The host stores each lazy plugin's entry source + its triggers. When
//! the editor runs a command / opens a filetype / fires an event, the
//! matching not-yet-activated plugins have their entries applied to live
//! [`EditorState`](crate::EditorState) through the SAME escriba-lisp
//! apply paths a user rc uses. Activation is one-shot (a plugin's entry
//! is applied at most once).
//!
//! This closes the gap where `escriba-plugin`'s `PluginCaixa` loader
//! could parse triggers but nothing in the runtime ever fired them.

/// A lazy-load trigger — the runtime-native projection of
/// `escriba_plugin::ActivationTrigger` (the binary parses the caixa's
/// `:ativar-em` strings and registers these). `Startup` plugins are
/// applied eagerly by the binary, never registered here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LazyTrigger {
    /// Activate when a buffer of this filetype opens.
    FileType(String),
    /// Activate when this editor event fires.
    Event(String),
    /// Activate when this command is first invoked.
    Command(String),
}

/// One registered lazy plugin: identity, triggers, its escriba entry
/// source, and whether it has activated yet.
#[derive(Debug, Clone)]
struct LazyPlugin {
    name: String,
    triggers: Vec<LazyTrigger>,
    entry_src: String,
    activated: bool,
}

/// Holds the editor's registered lazy plugins and decides which to
/// activate when a trigger fires. The returned entry sources are
/// applied by [`EditorState`](crate::EditorState) (which owns the
/// keymap / command registry / option store).
#[derive(Debug, Clone, Default)]
pub struct PluginHost {
    plugins: Vec<LazyPlugin>,
}

impl PluginHost {
    /// Register a lazy plugin. `triggers` empty ⇒ the plugin would be
    /// eager; the binary applies those at boot and never registers them
    /// here, but an empty-trigger registration is harmless (it simply
    /// never activates).
    pub fn register(&mut self, name: impl Into<String>, triggers: Vec<LazyTrigger>, entry_src: impl Into<String>) {
        self.plugins.push(LazyPlugin {
            name: name.into(),
            triggers,
            entry_src: entry_src.into(),
            activated: false,
        });
    }

    /// How many registered plugins have not activated yet.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.plugins.iter().filter(|p| !p.activated).count()
    }

    /// Names of the registered lazy plugins (for `plugin list` / tests).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.plugins.iter().map(|p| p.name.as_str())
    }

    /// Mark + drain the entry sources of every not-yet-activated plugin
    /// whose triggers match `want`. The caller applies the returned
    /// sources to live editor state.
    fn take_matching(&mut self, want: &LazyTrigger) -> Vec<String> {
        let mut out = Vec::new();
        for p in &mut self.plugins {
            if !p.activated && p.triggers.iter().any(|t| t == want) {
                p.activated = true;
                out.push(p.entry_src.clone());
            }
        }
        out
    }

    /// Entry sources to apply when `command` is first invoked.
    pub fn pending_for_command(&mut self, command: &str) -> Vec<String> {
        self.take_matching(&LazyTrigger::Command(command.to_string()))
    }

    /// Entry sources to apply when a buffer of `filetype` opens.
    pub fn pending_for_filetype(&mut self, filetype: &str) -> Vec<String> {
        self.take_matching(&LazyTrigger::FileType(filetype.to_string()))
    }

    /// Entry sources to apply when `event` fires.
    pub fn pending_for_event(&mut self, event: &str) -> Vec<String> {
        self.take_matching(&LazyTrigger::Event(event.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> PluginHost {
        let mut h = PluginHost::default();
        h.register(
            "user-trouble",
            vec![LazyTrigger::Command("Trouble".into())],
            r#"(defkeybind :mode "normal" :key "<leader>xx" :action "trouble.toggle")"#,
        );
        h.register(
            "user-markdown",
            vec![LazyTrigger::FileType("markdown".into())],
            r#"(defoption :name "md" :value "on")"#,
        );
        h
    }

    #[test]
    fn command_trigger_returns_entry_once() {
        let mut h = host();
        assert_eq!(h.pending(), 2);
        let first = h.pending_for_command("Trouble");
        assert_eq!(first.len(), 1);
        assert!(first[0].contains("trouble.toggle"));
        assert_eq!(h.pending(), 1, "activated plugin is no longer pending");
        // Second fire of the same command yields nothing (one-shot).
        assert!(h.pending_for_command("Trouble").is_empty());
    }

    #[test]
    fn filetype_trigger_isolated_from_command() {
        let mut h = host();
        assert!(h.pending_for_command("Other").is_empty());
        let md = h.pending_for_filetype("markdown");
        assert_eq!(md.len(), 1);
        assert!(md[0].contains(":name \"md\""));
        assert_eq!(h.pending(), 1);
    }

    #[test]
    fn non_matching_trigger_activates_nothing() {
        let mut h = host();
        assert!(h.pending_for_event("BufWritePre").is_empty());
        assert_eq!(h.pending(), 2);
    }
}
