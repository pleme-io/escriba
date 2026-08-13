//! `unsoku` (運足) — **footwork**: vim motions, operators and text objects
//! resolved against a single line of text.
//!
//! # What this is for
//!
//! Between "a `String` you append to" and "a whole modal editor" the fleet had
//! nothing. A picker's filter box, a command palette, a search field — each
//! wants `w`, `b`, `dw`, `ciw`, `x`, `D` — and none of them can carry a rope,
//! an LSP client and a plugin host to get them. `unsoku` is that gap and
//! nothing else.
//!
//! # What it deliberately does NOT own
//!
//! **The vocabulary.** [`escriba_core::Motion`], [`Operator`] and [`TextObject`]
//! are already published and are pure enums carrying no editor state. A second
//! `Motion` enum beside a published one is exactly the duplication the fleet's
//! PRIME DIRECTIVE forbids — so unsoku declares none, and that is why it is
//! small.
//!
//! **The composition FSM.** `escriba_mode::OperatorPending` is the
//! `{count}{operator}{count}{motion}` machine, on `zenmai`.
//!
//! **The text.** Reached through [`TextTarget`], so unsoku is coupled to no
//! widget crate and no renderer.
//!
//! # The rules, and why they are stated rather than assumed
//!
//! An adversarial review of five independent designs for this crate found the
//! vim *semantics* missing from every one of them — which is the failure mode
//! of re-deriving a resolver that already exists. Each rule below is a named,
//! tested invariant here:
//!
//! 1. **Exclusive vs INCLUSIVE.** `dw` is exclusive; `de` and `d$` are
//!    inclusive. Treat every operator range as `[caret, target)` and `de`
//!    leaves the word's last character behind — the classic bug.
//! 2. **Motions always advance.** `e` pressed twice moves twice. "The end of
//!    the run containing the caret" is a *fixed point*, i.e. a cursor that
//!    stops responding to a held key.
//! 3. **Whitespace is a gap, not a destination** — for `w`/`b`/`e`. But `iw`
//!    on a blank selects the blanks, so the boundary query and the span query
//!    need **opposite** whitespace treatment.
//! 4. **Backward ranges invert.** `db`, `d0`, `dh` resolve left of the caret;
//!    a naive `caret..target` is inverted. Normalised at the one place a span
//!    is built.
//! 5. **Counted overrun operates over what was reached.** `d5w` with two words
//!    left deletes to the end rather than refusing.
//! 6. **`cw` is not `ce`.** On a non-blank, `cw` changes to the end of the
//!    *current* run and never crosses into the next word.
//! 7. **Normal clamps, Insert does not.** The Normal caret cannot rest past
//!    the last character.
//! 8. **Every removal fills the register** — `d c x X D C s`, plus `y`. Miss it
//!    and `xp`, the commonest one-line vim idiom, is silently broken.
//!
//! # Offsets are BYTES
//!
//! Every offset in this crate is a byte offset into `TextTarget::text()`, and
//! every one returned lands on a `char` boundary. Bytes rather than chars
//! because that is what `str` indexing takes; char-boundary-safe because a
//! caret between two bytes of one `char` would panic the first time anything
//! sliced with it.

use std::ops::Range;

/// The vim vocabulary, re-exported.
///
/// unsoku declares none of these — they are `escriba_core`'s, and that is the
/// point. But a consumer resolving a [`Motion`] must NAME one, and making it
/// declare `escriba-core` too would be a second dependency for a vocabulary it
/// reaches only through this crate: the caller ends up pinning two versions
/// that must agree, which is a drift class rather than a convenience.
///
/// One dependency buys the whole model. That is the contract.
pub use escriba_core::{Motion, Operator, TextObject};

/// A single line of text with a caret, that unsoku can move within and edit.
///
/// One trait rather than a concrete type so unsoku couples to no widget crate.
/// The orphan rule lets a *consumer* implement it for a foreign type, because
/// the trait is theirs to reach — which is how `egaku::TextInput` gets vim
/// editing without egaku ever learning what a [`Motion`] is.
pub trait TextTarget {
    /// The whole line. Never contains `\n` — see the crate docs.
    fn text(&self) -> &str;
    /// The caret, as a byte offset into [`Self::text`].
    fn caret(&self) -> usize;
    /// Move the caret. Implementors may assume the offset is on a `char`
    /// boundary and within `text().len()`; unsoku never produces one that is
    /// not.
    fn set_caret(&mut self, at: usize);
    /// Replace `range` with `with`. Implementors may assume both ends are on
    /// `char` boundaries and `range.start <= range.end <= text().len()`.
    fn replace(&mut self, range: Range<usize>, with: &str);
}

