use thiserror::Error;

#[derive(Debug, Error)]
pub enum BufferError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid position {line}:{column} — buffer has {total_lines} line(s)")]
    InvalidPosition {
        line: u32,
        column: u32,
        total_lines: u32,
    },
    #[error("invalid range {start}..{end}")]
    InvalidRange { start: String, end: String },
    #[error("buffer has no path — save requires a path")]
    NoPath,
    #[error("nothing to undo")]
    NothingToUndo,
    #[error("nothing to redo")]
    NothingToRedo,
}
