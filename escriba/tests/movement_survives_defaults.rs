//! The vim movement suite survives the SHIPPED boot plan.
//!
//! Same class as `insert_erase_survives_defaults.rs`, and the same reason it
//! needs its own file: every unit test in the repo builds
//! `Keymap::default_vim()`, which is correct, while the BINARY builds that
//! and then applies the baked-in defaults plus the 45-caixa catalog on top.
//! Applying a plan binds over what is already there — deliberately, an rc may
//! override a default — so a caixa can take a core motion key and every test
//! in the workspace stays green while the editor loses the key.
//!
//! `<C-h>` was found that way (a snippet caixa took backspace). This gate
//! covers the MOTIONS: they are the keys an operator presses most and the
//! ones whose loss is least likely to be reported as a bug rather than as
//! "the editor feels wrong".

use escriba_core::{Action, Mode, Motion};
use escriba_keymap::{Key, Keymap};

/// The keymap the binary boots with: vim defaults, then the shipped plan.
fn shipped_keymap() -> Keymap {
    let plan = escriba::default_plan(false).expect("shipped defaults parse");
    let mut km = Keymap::default_vim();
    escriba_lisp::apply_plan_to_keymap(&plan, &mut km);
    km
}

/// Every single-key motion, and the motion it must still resolve to.
const MOTION_KEYS: &[(char, Motion)] = &[
    ('h', Motion::Left),
    ('l', Motion::Right),
    ('j', Motion::Down),
    ('k', Motion::Up),
    ('w', Motion::WordStartNext),
    ('b', Motion::WordStartPrev),
    ('e', Motion::WordEndNext),
    ('W', Motion::BigWordStartNext),
    ('B', Motion::BigWordStartPrev),
    ('E', Motion::BigWordEndNext),
    ('0', Motion::LineStart),
    ('^', Motion::LineFirstNonBlank),
    ('$', Motion::LineEnd),
    ('G', Motion::DocEnd),
    ('%', Motion::MatchPair),
    ('{', Motion::ParagraphPrev),
    ('}', Motion::ParagraphNext),
    ('(', Motion::SentencePrev),
    (')', Motion::SentenceNext),
    ('H', Motion::ScreenTop),
    ('M', Motion::ScreenMiddle),
    ('L', Motion::ScreenBottom),
    ('+', Motion::LineDownFirstNonBlank),
    ('-', Motion::LineUpFirstNonBlank),
    ('|', Motion::Column(1)),
    (';', Motion::RepeatFind { reverse: false }),
];

#[test]
fn no_bundled_caixa_shadows_a_movement_key() {
    let km = shipped_keymap();
    let mut lost = Vec::new();
    for (c, motion) in MOTION_KEYS {
        let found = km
            .entries_sorted()
            .into_iter()
            .find(|(m, k, _)| **m == Mode::Normal && **k == Key::Char(*c))
            .map(|(_, _, b)| b.action.clone());
        if found.as_ref() != Some(&Action::Move(*motion)) {
            lost.push(format!("`{c}` → {found:?} (wanted {motion:?})"));
        }
    }
    assert!(
        lost.is_empty(),
        "a bundled caixa took a movement key out of the shipped build:\n  {}",
        lost.join("\n  "),
    );
}

/// The operators, for the same reason and with more at stake: an armed
/// operator is what turns every motion above into an edit, so a caixa taking
/// `d` would lose the whole suite at once rather than one key.
#[test]
fn the_operator_keys_survive_too() {
    use escriba_core::Operator;
    let km = shipped_keymap();
    for (c, op) in [
        ('d', Operator::Delete),
        ('c', Operator::Change),
        ('y', Operator::Yank),
    ] {
        let found = km
            .entries_sorted()
            .into_iter()
            .find(|(m, k, _)| **m == Mode::Normal && **k == Key::Char(c))
            .map(|(_, _, b)| b.action.clone());
        assert_eq!(
            found,
            Some(Action::Operator(op)),
            "`{c}` was displaced in the shipped build",
        );
    }
}

/// The `g`-prefixed motions, which live in the SEQUENCE table rather than the
/// single-key one — a different lookup, so a different way to lose them.
#[test]
fn the_g_prefixed_motions_survive_too() {
    let km = shipped_keymap();
    for (second, motion) in [
        ('e', Motion::WordEndPrev),
        ('E', Motion::BigWordEndPrev),
        ('_', Motion::LineLastNonBlank),
    ] {
        let seq = vec![Key::Char('g'), Key::Char(second)];
        let found = km.lookup_sequence(Mode::Normal, &seq).map(|b| &b.action);
        assert_eq!(
            found,
            Some(&Action::Move(motion)),
            "`g{second}` was displaced in the shipped build",
        );
    }
}

/// `f`/`F`/`t`/`T` must stay UNBOUND, which is the opposite property and just
/// as load-bearing: the runtime claims them before the keymap is consulted,
/// so a binding on one of them is unreachable — a table entry nobody can
/// press, which reads as configured and behaves as absent.
#[test]
fn the_find_keys_are_deliberately_unbound() {
    let km = shipped_keymap();
    for c in ['f', 'F', 't', 'T'] {
        let found = km
            .entries_sorted()
            .into_iter()
            .find(|(m, k, _)| **m == Mode::Normal && **k == Key::Char(c))
            .map(|(_, _, b)| b.action.clone());
        assert!(
            found.is_none(),
            "`{c}` is bound to {found:?}, but the runtime claims it first — \
             that binding can never fire",
        );
    }
}