/// The stance — which reading a keystroke gets.
///
/// The name is the crate's: a mode IS a stance, and the stance decides what a
/// key means. Two variants in v1; `Visual` is deliberately absent rather than
/// stubbed, because a variant nothing handles is a screen state that can be
/// entered and not left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stance {
    /// Keys are commands. The caret is clamped to the last character.
    #[default]
    Normal,
    /// Keys are text. The caret may rest one past the last character.
    Insert,
}

impl Stance {
    /// Every stance — the catalog-reflection surface.
    pub const ALL: &'static [Stance] = &[Stance::Normal, Stance::Insert];

    /// The operator-facing badge. Short, because it is drawn in a status line
    /// beside things that matter more.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Stance::Normal => "NORMAL",
            Stance::Insert => "INSERT",
        }
    }
}

/// Which character class a byte belongs to, for word scanning.
///
/// Three classes, not two, and that is vim's model rather than a refinement of
/// it: `w` stops at the `-` in `foo-bar` because punctuation is its own class,
/// and folding punctuation into "not a word character" would make `w` skip the
/// whole token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Punct,
    Space,
}

impl Class {
    fn of(c: char) -> Self {
        if c.is_whitespace() {
            Class::Space
        } else if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else {
            Class::Punct
        }
    }

    /// The big-word (`W`/`B`/`E`) reading: everything non-blank is one class,
    /// so `foo-bar` is a single WORD.
    fn of_big(c: char) -> Self {
        if c.is_whitespace() {
            Class::Space
        } else {
            Class::Word
        }
    }
}

/// Small word (`w`) or big WORD (`W`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    /// `w` / `b` / `e` — alphanumerics and punctuation are different classes.
    Small,
    /// `W` / `B` / `E` — every non-blank is one class.
    Big,
}

impl Width {
    fn class(self, c: char) -> Class {
        match self {
            Width::Small => Class::of(c),
            Width::Big => Class::of_big(c),
        }
    }
}

/// The `(offset, char)` pairs of `text`, so scans read one way in both
/// directions.
fn chars(text: &str) -> Vec<(usize, char)> {
    text.char_indices().collect()
}

/// Index into `chars` of the char containing `at`, or `len` when `at` is at or
/// past the end.
fn index_of(cs: &[(usize, char)], at: usize) -> usize {
    cs.iter().position(|(i, _)| *i >= at).unwrap_or(cs.len())
}

/// `w` — the start of the next word.
///
/// **Always advances** (rule 2) and **never lands on whitespace** (rule 3):
/// whitespace is crossed, not stopped in. At the end of the text it saturates
/// at `text.len()`, which is what makes `dw` on the last word delete all of it
/// rather than nothing.
#[must_use]
pub fn word_start_next(text: &str, from: usize, width: Width) -> usize {
    let cs = chars(text);
    let mut i = index_of(&cs, from);
    if i >= cs.len() {
        return text.len();
    }
    // Step off the current run: `w` from anywhere inside a word lands on the
    // NEXT word, never on the same one.
    let start_class = width.class(cs[i].1);
    while i < cs.len() && width.class(cs[i].1) == start_class {
        i += 1;
    }
    // Cross any whitespace. A word start is never inside a blank run.
    while i < cs.len() && width.class(cs[i].1) == Class::Space {
        i += 1;
    }
    cs.get(i).map_or(text.len(), |(off, _)| *off)
}

/// `b` — the start of the previous word. Always retreats (rule 2).
#[must_use]
pub fn word_start_prev(text: &str, from: usize, width: Width) -> usize {
    let cs = chars(text);
    let mut i = index_of(&cs, from);
    if i == 0 {
        return 0;
    }
    i -= 1;
    // Back over whitespace first — it is a gap, never a destination.
    while i > 0 && width.class(cs[i].1) == Class::Space {
        i -= 1;
    }
    if width.class(cs[i].1) == Class::Space {
        return cs[i].0;
    }
    // Then to the start of the run we are now in.
    let run = width.class(cs[i].1);
    while i > 0 && width.class(cs[i - 1].1) == run {
        i -= 1;
    }
    cs[i].0
}

