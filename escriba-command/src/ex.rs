//! The ex-command NAME GRAMMAR — vim's abbreviations, resolved in one place.
//!
//! `:wq` is not a command name. It is a *spelling* of one, and vim has a
//! whole grammar of them: every ex command has a full name, a minimum prefix
//! that selects it, and an optional `!`. `:w`, `:wr`, `:writ`, `:write` are
//! one command; `:q`, `:qu`, `:quit` are another; `:qa` and `:quita` are a
//! third, and the reason `:qu` is not the third is that `quitall`'s minimum
//! is five characters, not one.
//!
//! The runtime used to carry that knowledge as three arms —
//! `"w" => "save", "q" => "quit", "u" => "undo"` — and every other spelling
//! fell through to a registry lookup that could not possibly hold it. `:wq`
//! reported "command not found" while both halves of it worked.
//!
//! So the grammar is a TABLE, not a chain of ifs, and the table is the only
//! thing that knows how a typed word becomes a registered command. Two
//! properties follow, and both are asserted rather than asserted-to:
//!
//! - **Every valid abbreviation resolves.** For each verb, every prefix from
//!   its minimum to its full spelling selects it (`abbreviations_all_resolve`).
//! - **No abbreviation is ambiguous.** No typed word is a valid abbreviation
//!   of two verbs (`no_abbreviation_is_ambiguous`) — which is a property of
//!   the *minimums*, and the reason vim gives `quitall` a five-character one.
//!
//! A word the grammar does not know is passed through UNCHANGED to the
//! registry, so `:noh`, `:picker.files` and every plugin-registered command
//! still dispatch. The grammar covers the vim vocabulary; it does not fence
//! the command namespace.

/// One ex command's spelling rule.
///
/// `plain` and `forced` are separate registered command names rather than one
/// name plus a bang argument. A command body receives `&[String]` and nothing
/// else, so a bang passed as an argument is a convention every body has to
/// remember to read — and the one that forgets quits without asking. Two
/// names cannot be misread, and both show up in `--commands` saying what they
/// do. Verbs for which `!` changes nothing (writing has no force semantics
/// here — escriba has no read-only flag to override) point both fields at the
/// same command, which is the honest encoding of "the bang is accepted and
/// means nothing".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExVerb {
    /// The full spelling, as `:help ex-cmd-index` writes it.
    pub full: &'static str,
    /// The fewest characters that select it. `full[..min]` is what vim
    /// prints in square-bracket notation: `:q[uit]` is `("quit", 1)`.
    pub min: usize,
    /// The registered command the plain form dispatches to.
    pub plain: &'static str,
    /// The registered command the `!` form dispatches to.
    pub forced: &'static str,
}

impl ExVerb {
    /// Does `word` spell this verb? Prefix of the full name, at least `min`
    /// characters long — vim's rule exactly.
    #[must_use]
    pub fn spelled_by(&self, word: &str) -> bool {
        word.len() >= self.min && self.full.len() >= word.len() && self.full.starts_with(word)
    }

    /// The command name for this verb, banged or not.
    #[must_use]
    pub const fn command(&self, bang: bool) -> &'static str {
        if bang { self.forced } else { self.plain }
    }
}

/// The vim write/quit family plus the two verbs the old three-arm table
/// carried, with vim's own minimum prefixes.
///
/// Deliberately NOT the whole ex vocabulary: a verb belongs here once escriba
/// has something for it to dispatch to. An entry naming a command that is not
/// registered would report "declared but not implemented yet" — announced,
/// per [`crate::CommandError::Unhandled`], but still a promise the editor
/// cannot keep.
pub const VERBS: &[ExVerb] = &[
    // ── write ────────────────────────────────────────────────────────
    ExVerb { full: "write", min: 1, plain: "save", forced: "save" },
    ExVerb { full: "wall", min: 2, plain: "buffer.write-all", forced: "buffer.write-all" },
    // ── write-and-quit ───────────────────────────────────────────────
    ExVerb { full: "wq", min: 2, plain: "write-quit", forced: "write-quit" },
    ExVerb { full: "wqall", min: 3, plain: "write-quit-all", forced: "write-quit-all" },
    // `:x` differs from `:wq` by ONE thing and it is the thing that matters
    // to anything watching the file: it writes only when the buffer is
    // modified, so `:x` on an untouched file leaves the mtime alone and a
    // watching build does not rebuild.
    ExVerb { full: "xit", min: 1, plain: "exit-write", forced: "exit-write" },
    ExVerb { full: "xall", min: 2, plain: "write-quit-all", forced: "write-quit-all" },
    ExVerb { full: "exit", min: 3, plain: "exit-write", forced: "exit-write" },
    // ── quit ─────────────────────────────────────────────────────────
    ExVerb { full: "quit", min: 1, plain: "quit", forced: "quit!" },
    ExVerb { full: "qall", min: 2, plain: "quit-all", forced: "quit-all!" },
    ExVerb { full: "quitall", min: 5, plain: "quit-all", forced: "quit-all!" },
    // ── the two the old table carried ────────────────────────────────
    ExVerb { full: "undo", min: 1, plain: "undo", forced: "undo" },
    ExVerb { full: "redo", min: 3, plain: "redo", forced: "redo" },
];

