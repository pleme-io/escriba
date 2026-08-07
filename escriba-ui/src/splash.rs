//! `Splash` — the start screen escriba paints when it opens with no file.
//!
//! ## Why this is a model and not three renderers
//!
//! Every editor in the category ships one of these (vim's `:intro`,
//! alpha.nvim / startify, emacs' `*GNU Emacs*` buffer, the VSCode welcome
//! tab) and every one of them re-implements the centering, the art, and
//! the menu inside its own draw path. escriba has THREE faces — ratatui,
//! GPU, and the one-shot ANSI dump — so a per-face start screen would be
//! the same layout arithmetic written three times, drifting three ways.
//!
//! This module is the one model. It answers exactly one question —
//! *"given a `width × height` canvas, what goes where, and in which
//! ROLE?"* — and returns [`SplashRow`]s. It never touches a color, a
//! terminal, or a GPU: a role becomes a color at the [`ChromePalette`]
//! seam, which is the same seam the rest of the chrome resolves through,
//! so the start screen tracks a theme change for free.
//!
//! ## Why the content is not here
//!
//! There is deliberately no `Splash::default()` carrying escriba's art
//! and menu. The content is authored — `(defsplash …)` in
//! `configs/blnvim-defaults.lisp` — and lowered into this type by
//! `escriba_lisp::apply_plan_to_splash`. A hand-written default factory
//! for something this configurable is exactly the shape this repo's
//! conventions forbid ("if it's configurable, it's a def-form").

use escriba_core::Action;
use ishou_tokens::Rgb;
use serde::{Deserialize, Serialize};

use crate::chrome::ChromePalette;

/// What a run of splash text MEANS. Roles, never hues — `MenuKey` is the
/// accent on every theme, and no call site knows which accent that is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplashRole {
    /// The wordmark.
    Art,
    /// The one-line statement under the wordmark.
    Tagline,
    /// The divider rule.
    Rule,
    /// The key an operator presses to take a menu entry.
    MenuKey,
    /// What that key does.
    MenuLabel,
    /// The bottom fact strip (version, counts, theme).
    Footer,
}

impl SplashRole {
    /// Resolve this role against a chrome palette.
    ///
    /// Total over the enum — no wildcard arm — so adding a role fails
    /// this to compile rather than silently painting it as body text.
    #[must_use]
    pub fn color(self, c: &ChromePalette) -> Rgb {
        match self {
            Self::Art => c.info,
            // The tagline is a statement, not chrome — `text_dim` on Nord
            // is the comment colour and sinks it into the background.
            Self::Tagline | Self::MenuLabel => c.text,
            Self::Rule | Self::Footer => c.text_dim,
            Self::MenuKey => c.accent,
        }
    }
}

/// One styled run within a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplashSpan {
    pub text: String,
    pub role: SplashRole,
}

/// One laid-out row: which terminal row, which starting column, and the
/// spans that fill it. Faces render this verbatim — there is no layout
/// arithmetic left for them to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplashRow {
    pub row: u16,
    pub col: u16,
    pub spans: Vec<SplashSpan>,
}

impl SplashRow {
    /// The row's text with styling dropped — what the ANSI-free and
    /// test paths compare against.
    #[must_use]
    pub fn plain(&self) -> String {
        let mut s = String::new();
        for span in &self.spans {
            s.push_str(&span.text);
        }
        s
    }

    /// Printable width in cells.
    #[must_use]
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| s.text.chars().count()).sum()
    }
}

/// A menu entry — a key, what it does in words, and the typed action it
/// dispatches. The action is already RESOLVED: the authoring layer turns
/// `:action "quit"` into [`Action::Quit`] before it reaches here, so the
/// runtime dispatches a menu entry through the same path a keybinding
/// takes, and an entry can never be a string the editor cannot honour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplashEntry {
    pub key: char,
    pub label: String,
    pub action: Action,
}

/// The start screen, as data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Splash {
    /// Wordmark rows, top to bottom. Dropped wholesale when the canvas
    /// is too small for them (see [`Splash::rows`]).
    pub art: Vec<String>,
    /// The line under the wordmark.
    pub tagline: String,
    /// The menu, in display order.
    pub entries: Vec<SplashEntry>,
    /// Footer facts — joined with `·` at render time. Filled by the
    /// binary with live values (version, plugin count, theme), so the
    /// strip states what this build actually is rather than what a
    /// static string once claimed.
    pub facts: Vec<String>,
}