/// `e` — the LAST character of the next word.
///
/// Returns the offset of that character, not one past it: this is where the
/// caret lands. The operator's exclusive end is derived by
/// [`operated_span`]'s inclusive widening, never returned here — one type for
/// both readings is how `de` comes to leave a character behind (rule 1).
#[must_use]
pub fn word_end_next(text: &str, from: usize, width: Width) -> usize {
    let cs = chars(text);
    if cs.is_empty() {
        return 0;
    }
    let mut i = index_of(&cs, from);
    // Always advance: on the last character of a word, `e` goes to the end of
    // the NEXT one rather than standing still (rule 2).
    i += 1;
    while i < cs.len() && width.class(cs[i].1) == Class::Space {
        i += 1;
    }
    if i >= cs.len() {
        return cs[cs.len() - 1].0;
    }
    let run = width.class(cs[i].1);
    while i + 1 < cs.len() && width.class(cs[i + 1].1) == run {
        i += 1;
    }
    cs[i].0
}

/// `ge` — the LAST character of the PREVIOUS word. Mirror of
/// [`word_end_next`], and inclusive for the same reason.
#[must_use]
pub fn word_end_prev(text: &str, from: usize, width: Width) -> usize {
    let cs = chars(text);
    if cs.is_empty() {
        return 0;
    }
    let start = index_of(&cs, from);
    // Always retreat, so standing on a word's last character does not stand
    // still — the mirror of `e`'s always-advance.
    let Some(mut i) = start.checked_sub(1) else {
        return 0;
    };
    // Leave the current run first, or `ge` from inside a word lands one
    // character left — still inside the word it was asked to leave.
    if let Some(&(_, here)) = cs.get(start) {
        let run = width.class(here);
        if run != Class::Space {
            while i > 0 && width.class(cs[i].1) == run {
                i -= 1;
            }
        }
    }
    while i > 0 && width.class(cs[i].1) == Class::Space {
        i -= 1;
    }
    cs[i].0
}

/// `f` / `F` / `t` / `T` — the character search. `None` when the character is
/// not on the line, so an operator over it aborts rather than reaching the
/// line edge.
#[must_use]
pub fn find_char(text: &str, from: usize, ch: char, backward: bool, till: bool) -> Option<usize> {
    let cs = chars(text);
    let cur = index_of(&cs, from);
    let hit = if backward {
        // `T` stops AFTER the character, so it starts one further back or a
        // repeat would never leave the spot it already reached.
        let end = if till { cur.checked_sub(1)? } else { cur };
        (0..end).rev().find(|&i| cs[i].1 == ch)?
    } else {
        let start = if till { cur + 2 } else { cur + 1 };
        (start.min(cs.len())..cs.len()).find(|&i| cs[i].1 == ch)?
    };
    let i = match (backward, till) {
        (false, true) => hit - 1,
        (true, true) => hit + 1,
        _ => hit,
    };
    Some(cs[i].0)
}

/// The last non-blank of the line (`g_`). Inclusive, unlike `$`.
#[must_use]
pub fn last_non_blank(text: &str) -> usize {
    text.char_indices()
        .rfind(|(_, c)| !c.is_whitespace())
        .map_or(0, |(i, _)| i)
}

/// The first non-blank of the line (`^`).
#[must_use]
pub fn first_non_blank(text: &str) -> usize {
    text.char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map_or(0, |(i, _)| i)
}

/// One char left of `at`, saturating at 0.
#[must_use]
pub fn prev_char(text: &str, at: usize) -> usize {
    text[..at.min(text.len())]
        .char_indices()
        .next_back()
        .map_or(0, |(i, _)| i)
}

/// One char right of `at`, saturating at `text.len()`.
#[must_use]
pub fn next_char(text: &str, at: usize) -> usize {
    text[at.min(text.len())..]
        .char_indices()
        .nth(1)
        .map_or(text.len(), |(i, _)| at + i)
}

/// Where a [`Motion`] lands, applied `count` times.
///
/// `None` for a motion that has no single-line reading — `Up`, `Down`,
/// `GotoLine`, every paredit-structural arm, and the two search motions (a
/// picker's query IS the search; there is no separate match set). A typed
/// `None` rather than a silent no-op, so a caller can tell "this key does
/// nothing here" from "this key did nothing".
///
/// **Counted overrun stops early rather than failing** (rule 5): a motion that
/// stops making progress ends the loop and the position reached is returned,
/// which is what makes `d5w` near the end of a line delete to the end.
#[must_use]
pub fn resolve(text: &str, caret: usize, motion: Motion, count: usize) -> Option<usize> {
    let mut at = caret;
    for _ in 0..count.max(1) {
        let next = resolve_once(text, at, motion)?;
        if next == at {
            break;
        }
        at = next;
    }
    Some(at)
}

