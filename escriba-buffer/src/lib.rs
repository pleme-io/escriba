//! `escriba-buffer` — rope-backed text buffer.
//!
//! One [`Buffer`] owns a ropey `Rope`, knows its file path, detected line
//! ending, encoding, and a simple undo tree. Exposes position↔char
//! conversion (UTF-8 safe) and primitive [`escriba_core::Edit`] application.

extern crate self as escriba_buffer;

pub mod buffer;
pub mod encoding;
pub mod error;
pub mod line_ending;
pub mod undo;

pub use buffer::{Buffer, BufferSet, BufferSummary};
pub use encoding::Encoding;
pub use error::BufferError;
pub use line_ending::LineEnding;
pub use undo::{UndoEntry, UndoTree};
