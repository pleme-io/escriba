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
    // `_` and `^` land on the same character and differ only in KIND — `_` is
    // linewise, so `d_` takes the line and `d^` takes back to the indent.
    // Listed adjacent on purpose: an "obvious simplification" that aliases
    // them silently turns `d_` back into the no-op it was until 2026-08-14.
    ('_', Motion::LinewiseDown),
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

/// The `z`-prefixed viewport verbs, which are `Action::ScrollView` rather
/// than motions — a different arm of the same table, so a different way to
/// lose them.
#[test]
fn the_scroll_verbs_survive_too() {
    use escriba_core::ViewAlign;
    let km = shipped_keymap();
    for (second, align) in [
        ('t', ViewAlign::Top),
        ('z', ViewAlign::Center),
        ('b', ViewAlign::Bottom),
    ] {
        let seq = vec![Key::Char('z'), Key::Char(second)];
        let found = km.lookup_sequence(Mode::Normal, &seq).map(|b| &b.action);
        assert_eq!(
            found,
            Some(&Action::ScrollView(align)),
            "`z{second}` was displaced in the shipped build",
        );
    }
}

/// `m`, `` ` `` and `'` must stay UNBOUND, for the same reason `f` must:
/// the runtime claims them before the keymap, so a binding on one is a table
/// entry no keypress can reach.
#[test]
fn the_mark_keys_are_deliberately_unbound() {
    let km = shipped_keymap();
    for c in ['m', '`', '\''] {
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

/// `<C-u>` in NORMAL is half-page-up, and the erase verb of the same name in
/// Insert must survive alongside it. Bindings are per-mode, and this pins
/// that both readings coexist rather than one having quietly replaced the
/// other.
#[test]
fn ctrl_u_is_half_page_up_in_normal_and_still_erases_in_insert() {
    let km = shipped_keymap();
    let at = |mode: Mode| {
        km.entries_sorted()
            .into_iter()
            .find(|(m, k, _)| **m == mode && **k == Key::Ctrl('u'))
            .map(|(_, _, b)| b.action.clone())
    };
    assert_eq!(at(Mode::Normal), Some(Action::Move(Motion::HalfPageUp)));
    assert_eq!(at(Mode::Insert), Some(Action::DeleteToLineStart));
}

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

/// The put keys, which are the other half of every operator above.
///
/// Held to the same bar for the same reason: an operator that captures text
/// the editor cannot put back is a delete key with extra steps, and `p` is
/// short, unmodified, and exactly the kind of key a picker or a git caixa
/// reaches for.
#[test]
fn the_put_keys_survive_too() {
    let km = shipped_keymap();
    for (c, before) in [('p', false), ('P', true)] {
        let found = km
            .entries_sorted()
            .into_iter()
            .find(|(m, k, _)| **m == Mode::Normal && **k == Key::Char(c))
            .map(|(_, _, b)| b.action.clone());
        assert_eq!(
            found,
            Some(Action::Put { before }),
            "`{c}` was displaced in the shipped build",
        );
    }
}

/// The single-key edit verbs.
///
/// Five of them are keymap entries over `Action::ApplyOperator` — they ARE the
/// compositions vim spells shorter — so losing one to a caixa loses a key that
/// LOOKS like it should be core. `x` and `s` in particular are short,
/// unmodified, and exactly what a picker or git caixa reaches for.
#[test]
fn the_edit_verbs_survive_too() {
    use escriba_core::{Motion, Operator, TextObject};
    let km = shipped_keymap();
    let expected: &[(char, Action)] = &[
        (
            'x',
            Action::ApplyOperator {
                op: Operator::Delete,
                motion: Motion::Right,
            },
        ),
        (
            'X',
            Action::ApplyOperator {
                op: Operator::Delete,
                motion: Motion::Left,
            },
        ),
        (
            'D',
            Action::ApplyOperator {
                op: Operator::Delete,
                motion: Motion::LineEnd,
            },
        ),
        (
            'C',
            Action::ApplyOperator {
                op: Operator::Change,
                motion: Motion::LineEnd,
            },
        ),
        // neovim's `Y` (= `y$`), not classic vim's (= `yy`). See the keymap.
        (
            'Y',
            Action::ApplyOperator {
                op: Operator::Yank,
                motion: Motion::LineEnd,
            },
        ),
        (
            's',
            Action::ApplyOperator {
                op: Operator::Change,
                motion: Motion::Right,
            },
        ),
        (
            'S',
            Action::ApplyOperatorObject {
                op: Operator::Change,
                object: TextObject::Line,
            },
        ),
        ('J', Action::JoinLines { space: true }),
    ];
    let mut lost = Vec::new();
    for (c, want) in expected {
        let found = km
            .entries_sorted()
            .into_iter()
            .find(|(m, k, _)| **m == Mode::Normal && **k == Key::Char(*c))
            .map(|(_, _, b)| b.action.clone());
        if found.as_ref() != Some(want) {
            lost.push(format!("`{c}` → {found:?} (wanted {want:?})"));
        }
    }
    assert!(
        lost.is_empty(),
        "a bundled caixa took an edit verb out of the shipped build:\n  {}",
        lost.join("\n  "),
    );
}

/// `r` must stay UNBOUND, and that is a REQUIREMENT rather than an oversight.
///
/// Its operand is a key claimed above the keymap (`consume_replace_key`), so a
/// binding on `r` is a table entry no keypress can reach — it reads as
/// configured and behaves as absent. Exactly the trap `f`/`t`/`T` documented,
/// asserted here so a caixa cannot quietly re-create it.
#[test]
fn the_replace_key_stays_unbound() {
    let km = shipped_keymap();
    let found = km
        .entries_sorted()
        .into_iter()
        .find(|(m, k, _)| **m == Mode::Normal && **k == Key::Char('r'))
        .map(|(_, _, b)| b.action.clone());
    assert_eq!(
        found, None,
        "`r` is bound in the shipped build; its operand capture makes that unreachable",
    );
}

/// The `g`-prefixed motions, which live in the SEQUENCE table rather than the
/// single-key one — a different lookup, so a different way to lose them.
#[test]
fn the_g_prefixed_motions_survive_too() {
    let km = shipped_keymap();
    for (second, motion) in [
        // `gg` was NEVER bound until 2026-08-13 — the only `gg` in the repo
        // was a unit test that bound it itself before pressing it. That is
        // the shape this whole file exists to catch, arriving from the other
        // direction: not a caixa taking a key, but a key nobody ever gave.
        ('g', Motion::DocStart),
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