fn resolve_once(text: &str, at: usize, motion: Motion) -> Option<usize> {
    Some(match motion {
        Motion::Left => prev_char(text, at),
        Motion::Right => next_char(text, at),
        Motion::WordStartNext => word_start_next(text, at, Width::Small),
        Motion::WordStartPrev => word_start_prev(text, at, Width::Small),
        Motion::WordEndNext => word_end_next(text, at, Width::Small),
        Motion::WordEndPrev => word_end_prev(text, at, Width::Small),
        Motion::BigWordStartNext => word_start_next(text, at, Width::Big),
        Motion::BigWordStartPrev => word_start_prev(text, at, Width::Big),
        Motion::BigWordEndNext => word_end_next(text, at, Width::Big),
        Motion::BigWordEndPrev => word_end_prev(text, at, Width::Big),
        Motion::LineLastNonBlank => last_non_blank(text),
        Motion::FindChar { ch, backward, till } => find_char(text, at, ch, backward, till)?,
        // `|` is 1-based and clamped, which is what makes `500|` land on the
        // last character instead of refusing.
        Motion::Column(n) => {
            let cs = chars(text);
            cs.get((n.max(1) as usize) - 1).map_or(text.len(), |c| c.0)
        }
        // `gg` and `0` coincide here, as do `G` and `$` — on one line the
        // document IS the line. Merged rather than written twice so the
        // coincidence is stated instead of looking like a copy-paste.
        Motion::LineStart | Motion::DocStart => 0,
        Motion::LineFirstNonBlank => first_non_blank(text),
        Motion::LineEnd | Motion::DocEnd => text.len(),
        // No vertical axis on one line, and no separate search-match set.
        // Reported as "not applicable here", never quietly treated as a no-op.
        Motion::Up
        | Motion::Down
        | Motion::PageUp
        | Motion::PageDown
        | Motion::HalfPageUp
        | Motion::HalfPageDown
        | Motion::GotoLine(_)
        | Motion::SearchNext
        | Motion::SearchPrev
        | Motion::ForwardSexp
        | Motion::BackwardSexp
        | Motion::UpList
        | Motion::DownList
        | Motion::BeginningOfDefun
        | Motion::EndOfDefun
        | Motion::BeginningOfSexp
        | Motion::EndOfSexp
        // Multi-line by definition — a paragraph, a sentence boundary and a
        // screen row have no single-line reading. Reported, never faked.
        | Motion::ParagraphNext
        | Motion::ParagraphPrev
        | Motion::SentenceNext
        | Motion::SentencePrev
        | Motion::ScreenTop
        | Motion::ScreenMiddle
        | Motion::ScreenBottom
        | Motion::LineDownFirstNonBlank
        | Motion::LineUpFirstNonBlank
        // `%` needs a buffer to scan; a bracket that closes on the same line
        // is exactly the case nobody presses `%` for.
        | Motion::MatchPair
        // `;` repeats the LAST find, and unsoku holds no state between calls
        // — a stateless resolver cannot answer it. A caller that wants `;`
        // remembers its own `FindChar` and passes that.
        | Motion::RepeatFind { .. }
        // A mark is a remembered position in a DOCUMENT. unsoku holds one
        // line and no history, so it has nowhere for `ma` to have happened.
        | Motion::MarkExact(_)
        | Motion::MarkLine(_) => return None,
    })
}

/// Whether an operator's range over this motion **includes** the character the
/// motion landed on (rule 1).
///
/// This is the distinction whose absence is the classic vim-implementation
/// bug. `dw` is exclusive — it deletes up to but not including the next word's
/// first character. `de` is INCLUSIVE — it deletes through the character `e`
/// names. `d$` is inclusive for the same reason: `$` lands on the last
/// character and the operator must take it.
#[must_use]
pub fn is_inclusive(motion: Motion) -> bool {
    matches!(
        motion,
        Motion::WordEndNext
            | Motion::BigWordEndNext
            | Motion::LineEnd
            | Motion::DocEnd
            | Motion::LineLastNonBlank
            | Motion::FindChar {
                backward: false,
                ..
            }
    )
}

