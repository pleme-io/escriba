use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Cursor motions — primitive movements the keymap compiles user keys to.
///
/// Two families:
///   - **Text motions** — vim-ish char/word/line/doc/page motions.
///   - **Structural motions** — Lisp-aware `(forward-sexp)` / `(backward-sexp)`
///     / `(up-list)` / `(down-list)` equivalents. Enabled on buffers whose
///     major mode opts in via `(defmajor-mode … :structural-lisp #t)`.
///     Matches paredit's model — equal-or-superior to emacs on Lisp UX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Motion {
    // ── Text motions (vim-ish base) ────────────────────────────────
    Left,
    Right,
    Up,
    Down,
    WordStartNext,
    WordEndNext,
    WordStartPrev,
    /// `ge` — to the END of the previous word. vim's only backward-inclusive
    /// motion, and the reason `is_inclusive` cannot simply mean "widen right".
    WordEndPrev,
    // ── WORD motions (`W`/`E`/`B`/`gE`) ────────────────────────────
    //
    // vim's second word width: whitespace-delimited, so `foo.bar` is ONE
    // WORD and three words. A separate arm rather than a `Width` field
    // because every call site that resolves a motion has to decide, and a
    // field is a decision a `match` arm cannot forget to make.
    BigWordStartNext,
    BigWordEndNext,
    BigWordStartPrev,
    BigWordEndPrev,
    LineStart,
    LineFirstNonBlank,
    /// `g_` — the LAST non-blank on the line. Inclusive, unlike `$`.
    LineLastNonBlank,
    LineEnd,
    /// `|` — to a 1-based screen column on the current line.
    Column(u32),
    /// `+` / `<CR>` — first non-blank of the next line.
    LineDownFirstNonBlank,
    /// `-` — first non-blank of the previous line.
    LineUpFirstNonBlank,
    /// `_` — count-1 lines downward, on the first non-blank. LINEWISE, which
    /// is the whole reason it is not an alias of [`Self::LineFirstNonBlank`]:
    /// `^` and `_` land the cursor on the same character, and `d^` deletes
    /// back to the indent while `d_` deletes the whole line. Aliasing them —
    /// which escriba did until 2026-08-14 — makes `d_` a no-op at column 0,
    /// because the exclusive range `[cursor, first-non-blank)` is empty there.
    LinewiseDown,
    DocStart,
    DocEnd,
    // ── character search (`f` / `F` / `t` / `T`, and `;` / `,`) ─────
    /// `f{c}` (`backward=false, till=false`), `t{c}` (`till=true`),
    /// `F{c}` / `T{c}` (`backward=true`). The character is carried IN the
    /// motion so `df(` is one composed `ApplyOperator` like every other
    /// operated motion — a separate "pending char" the operator had to read
    /// would be a second composition mechanism beside the FSM.
    FindChar {
        ch: char,
        backward: bool,
        till: bool,
    },
    /// `;` (`reverse=false`) / `,` (`reverse=true`) — repeat the last
    /// [`Motion::FindChar`]. Resolved against runtime state, so like
    /// [`Motion::SearchNext`] the enum stays a pure description.
    RepeatFind {
        reverse: bool,
    },
    /// `%` — to the match of the bracket under (or next on) the cursor.
    MatchPair,
    // ── marks (`m` sets, `` ` `` and `'` jump) ─────────────────────
    /// `` `{a-z} `` — to a mark's exact line AND column.
    MarkExact(char),
    /// `'{a-z}` — to the first non-blank of a mark's LINE. vim's two spellings
    /// are two motions, not one motion and a modifier: `` `a `` is exclusive
    /// and `'a` is linewise, so `d'a` and ``d`a`` delete different things.
    MarkLine(char),
    // ── paragraph / sentence ───────────────────────────────────────
    /// `}` — to the next blank line (paragraph boundary).
    ParagraphNext,
    /// `{` — to the previous blank line.
    ParagraphPrev,
    /// `)` — to the start of the next sentence.
    SentenceNext,
    /// `(` — to the start of the previous sentence.
    SentencePrev,
    // ── viewport-relative (`H` / `M` / `L`) ────────────────────────
    ScreenTop,
    ScreenMiddle,
    ScreenBottom,
    PageUp,
    PageDown,
    HalfPageUp,
    HalfPageDown,
    GotoLine(u32),

    // ── Structural Lisp motions (paredit-grade) ────────────────────
    /// Move to the start of the next sibling s-expression.
    ForwardSexp,
    /// Move to the start of the previous sibling s-expression.
    BackwardSexp,
    /// Move up one parenthesis level — to the opening `(` of the enclosing list.
    UpList,
    /// Move down into the current list — past the opening `(`.
    DownList,
    /// Move to the start of the enclosing top-level defun / top form.
    BeginningOfDefun,
    /// Move to the end of the enclosing top-level defun / top form.
    EndOfDefun,
    /// Move to the start of the current s-expression (current atom / list open).
    BeginningOfSexp,
    /// Move to the end of the current s-expression (matching close).
    EndOfSexp,

    // ── search motions ────────────────────────────────────────────────
    /// To the next search match — vim's `n` used as a MOTION, which is what
    /// makes `d/foo<CR>`, `dn` and `y*` work. Search being a motion rather
    /// than a bare cursor jump is the difference between a search box and vim
    /// search; resolving it needs the committed `SearchState`, so the executor
    /// supplies it — the enum stays a pure description, like every other arm.
    SearchNext,
    /// To the previous search match (vim's `N` as a motion).
    SearchPrev,
}

