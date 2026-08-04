use escriba_search::Direction as SearchDirection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::edit::Edit;
use crate::mode::Mode;
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

    /// `<C-o>` — walk back to where the last far jump was taken from.
    ///
    /// Lives beside the search actions because search is what made it
    /// necessary — committing a `/` used to be a one-way door — but it is not
    /// a search action: `G`, `gg`, `%` and tag jumps are the other consumers.
    JumpBack,
    /// `<C-i>` — walk forward again after [`Action::JumpBack`].
    JumpForward,
    /// Backspace inside a command-line or search prompt.
    ///
    /// Key::Backspace was previously bound in NO mode, so the minibuffer could
    /// be typed into but never corrected — a typo meant Esc and start again.
    /// One action serves both prompts; the runtime routes it by whether a
    /// search prompt is open.
    PromptBackspace,
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
    /// `Command`/`SubmitCommand` can run an ex-command that edits, and the
    /// keymap's `PromptBackspace` doubles as the buffer Backspace outside a
    /// prompt. Over-reporting costs one extra scan; under-reporting paints
    /// the wrong columns, so the asymmetry decides the doubtful cases.
    #[must_use]
    pub const fn text_effect(&self) -> TextEffect {
        match self {
            Self::Edit(_)
            | Self::InsertChar(_)
            | Self::ApplyOperator { .. }
            | Self::Undo
            | Self::Redo
            | Self::PromptBackspace
            | Self::Command { .. }
            | Self::SearchSubmitOperated { .. }
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
            | Self::PromptHistory { .. }
            | Self::Pending => TextEffect::Preserves,
        }
    }
}
