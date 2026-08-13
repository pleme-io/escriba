use escriba_search::Direction as SearchDirection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::edit::Edit;
use crate::mode::Mode;
use crate::motion::TextObject;
use crate::motion::{Motion, Operator};

/// WHERE the caret lands when Insert mode is entered — vim's insert-entry
/// family (`i` `I` `a` `A` `o` `O`) as ONE typed surface.
///
/// Until 2026-08-12 the family was a single key. `i` was bound straight to
/// `Action::ChangeMode(Mode::Insert)` and the other five did not exist —
/// pressing `A` in escriba 0.1.71 moved nothing, changed no mode, and reported
/// nothing, because an unbound Normal-mode key resolves to `Action::Pending`.
/// The absence was invisible from inside: `escriba --keymap` lists what IS
/// bound, so nothing named the four keys a vim user reaches for first.
///
/// Modelling the ENTRY POINT rather than the destination mode is what makes
/// that class of omission unrepresentable. A new entry is a variant here, and
/// [`Action::text_effect`], [`Action::highlight_effect`],
/// [`Action::edits_prompt`] and the runtime's damage classifier are all total
/// over `Action` — so adding one cannot compile until every consequence has
/// been decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum InsertAt {
    /// `i` — insert BEFORE the caret. The caret does not move.
    Caret,
    /// `I` — the first non-blank character of the line.
    FirstNonBlank,
    /// `a` — one column right of the caret ("append"), clamped to one past the
    /// last character so `a` on the final character still appends.
    AfterCaret,
    /// `A` — one column past the last character of the line.
    LineEnd,
    /// `o` — open a fresh line BELOW the caret's and land on it.
    OpenBelow,
    /// `O` — open a fresh line ABOVE the caret's and land on it.
    OpenAbove,
}

/// Where `zt` / `zz` / `zb` put the cursor's line on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ViewAlign {
    /// `zt` — the cursor's line becomes the top visible line.
    Top,
    /// `zz` — the cursor's line is centred.
    Center,
    /// `zb` — the cursor's line becomes the bottom visible line.
    Bottom,
}

impl ViewAlign {
    /// The stable label — `escriba --keymap`, the rc's `:action` names.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Top => "scroll-top",
            Self::Center => "scroll-center",
            Self::Bottom => "scroll-bottom",
        }
    }
}

impl InsertAt {
    /// Does this entry ADD a line to the buffer?
    ///
    /// `o` and `O` are the only two members of the family that change text;
    /// the other four move the caret and nothing else. Read by
    /// [`Action::text_effect`], so the answer lives here beside the variants
    /// rather than being re-derived by each classifier that needs it.
    #[must_use]
    pub const fn opens_a_line(self) -> bool {
        matches!(self, Self::OpenBelow | Self::OpenAbove)
    }