/// Gap in cells between a menu key and its label.
const KEY_GAP: usize = 3;
/// Minimum side margin — the canvas must fit the block plus this on
/// each side or the block degrades.
const MARGIN: usize = 2;
/// The compact wordmark used when the canvas is too narrow for the art.
const COMPACT_ART: &str = "escriba";
/// Separator between footer facts.
const FACT_SEP: &str = " · ";

impl Splash {
    /// Nothing to paint — no art, no tagline, no entries, no facts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.art.is_empty()
            && self.tagline.is_empty()
            && self.entries.is_empty()
            && self.facts.is_empty()
    }

    /// The entry `key` selects, if any.
    #[must_use]
    pub fn entry_for(&self, key: char) -> Option<&SplashEntry> {
        self.entries.iter().find(|e| e.key == key)
    }

    /// Lay the splash out on a `width × height` canvas.
    ///
    /// Degrades rather than overflowing, in a fixed order: the art drops
    /// to a one-line wordmark when it does not fit the width, the whole
    /// art block drops when it does not fit the height, and the menu
    /// truncates from the bottom last. An empty result means the canvas
    /// is too small for even the wordmark — the caller paints nothing,
    /// never a mangled frame.
    ///
    /// Rows are guaranteed to sit inside the canvas: `row < height` and
    /// `col + row_width <= width` for every row returned.
    #[must_use]
    pub fn rows(&self, width: u16, height: u16) -> Vec<SplashRow> {
        let (w, h) = (width as usize, height as usize);
        if self.is_empty() || w <= MARGIN * 2 || h == 0 {
            return Vec::new();
        }
        let usable = w - MARGIN * 2;

        let mut blocks = self.blocks(usable);
        // Height degradation, worst-affordable-first: the art is the
        // largest and least load-bearing block, so it goes before a
        // single menu entry does.
        if total_lines(&blocks) > h {
            blocks.retain(|b| b.kind != BlockKind::Art);
        }
        while total_lines(&blocks) > h {
            if !truncate_menu(&mut blocks) {
                return Vec::new();
            }
        }

        let total = total_lines(&blocks);
        // Optical centre, not geometric: a block placed at exact centre
        // reads as sitting low, so it is nudged to two-fifths down.
        let top = (h - total) * 2 / 5;

        let mut out = Vec::with_capacity(total);
        let mut row = top;
        for block in &blocks {
            // Each block is centred as a UNIT on its own widest line, so
            // menu entries stay column-aligned with each other instead of
            // each entry drifting to its own centre.
            let bw = block.width();
            let left = MARGIN + (usable.saturating_sub(bw)) / 2;
            for line in &block.lines {
                if !line.is_empty() {
                    // Both fit: `row` is bounded by `height` and `left` by
                    // `width`, each of which arrived as a u16. The saturating
                    // conversion is belt-and-braces, never reached.
                    out.push(SplashRow {
                        row: u16::try_from(row).unwrap_or(u16::MAX),
                        col: u16::try_from(left).unwrap_or(u16::MAX),
                        spans: line.clone(),
                    });
                }
                row += 1;
            }
        }
        out
    }

    /// The same layout, flattened into one screen-shaped stream of
    /// `(text, role)` chunks whose concatenation IS the screen — padding
    /// and newlines included.
    ///
    /// [`Self::rows`] suits a face that positions widgets (ratatui); this
    /// suits the two that emit a character stream (the ANSI dump and the
    /// GPU's rich-text buffer, which requires a coverage-complete
    /// partition). Both derive from `rows`, so a face can pick the shape
    /// it wants without a second layout ever existing.
    ///
    /// Trailing blank lines are not emitted — nothing is gained by
    /// painting empty rows to the bottom of the canvas.
    #[must_use]
    pub fn screen_chunks(&self, width: u16, height: u16) -> Vec<SplashSpan> {
        let rows = self.rows(width, height);
        let mut out: Vec<SplashSpan> = Vec::new();
        let mut cursor_row = 0u16;
        for r in &rows {
            for _ in cursor_row..r.row {
                out.push(pad("\n"));
            }
            cursor_row = r.row + 1;
            if r.col > 0 {
                out.push(pad(&" ".repeat(r.col as usize)));
            }
            out.extend(r.spans.iter().cloned());
            out.push(pad("\n"));
        }
        out
    }

    /// Compose the vertical blocks, already clipped to `usable` width.
    fn blocks(&self, usable: usize) -> Vec<Block> {
        let mut blocks = Vec::new();

        if !self.art.is_empty() {
            let fits = self.art.iter().all(|l| l.chars().count() <= usable);
            let lines: Vec<Vec<SplashSpan>> = if fits {
                self.art
                    .iter()
                    .map(|l| span_line(l, SplashRole::Art))
                    .collect()
            } else {
                vec![span_line(COMPACT_ART, SplashRole::Art)]
            };
            blocks.push(Block::new(BlockKind::Art, lines));
        }

        if !self.tagline.is_empty() {
            let tagline = clip(&self.tagline, usable);
            let rule_width = tagline.chars().count().min(usable);
            blocks.push(Block::new(
                BlockKind::Head,
                vec![
                    Vec::new(),
                    span_line(&tagline, SplashRole::Tagline),
                    span_line(&"─".repeat(rule_width), SplashRole::Rule),
                ],
            ));
        }

        if !self.entries.is_empty() {
            let label_room = usable.saturating_sub(1 + KEY_GAP);
            let mut lines = vec![Vec::new()];
            for e in &self.entries {
                let mut gap = String::with_capacity(KEY_GAP);
                for _ in 0..KEY_GAP {
                    gap.push(' ');
                }
                gap.push_str(&clip(&e.label, label_room));
                lines.push(vec![
                    SplashSpan {
                        text: e.key.to_string(),
                        role: SplashRole::MenuKey,
                    },
                    SplashSpan {
                        text: gap,
                        role: SplashRole::MenuLabel,
                    },
                ]);
            }
            blocks.push(Block::new(BlockKind::Menu, lines));
        }

        if !self.facts.is_empty() {
            let strip = clip(&self.facts.join(FACT_SEP), usable);
            blocks.push(Block::new(
                BlockKind::Foot,
                vec![Vec::new(), span_line(&strip, SplashRole::Footer)],
            ));
        }

        blocks
    }
}

