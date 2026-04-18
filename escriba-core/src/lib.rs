//! `escriba-core` — foundational types.
//!
//! No I/O. No rendering. No tatara-lisp. Just the typed vocabulary every
//! other escriba crate speaks:
//!
//!   - [`Position`] — line + column, 0-indexed, UTF-8 char offsets.
//!   - [`Range`] — half-open `[start, end)`.
//!   - [`Cursor`] — anchor + head (for selection growth).
//!   - [`Selection`] — multi-cursor ordered set.
//!   - [`Mode`] — the vim-ish modal state.
//!   - [`Motion`] / [`Operator`] — compose into commands.
//!   - [`Edit`] — primitive mutation.
//!   - [`Action`] — top-level dispatched-to-buffer command.
//!   - [`BufferId`] / [`WindowId`] — opaque identifiers.
//!
//! Every type derives `schemars::JsonSchema` so escriba-api can emit an
//! OpenAPI spec over the full domain surface — the editor's entire public
//! shape is spec-first by construction.

extern crate self as escriba_core;

pub mod action;
pub mod edit;
pub mod id;
pub mod mode;
pub mod motion;
pub mod position;
pub mod range;
pub mod selection;

pub use action::{Action, CountedAction};
pub use edit::{Edit, EditKind};
pub use id::{BufferId, CaretId, WindowId};
pub use mode::{Mode, ModeTransition};
pub use motion::{Motion, Operator};
pub use position::Position;
pub use range::Range;
pub use selection::{Cursor, Selection};
