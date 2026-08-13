//! The register — captured text plus **how it was captured**.
//!
//! A register is not a `String`. `dw` and `dd` both leave text behind, and a
//! put has to replay them differently: `dw`'s text goes back *inside* a line
//! at a column, `dd`'s goes back *as* a line. vim carries that distinction on
//! the register, not on the put key, which is why `p` after `dw` and `p` after
//! `dd` do visibly different things from the same keystroke.
//!
//! Carrying it as a typed [`RegisterKind`] rather than a `linewise: bool`
//! makes the put's `match` total: visual-block's `Blockwise` — the one kind
//! vim has that escriba does not yet — fails to compile at every consumer when
//! it lands, instead of silently taking the charwise arm and pasting a
//! rectangle as a run of text.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How a register's text was captured, and therefore how a put must replay it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum RegisterKind {
    /// Captured as a run of characters (`dw`, `y$`, `diw`, `d/pat`).
    /// A put inserts it at a column, inside whatever line the cursor is on.
    Charwise,
    /// Captured as whole lines, terminators included (`dd`, `yy`, `3dd`).
    /// A put opens new lines above or below the cursor's line.
    Linewise,
}

/// The unnamed register's contents.
///
/// `text` is stored exactly as it was captured — a linewise capture keeps its
/// trailing newline, because that newline is what makes it a *line*. The put
/// normalizes rather than the capture, so the register always reads back what
/// the buffer gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Register {
    pub text: String,
    pub kind: RegisterKind,
}

impl Register {
    #[must_use]
    pub fn new(text: impl Into<String>, kind: RegisterKind) -> Self {
        Self {
            text: text.into(),
            kind,
        }
    }

    #[must_use]
    pub fn charwise(text: impl Into<String>) -> Self {
        Self::new(text, RegisterKind::Charwise)
    }

    #[must_use]
    pub fn linewise(text: impl Into<String>) -> Self {
        Self::new(text, RegisterKind::Linewise)
    }

    #[must_use]
    pub const fn is_linewise(&self) -> bool {
        matches!(self.kind, RegisterKind::Linewise)
    }

    /// The text a put of `count` copies should insert.
    ///
    /// Linewise content is newline-TERMINATED before repeating, so `2p` of a
    /// register captured from a file with no trailing newline still yields two
    /// separate lines rather than one glued pair. The capture is left alone
    /// (see the struct note) and the normalization happens here, once, where
    /// both put directions share it.
    #[must_use]
    pub fn replayed(&self, count: u32) -> String {
        let unit = match self.kind {
            RegisterKind::Charwise => self.text.clone(),
            RegisterKind::Linewise if self.text.ends_with('\n') => self.text.clone(),
            RegisterKind::Linewise => {
                let mut t = self.text.clone();
                t.push('\n');
                t
            }
        };
        unit.repeat(count.max(1) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_linewise_replay_is_newline_terminated_even_when_the_capture_was_not() {
        // The case a naive `text.repeat(n)` gets wrong: the last line of a
        // file with no trailing newline yanks as `"foxtrot"`, and `2p` of it
        // must be two lines, not `"foxtrotfoxtrot"`.
        assert_eq!(
            Register::linewise("foxtrot").replayed(2),
            "foxtrot\nfoxtrot\n"
        );
    }

    #[test]
    fn a_charwise_replay_is_verbatim() {
        assert_eq!(Register::charwise("ab").replayed(3), "ababab");
    }

    #[test]
    fn a_zero_count_still_puts_once() {
        // Counts reach here already defaulted to 1, but a 0 that slipped
        // through must not silently delete the put.
        assert_eq!(Register::charwise("x").replayed(0), "x");
    }
}
