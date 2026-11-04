use std::path::PathBuf;

use crate::{Position, Range};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChange {
    pub range: Range,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionEntry {
    pub label: String,
    pub detail: Option<String>,
    pub tier: Option<String>,
    pub documentation: Option<String>,
    pub filter: String,
    pub sort: String,
    pub insert: String,
    pub edit: Option<TextChange>,
    pub additional: Vec<TextChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHint {
    pub position: Position,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    Starting,
    Ready {
        generation: u32,
        pid: Option<u32>,
        completes: bool,
        completion_triggers: Vec<char>,
    },
    Slow {
        waited_ms: u64,
    },
    Restarting {
        attempt: u32,
        limit: u32,
        after_ms: u64,
        reason: String,
    },
    Dead {
        reason: String,
    },
    Missing {
        command: String,
    },
}

impl ServerState {
    #[must_use]
    pub const fn answers(&self) -> bool {
        matches!(self, Self::Ready { .. } | Self::Slow { .. })
    }

    pub fn render_into(&self, out: &mut String) {
        match self {
            Self::Starting => out.push_str("starting"),
            Self::Ready { .. } => out.push_str("ready"),
            Self::Slow { waited_ms } => {
                out.push_str("slow ");
                out.push_str(&waited_ms.to_string());
                out.push_str("ms");
            }
            Self::Restarting { attempt, limit, .. } => {
                out.push_str("restarting ");
                out.push_str(&attempt.to_string());
                out.push('/');
                out.push_str(&limit.to_string());
            }
            Self::Dead { reason } => {
                out.push_str("DEAD ");
                out.push_str(reason);
            }
            Self::Missing { command } => {
                out.push_str("MISSING ");
                out.push_str(command);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerStatus {
    pub languages: Vec<String>,
    pub command: String,
    pub root: PathBuf,
    pub state: ServerState,
}