/// The byte range an operator would act over, `None` when the motion has no
/// single-line reading.
///
/// Three rules meet here, and each of them is a bug when omitted:
///
/// - **inclusive widening** (rule 1) — an inclusive motion's landing character
///   is taken, so the range end steps one char past it;
/// - **normalisation** (rule 4) — a backward motion resolves left of the
///   caret, so the ends are ordered rather than handed over inverted;
/// - **`cw` is not `ce`** (rule 6) — on a non-blank, `c`+`WordStartNext`
///   changes to the end of the *current* run, never crossing into the next
///   word. Implementing it as `ce` eats the following word whenever the caret
///   is already on a word's last character.
#[must_use]
pub fn operated_span(
    text: &str,
    caret: usize,
    op: Operator,
    motion: Motion,
    count: usize,
) -> Option<Range<usize>> {
    // vim's documented `cw` quirk, and it is a quirk about the RUN rather than
    // about `e`: on a blank, `cw` behaves like `dw`.
    if op == Operator::Change
        && motion == Motion::WordStartNext
        && text[caret.min(text.len())..]
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace())
    {
        let end = word_end_next(text, prev_char_or_same(text, caret), Width::Small);
        return Some(caret..next_char(text, end));
    }

    let target = resolve(text, caret, motion, count)?;
    let (lo, hi) = if target >= caret {
        let hi = if is_inclusive(motion) {
            next_char(text, target)
        } else {
            target
        };
        (caret, hi)
    } else {
        // Backward: the caret is the exclusive end. No backward motion in the
        // v1 alphabet is inclusive, so no widening applies here.
        (target, caret)
    };
    Some(lo..hi)
}

/// `caret` itself when it already starts a run, else one char back — so
/// [`word_end_next`]'s always-advance rule does not skip the run the caret is
/// standing in. Only `cw` needs this; every other caller wants the advance.
fn prev_char_or_same(text: &str, caret: usize) -> usize {
    if caret == 0 {
        0
    } else {
        prev_char(text, caret)
    }
}

/// The range a [`TextObject`] names, independent of where the caret would
/// move.
///
/// `iw` takes the run under the caret — **including a whitespace run**, which
/// is rule 3's other half: `w` never stops in blanks, but `iw` on a blank
/// selects the blanks. `aw` takes the run plus its trailing whitespace, and
/// falls back to LEADING whitespace when there is none — which is the case
/// that actually fires in a query, because the caret is usually at the end.
///
/// `None` for the objects a single line cannot express: [`TextObject::Line`]
/// (a line and its terminator, where there is one line and no terminator) and
/// the two search-match objects.
#[must_use]
pub fn object_span(text: &str, caret: usize, object: TextObject) -> Option<Range<usize>> {
    match object {
        TextObject::Word { around } => {
            let (start, end) = run_around(text, caret)?;
            if !around {
                return Some(start..end);
            }
            // Trailing whitespace first; only if there is none, leading.
            let after = text[end..]
                .char_indices()
                .take_while(|(_, c)| c.is_whitespace())
                .last()
                .map(|(i, c)| end + i + c.len_utf8());
            if let Some(after) = after {
                return Some(start..after);
            }
            let before = text[..start]
                .char_indices()
                .rev()
                .take_while(|(_, c)| c.is_whitespace())
                .last()
                .map(|(i, _)| i);
            Some(before.unwrap_or(start)..end)
        }
        TextObject::Delimited {
            open,
            close,
            around,
        } => delimited_span(text, caret, open, close, around),
        // A single line has no line object and no separate match set.
        TextObject::Line | TextObject::NextMatch | TextObject::PrevMatch => None,
    }
}

/// The maximal run of one class containing `caret`.
fn run_around(text: &str, caret: usize) -> Option<(usize, usize)> {
    let cs = chars(text);
    if cs.is_empty() {
        return None;
    }
    let i = index_of(&cs, caret).min(cs.len() - 1);
    let run = Class::of(cs[i].1);
    let mut lo = i;
    while lo > 0 && Class::of(cs[lo - 1].1) == run {
        lo -= 1;
    }
    let mut hi = i;
    while hi + 1 < cs.len() && Class::of(cs[hi + 1].1) == run {
        hi += 1;
    }
    Some((cs[lo].0, cs[hi].0 + cs[hi].1.len_utf8()))
}

