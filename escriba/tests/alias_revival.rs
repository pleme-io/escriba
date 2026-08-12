//! How many of the shipped catalog's `defcmd` aliases actually dispatch?
//!
//! Every `(defcmd … :action "x.y")` registers a `Handler::Action`. Before
//! action resolution went through ONE bounded path, that symbol was matched
//! against a hardcoded 7-arm table and nothing else — so an alias whose
//! target was a registered native (`comment.toggle-line`) died as
//! `Unhandled` anyway. This measures the real catalog rather than a fixture.

use escriba_command::{CommandError, CommandRegistry};
use escriba_madoguchi::FakeSnapshot;

const DEFAULT_RC: &str = include_str!("../configs/blnvim-defaults.lisp");

#[test]
fn the_shipped_aliases_that_name_a_registered_command_now_dispatch() {
    // The BASELINE plus the 45-plugin bundled catalog — the plan the binary
    // actually boots. The catalog is where the aliases live; measuring the
    // baseline alone would report a healthy 0 and prove nothing.
    let mut plan = escriba_lisp::apply_source(DEFAULT_RC).expect("defaults parse");
    plan.merge(
        escriba::catalog_bundle::bundled_plan_excluding(&Default::default())
            .expect("bundled catalog parses"),
    );
    let mut reg = CommandRegistry::default_set();
    escriba_lisp::apply_plan_to_commands(&plan, &mut reg);

    let snap = FakeSnapshot::default();
    let mut unhandled = Vec::new();
    let mut dispatched = 0usize;
    for name in reg.names() {
        match reg.run(name, &snap, &[]) {
            Ok(_) => dispatched += 1,
            Err(CommandError::Unhandled(sym)) => unhandled.push(sym),
            Err(_) => {}
        }
    }

    // The specific alias that motivated the fix: `:CommentToggle` names
    // `comment.toggle-line`, which IS a registered native.
    assert!(
        !unhandled.iter().any(|s| s == "comment.toggle-line"),
        "`comment.toggle-line` is a registered native — an alias naming it \
         must reach it. Still unhandled: {unhandled:?}",
    );
    assert!(
        dispatched > 0,
        "some command must dispatch, or this test proves nothing",
    );

    // THE SECOND RATCHET.
    //
    // `action_resolution.rs` pins the inert KEYBIND actions. It walks
    // `plan.keybinds` and nothing else, so this population — command names
    // declared by `(defcmd … :action …)` — was invisible to it. 41 of them
    // resolve to nothing, and none of that was gated.
    //
    // Set equality, same as the keybind ratchet: a new dead alias fails
    // because it is not in the list, and wiring a subsystem fails until its
    // entries are removed. `Unhandled` is the CORRECT answer for a name whose
    // subsystem does not exist — what must never happen is it being UNCOUNTED.
    unhandled.sort();
    unhandled.dedup();
    let expected: Vec<String> = UNHANDLED_ALIASES.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(
        unhandled, expected,
        "the unhandled-alias inventory moved.\n\
         Wiring a subsystem? Remove its entries from UNHANDLED_ALIASES.\n\
         Added a plugin? Its action must either resolve or be listed.\n\
         dispatched={dispatched}",
    );
}

/// Command names the shipped catalog declares whose action symbol resolves to
/// no body. Every one names a subsystem escriba has not built.
///
/// This list only shrinks. It is the `defcmd` peer of
/// `escriba/tests/action_resolution.rs`'s keybind inventory.
const UNHANDLED_ALIASES: &[&str] = &[
    "ai.chat",
    "ai.inline",
    "autopairs.toggle",
    "cmp.toggle",
    "colorizer.toggle",
    "comment.toggle-context-aware",
    "dap.continue",
    "dap.toggle-breakpoint",
    "git.diff",
    "git.status",
    "indent.toggle",
    "leap.forward",
    // `lsp.format` dispatches as of 2026-08-12 — see `escriba-command`'s
    // `LspFormat`. It left this list and `action_resolution.rs`'s INERT set in
    // the same commit; both are set-equality ratchets, and needing to edit BOTH
    // is the point — two independent inventories of what is dead cannot quietly
    // disagree about it.
    "lsp.info",
    "lsp.outline",
    "lsp.restart",
    "lsp.signature",
    "markdown.toggle-render",
    "mason.install",
    "mason.open",
    "noice.dismiss",
    "notify.history",
    "snacks.dashboard",
    "snacks.zen",
    "task.run",
    "task.toggle",
    "test.run-nearest",
    "test.toggle-summary",
    "todo.quickfix",
    "tree.find-file",
    "tree.toggle",
    "treesitter.install",
    "treesitter.playground",
    "treesitter.update",
    "ts-autotag.toggle",
    "ts-context.toggle",
    "whichkey.show",
];
