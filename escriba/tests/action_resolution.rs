//! Every `:action` string the SHIPPED config authors, checked against what
//! the editor can actually do with it.
//!
//! ## The class this exists for
//!
//! An `:action` that resolves to no typed [`Action`] variant falls back to
//! `Action::Command { name }`, and a `Command` naming nothing in the
//! registry dispatches to nothing at all — silently. That fallback is
//! deliberate and load-bearing (it is how a plugin-registered command
//! resolves later), but it means a TYPO and a NOT-YET-WIRED SUBSYSTEM are
//! indistinguishable at parse time, at apply time, and at dispatch time.
//!
//! The bug that motivated this: `resolve_action` had no `"search-forward"`
//! entry, so `:action "search-forward"` quietly became a lookup for a
//! command that has never existed. Nothing anywhere said so.
//!
//! ## The gate
//!
//! `INERT` below is the exact, complete inventory of shipped keybind
//! actions that resolve to nothing today — the "bound-but-inert" set
//! CLAUDE.md describes in prose. Asserting SET EQUALITY makes that prose
//! checkable and makes the list a ratchet in both directions:
//!
//! - a NEW unresolvable action (a typo, or a subsystem wired to a name
//!   nobody registers) fails, because it is not in the list;
//! - WIRING a subsystem fails until its names are removed from the list,
//!   so the honest inventory cannot silently overstate what is missing.
//!
//! Splash entries are held to the stricter bar — see the second test.

use std::collections::BTreeSet;

use escriba_command::CommandRegistry;
use escriba_core::Action;

/// Shipped keybind actions that resolve to no typed action AND no
/// registered command.
///
/// This list only ever SHRINKS. `buffer.{next,prev,delete}` and
/// `comment.toggle-{line,block}` left it in madoguchi M5 — the first five,
/// and the moment the ratchet was shown to turn downward rather than merely
/// hold. Every one is a real capability awaiting its
/// subsystem wave (a running LSP client, a picker, a git layer, DAP, …),
/// not a mistake. Grouped by the subsystem that will retire it.
const INERT: &[&str] = &[
    // AI assist — needs an MCP *client* inside the editor.
    "ai.chat",
    "ai.inline",
    // Completion — needs a completion engine.
    "cmp.abort",
    "cmp.complete",
    // Merge conflicts — needs the git layer.
    "conflict.choose-both",
    "conflict.choose-ours",
    "conflict.choose-theirs",
    "conflict.next",
    "conflict.prev",
    // Debugging — needs escriba-dap-client (Wave 3).
    "dap.continue",
    "dap.repl",
    "dap.step-into",
    "dap.step-out",
    "dap.step-over",
    "dap.toggle-breakpoint",
    "dap.toggle-ui",
    // File browsing — needs escriba-tree (Wave 3).
    "files.open",
    "files.open-parent",
    // Folding — needs the tree-sitter fold layer (Wave 4).
    "fold.close-all",
    "fold.open-all",
    "fold.toggle",
    // Git — needs escriba-git (Wave 3).
    "git.blame",
    "git.commit",
    "git.diff",
    "git.next-hunk",
    "git.prev-hunk",
    "git.preview-hunk",
    "git.reset-hunk",
    "git.stage-hunk",
    "git.status",
    // Reference highlighting — needs the LSP client.
    "illuminate.next",
    "illuminate.prev",
    // Motion plugin — needs the leap layer.
    "leap.backward",
    "leap.forward",
    // LSP — needs a RUNNING escriba-lsp-client (Wave 3).
    "lsp.code-action",
    "lsp.definition",
    "lsp.diagnostic-next",
    "lsp.diagnostic-prev",
    "lsp.format",
    "lsp.hover",
    "lsp.implementation",
    "lsp.incoming-calls",
    "lsp.outgoing-calls",
    "lsp.outline",
    "lsp.peek-definition",
    "lsp.references",
    "lsp.rename",
    "lsp.signature",
    "lsp.type-definition",
    // Window panes — needs split layout.
    "pane.down",
    "pane.left",
    "pane.right",
    "pane.up",
    "picker.symbols",
    // Fuzzy picker — needs escriba-picker (Wave 2).
    "picker.lsp-references",
    // snacks.nvim parity — needs the terminal + zen layers.
    "snacks.gitbrowse",
    "snacks.terminal",
    "snacks.zen",
    // Snippets — needs the snippet expander (Wave 4).
    "snippet.jump-next",
    "snippet.jump-prev",
    // Surround — needs the surround layer.
    "surround.add",
    "surround.change",
    "surround.delete",
    // Test running — needs the neotest layer.
    "test.run-file",
    "test.run-nearest",
    "test.run-suite",
    "test.toggle-output",
    "test.toggle-summary",
    // File tree — needs escriba-tree (Wave 3).
    "tree.toggle",
    // Diagnostics list — needs the LSP client.
    "trouble.document",
    "trouble.toggle",
    "trouble.workspace",
    // Which-key popup — needs the which-key layer (Wave 2).
    "whichkey.show",
];