    /// The stable label — `escriba --keymap`, the command palette, the rc's
    /// `:action` names.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Caret => "caret",
            Self::FirstNonBlank => "first-non-blank",
            Self::AfterCaret => "after-caret",
            Self::LineEnd => "line-end",
            Self::OpenBelow => "open-below",
            Self::OpenAbove => "open-above",
        }
    }

    /// Every entry, so a matrix test cannot silently miss one.
    pub const ALL: [Self; 6] = [
        Self::Caret,
        Self::FirstNonBlank,
        Self::AfterCaret,
        Self::LineEnd,
        Self::OpenBelow,
        Self::OpenAbove,
    ];
}

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
    /// Enter Insert mode AT a named place — vim's `i` `I` `a` `A` `o` `O`.
    ///
    /// Distinct from `ChangeMode(Mode::Insert)`, which is still what the
    /// *runtime* emits when an operator leaves you inserting (`ciw`) and what
    /// `<Esc>`'s counterpart looks like. This variant is the KEYBOARD's entry
    /// point, and it carries the one thing `ChangeMode` cannot: where the caret
    /// goes. See [`InsertAt`].
    EnterInsert(InsertAt),
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

    /// `p` / `P` — put the register back into the buffer.
    ///
    /// Named `Put` after vim's own verb, not `Paste`, and the distinction is
    /// load-bearing rather than pedantic: a put replays escriba's REGISTER
    /// (whatever `d`/`y` last captured, with its [`crate::RegisterKind`]),
    /// while a paste will replay the SYSTEM CLIPBOARD via `hasami` — a
    /// different source, arriving as bracketed-paste bytes rather than a
    /// keypress, with no linewise/charwise distinction to honour. Reusing one
    /// name for both is how they would end up sharing an executor that is
    /// wrong for one of them.
    ///
    /// `before` is vim's `P`: a charwise put lands at the cursor column
    /// instead of after it, a linewise put opens above the line instead of
    /// below. Every other rule is shared, which is why this is one variant
    /// with a flag rather than two.
    Put {
        before: bool,
    },

    /// `r{char}` — overwrite the character(s) under the cursor with `char`.
    ///
    /// Carries the replacement rather than reading it from pending state, so
    /// the action is self-contained and `.` can replay it. The KEY that
    /// supplies it is captured at the key layer (`consume_replace_key`),
    /// the same place `f`'s operand and `` ` ``'s mark letter are: `rw` must
    /// not read as `r` then *move a word*.
    ///
    /// Not a `Change` operator over one character — `r` does not enter Insert,
    /// does not touch the register, and refuses at end of line rather than
    /// appending, all three of which an operator composition would get wrong.
    ReplaceChar(char),

    /// `J` / `gJ` — join the following line onto this one.
    ///
    /// `space: true` is `J`: the next line's leading whitespace is dropped and
    /// a single space takes the newline's place. `space: false` is `gJ`, which
    /// splices the lines exactly as they are — the reason to reach for it is
    /// that `J` is lossy, and a `gJ` spelled as "`J` without the fixup" would
    /// still have stripped the indent.
    JoinLines {
        space: bool,
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

    /// `m{a-z}` — name the cursor's position so `` `{a-z} `` can return to it.
    ///
    /// The letter is carried IN the action for the same reason
    /// [`Motion::FindChar`] carries its character: the second keystroke is an
    /// OPERAND, and an action that had to be paired with separate pending
    /// state is an action a face could dispatch half of.
    SetMark(char),

    /// `zt` / `zz` / `zb` — move the VIEWPORT so the cursor's line sits at a
    /// named place on screen, without moving the cursor.
    ///
    /// Not a [`Motion`], and the distinction is the whole point: a motion
    /// changes where you are, and this changes only what you can see. Folding
    /// it into `Motion` would make it composable with an operator, and `dzz`
    /// is not a thing.
    ScrollView(ViewAlign),
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
            // A put with an EMPTY register mutates nothing, but the classifier
            // cannot see the register — and over-reporting costs one re-scan
            // while under-reporting paints stale columns over freshly pasted
            // text. The asymmetry decides it, exactly as for `Command`.
            | Self::Put { .. }
            | Self::ReplaceChar(_)
            | Self::JoinLines { .. }
            | Self::SubmitCommand => TextEffect::Mutates,

            // `o`/`O` add a line; `i`/`I`/`a`/`A` move the caret and nothing
            // else. Classified from the payload rather than reported as
            // "Mutates" wholesale: over-reporting here costs a re-scan of the
            // match set on every `i`, which is the most-pressed key in the
            // editor.
            Self::EnterInsert(at) => {
                if at.opens_a_line() {
                    TextEffect::Mutates
                } else {
                    TextEffect::Preserves
                }
            }

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
            // A mark names a position and a scroll moves the window; neither
            // touches a byte.
            | Self::SetMark(_)
            | Self::ScrollView(_)
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
            // Neither moves the cursor off a match: `zz` re-frames the same
            // line and `ma` names it. Clearing here would make "centre the
            // view so I can see the other hits" the gesture that removes them.
            | Self::SetMark(_)
            | Self::ScrollView(_)
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

            // Every member of the family begins editing, so the search is
            // over — the same reading as `ChangeMode(Insert)` above, and it
            // must not drift from it.
            Self::EnterInsert(_) => HighlightEffect::Clear,

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
            | Self::Put { .. }
            | Self::ReplaceChar(_)
            | Self::JoinLines { .. }
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
            // Insert-entry acts on the BUFFER, never on an open prompt — and
            // it is unreachable while one is open anyway, because Command mode
            // preempts every printable key before the table is consulted.
            | Self::EnterInsert(_)
            | Self::Command { .. }
            | Self::SubmitCommand
            // These edit the BUFFER. They are also unreachable while a prompt
            // is open, for the same reason `EnterInsert` is.
            | Self::Put { .. }
            | Self::ReplaceChar(_)
            | Self::JoinLines { .. }
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
            | Self::SetMark(_)
            | Self::ScrollView(_)
            | Self::Pending => false,
        }
    }
}
