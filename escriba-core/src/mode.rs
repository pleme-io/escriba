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
}
