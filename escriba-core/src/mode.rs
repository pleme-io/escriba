use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Editor modes — vim-inspired with a small ceiling.
///
/// More modes (operator-pending, replace, insert-visual) are phase-2 add-ons
/// layered through pending state, not new enum variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
    VisualLine,
    Command,
}

impl Mode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::VisualLine => "V-LINE",
            Self::Command => "COMMAND",
        }
    }

    #[must_use]
    pub const fn is_insertish(self) -> bool {
        matches!(self, Self::Insert | Self::Command)
    }

    #[must_use]
    pub const fn is_visualish(self) -> bool {
        matches!(self, Self::Visual | Self::VisualLine)
    }

    /// The [`CursorShape`] this mode renders, per the vim editor
    /// convention every backend (TUI + GPU) shares. Routing both renderers
    /// through this one typed function makes "the GPU and TUI cursors
    /// disagree on shape" unrepresentable — the shape is derived from the
    /// mode in exactly one place.
    #[must_use]
    pub const fn cursor_shape(self) -> CursorShape {
        match self {
            // Normal navigation + command-line entry: a solid block.
            Self::Normal | Self::Command => CursorShape::Block,
            // Insert: a thin vertical bar between glyphs (vim `i`-mode).
            Self::Insert => CursorShape::Bar,
            // Visual selection: an underline so the selected cell stays
            // legible under the highlight.
            Self::Visual | Self::VisualLine => CursorShape::Underline,
        }
    }
}

/// The on-screen shape a cursor takes. A typed enum so the per-mode shape
/// is a total, exhaustively-matched mapping (a new mode forces a new arm)
/// rather than a renderer-local `if mode == …` ladder that can silently
/// drift between backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub enum CursorShape {
    /// Full-cell solid block — Normal / Command modes.
    #[default]
    Block,
    /// Thin vertical bar between cells — Insert mode.
    Bar,
    /// Underline along the cell's baseline — Visual / VisualLine modes.
    Underline,
}

impl CursorShape {
    /// A short stable label — for `--spec` surfaces, status introspection,
    /// and tests.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Bar => "bar",
            Self::Underline => "underline",
        }
    }
}

/// A one-shot transition request — emitted by the keymap, consumed by the
/// editor loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ModeTransition {
    pub from: Mode,
    pub to: Mode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_normal() {
        assert_eq!(Mode::default(), Mode::Normal);
    }

    #[test]
    fn classifiers() {
        assert!(Mode::Insert.is_insertish());
        assert!(Mode::Command.is_insertish());
        assert!(!Mode::Normal.is_insertish());
        assert!(Mode::Visual.is_visualish());
        assert!(Mode::VisualLine.is_visualish());
    }

    #[test]
    fn display_labels() {
        assert_eq!(Mode::Normal.as_str(), "NORMAL");
        assert_eq!(Mode::Insert.as_str(), "INSERT");
    }

    #[test]
    fn cursor_shape_per_mode() {
        // Block in navigation/command, bar in insert, underline in visual.
        assert_eq!(Mode::Normal.cursor_shape(), CursorShape::Block);
        assert_eq!(Mode::Command.cursor_shape(), CursorShape::Block);
        assert_eq!(Mode::Insert.cursor_shape(), CursorShape::Bar);
        assert_eq!(Mode::Visual.cursor_shape(), CursorShape::Underline);
        assert_eq!(Mode::VisualLine.cursor_shape(), CursorShape::Underline);
    }

    #[test]
    fn cursor_shape_labels() {
        assert_eq!(CursorShape::Block.as_str(), "block");
        assert_eq!(CursorShape::Bar.as_str(), "bar");
        assert_eq!(CursorShape::Underline.as_str(), "underline");
    }

    #[test]
    fn insert_is_the_only_bar_mode() {
        // Forcing function: exactly one mode renders a bar cursor (Insert).
        let modes = [
            Mode::Normal,
            Mode::Insert,
            Mode::Visual,
            Mode::VisualLine,
            Mode::Command,
        ];
        let bars = modes
            .into_iter()
            .filter(|m| m.cursor_shape() == CursorShape::Bar)
            .count();
        assert_eq!(bars, 1);
    }
}