/// The composite plan the editor really boots with, plus the command
/// registry as it exists after every `defcmd` has been applied.
fn shipped() -> (escriba_lisp::ApplyPlan, CommandRegistry) {
    let plan = escriba::default_plan(false).expect("shipped defaults parse");
    let mut registry = CommandRegistry::default_set();
    escriba_lisp::apply_plan_to_commands(&plan, &mut registry);
    (plan, registry)
}

/// Does this action string reach something the editor can execute?
fn resolves(action: &str, registry: &CommandRegistry) -> bool {
    match escriba_lisp::resolve_action(action) {
        // A typed variant is executable by construction.
        (_, false) => true,
        // The registry fallback is executable only if the name is there.
        (Action::Command { name, .. }, true) => registry.contains(&name),
        (_, true) => unreachable!("the deferred arm always yields Action::Command"),
    }
}

#[test]
fn the_inert_action_inventory_is_exactly_what_we_claim_it_is() {
    let (plan, registry) = shipped();
    let found: BTreeSet<String> = plan
        .keybinds
        .iter()
        .map(|kb| kb.action.clone())
        .filter(|a| !resolves(a, &registry))
        .collect();
    let claimed: BTreeSet<String> = INERT.iter().map(|s| (*s).to_string()).collect();

    let unexpected: Vec<&String> = found.difference(&claimed).collect();
    assert!(
        unexpected.is_empty(),
        "NEW unresolvable action(s) — a typo, or a subsystem bound to a name \
         nothing registers. Fix the name, register the command, or add it to \
         INERT with the subsystem that will retire it:\n{unexpected:#?}",
    );

    let retired: Vec<&String> = claimed.difference(&found).collect();
    assert!(
        retired.is_empty(),
        "these are listed as inert but now RESOLVE — the subsystem landed. \
         Delete them from INERT so the inventory stops overstating what is \
         missing:\n{retired:#?}",
    );
}

#[test]
fn every_start_screen_entry_actually_does_something() {
    // Stricter than the keymap bar, deliberately. An inert KEYBIND is
    // invisible until someone presses it; an inert MENU ENTRY is printed
    // on the start screen as an offer. A menu that advertises a key and
    // then does nothing when pressed is worse than a shorter menu.
    let (plan, registry) = shipped();
    let splash = escriba_lisp::apply_plan_to_splash(&plan)
        .expect("the shipped rc declares an enabled start screen");
    assert!(!splash.entries.is_empty(), "a menu with no entries");

    for entry in &splash.entries {
        let executable = match &entry.action {
            Action::Command { name, .. } => registry.contains(name),
            _ => true,
        };
        assert!(
            executable,
            "start screen offers `{}` ({}) but nothing would happen",
            entry.key, entry.label,
        );
    }
}

#[test]
fn start_screen_keys_are_unique() {
    // Two entries on one key means the second is unreachable — a dead row
    // that still takes up a line and still reads as an offer.
    let (plan, _) = shipped();
    let splash = escriba_lisp::apply_plan_to_splash(&plan).expect("a start screen");
    let mut seen = BTreeSet::new();
    for e in &splash.entries {
        assert!(
            seen.insert(e.key),
            "duplicate start-screen key `{}` — the later entry is unreachable",
            e.key,
        );
    }
}

#[test]
fn a_typo_in_an_action_is_caught_by_the_same_check_the_gate_uses() {
    // Proves the gate's predicate has teeth rather than being vacuously
    // true — a deliberately broken name must read as unresolvable.
    let (_, registry) = shipped();
    assert!(
        !resolves("serch-forwrd", &registry),
        "a misspelled action must not read as resolvable",
    );
    assert!(
        resolves("search-forward", &registry),
        "the real one must — this is the exact string that was missing",
    );
}