/// Which vertical block a group of lines belongs to. Only the degrade
/// order reads this, but naming the groups is what makes that order
/// reviewable instead of an index into a vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Art,
    Head,
    Menu,
    Foot,
}

/// A vertical run of lines centred as one unit.
struct Block {
    kind: BlockKind,
    lines: Vec<Vec<SplashSpan>>,
}

impl Block {
    fn new(kind: BlockKind, lines: Vec<Vec<SplashSpan>>) -> Self {
        Self { kind, lines }
    }

    /// The block's widest line — what it is centred on.
    fn width(&self) -> usize {
        self.lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.chars().count()).sum::<usize>())
            .max()
            .unwrap_or(0)
    }
}

fn total_lines(blocks: &[Block]) -> usize {
    blocks.iter().map(|b| b.lines.len()).sum()
}

/// Drop the last menu entry. Returns `false` when there is nothing left
/// to give up, which is the caller's signal to render nothing at all.
fn truncate_menu(blocks: &mut Vec<Block>) -> bool {
    let Some(menu) = blocks.iter_mut().find(|b| b.kind == BlockKind::Menu) else {
        return false;
    };
    // `> 1` keeps the leading blank spacer honest: a menu block that is
    // only its spacer is no menu, so it is removed outright below.
    if menu.lines.len() > 1 {
        menu.lines.pop();
        if menu.lines.len() == 1 {
            blocks.retain(|b| b.kind != BlockKind::Menu);
        }
        return true;
    }
    blocks.retain(|b| b.kind != BlockKind::Menu);
    !blocks.is_empty()
}

/// Whitespace carrying no meaning. It needs SOME role to keep the stream a
/// complete partition (glyphon's `set_rich_text` requires one); `Footer` is
/// the quietest, and spaces render the same in any of them.
fn pad(text: &str) -> SplashSpan {
    SplashSpan {
        text: text.to_string(),
        role: SplashRole::Footer,
    }
}

fn span_line(text: &str, role: SplashRole) -> Vec<SplashSpan> {
    vec![SplashSpan {
        text: text.to_string(),
        role,
    }]
}

