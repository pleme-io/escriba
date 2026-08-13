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
    /// `WordEndNext` is the only inclusive motion escriba has today. `f`/`t`/
    /// `%` are the others in vim and are not bound yet; each lands here when
    /// it does, which is the point of asking the MOTION rather than
    /// special-casing `e` at the operator.
    #[must_use]
    /// `RepeatFind` is deliberately absent: whether `;` is inclusive depends
    /// on the direction of the find it repeats, which is runtime state. The
    /// executor resolves it to the concrete [`Motion::FindChar`] and asks
    /// THAT — so there is still exactly one rule, applied to a known motion.
    pub const fn is_inclusive(self) -> bool {
        matches!(
            self,
            Self::WordEndNext
                | Self::BigWordEndNext
                | Self::LineLastNonBlank
                | Self::MatchPair
                | Self::FindChar {
                    backward: false,
                    ..
                }
        )
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
