//! `escriba-shirube` (標, "a waymark") — one typed located finding, and the
//! named lists of them you navigate.
//!
//! ## The bet
//!
//! Seven subsystems in escriba's backlog produce the same shape: a
//! diagnostic, a git hunk, a test failure, a grep hit, a TODO, a merge
//! conflict, an LSP reference. Each is a place in a file with something to
//! say about it. Modelling it once means each producer ships a SOURCE rather
//! than a subsystem, and the gutter, the list surface and the `]x`/`[x`
//! verbs are written once instead of seven times.
//!
//! ## What is load-bearing
//!
//! - [`Anchor`] is an axis SET, not a revision. A git hunk list is stale when
//!   the buffer moves **or** the index moves; a test result when the source
//!   moves **or** the binary is rebuilt. Getting this wrong is a gutter that
//!   lies, discovered long after the cause.
//! - A stale list reads as **empty**, not as its old contents
//!   ([`ResultList::fresh`]). A caller cannot paint yesterday's diagnostics
//!   because it cannot reach them without passing the current world.
//! - Navigation reuses `memori::Bound`, the stepper behind `n`/`N`, so result
//!   and search navigation wrap identically by construction.
//! - Producers publish COMPLETE lists, never deltas.
//!
//! ## Status
//!
//! **SHIPPED AND WIRED.** `EditorState.results` is a `ListRegistry`, `world()`
//! builds the axis set, and all three faces paint gutter marks through
//! `escriba_ui::gutter`. The model and the first producer (TODO scanning) are
//! here. Diagnostics, hunks and test results arrive with their
//! subsystems in Phase 6 — each of them a source, not a rewrite.

extern crate self as escriba_shirube;

/// Merge-conflict regions — a producer that needs no git at all. A conflict
/// is text a merge tool wrote into the buffer, and resolving one is an edit
/// over lines already open.
pub mod conflict;

pub mod anchor;
pub mod finding;
pub mod list;
pub mod text;

pub use anchor::{Anchor, Axis, IndexRev, NonEmptyAnchor, SessionGen, SessionKind};
pub use finding::{Finding, Origin, Severity, Site};
pub use list::{ListRegistry, ResultList};
pub use text::scan_markers;

pub use escriba_memori::Bound;
