//! Live-apply layer — resolve an [`ApplyPlan`] into runtime editor
//! state.
//!
//! Parsing the rc into a plan is the decoupled first pass; this
//! module is the second pass that actually mutates a [`Keymap`] (and,
//! in future waves, command registries, theme state, hook tables).
//!
//! Kept separate from the parse layer so tests can build a plan in
//! memory without touching live runtime types.
//!
//! # Key-string grammar
//!
//! Mirrors the zsh / vim / helix convention users already know:
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
//!
//! # Action-string grammar
//!
//! A curated set of well-known action names resolves directly into
//! [`Action`] variants (so the rc can bind `"move-left"` without
//! caring about the enum shape). Unknown action names fall back to
//! [`Action::Command`] — the command dispatcher resolves them at
//! runtime against the command registry.

use escriba_core::{Action, Mode, Motion};
use escriba_keymap::{Key, Keymap};

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
    /// Human-readable warnings for anything we had to skip entirely
    /// (unknown keys, malformed multi-key sequences). Never a
    /// fatal error — partial application is preferable to a broken
    /// editor on one typo.
    pub warnings: Vec<String>,
}

impl ApplyReport {
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "keybinds={} deferred={} warnings={}",
            self.keybinds_applied,
            self.keybinds_deferred_to_commands,
            self.warnings.len(),
        )
    }
}

/// Apply every [`KeybindSpec`] in `plan` to `keymap`. Multi-key
/// sequences like `"gh"` are currently deferred with a warning —
/// the keymap's pending-stroke machinery is the next step. Everything
/// else writes directly.
pub fn apply_plan_to_keymap(plan: &ApplyPlan, keymap: &mut Keymap) -> ApplyReport {
    let mut report = ApplyReport::default();
    for spec in &plan.keybinds {
        match apply_keybind(spec, keymap) {
            Ok(deferred) => {
                report.keybinds_applied += 1;
                if deferred {
                    report.keybinds_deferred_to_commands += 1;
                }
            }
            Err(warning) => {
                report.warnings.push(warning);
            }
        }
    }
    report
}

/// Apply a single keybind. Returns `Ok(true)` if the action resolved
/// via [`Action::Command`] fallback, `Ok(false)` if it hit a typed
/// variant, or `Err(warning)` if the spec couldn't be applied at
/// all.
fn apply_keybind(spec: &KeybindSpec, keymap: &mut Keymap) -> Result<bool, String> {
    let mode = parse_mode(&spec.mode)
        .map_err(|e| format!("defkeybind :mode {:?} — {e}", spec.mode))?;
    let key = parse_key(&spec.key)
        .map_err(|e| format!("defkeybind :key {:?} — {e}", spec.key))?;
    let (action, deferred) = resolve_action(&spec.action);
    let description = if spec.description.is_empty() {
        spec.action.clone()
    } else {
        spec.description.clone()
    };
    keymap.bind(mode, key, action, description);
    Ok(deferred)
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

/// Parse a key string into a [`Key`]. Returns an error for malformed
/// or unsupported sequences (including multi-key sequences — the
/// caller decides how to defer those).
fn parse_key(s: &str) -> Result<Key, String> {
    // Multi-key sequences (`"gh"`, `"jk"`) are not yet wired into the
    // keymap's pending-stroke machinery. Surface as a warning so the
    // user sees what happened.
    if s.chars().count() > 1 && !s.starts_with('<') {
        return Err(format!(
            "multi-key sequences aren't wired yet — `{s}` needs pending-stroke support"
        ));
    }

    // Single char form.
    if s.chars().count() == 1 {
        return s
            .chars()
            .next()
            .map(Key::Char)
            .ok_or_else(|| format!("`{s}` is not a printable char"));
    }

    // Bracketed form — parse inner token.
    if let Some(inner) = s.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
        return parse_bracket_key(inner)
            .ok_or_else(|| format!("unknown bracketed key `<{inner}>`"));
    }

    Err(format!("unrecognised key `{s}`"))
}