impl Motion {
    /// Does this motion name a character to ACT ON, rather than a boundary to
    /// stop before?
    ///
    /// vim's exclusive/inclusive split, and it is not cosmetic: `dw` deletes
    /// up to the next word and `de` deletes *through* the current one. An
    /// operator range is `[cursor, target)`, so an inclusive motion's target
    /// has to be widened by one character or the operator leaves the last
    /// character behind — off by exactly one, on the key most likely to be
    /// used to delete a word without its trailing space.
    ///
    /// `RepeatFind` answers `false` here and that is not a classification:
    /// whether `;` is inclusive depends on the direction of the find it
    /// repeats, which is runtime state. The executor resolves it to the
    /// concrete [`Motion::FindChar`] and asks THAT — so there is still exactly
    /// one rule, applied to a known motion.
    ///
    /// Exhaustive `match` since 2026-08-14, for the reason
    /// [`Self::is_linewise`] gives at length: as a `matches!` this answered
    /// `false` for every variant added after it was written, and it had
    /// already been wrong that way. `ge` / `gE` were **backward-inclusive**
    /// (the enum's own note on [`Self::WordEndPrev`] said so) and unlisted, so
    /// `dge` dropped the character under the cursor — `"foo bar baz"` at the
    /// `b` of `baz` gave `"foo babaz"` where vim gives `"foo baaz"`.
    #[must_use]
    pub const fn is_inclusive(self) -> bool {
        match self {
            // Forward-inclusive: the target character is ACTED ON.
            Self::WordEndNext
            | Self::BigWordEndNext
            | Self::LineLastNonBlank
            | Self::MatchPair
            // `f`/`t` only. `F`/`T` are EXCLUSIVE in vim, which is why the
            // pattern binds `backward` rather than using `..` for both.
            | Self::FindChar { backward: false, .. }
            // Backward-inclusive: `ge` / `gE`. vim's rule is "the last
            // character towards the END of the buffer is included", and for a
            // backward motion that end is the CURSOR, not the target — so the
            // widening flips direction. Handled at the operator, which is the
            // only place that knows which way the motion ran.
            | Self::WordEndPrev
            | Self::BigWordEndPrev => true,

            Self::Left
            | Self::Right
            | Self::Up
            | Self::Down
            | Self::WordStartNext
            | Self::WordStartPrev
            | Self::BigWordStartNext
            | Self::BigWordStartPrev
            | Self::LineStart
            | Self::LineFirstNonBlank
            // `$` is exclusive, `g_` inclusive — the whole reason they are two
            // motions and not one plus an offset.
            | Self::LineEnd
            | Self::Column(_)
            | Self::LineDownFirstNonBlank
            | Self::LineUpFirstNonBlank
            | Self::LinewiseDown
            | Self::DocStart
            | Self::DocEnd
            | Self::GotoLine(_)
            | Self::FindChar { backward: true, .. }
            | Self::RepeatFind { .. }
            | Self::MarkExact(_)
            | Self::MarkLine(_)
            | Self::ParagraphNext
            | Self::ParagraphPrev
            | Self::SentenceNext
            | Self::SentencePrev
            | Self::ScreenTop
            | Self::ScreenMiddle
            | Self::ScreenBottom
            | Self::PageUp
            | Self::PageDown
            | Self::HalfPageUp
            | Self::HalfPageDown
            | Self::ForwardSexp
            | Self::BackwardSexp
            | Self::UpList
            | Self::DownList
            | Self::BeginningOfDefun
            | Self::EndOfDefun
            | Self::BeginningOfSexp
            | Self::EndOfSexp
            | Self::SearchNext
            | Self::SearchPrev => false,
        }
    }

