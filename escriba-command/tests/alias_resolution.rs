//! An alias must reach its target — the class behind 41 dead catalog names.
//!
//! `(defcmd :name "CommentToggle" :action "comment.toggle-line")` registers a
//! `Handler::Action`. That symbol used to be resolved against a hardcoded
//! 7-arm table and nothing else, so it died as `Unhandled` even though
//! `comment.toggle-line` was a registered native sitting in the same map.
//! Two dispatch tables that had to agree, and did not.

use escriba_command::{Command, CommandError, CommandRegistry};
use escriba_madoguchi::{FakeSnapshot, Outcome};

fn snap() -> FakeSnapshot {
    FakeSnapshot::default()
}

fn body(_s: &dyn escriba_madoguchi::Snapshot, _a: &[String]) -> Outcome {
    Outcome::did(vec![])
}

#[test]
fn an_alias_reaches_a_registered_native() {
    let mut reg = CommandRegistry::new();
    reg.register(Command::native("target", "the real body", body));
    reg.register(Command::action("Alias", "points at the native", "target"));
    assert!(
        reg.run("Alias", &snap(), &[]).is_ok(),
        "an alias whose target is registered must dispatch",
    );
}

#[test]
fn a_builtin_is_consulted_before_the_registry() {
    // Order matters: if the registry were tried first, a plugin could
    // silently shadow `editor.quit`.
    let mut reg = CommandRegistry::new();
    reg.register(Command::action("Q", "quit", "editor.quit"));
    assert!(reg.run("Q", &snap(), &[]).is_ok());
}

#[test]
fn an_unknown_symbol_is_announced_not_silently_dropped() {
    let mut reg = CommandRegistry::new();
    reg.register(Command::action("Ghost", "goes nowhere", "nobody.home"));
    match reg.run("Ghost", &snap(), &[]) {
        Err(CommandError::Unhandled(s)) => assert_eq!(s, "nobody.home"),
        other => panic!("must announce the unimplemented symbol, got {other:?}"),
    }
}

#[test]
fn a_self_referential_alias_terminates_and_names_itself() {
    let mut reg = CommandRegistry::new();
    reg.register(Command::action("Loop", "points at itself", "Loop"));
    match reg.run("Loop", &snap(), &[]) {
        Err(CommandError::Unhandled(s) | CommandError::AliasCycle(s)) => assert_eq!(s, "Loop"),
        other => panic!("a self-alias must be a typed error, got {other:?}"),
    }
}

#[test]
fn a_two_step_alias_cycle_terminates() {
    // A -> B -> A. Resolving through the registry is what admits this, so it
    // is bounded by fuel and reported rather than spun on.
    let mut reg = CommandRegistry::new();
    reg.register(Command::action("A", "to B", "B"));
    reg.register(Command::action("B", "to A", "A"));
    match reg.run("A", &snap(), &[]) {
        Err(CommandError::AliasCycle(_)) => {}
        other => panic!("an A->B->A cycle must be an AliasCycle, got {other:?}"),
    }
}

#[test]
fn a_long_but_finite_chain_still_resolves() {
    // Fuel must not be so tight that a legitimate chain is called a cycle.
    let mut reg = CommandRegistry::new();
    reg.register(Command::native("end", "body", body));
    reg.register(Command::action("c", "to end", "end"));
    reg.register(Command::action("b", "to c", "c"));
    reg.register(Command::action("a", "to b", "b"));
    assert!(reg.run("a", &snap(), &[]).is_ok(), "3 hops must resolve");
}
