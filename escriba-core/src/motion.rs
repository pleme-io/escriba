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
    LineStart,
    LineFirstNonBlank,
    LineEnd,
    DocStart,
    DocEnd,
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
}

impl Motion {
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