    /// Does an operator over this motion act on WHOLE LINES?
    ///
    /// vim has three motion kinds, not two — exclusive, inclusive, and
    /// **linewise** — and escriba modelled only the first two until
    /// 2026-08-14. The consequence was a whole silently-wrong class rather
    /// than one bad key: `dj` deleted one line instead of two, `dgg` stopped a
    /// line short, and every one of them left a **charwise** register, so
    /// `yjp` spliced two lines into the middle of a third instead of opening
    /// lines below. The text was plausible and the register kind was invisible
    /// until a later put, which is why nothing caught it.
    ///
    /// Written as an exhaustive `match` rather than [`matches!`] **on purpose**
    /// — and that is the load-bearing difference from [`Self::is_inclusive`],
    /// which is a `matches!` and therefore answers `false` for any variant
    /// added after it was written. That silent default is exactly how this
    /// class was born: `Down`, `DocEnd`, `ScreenTop` and the rest arrived as
    /// cursor motions, and nobody was ever asked whether they were linewise.
    /// Here a new [`Motion`] fails to compile until it is classified, so the
    /// question cannot be skipped a second time.
    #[must_use]
    pub const fn is_linewise(self) -> bool {
        match self {
            // `j` `k` — the pair the class is most often noticed through.
            Self::Up
            | Self::Down
            // `gg` `G` `{n}G`.
            | Self::DocStart
            | Self::DocEnd
            | Self::GotoLine(_)
            // `H` `M` `L`.
            | Self::ScreenTop
            | Self::ScreenMiddle
            | Self::ScreenBottom
            // `+` `<CR>` `-` `_`.
            | Self::LineDownFirstNonBlank
            | Self::LineUpFirstNonBlank
            | Self::LinewiseDown
            // `'a`. Its sibling `` `a `` is exclusive — two spellings, two
            // motions, which is why they are two variants.
            | Self::MarkLine(_)
            // `<C-f>` `<C-b>` `<C-d>` `<C-u>`. vim does not accept these in
            // operator-pending at all, so there is no vim answer to copy —
            // but escriba DOES bind them as motions, so `d<C-d>` resolves to
            // something either way. Whole lines is the only defensible
            // reading of an operated half-page; charwise ends mid-line at
            // whatever column the cursor happened to hold.
            | Self::PageUp
            | Self::PageDown
            | Self::HalfPageUp
            | Self::HalfPageDown => true,

            // Charwise — exclusive or inclusive, decided by `is_inclusive`.
            Self::Left
            | Self::Right
            | Self::WordStartNext
            | Self::WordEndNext
            | Self::WordStartPrev
            | Self::WordEndPrev
            | Self::BigWordStartNext
            | Self::BigWordEndNext
            | Self::BigWordStartPrev
            | Self::BigWordEndPrev
            | Self::LineStart
            // `^` — the exclusive sibling of `_` above.
            | Self::LineFirstNonBlank
            | Self::LineLastNonBlank
            | Self::LineEnd
            | Self::Column(_)
            | Self::FindChar { .. }
            | Self::RepeatFind { .. }
            | Self::MatchPair
            | Self::MarkExact(_)
            // `{` `}` `(` `)` are EXCLUSIVE in vim, not linewise — a
            // reasonable-sounding guess that would make `d}` eat the blank
            // line terminating the paragraph.
            | Self::ParagraphNext
            | Self::ParagraphPrev
            | Self::SentenceNext
            | Self::SentencePrev
            | Self::ForwardSexp
            | Self::BackwardSexp
            | Self::UpList
            | Self::DownList
            | Self::BeginningOfDefun
            | Self::EndOfDefun
            | Self::BeginningOfSexp
            | Self::EndOfSexp
            // `d/foo<CR>` and `dn` are exclusive charwise in vim.
            | Self::SearchNext
            | Self::SearchPrev => false,
        }
    }

