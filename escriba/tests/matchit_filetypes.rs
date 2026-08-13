//! Every filetype `%`'s word-pair table names is one the editor can actually
//! resolve.
//!
//! The table is keyed by FILETYPE NAME, and the names come from the shipped
//! `(defmajor-mode …)` declarations. Those two lists live in different files
//! and neither compiler nor test previously connected them, so a table entry
//! for a name nothing produces would be dead: `%` on a shell `if` would fall
//! back to bracket-only and report nothing at all, which reads exactly like
//! "escriba doesn't do matchit" rather than "the key is spelled wrong".
//!
//! Asserted in ONE direction only. A shipped major mode with no word pairs is
//! correct and common — Rust, JSON, TOML, and every other brace language
//! belongs in that set, and demanding an entry for each would be demanding
//! entries that should not exist.
//!
//! ## The inert rows are pinned as a SET, not tolerated
//!
//! Four of the six word-pair languages have no shipped `(defmode …)` yet, so
//! `%` in a Ruby file is bracket-only today. The rows stay — the grammar of
//! `if`/`elsif`/`end` does not change when a major mode lands, and deleting
//! correct knowledge to make a list shorter is how it gets re-derived wrong
//! later — but they are pinned by SET EQUALITY, the same shape
//! `escriba/tests/action_resolution.rs` uses for bound-but-inert keybinds.
//!
//! So: a NEW dead row fails (it is not in the list), and shipping the major
//! mode ALSO fails (it must be promoted out of the list). Neither direction
//! can drift quietly, which is the only way "declared but unreachable" is an
//! honest state rather than a wrong one.

use escriba_core::FiletypeTable;

/// The filetype names `escriba_runtime`'s `WORD_PAIRS` table is keyed by.
///
/// Duplicated here deliberately: the table is a private implementation detail
/// of the runtime, and widening the public surface just so a test could read
/// it costs more than one short list.
const MATCHIT_FILETYPES: &[&str] = &["lua", "ruby", "sh", "bash", "elixir", "vim"];

/// The word-pair languages the editor cannot yet resolve, so `%` falls back
/// to bracket-only in them. Exact — see the module docs.
const NOT_YET_A_MAJOR_MODE: &[&str] = &["bash", "elixir", "ruby", "vim"];

fn shipped_filetypes() -> FiletypeTable {
    let plan = escriba::default_plan(false).expect("shipped defaults parse");
    let mut table = FiletypeTable::new();
    escriba_lisp::apply_plan_to_filetypes(&plan, &mut table);
    table
}

#[test]
fn the_unreachable_word_pair_languages_are_exactly_the_declared_ones() {
    let table = shipped_filetypes();
    let mut unreachable: Vec<&str> = MATCHIT_FILETYPES
        .iter()
        .copied()
        .filter(|name| table.by_name(name).is_none())
        .collect();
    unreachable.sort_unstable();
    assert_eq!(
        unreachable, NOT_YET_A_MAJOR_MODE,
        "the set of word-pair languages `%` cannot reach has changed. A NEW \
         one means a row keyed by a filetype nothing produces; a MISSING one \
         means its `(defmode …)` shipped and the row is now live — promote \
         it out of `NOT_YET_A_MAJOR_MODE` either way.",
    );
}

#[test]
fn at_least_one_word_pair_language_is_actually_reachable() {
    // Without this, the test above is satisfied by a table where EVERY row is
    // dead — which is a matchit implementation nobody can press.
    let table = shipped_filetypes();
    let live: Vec<&str> = MATCHIT_FILETYPES
        .iter()
        .copied()
        .filter(|name| table.by_name(name).is_some())
        .collect();
    assert!(
        !live.is_empty(),
        "no word-pair language resolves, so `%` is bracket-only everywhere",
    );
}
