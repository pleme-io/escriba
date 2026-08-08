//! Does escriba claim a chord the WORLD owns?
//!
//! A binding that collides with the OS or the window manager produces no
//! error, no warning, and no keypress — the event is consumed before escriba
//! sees it. It looks exactly like a bug in whatever the key was bound to,
//! which is the worst failure shape available.
//!
//! `awase::Reserved` carries that fact for the fleet, sourced from
//! `nix/profiles/darwin-developer/home/window-management.nix` — the file that
//! actually puts `alt-hjkl` beyond an app's reach.
//!
//! **The grammar this protects:** `hjkl` is DIRECTION everywhere in the
//! fleet, and the PREFIX is the scope. `alt-` moves between OS windows;
//! `<C-w>` moves between escriba's panes. Three places already agree on that
//! by convention and none of them wrote it down. This is the half that can be
//! enforced.

use awase::Reserved;
use escriba_core::Mode;
use escriba_keymap::{Keymap, to_hotkey};

const DEFAULT_RC: &str = include_str!("../configs/blnvim-defaults.lisp");

/// Every mode escriba binds in.
const MODES: &[Mode] = &[
    Mode::Normal,
    Mode::Insert,
    Mode::Visual,
    Mode::VisualLine,
    Mode::Command,
];

/// The keymap escriba actually boots with: typed defaults + the shipped rc +
/// the whole bundled plugin catalog.
fn shipped_keymap() -> Keymap {
    let mut km = Keymap::default_vim();
    let mut plan = escriba_lisp::apply_source(DEFAULT_RC).expect("defaults parse");
    plan.merge(
        escriba::catalog_bundle::bundled_plan_excluding(&Default::default())
            .expect("bundled catalog parses"),
    );
    escriba_lisp::apply_plan_to_keymap(&plan, &mut km);
    km
}

#[test]
fn no_shipped_binding_claims_a_chord_the_world_owns() {
    let reserved = Reserved::fleet_darwin();
    let km = shipped_keymap();
    let mut refused = Vec::new();

    for mode in MODES {
        // Single keys.
        for (m, key, b) in km.entries_sorted() {
            if m != mode {
                continue;
            }
            if let Some(hk) = to_hotkey(key) {
                if let Some(why) = reserved.refuse(&hk) {
                    refused.push(format!("{:?} {key:?} ({}) — {why}", m, b.description));
                }
            }
        }
        // …and the FIRST key of every sequence. A sequence whose opener is
        // reserved can never begin, and `sequences` was invisible from
        // outside escriba-keymap until `sequences_extending` landed.
        for (seq, b) in km.sequences_extending(*mode, &[]) {
            let Some(first) = seq.first() else { continue };
            if let Some(hk) = to_hotkey(first) {
                if let Some(why) = reserved.refuse(&hk) {
                    refused.push(format!(
                        "{mode:?} {seq:?} ({}) — its opener is {why}",
                        b.description
                    ));
                }
            }
        }
    }

    assert!(
        refused.is_empty(),
        "escriba binds {} chord(s) the world owns; each would silently never \
         fire:\n  {}",
        refused.len(),
        refused.join("\n  "),
    );
}

#[test]
fn the_audit_can_actually_see_escribas_bindings() {
    // A vacuous audit passes by checking nothing. This asserts the corpus is
    // real — the failure mode where `to_hotkey` returns None for everything,
    // or the plan does not load, would otherwise read as a clean bill.
    let km = shipped_keymap();
    let singles = km.entries_sorted().len();
    let seqs = km.sequences_extending(Mode::Normal, &[]).len();
    assert!(singles > 20, "only {singles} single-key bindings seen");
    assert!(seqs > 20, "only {seqs} sequences seen");

    // What the audit CANNOT see must be named, not tolerated silently. This
    // guard found `Ctrl(' ')` — Ctrl+Space, bound for completion and owned by
    // the OS — hiding behind a `None` from the conversion.
    let unmapped: Vec<String> = km
        .entries_sorted()
        .iter()
        .filter(|(_, k, _)| to_hotkey(k).is_none())
        .map(|(m, k, _)| format!("{m:?} {k:?}"))
        .collect();
    let expected: Vec<&str> = vec![
        // Shifted punctuation, genuinely outside awase's `Key` vocabulary.
        // Unaudited, and listed so that a NEW unmappable key fails here
        // rather than quietly widening the blind spot.
        "Normal Char('#')",
        "Normal Char('$')",
        "Normal Char('*')",
    ];
    assert_eq!(
        unmapped, expected,
        "the set of keys the audit cannot see has changed. A key with no \
         fleet spelling is UNAUDITED, not clean.",
    );
}

#[test]
fn the_audit_would_catch_a_collision_if_one_were_added() {
    // The RED RUN. Without this the test above is unfalsifiable: it passes
    // today because escriba is clean, and would keep passing if `refuse`
    // silently returned None for everything.
    let reserved = Reserved::fleet_darwin();
    let alt_j = to_hotkey(&escriba_keymap::Key::Alt('j')).expect("alt-j maps");
    assert!(
        reserved.refuse(&alt_j).is_some(),
        "binding alt-j must be refused — it is aerospace's focus-down",
    );
    // And escriba's real Alt bindings are NOT reserved, so the pass above is
    // a fact about escriba rather than about an empty reserved set.
    for c in ['f', 'b', 'u', 'd'] {
        let hk = to_hotkey(&escriba_keymap::Key::Alt(c)).expect("maps");
        assert!(
            reserved.refuse(&hk).is_none(),
            "alt-{c} is escriba's sexp navigation and must stay available",
        );
    }
}
