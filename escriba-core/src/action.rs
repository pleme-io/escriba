use escriba_search::Direction as SearchDirection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::edit::Edit;
use crate::mode::Mode;
use crate::motion::TextObject;
use crate::motion::{Motion, Operator};

/// A fully-resolved editor action — what the keymap emits, what the buffer
/// consumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Action {
    /// Move every cursor by `motion`.
    Move(Motion),
    /// Begin an operator (the `d`/`c`/`y` key). The editor enters
    /// operator-pending: the next motion composes into an [`Action::ApplyOperator`].
    /// Resolved by the operator-pending FSM, never executed directly.
    Operator(Operator),
    /// Apply a pending operator over a motion (delete-word, yank-line, etc.).
    ApplyOperator {
        op: Operator,
        motion: Motion,
    },
    /// Apply a primitive edit at each cursor.
    Edit(Edit),
    /// Enter the given mode.
    ChangeMode(Mode),
    /// Run a named command (via the command registry).
    Command {
        name: String,
        args: Vec<String>,
    },
    /// Insert a character at each caret. Separate from Edit so the keymap
    /// can stay ignorant of rope details.
    InsertChar(char),
    /// Submit a minibuffer / command-mode line (e.g. `:w`, `:q`).
    SubmitCommand,
    /// Undo / redo one change.
    Undo,
    Redo,
    /// Save the current buffer.
    Save,
    /// Quit the editor.
    Quit,
    // ── search (vim `/`, `?`, `n`, `N`, `*`, `#`) ──────────────────────
    /// Open the search prompt in `direction` (the `/` and `?` keys).
    ///
    /// The prompt reuses `Mode::Command` rather than adding a mode variant:
    /// vim's `/` IS the command-line with a different prompt character, and
    /// this module's own doc states new modes are layered through pending
    /// state, not new variants. `SearchState`'s typed `Option<Prompt>` is what
    /// disambiguates a `<CR>` that submits a search from one that submits an
    /// ex-command — a discriminator that cannot be forgotten, unlike a bool.
    SearchOpen(SearchDirection),
    /// `n` (`reverse = false`) / `N` (`reverse = true`) — jump to the next
    /// match, relative to the direction the search was committed with, so `N`
    /// after a `?` search moves forward.
    SearchRepeat {
        reverse: bool,
    },
    /// `*` (`reverse = false`) / `#` (`reverse = true`) — search the whole word
    /// under the cursor. Literal, not regex: the word may contain `.` or `[`
    /// and the user means those characters.
    SearchWord {
        reverse: bool,
    },
    /// `:noh` — stop highlighting matches while keeping the pattern, so `n`
    /// still works. Distinct from cancelling a search.
    ClearSearchHighlight,

    /// `d/foo<CR>` — commit the open search prompt and apply `op` from the
    /// prompt's ORIGIN to wherever the search lands.
    ///
    /// Emitted only by the operator-pending machine; no keymap produces it.
    /// It exists because committing a search MOVES the cursor, and the
    /// operator needs the pre-move position as its start point. Carrying the
    /// operator through the commit makes "operate over a search" one atomic
    /// action instead of two steps racing to own the cursor.
    SearchSubmitOperated {
        op: Operator,
    },

    /// `gn` / `gN` — the next/previous match AS AN OBJECT.
    ///
    /// Not a motion. A motion resolves to a POINT and an operator acts over
    /// `[cursor, point)`; `gn` names an EXTENT that need not start at the
    /// cursor, so `dgn` deletes the whole match wherever it is. That
    /// distinction is why this is its own action rather than a `Motion`
    /// variant — folding it into `Motion` would silently give
    /// `[cursor, match.start)`, which deletes the text BEFORE the match.
    TextObject(TextObject),

    /// `{operator}gn` — apply `op` over a text object's extent.
    ///
    /// Emitted only by the operator-pending machine.
    ApplyOperatorObject {
        op: Operator,
        object: TextObject,
    },

    /// `.` — repeat the last text change.
    ///
    /// Vim's most-used key, and the half that makes `cgn` a workflow rather
    /// than a curiosity: `cgn` changes the next match, then `.` changes the
    /// one after it, giving a per-instance confirmable rename with no
    /// multi-cursor machinery.
    RepeatLastChange,

    /// `<C-o>` — walk back to where the last far jump was taken from.
    ///
    /// Lives beside the search actions because search is what made it
    /// necessary — committing a `/` used to be a one-way door — but it is not
    /// a search action: `G`, `gg`, `%` and tag jumps are the other consumers.
    JumpBack,
    /// `<C-i>` — walk forward again after [`Action::JumpBack`].
    JumpForward,
    /// `<BS>` — delete the character BEFORE the caret, wherever the caret is.
    ///
    /// ONE action, three targets, routed by the runtime: the search prompt,
    /// the ex command-line, or the buffer in Insert mode. It is deliberately
    /// not three actions — a face binding `<BS>` should not have to know which
    /// of the three the operator is currently typing into, and the routing
    /// question ("is a prompt open?") is already answered by typed state the
    /// runtime owns.
    ///
    /// Named `PromptBackspace` until 2026-08-09, when the Insert-mode target
    /// landed. The old name was the honest one while the buffer arm did not
    /// exist — `text_effect` below already described the buffer arm as though
    /// it did, which is how it went unnoticed that Insert mode had NO way to
    /// erase a character.
    Backspace,

    /// Move the caret inside an open prompt (`←` `→` `Home` `End`).
    ///
    /// The prompt was append-only until this existed, so a typo in the middle
    /// of a pattern could only be fixed by deleting back to it.
    PromptCaret {
        to: escriba_search::CaretMove,
    },
    /// `<C-g>` / `<C-t>` — step the search PREVIEW to the next/previous
    /// match without committing.
    ///
    /// Distinct from `n` in the one way that matters: this is still
    /// cancellable. Escape returns to where the search started, which `n`
    /// after a commit cannot do.
    SearchPreviewStep {
        forward: bool,
    },
    /// `<Del>` — delete the character AT the caret, wherever the caret is.
    ///
    /// The forward-delete sibling of [`Action::Backspace`], routed the same
    /// way. Never closes a prompt: emptying the text by deleting rightwards is
    /// not the "backspaced past the `/`" gesture that means "I changed my
    /// mind".
    DeleteForward,
    /// `<C-w>` — delete the word before the caret, wherever the caret is.
    ///
    /// The word-sized member of the same erase family as [`Action::Backspace`]:
    /// ONE action, three targets (search prompt / ex line / buffer), routed by
    /// the runtime on typed state it already owns.
    ///
    /// Named `PromptDeleteWord` until 2026-08-09, when the Insert-mode target
    /// landed. `<BS>` and `<Del>` had been given their buffer arm that morning
    /// and these two were left behind, so Insert mode could erase one character
    /// at a time and nothing larger — the half-migration is exactly what the
    /// `Prompt` prefix was hiding.
    DeleteWordBefore,
    /// `<C-u>` — delete from the caret back to the start of the line.
    ///
    /// The line-sized member of the erase family; routed exactly like
    /// [`Action::DeleteWordBefore`]. In the buffer it stops at the first
    /// non-blank before falling through to column 0, so the first press on an
    /// indented line clears what was typed and the second clears the indent —
    /// vim's two-step, which is what keeps `<C-u>` from eating alignment you
    /// wanted to keep.
    DeleteToLineStart,
    /// Up/Down inside a prompt — walk search history.
    ///
    /// `back = true` is older. Stepping forward past the newest entry restores
    /// the text that was being typed when browsing began, so arrowing through
    /// history and back never destroys a half-typed pattern.
    PromptHistory {
        back: bool,
    },
    /// No-op — used when a key sequence is pending but not yet complete.
    Pending,
}