/// `i(` / `a"` — the region between a matched pair.
///
/// Nesting is honoured when `open != close`; a quote (`open == close`) cannot
/// nest, so it scans for the nearest pair instead. One function rather than
/// two because that is the only thing that differs.
fn delimited_span(
    text: &str,
    caret: usize,
    open: char,
    close: char,
    around: bool,
) -> Option<Range<usize>> {
    let cs = chars(text);
    let at = index_of(&cs, caret).min(cs.len().saturating_sub(1));

    let (o, c) = if open == close {
        // A quote cannot nest, so the nearest enclosing pair is the last
        // opener at or before the caret.
        let enclosing = cs[..=at.min(cs.len().saturating_sub(1))]
            .iter()
            .rposition(|(_, ch)| *ch == open)
            .and_then(|before| {
                let after = cs[before + 1..].iter().position(|(_, ch)| *ch == close)?;
                Some((before, after + before + 1))
            });
        // **vim searches FORWARD when the caret is not inside a quoted
        // string** (`:h v_i"` — "if the cursor is not inside a quoted string,
        // find the next quoted string"). Without the fallback, `ci"` on
        // `name="old"` with the caret at the start does nothing — and the
        // start is exactly where a caret sits after typing a filter.
        if let Some(pair) = enclosing {
            pair
        } else {
            let from = at.min(cs.len().saturating_sub(1));
            let o = cs[from..].iter().position(|(_, ch)| *ch == open)? + from;
            let c = cs[o + 1..].iter().position(|(_, ch)| *ch == close)? + o + 1;
            (o, c)
        }
    } else {
        let mut depth = 0i32;
        let mut o = None;
        for i in (0..=at.min(cs.len().saturating_sub(1))).rev() {
            let ch = cs[i].1;
            if ch == close && i != at {
                depth += 1;
            } else if ch == open {
                if depth == 0 {
                    o = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
        let o = o?;
        let mut depth = 0i32;
        let mut c = None;
        for (i, (_, ch)) in cs.iter().enumerate().skip(o + 1) {
            if *ch == open {
                depth += 1;
            } else if *ch == close {
                if depth == 0 {
                    c = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
        (o, c?)
    };

    if around {
        Some(cs[o].0..cs[c].0 + cs[c].1.len_utf8())
    } else {
        Some(cs[o].0 + cs[o].1.len_utf8()..cs[c].0)
    }
}

/// Clamp a caret for a stance (rule 7).
///
/// Normal cannot rest past the last character; Insert can. Beyond
/// correctness this is what keeps a block caret drawable: a Normal caret
/// parked at `text.len()` has no character under it, so the badge says NORMAL
/// while the screen looks like INSERT.
#[must_use]
pub fn clamp(text: &str, at: usize, stance: Stance) -> usize {
    let at = at.min(text.len());
    match stance {
        Stance::Insert => at,
        Stance::Normal => {
            if at >= text.len() {
                prev_char(text, text.len())
            } else {
                at
            }
        }
    }
}

/// The unnamed register — charwise, which is the only width a single line has.
///
/// Every act that REMOVES text fills it (rule 8): `d c x X D C s`, plus `y`.
/// The rule is enforced structurally rather than remembered per arm — see
/// [`take`], which is the only way text leaves a target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Register {
    text: String,
}

impl Register {
    /// What was last removed or yanked.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether anything has been captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// Remove `span` from `target`, returning it and filling `register`.
///
/// The single removal path, so "every removal fills the register" is a
/// property of the code rather than a rule each caller remembers. The caret
/// lands at `span.start` — vim's landing for both `dw` and `db`, and the
/// reason the two feel symmetric.
pub fn take<T: TextTarget>(target: &mut T, span: Range<usize>, register: &mut Register) -> String {
    let removed = target.text()[span.clone()].to_owned();
    register.text.clone_from(&removed);
    let start = span.start;
    target.replace(span, "");
    target.set_caret(start);
    removed
}

/// Copy `span` into `register` without removing it (`y`).
///
/// The caret moves to `span.start` — the same landing as a delete, and the
/// reason `yb` visibly moves while `yw` does not.
pub fn yank<T: TextTarget>(target: &mut T, span: Range<usize>, register: &mut Register) {
    target.text()[span.clone()].clone_into(&mut register.text);
    target.set_caret(span.start);
}

/// `p` / `P` — paste the register.
///
/// `after` is `p`: charwise, it inserts *after* the character under the caret.
/// `P` inserts at the caret. The caret then lands ON the last pasted
/// character, not past it, which also satisfies the Normal clamp.
pub fn paste<T: TextTarget>(target: &mut T, register: &Register, after: bool, count: usize) {
    if register.is_empty() {
        return;
    }
    let text = register.text.repeat(count.max(1));
    let at = if after {
        next_char(target.text(), target.caret())
    } else {
        target.caret()
    };
    target.replace(at..at, &text);
    let end = at + text.len();
    target.set_caret(prev_char(target.text(), end));
}

#[cfg(test)]
mod tests;
