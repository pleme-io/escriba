//! The Insert-mode erase verbs survive the SHIPPED boot plan.
//!
//! ## The class this exists for
//!
//! `Keymap::default_vim()` binds the erase family, and then the baked-in
//! defaults + the 45-caixa catalog are applied ON TOP of that same keymap.
//! Applying a plan only ever *adds* bindings — but adding one for a key that
//! is already bound DISPLACES what was there. `note_collisions` records that
//! as a `Collision::Displaced` and binds anyway, deliberately: an rc is
//! allowed to override a default.
//!
//! What it is not allowed to do is override a working core verb with an
//! action nothing can execute. That is what happened: `escriba-luasnip`
//! bound `<C-h>` to `snippet.jump-prev`, the snippet engine is not wired, and
//! so `<C-h>` — which is backspace on every terminal that sends 0x08 —
//! silently became a dead key in the DEFAULT build. Unit tests could not see
//! it; they all run against `Keymap::default_vim()`, which is correct. Only
//! the composite plan is wrong, and only a test that builds the composite
//! plan can say so.
//!
//! ## The gate
//!
//! Build the keymap the binary really boots with and assert that every erase
//! key still reaches its typed erase `Action`. A caixa that takes one of
//! these keys fails the build with the caixa's own action name in the
//! message, so the fix is obvious from the failure alone.
//!
//! Deliberately narrow: it pins the ERASE family only. Every other key stays
//! freely overridable, because overriding is what an rc is FOR.

use escriba_core::{Action, Mode};
use escriba_keymap::{Key, Keymap};

/// The erase verbs, and what each must still do after the defaults land.
///
/// `<C-h>` is here for the same reason `<BS>` is: which of the two a given
/// terminal reports for the physical Backspace key is the terminal's
/// business, so both have to work or the operator's Backspace works on some
/// machines and not others.
const ERASE_VERBS: &[(Key, Action, &str)] = &[
    (Key::Backspace, Action::Backspace, "<BS>"),
    (Key::Ctrl('h'), Action::Backspace, "<C-h>"),
    (Key::Delete, Action::DeleteForward, "<Del>"),
    (Key::Ctrl('w'), Action::DeleteWordBefore, "<C-w>"),
    (Key::Ctrl('u'), Action::DeleteToLineStart, "<C-u>"),
];

/// The keymap the binary boots with: vim defaults, then the shipped plan.
fn shipped_keymap() -> Keymap {
    let plan = escriba::default_plan(false).expect("shipped defaults parse");
    let mut km = Keymap::default_vim();
    escriba_lisp::apply_plan_to_keymap(&plan, &mut km);
    km
}

#[test]
fn no_bundled_caixa_shadows_an_insert_mode_erase_verb() {
    let km = shipped_keymap();
    for (key, expected, spelling) in ERASE_VERBS {
        let found = km
            .lookup(Mode::Insert, key)
            .map(|b| b.action.clone())
            .unwrap_or_else(|| panic!("Insert-mode {spelling} is unbound in the shipped keymap"));
        assert_eq!(
            &found, expected,
            "Insert-mode {spelling} no longer erases — a shipped caixa or the \
             defaults rc rebound it to {found:?}. Erase verbs are the one \
             family an rc may not take: move the new binding to a free key.",
        );
    }
}

#[test]
fn the_gate_would_notice_a_key_that_is_merely_bound() {
    // Teeth, not vacuity. The assertion above compares against the TYPED
    // action, so a caixa that rebinds an erase key to a plausible-looking
    // `Action::Command` fails it — which is exactly the shape of the defect
    // that motivated this file (`<C-h>` was bound the whole time; it was
    // bound to something that does nothing).
    let km = shipped_keymap();
    let bs = km
        .lookup(Mode::Insert, &Key::Ctrl('h'))
        .map(|b| b.action.clone());
    assert_ne!(
        bs,
        Some(Action::Command {
            name: "snippet.jump-prev".to_string(),
            args: Vec::new(),
        }),
        "the exact regression this file was written for",
    );
}

#[test]
fn the_erase_verbs_are_reachable_in_the_default_keymap_too() {
    // The other half of the same claim: the plan cannot preserve what was
    // never there. Cheap, and it localises a failure to "the default keymap"
    // vs "the shipped plan" instead of leaving both suspect.
    let km = Keymap::default_vim();
    for (key, expected, spelling) in ERASE_VERBS {
        assert_eq!(
            km.lookup(Mode::Insert, key)
                .map(|b| b.action.clone())
                .as_ref(),
            Some(expected),
            "Insert-mode {spelling} is wrong in Keymap::default_vim()",
        );
    }
}