/// An [`Action`] with an optional repetition count (vim's `5dd`, `10k`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CountedAction {
    pub count: u32,
    pub action: Action,
}

impl CountedAction {
    #[must_use]
    pub fn once(action: Action) -> Self {
        Self { count: 1, action }
    }

    #[must_use]
    pub fn repeated(count: u32, action: Action) -> Self {
        Self {
            count: count.max(1),
            action,
        }
    }
}

/// Whether an action can change buffer TEXT.
///
/// This exists so that anything cached against the buffer's contents — today
/// the search-match set, tomorrow anything else derived from it — is
/// invalidated by construction rather than by remembering to. Search
/// highlights were stale after every edit precisely because that invalidation
/// was a thing to remember: `SearchState::refresh` existed and had zero
/// callers, so inserting four characters repainted the highlight four columns
/// off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEffect {
    /// May edit the active buffer. Derived state must be recomputed.
    Mutates,
    /// Cannot edit the active buffer.
    Preserves,
}

impl Action {
    /// Classify this action's effect on buffer text.
    ///
    /// **Total over `Action` — no wildcard arm.** A new variant fails to
    /// compile here rather than silently defaulting to "preserves", which is
    /// the direction that produces a stale-cache bug rather than a slow one.
    ///
    /// Deliberately CONSERVATIVE where a variant's reach is open-ended:
    /// `Command`/`SubmitCommand` can run an ex-command that edits. It is not
    /// conservative for `Backspace`/`DeleteForward` — those genuinely edit the
    /// buffer outside a prompt. This paragraph claimed they did for months
    /// before the Insert-mode arm was written; the classifier was right about
    /// the design and the executor had simply never implemented it.
    /// Over-reporting costs one extra scan; under-reporting paints the wrong
    /// columns, so the asymmetry decides the genuinely doubtful cases.
    #[must_use]
    pub const fn text_effect(&self) -> TextEffect {
        match self {
            Self::Edit(_)
            | Self::InsertChar(_)
            | Self::ApplyOperator { .. }
            | Self::ApplyOperatorObject { .. }
            | Self::Undo
            | Self::Redo
            | Self::Backspace
            | Self::DeleteForward
            | Self::DeleteWordBefore
            | Self::DeleteToLineStart
            | Self::TextObject(_)
            | Self::Command { .. }
            | Self::SearchSubmitOperated { .. }
            | Self::RepeatLastChange
            | Self::SubmitCommand => TextEffect::Mutates,

            Self::Move(_)
            | Self::Operator(_)
            | Self::ChangeMode(_)
            | Self::Save
            | Self::Quit
            | Self::SearchOpen(_)
            | Self::SearchRepeat { .. }
            | Self::SearchWord { .. }
            | Self::ClearSearchHighlight
            | Self::JumpBack
            | Self::JumpForward
            | Self::PromptCaret { .. }
            | Self::PromptHistory { .. }
            | Self::SearchPreviewStep { .. }
            | Self::Pending => TextEffect::Preserves,
        }
    }
}