fn parse_bracket_key(inner: &str) -> Option<Key> {
    // Named keys (case-insensitive for the common ones).
    let lower = inner.to_ascii_lowercase();
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
    ];
    for (n, k) in named {
        if lower == *n {
            return Some(k.clone());
        }
    }

    // Modifier+char form: `C-r`, `A-f`, `M-f`. Dash-separated, single
    // char after the modifier.
    if let Some(rest) = inner.strip_prefix("C-").or_else(|| inner.strip_prefix("c-")) {
        return single_char(rest).map(Key::Ctrl);
    }
    if let Some(rest) = inner
        .strip_prefix("A-")
        .or_else(|| inner.strip_prefix("a-"))
        .or_else(|| inner.strip_prefix("M-"))
        .or_else(|| inner.strip_prefix("m-"))
    {
        return single_char(rest).map(Key::Alt);
    }

    None
}

fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let c = chars.next()?;
    if chars.next().is_none() { Some(c) } else { None }
}

/// Resolve an action string into an [`Action`]. Known strings map to
/// typed variants; everything else becomes a [`Action::Command`]
/// handoff (the registry resolves it at dispatch time).
fn resolve_action(name: &str) -> (Action, bool) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply_source;

    #[test]
    fn parse_single_char_key() {
        assert_eq!(parse_key("a").unwrap(), Key::Char('a'));
        assert_eq!(parse_key("G").unwrap(), Key::Char('G'));
        assert_eq!(parse_key("0").unwrap(), Key::Char('0'));
    }

    #[test]
    fn parse_named_bracket_keys() {
        assert_eq!(parse_key("<Esc>").unwrap(), Key::Esc);
        assert_eq!(parse_key("<CR>").unwrap(), Key::Enter);
        assert_eq!(parse_key("<Enter>").unwrap(), Key::Enter);
        assert_eq!(parse_key("<Tab>").unwrap(), Key::Tab);
        assert_eq!(parse_key("<Backspace>").unwrap(), Key::Backspace);
        assert_eq!(parse_key("<BS>").unwrap(), Key::Backspace);
        assert_eq!(parse_key("<Left>").unwrap(), Key::Left);
        assert_eq!(parse_key("<PageUp>").unwrap(), Key::PageUp);
    }

    #[test]
    fn parse_modifier_bracket_keys() {
        assert_eq!(parse_key("<C-r>").unwrap(), Key::Ctrl('r'));
        assert_eq!(parse_key("<c-r>").unwrap(), Key::Ctrl('r'));
        assert_eq!(parse_key("<A-f>").unwrap(), Key::Alt('f'));
        assert_eq!(parse_key("<M-f>").unwrap(), Key::Alt('f'));
    }

    #[test]
    fn parse_multi_key_rejected() {
        assert!(parse_key("gh").is_err());
    }

    #[test]
    fn parse_unknown_bracket_rejected() {
        assert!(parse_key("<Galactus>").is_err());
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
        let plan = apply_source(
            r#"(defkeybind :mode "normal" :key "g" :action "goto-home")"#,
        )
        .unwrap();
        let mut km = Keymap::new();
        let report = apply_plan_to_keymap(&plan, &mut km);
        assert_eq!(report.keybinds_applied, 1);
        assert_eq!(report.keybinds_deferred_to_commands, 1);
    }

    #[test]
    fn apply_warns_on_multi_key_sequence_without_failing() {
        let plan = apply_source(
            r#"(defkeybind :mode "normal" :key "gh" :action "home")"#,
        )
        .unwrap();
        let mut km = Keymap::new();
        let report = apply_plan_to_keymap(&plan, &mut km);
        assert_eq!(report.keybinds_applied, 0);
        assert_eq!(report.warnings.len(), 1);
        assert!(
            report.warnings[0].contains("multi-key"),
            "warning should mention the unsupported form: {:?}",
            report.warnings[0]
        );
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
        let plan = apply_source(
            r#"(defmode :name "plain" :extensions ("txt" "log"))"#,
        )
        .unwrap();
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
        let plan = apply_source(
            r#"(defkeybind :mode "normal" :key "h" :action "move-right")"#,
        )
        .unwrap();
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
