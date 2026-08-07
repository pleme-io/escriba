//! `Outcome` — what a handler hands back over the counter.
//!
//! Slips plus a verdict. The verdict exists because Phase 0 established that
//! *did nothing* and *worked* must be distinguishable, and there are three
//! states, not two: a handler can succeed, can decline for a good reason, or
//! can fail. Collapsing decline into either of the others is what makes an
//! editor either lie ("done!") or cry wolf ("error!") when you press `]c`
//! with no conflicts left.

use crate::negai::Negai;

/// How a handler's attempt went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It did what was asked.
    Did,
    /// Nothing to do, and that is fine — `]h` with no hunks below, `n` with
    /// no matches, `gd` on a comment. NOT an error: the operator asked a
    /// reasonable question and the answer is "none". Carries the sentence to
    /// show, because "nothing happened" with no explanation is the silence
    /// Phase 0 removed.
    Declined(String),
    /// It could not do what was asked. A real failure the operator should
    /// see: a file that would not read, a formatter that exited non-zero.
    Failed(String),
}

impl Verdict {
    #[must_use]
    pub const fn is_failure(&self) -> bool {
        matches!(self, Self::Failed(_))
    }

    /// The sentence to show, if any. `Did` is silent — success must stay
    /// quiet or the message channel becomes noise and gets ignored, which
    /// costs exactly the visibility Phase 0 bought.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Did => None,
            Self::Declined(m) | Self::Failed(m) => Some(m),
        }
    }
}

/// What comes back over the counter: the slips, and how it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub slips: Vec<Negai>,
    pub verdict: Verdict,
}

impl Outcome {
    /// Did the work, asking for these slips.
    #[must_use]
    pub fn did(slips: impl Into<Vec<Negai>>) -> Self {
        Self {
            slips: slips.into(),
            verdict: Verdict::Did,
        }
    }

    /// Did the work, asking for nothing. Rare but legitimate: a handler whose
    /// whole job was to decide that nothing needs to change.
    #[must_use]
    pub const fn nothing() -> Self {
        Self {
            slips: Vec::new(),
            verdict: Verdict::Did,
        }
    }

    /// Nothing to do, and here is why.
    #[must_use]
    pub fn declined(why: impl Into<String>) -> Self {
        Self {
            slips: Vec::new(),
            verdict: Verdict::Declined(why.into()),
        }
    }

    /// Could not.
    #[must_use]
    pub fn failed(why: impl Into<String>) -> Self {
        Self {
            slips: Vec::new(),
            verdict: Verdict::Failed(why.into()),
        }
    }

    /// A failed outcome carries no slips — that invariant matters enough to
    /// be checkable, because half-applying the requests of a handler that
    /// then reported failure is how an editor ends up in a state nobody
    /// designed. Honest tier: **only-mitigated**. The constructors above
    /// cannot build such a value, but `Outcome` has public fields, so a
    /// caller can still assemble one by hand. Making it unrepresentable
    /// means sealing the fields, which is worth doing once M1 shows what
    /// real handlers need to construct.
    #[must_use]
    pub fn is_coherent(&self) -> bool {
        !(self.verdict.is_failure() && !self.slips.is_empty())
    }
}