/// What an action does to search HIGHLIGHTING.
///
/// vim leaves `hlsearch` lit until `:nohlsearch`, which is why nearly every
/// published vimrc remaps something to `:noh` — the highlight has done its job
/// the moment you start editing, and leaving it on turns the buffer into
/// confetti. escriba clears it on the first action that is plainly not part of
/// searching.
///
/// Clearing SUPPRESSES without forgetting: the pattern survives, so `n` still
/// works and re-lights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightEffect {
    /// Search highlighting stays as it is.
    Keep,
    /// Stop drawing highlights (the pattern is retained).
    Clear,
}

impl Action {
    /// Classify this action's effect on search highlighting.
    ///
    /// **Total over `Action` — no wildcard arm.** "We forgot to clear on the
    /// new command" becomes unconstructible rather than remembered: adding a
    /// variant forces the decision here.
    #[must_use]
    pub const fn highlight_effect(&self) -> HighlightEffect {
        match self {
            // Everything that IS searching, or that is operating the prompt.
            Self::SearchOpen(_)
            | Self::SearchRepeat { .. }
            | Self::SearchWord { .. }
            | Self::SearchSubmitOperated { .. }
            | Self::ClearSearchHighlight
            | Self::TextObject(_)
            | Self::SearchPreviewStep { .. }
            | Self::PromptHistory { .. }
            | Self::Backspace
            | Self::PromptCaret { .. }
            | Self::DeleteForward
            | Self::DeleteWordBefore
            | Self::DeleteToLineStart
            | Self::InsertChar(_)
            | Self::SubmitCommand
            | Self::Pending
            // A jump is how you USE the matches; extinguishing them mid-walk
            // would defeat the purpose.
            | Self::JumpBack
            | Self::JumpForward
            // Arming an operator is not yet a move — `d` then `n` must still
            // see its matches.
            | Self::Operator(_)
            | Self::Save
            | Self::Quit => HighlightEffect::Keep,

            // A search MOTION is searching, not moving on — `n` must not
            // extinguish the matches it is walking. Every other motion is a
            // departure.
            //
            // The `_` here is deliberate and is the SAFE direction, unlike
            // `text_effect`'s: a motion nobody has classified yet is "moving
            // on", which at worst clears a highlight early. The opposite
            // default would leave stale confetti on screen.
            // Entering Insert begins editing, so the search is over. Every
            // OTHER mode change is navigation or a CANCEL — and a cancel must
            // not erase the committed pattern's highlights. Both
            // `SearchState::cancel` and the runtime's own `ChangeMode` arm
            // promise that in writing ("cancelling a new search must not erase
            // the old highlights"), and a blanket `Clear` here landed on top of
            // the cancel it had just performed: `/foo<CR>` then `/bar<Esc>`
            // silently extinguished `foo`.
            //
            // Total over `Mode`, so a new mode must decide.
            Self::ChangeMode(m) => match m {
                Mode::Insert => HighlightEffect::Clear,
                Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::Command => {
                    HighlightEffect::Keep
                }
            },

            Self::Move(m) => match m {
                Motion::SearchNext | Motion::SearchPrev => HighlightEffect::Keep,
                _ => HighlightEffect::Clear,
            },

            // Moving on, or changing the text: the search is over.
            Self::ApplyOperator { .. }
            | Self::ApplyOperatorObject { .. }
            | Self::RepeatLastChange
            | Self::Edit(_)
            | Self::Command { .. }
            | Self::Undo
            | Self::Redo => HighlightEffect::Clear,
        }
    }
}