    #[must_use]
    pub const fn is_structural(self) -> bool {
        matches!(
            self,
            Self::ForwardSexp
                | Self::BackwardSexp
                | Self::UpList
                | Self::DownList
                | Self::BeginningOfDefun
                | Self::EndOfDefun
                | Self::BeginningOfSexp
                | Self::EndOfSexp,
        )
    }
}

/// Operators — vim-style verbs. Combined with a motion they produce an edit.
///
/// Structural operators (paredit-grade) compose with structural motions:
///   - `(slurp-forward)` — pull the next sibling into the current list
///   - `(barf-forward)` — push the last child out of the current list
///   - `(splice)` — unwrap the current list (remove parens, keep children)
///   - `(wrap)` — wrap the target in a new list
///   - `(raise)` — replace the enclosing list with the current sexp
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Operator {
    Delete,
    Yank,
    Change,
    Indent,
    Dedent,
    Filter,
    Format,
    // ── Structural (Lisp-aware) operators ──────────────────────────
    SlurpForward,
    SlurpBackward,
    BarfForward,
    BarfBackward,
    Splice,
    Wrap,
    Raise,
}

impl Operator {
    #[must_use]
    pub const fn leaves_register(self) -> bool {
        matches!(self, Self::Delete | Self::Yank | Self::Change)
    }

    #[must_use]
    pub const fn is_structural(self) -> bool {
        matches!(
            self,
            Self::SlurpForward
                | Self::SlurpBackward
                | Self::BarfForward
                | Self::BarfBackward
                | Self::Splice
                | Self::Wrap
                | Self::Raise,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_emitting_ops() {
        assert!(Operator::Delete.leaves_register());
        assert!(Operator::Yank.leaves_register());
        assert!(Operator::Change.leaves_register());
        assert!(!Operator::Format.leaves_register());
    }
}

/// A text EXTENT an operator can act over, as opposed to a point it moves to.
///
/// vim's `gn` is the motivating case and shows why the distinction matters:
/// `dgn` deletes the next match *wherever it is*, including when the cursor is
/// nowhere near it. Modelled as a motion it would resolve to the match's start
/// and the operator would act over `[cursor, match.start)` — deleting the text
/// BEFORE the match instead of the match. Same keys, opposite effect.
///
/// Closed, so an unhandled object cannot reach the executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum TextObject {
    /// `gn` — the next search match at or after the cursor.
    NextMatch,
    /// `gN` — the previous search match at or before the cursor.
    PrevMatch,
    /// `dd` / `cc` / `yy` — the current line, LINEWISE.
    ///
    /// A doubled operator in vim acts on whole lines, trailing newline
    /// included, which is why this is an object rather than a motion: there
    /// is no cursor-to-target range that expresses "this line and its
    /// terminator" without special-casing the last line.
    Line,
    /// `iw` / `aw` — the word under the cursor.
    ///
    /// `around: true` takes the trailing run of whitespace as well, which is
    /// the whole difference vim draws between `diw` and `daw`.
    Word { around: bool },
    /// `i(` `a{` `i"` … — the region between a matched pair.
    ///
    /// One variant covers brackets and quotes because the only thing that
    /// differs is whether the delimiters nest; carrying `open`/`close`
    /// separately lets a quote say `open == close` instead of needing its
    /// own arm.
    Delimited {
        open: char,
        close: char,
        around: bool,
    },
}

impl TextObject {
    /// How an operator over this object leaves the register — and therefore
    /// how a later `p` replays it.
    ///
    /// **Total over `TextObject`, no wildcard arm.** The mapping lives here,
    /// beside the variants, rather than at the one call site that needs it
    /// today: a new linewise object (vim's `ip`/`ap` paragraph objects are the
    /// obvious next ones) must decide, and a wildcard would silently answer
    /// `Charwise` for them — the direction that pastes a paragraph into the
    /// middle of whatever line the cursor happens to be on.
    #[must_use]
    pub const fn register_kind(self) -> crate::register::RegisterKind {
        use crate::register::RegisterKind as K;
        match self {
            Self::Line => K::Linewise,
            Self::NextMatch | Self::PrevMatch | Self::Word { .. } | Self::Delimited { .. } => {
                K::Charwise
            }
        }
    }
}