/// The verb `word` spells, if any. `word` carries no `!` and no arguments.
#[must_use]
pub fn resolve(word: &str) -> Option<&'static ExVerb> {
    VERBS.iter().find(|v| v.spelled_by(word))
}

/// A parsed ex line: which command to run, and with what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The registered command name to dispatch.
    pub command: String,
    /// Everything after the command word.
    pub args: Vec<String>,
}

/// Parse a command line into the command to dispatch and its arguments.
///
/// `None` for an empty line — `:` then `<CR>` does nothing, as in vim, rather
/// than dispatching the empty name and reporting it missing.
///
/// A word the grammar knows resolves through [`VERBS`]; anything else is
/// passed through with its `!` intact, so what the operator typed is what the
/// "not found" report names. Silently stripping a bang off an unknown word
/// would turn `:Ghost!` into a report about `:Ghost`, which is a report about
/// a different thing than the one that failed.
#[must_use]
pub fn parse(line: &str) -> Option<Invocation> {
    let line = line.trim();
    let line = line.strip_prefix(':').unwrap_or(line);
    let mut parts = line.split_whitespace();
    let word = parts.next()?;
    let args: Vec<String> = parts.map(str::to_string).collect();
    let (head, bang) = word
        .strip_suffix('!')
        .map_or((word, false), |stripped| (stripped, true));
    let command = resolve(head).map_or_else(|| word.to_string(), |v| v.command(bang).to_string());
    Some(Invocation { command, args })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every abbreviation vim would accept reaches its verb. This is the
    /// all-variants proof: it walks the table rather than sampling it, so a
    /// verb added with a wrong `min` fails here rather than in an operator's
    /// hands.
    #[test]
    fn abbreviations_all_resolve() {
        for v in VERBS {
            for len in v.min..=v.full.len() {
                let word = &v.full[..len];
                assert_eq!(
                    resolve(word),
                    Some(v),
                    "`:{word}` must resolve to `{}`",
                    v.full,
                );
            }
        }
    }

    /// No typed word spells two verbs. Ambiguity here is invisible in normal
    /// use — [`resolve`] takes the first match, so the SECOND verb simply
    /// becomes unreachable at that spelling, which nothing else would notice.
    #[test]
    fn no_abbreviation_is_ambiguous() {
        for v in VERBS {
            for len in v.min..=v.full.len() {
                let word = &v.full[..len];
                let hits: Vec<&str> = VERBS
                    .iter()
                    .filter(|c| c.spelled_by(word))
                    .map(|c| c.full)
                    .collect();
                assert_eq!(hits.len(), 1, "`:{word}` is ambiguous: {hits:?}");
            }
        }
    }

    /// A shorter-than-minimum prefix resolves to nothing rather than to the
    /// wrong thing. `:q` must never be `:qall`.
    #[test]
    fn below_the_minimum_selects_nothing() {
        for v in VERBS {
            for len in 1..v.min {
                let word = &v.full[..len];
                let hit = resolve(word);
                assert!(
                    hit.is_none_or(|h| h.full != v.full),
                    "`:{word}` is below `{}`'s minimum and must not select it",
                    v.full,
                );
            }
        }
    }

    #[test]
    fn the_write_quit_family_reaches_its_commands() {
        for (typed, expect) in [
            ("w", "save"),
            ("write", "save"),
            ("wq", "write-quit"),
            ("wq!", "write-quit"),
            ("wqa", "write-quit-all"),
            ("wqall", "write-quit-all"),
            ("x", "exit-write"),
            ("xit", "exit-write"),
            ("xa", "write-quit-all"),
            ("exi", "exit-write"),
            ("exit", "exit-write"),
            ("wa", "buffer.write-all"),
            ("q", "quit"),
            ("q!", "quit!"),
            ("quit", "quit"),
            ("qa", "quit-all"),
            ("qa!", "quit-all!"),
            ("quita", "quit-all"),
            ("quitall", "quit-all"),
            ("u", "undo"),
            ("red", "redo"),
        ] {
            assert_eq!(
                parse(typed).map(|i| i.command),
                Some(expect.to_string()),
                "`:{typed}`",
            );
        }
    }

    #[test]
    fn a_leading_colon_and_surrounding_space_are_not_part_of_the_name() {
        assert_eq!(parse(":wq").map(|i| i.command), Some("write-quit".into()));
        assert_eq!(parse("  wq  ").map(|i| i.command), Some("write-quit".into()));
    }

    #[test]
    fn an_empty_line_dispatches_nothing() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("   "), None);
        assert_eq!(parse(":"), None);
    }

    #[test]
    fn an_unknown_word_passes_through_with_its_bang() {
        // Registry names must survive the grammar untouched…
        assert_eq!(parse("noh").map(|i| i.command), Some("noh".into()));
        assert_eq!(
            parse("picker.files").map(|i| i.command),
            Some("picker.files".into()),
        );
        // …and an unknown bang is reported as the operator typed it.
        assert_eq!(parse("Ghost!").map(|i| i.command), Some("Ghost!".into()));
    }

    #[test]
    fn arguments_survive_the_verb() {
        let i = parse("w  foo.txt  bar").expect("a verb with arguments parses");
        assert_eq!(i.command, "save");
        assert_eq!(i.args, vec!["foo.txt".to_string(), "bar".to_string()]);
    }
}