/// Clip to `max` CHARACTERS (not bytes) so multibyte content never
/// splits mid-glyph.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use escriba_core::Mode;

    fn sample() -> Splash {
        Splash {
            art: vec!["  ___  ".into(), " |___| ".into()],
            tagline: "a modal editor".into(),
            entries: vec![
                SplashEntry {
                    key: 'e',
                    label: "start editing".into(),
                    action: Action::ChangeMode(Mode::Normal),
                },
                SplashEntry {
                    key: 'q',
                    label: "quit".into(),
                    action: Action::Quit,
                },
            ],
            facts: vec!["v0.1.0".into(), "nord".into()],
        }
    }

    #[test]
    fn empty_splash_paints_nothing() {
        assert!(Splash::default().rows(120, 40).is_empty());
    }

    #[test]
    fn rows_stay_inside_the_canvas() {
        // The invariant every face relies on: nothing this returns can
        // draw outside the area it was given.
        for (w, h) in [(120u16, 40u16), (80, 24), (40, 12), (20, 8), (10, 4)] {
            for r in sample().rows(w, h) {
                assert!(r.row < h, "row {} outside height {h}", r.row);
                assert!(
                    r.col as usize + r.width() <= w as usize,
                    "row {} overflows width {w}: col={} width={}",
                    r.row,
                    r.col,
                    r.width(),
                );
            }
        }
    }

    #[test]
    fn a_roomy_canvas_shows_art_tagline_menu_and_facts() {
        let text: Vec<String> = sample()
            .rows(120, 40)
            .iter()
            .map(SplashRow::plain)
            .collect();
        let joined = text.join("\n");
        assert!(joined.contains("|___|"), "art missing: {joined}");
        assert!(joined.contains("a modal editor"), "tagline missing");
        assert!(joined.contains("start editing"), "menu missing");
        assert!(joined.contains("nord"), "facts missing");
    }

    #[test]
    fn a_narrow_canvas_falls_back_to_the_compact_wordmark() {
        let s = Splash {
            art: vec!["#".repeat(60)],
            ..sample()
        };
        let joined = s
            .rows(30, 20)
            .iter()
            .map(SplashRow::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains(COMPACT_ART),
            "no compact wordmark: {joined}"
        );
        assert!(
            !joined.contains("######"),
            "wide art survived a narrow canvas"
        );
    }

    #[test]
    fn a_short_canvas_drops_the_art_before_the_menu() {
        // 7 lines: not enough for art + head + menu + foot. The art goes
        // first (largest, least actionable) and the menu truncates from the
        // BOTTOM, so the first entries — the ones authored as most useful —
        // are the ones that survive.
        let joined = sample()
            .rows(80, 7)
            .iter()
            .map(SplashRow::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !joined.contains("|___|"),
            "art should have dropped: {joined}"
        );
        assert!(
            joined.contains("start editing"),
            "the first menu entry must survive: {joined}",
        );
    }

    #[test]
    fn a_canvas_with_no_room_paints_nothing_rather_than_a_mangled_frame() {
        assert!(sample().rows(120, 1).is_empty());
        assert!(sample().rows(2, 40).is_empty());
    }

    #[test]
    fn menu_entries_share_one_left_column() {
        // Column alignment is the whole reason blocks are centred as a
        // unit; if each entry centred itself the keys would zig-zag.
        let rows = sample().rows(120, 40);
        let cols: Vec<u16> = rows
            .iter()
            .filter(|r| {
                r.spans
                    .first()
                    .is_some_and(|s| s.role == SplashRole::MenuKey)
            })
            .map(|r| r.col)
            .collect();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0], cols[1], "menu keys must share a column");
    }

    #[test]
    fn entry_lookup_resolves_the_typed_action() {
        let s = sample();
        assert_eq!(s.entry_for('q').map(|e| &e.action), Some(&Action::Quit));
        assert!(s.entry_for('z').is_none());
    }

    #[test]
    fn roles_resolve_to_distinct_chrome_colors() {
        // A start screen whose accent and body text resolve the same is
        // one where the menu keys stop reading as keys.
        let c = ChromePalette::prescribed();
        assert_ne!(
            SplashRole::MenuKey.color(&c).hex(),
            SplashRole::MenuLabel.color(&c).hex(),
        );
        assert_ne!(
            SplashRole::Art.color(&c).hex(),
            SplashRole::Footer.color(&c).hex(),
        );
    }

    #[test]
    fn screen_chunks_reconstruct_the_same_screen_as_rows() {
        // The two projections must never disagree — that would be two
        // layouts again, which is the thing this module exists to prevent.
        let s = sample();
        let chunks: String = s
            .screen_chunks(80, 24)
            .iter()
            .map(|c| c.text.as_str())
            .collect();
        for r in s.rows(80, 24) {
            let line = chunks.lines().nth(r.row as usize).unwrap_or("");
            assert_eq!(
                line,
                format!("{}{}", " ".repeat(r.col as usize), r.plain()),
                "row {} differs between rows() and screen_chunks()",
                r.row,
            );
        }
    }

    #[test]
    fn screen_chunks_are_a_complete_partition() {
        // glyphon's set_rich_text needs every byte covered exactly once.
        let s = sample();
        let chunks = s.screen_chunks(80, 24);
        assert!(!chunks.is_empty());
        assert!(
            chunks.iter().all(|c| !c.text.is_empty()),
            "an empty chunk is a wasted span",
        );
    }

    #[test]
    fn clip_never_splits_a_multibyte_glyph() {
        assert_eq!(clip("─────", 3), "───");
        assert_eq!(clip("abc", 10), "abc");
    }
}
