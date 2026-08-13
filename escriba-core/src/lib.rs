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
//!   - [`Register`] — captured text plus the [`RegisterKind`] a put replays it as.
//!   - [`Action`] — top-level dispatched-to-buffer command.
//!   - [`BufferId`] / [`WindowId`] — opaque identifiers.
//!
//! Every type derives `schemars::JsonSchema` so escriba-api can emit an
//! OpenAPI spec over the full domain surface — the editor's entire public
//! shape is spec-first by construction.

extern crate self as escriba_core;

pub mod action;
pub mod damage;
pub mod edit;
pub mod filetype;
pub mod r#gen;
pub mod id;
pub mod jumplist;
pub mod mode;
pub mod motion;
pub mod position;
pub mod range;
pub mod register;
pub mod selection;

pub use action::{Action, CountedAction, HighlightEffect, InsertAt, TextEffect, ViewAlign};
// `Action::SearchOpen` carries it, so anything that CONSTRUCTS an action needs
// it. Re-exported here rather than making every such crate take a direct
// escriba-search dependency for one enum.
pub use damage::{Damage, LineDelta};
pub use edit::{Edit, EditKind};
pub use escriba_search::Direction as SearchDirection;
pub use filetype::{CommentString, Filetype, FiletypeTable};
pub use r#gen::EditGen;
pub use id::{BufferId, CaretId, WindowId};
pub use jumplist::{JUMPLIST_LIMIT, JumpList, Spot};
// memori lives in its own leaf crate so `escriba-search` — which sits BELOW
// core in the dependency graph — can use it too. Re-exported here so position
// code in core and above reads naturally without naming a second crate.
pub use escriba_memori::{Anchored, Bound, Bytes, Chars, Offset, Ruler, Scale, Utf16Units};
pub use mode::{CursorShape, Mode, ModeTransition};
pub use motion::{Motion, Operator, TextObject};
pub use position::Position;
pub use range::Range;
pub use register::{Register, RegisterKind};
pub use selection::{Cursor, Cursors, Selection};