impl Action {
    /// Does this action edit or navigate an OPEN PROMPT, rather than doing
    /// something to the buffer?
    ///
    /// The operator-pending machine needs this: during `d/foo` the operator
    /// must survive every keystroke that is part of composing the pattern, and
    /// disarm on anything that is not.
    ///
    /// **Total over `Action` — no wildcard arm**, and that totality is the
    /// whole point. The machine originally listed the prompt actions inline;
    /// when `PromptCaret`, `DeleteForward`, `DeleteWordBefore`,
    /// `DeleteToLineStart` and `SearchPreviewStep` were added later, none was
    /// added to that list, so pressing `←` or `<C-g>` midway through `d/foo`
    /// silently disarmed the operator — reintroducing exactly the defect the
    /// `AwaitingSearch` state had been created to fix. A new prompt action now
    /// cannot be added without deciding here.
    #[must_use]
    pub const fn edits_prompt(&self) -> bool {
        match self {
            Self::InsertChar(_)
            | Self::Backspace
            | Self::PromptHistory { .. }
            | Self::PromptCaret { .. }
            | Self::DeleteForward
            | Self::DeleteWordBefore
            | Self::DeleteToLineStart
            | Self::SearchPreviewStep { .. } => true,

            Self::Move(_)
            | Self::Operator(_)
            | Self::ApplyOperator { .. }
            | Self::ApplyOperatorObject { .. }
            | Self::TextObject(_)
            | Self::Edit(_)
            | Self::ChangeMode(_)
            | Self::Command { .. }
            | Self::SubmitCommand
            | Self::Undo
            | Self::Redo
            | Self::Save
            | Self::Quit
            | Self::SearchOpen(_)
            | Self::SearchRepeat { .. }
            | Self::SearchWord { .. }
            | Self::SearchSubmitOperated { .. }
            | Self::ClearSearchHighlight
            | Self::RepeatLastChange
            | Self::JumpBack
            | Self::JumpForward
            | Self::Pending => false,
        }
    }
}
