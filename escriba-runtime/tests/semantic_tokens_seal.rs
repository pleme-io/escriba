//! Server-authored colour is only true of the text it was computed from.
//!
//! Diagnostics have this property and shirube's `ResultList` already enforces
//! it: a stale list reads as EMPTY rather than as its old contents. Semantic
//! tokens have the SAME property and a worse failure mode when it is missed.
//!
//! A stale diagnostic paints a mark in the gutter beside the wrong line, which
//! looks wrong. A stale token paints the RIGHT-LOOKING colour on the wrong
//! bytes: the operator types one character and every word after it on the line
//! shifts, so the identifier `foo` keeps `bar`'s colour and the screen is
//! entirely plausible while being entirely wrong. Nothing errors, and there is
//! nothing on screen to suggest anything happened.
//!
//! So the state is sealed and read through one accessor. There are three ways
//! to be stale and every one of them must read empty:
//!
//!   - the text moved (the operator typed);
//!   - the ANSWER is about another buffer;
//!   - the server session moved (a restart — the LSP axis).

use escriba_buffer::BufferSet;
use escriba_madoguchi::{HlClass, Negai, Outcome, SemanticSpan};
use escriba_runtime::EditorState;
use escriba_shirube::{Axis, NonEmptyAnchor, SessionKind};

/// The anchor a diagnostics errand is sealed with — `Axis::Text(buffer, rev) ∧
/// Session(Lsp, gen)`, built from the LIVE world rather than from literals so
/// the test cannot silently drift from `EditorState::seal`.
fn diagnostics_anchor(st: &EditorState, buffer: escriba_core::BufferId) -> escriba_shirube::Anchor {
    let world = st.world();
    let text = *world
        .axes()
        .iter()
        .find(|a| matches!(a, Axis::Text(b, _) if *b == buffer))
        .expect("the world names every open buffer");
    let session = *world
        .axes()
        .iter()
        .find(|a| matches!(a, Axis::Session(SessionKind::Lsp, _)))
        .expect("the world always names the lsp session");
    NonEmptyAnchor::on(text).and(session).into_anchor()
}

fn two_buffers() -> (EditorState, escriba_core::BufferId, escriba_core::BufferId) {
    let mut bufs = BufferSet::new();
    let a = bufs.scratch("def add\nx = 1\n");
    let b = bufs.scratch("other\n");
    let mut st = EditorState::new_with_buffer(bufs, a);
    st.dismiss_splash();
    (st, a, b)
}

fn a_token() -> SemanticSpan {
    SemanticSpan {
        line: 0,
        start_char: 0,
        len_chars: 3,
        class: HlClass::Keyword,
    }
}

/// An off-tick reply keeps its OWN anchor, and that anchor is what the reader
/// checks.
///
/// Sealing at `world()` instead would pass the freshness gate and then widen
/// the claim: colours that depended on one buffer at one revision would be
/// recorded as depending on the whole world, so an edit in an unrelated buffer
/// would blank them — and, worse, an unearned claim would have been made
/// durable. This is the same correction the `PublishFindings` arm carries.
#[test]
fn a_reply_that_is_fresh_reaches_the_face() {
    let (mut st, a, _b) = two_buffers();
    let anchor = diagnostics_anchor(&st, a);

    st.interpret(Outcome::did(vec![Negai::ErrandReply {
        anchor,
        then: Box::new(Negai::PublishSemanticTokens {
            buffer: a,
            tokens: vec![a_token()],
        }),
    }]));

    assert_eq!(st.semantic_spans(a), &[a_token()][..]);
}

/// **The gate that matters.** One keystroke after the reply lands, the colour
/// is gone rather than wrong.
///
/// RED RUN 2026-08-12: making `SemanticPaint::fresh` return `&self.spans`
/// unconditionally leaves the token readable after the edit and fails here.
#[test]
fn an_edit_after_the_reply_makes_the_colour_absent_rather_than_wrong() {
    let (mut st, a, _b) = two_buffers();
    let anchor = diagnostics_anchor(&st, a);
    st.interpret(Outcome::did(vec![Negai::ErrandReply {
        anchor,
        then: Box::new(Negai::PublishSemanticTokens {
            buffer: a,
            tokens: vec![a_token()],
        }),
    }]));
    assert!(!st.semantic_spans(a).is_empty(), "precondition: it landed");

    // One character, at the front of the very line the token describes — the
    // real shape of the failure, where every column on that line moves by one.
    st.interpret(Outcome::did(vec![Negai::SetCursor {
        buffer: a,
        to: escriba_core::Position::new(0, 0),
    }]));
    st.interpret(Outcome::did(vec![Negai::InsertText("z".to_string())]));

    assert!(
        st.semantic_spans(a).is_empty(),
        "colour computed against the previous revision must not be painted",
    );
}

/// **The gate that the reply keeping its OWN anchor actually buys.**
///
/// Editing a DIFFERENT buffer must not blank this one's colour. Both other
/// mutations of the seal are caught by the tests above, but resealing an
/// `ErrandReply` at `world()` — which is exactly what falling through to the
/// ordinary `PublishSemanticTokens` arm does — is invisible to all of them:
/// `world()` is a superset of a reply that just passed the freshness gate, so
/// every check they make still answers the same way. The one thing it changes
/// is the WIDTH of the claim, and width is only observable by moving an axis
/// the reply never depended on.
///
/// RED RUN 2026-08-12, and it took two attempts. Deleting the
/// `Negai::PublishSemanticTokens` arm from the `ErrandReply` match (so the
/// reply reseals at `world()`) left the five tests around this one GREEN — a
/// mutation that could not violate any property they state. With this test it
/// fails on `an edit elsewhere must not blank this buffer's colour`, which is
/// what an operator with two files open would have seen: type in one, watch
/// the other lose its colouring.
#[test]
fn an_edit_in_another_buffer_leaves_this_buffers_colour_alone() {
    let (mut st, a, b) = two_buffers();
    let anchor = diagnostics_anchor(&st, a);
    st.interpret(Outcome::did(vec![Negai::ErrandReply {
        anchor,
        then: Box::new(Negai::PublishSemanticTokens {
            buffer: a,
            tokens: vec![a_token()],
        }),
    }]));
    assert!(!st.semantic_spans(a).is_empty(), "precondition: it landed");

    // Type into B. A has not moved, and A's colour depends on A alone.
    st.interpret(Outcome::did(vec![Negai::Edit {
        buffer: b,
        edit: escriba_core::Edit::insert(escriba_core::Position::new(0, 0), "z"),
    }]));

    assert_eq!(
        st.semantic_spans(a),
        &[a_token()][..],
        "an edit elsewhere must not blank this buffer's colour",
    );
}

/// A reply about ANOTHER buffer is not this buffer's colour.
///
/// The buffer check is separate from the anchor check on purpose: an anchor
/// that names buffer B is fresh whenever B is unedited, so asking only the
/// anchor would happily hand B's tokens to A's renderer — every column
/// plausible, every one from a different file.
///
/// RED RUN 2026-08-12: dropping `self.buffer == buffer` from
/// `SemanticPaint::fresh` returns B's token for A and fails here.
#[test]
fn a_reply_about_another_buffer_is_not_this_buffers_colour() {
    let (mut st, a, b) = two_buffers();
    let anchor = diagnostics_anchor(&st, b);
    st.interpret(Outcome::did(vec![Negai::ErrandReply {
        anchor,
        then: Box::new(Negai::PublishSemanticTokens {
            buffer: b,
            tokens: vec![a_token()],
        }),
    }]));

    assert_eq!(st.semantic_spans(b).len(), 1, "B's colour is B's");
    assert!(st.semantic_spans(a).is_empty(), "and never A's");
}

/// A restarted server supersedes what the previous one said.
///
/// The LSP session axis is half the diagnostics anchor and it is carried here
/// for the same reason: a server that restarted may have a different legend
/// entirely, so its predecessor's indices no longer mean what they meant.
#[test]
fn a_server_restart_supersedes_the_colour_it_published() {
    let (mut st, a, _b) = two_buffers();
    let anchor = diagnostics_anchor(&st, a);
    st.interpret(Outcome::did(vec![Negai::ErrandReply {
        anchor,
        then: Box::new(Negai::PublishSemanticTokens {
            buffer: a,
            tokens: vec![a_token()],
        }),
    }]));
    assert!(!st.semantic_spans(a).is_empty());

    st.bump_lsp_gen();
    assert!(st.semantic_spans(a).is_empty());
}

/// A reply whose world has already moved is DROPPED at the gate, not stored
/// and filtered later.
///
/// The distinction is observable: storing it would replace whatever colour is
/// currently on screen with something that reads empty, so a correct paint
/// would blank the instant a superseded reply arrived.
#[test]
fn a_stale_reply_never_displaces_the_colour_already_up() {
    let (mut st, a, _b) = two_buffers();
    let good = diagnostics_anchor(&st, a);
    st.interpret(Outcome::did(vec![Negai::ErrandReply {
        anchor: good,
        then: Box::new(Negai::PublishSemanticTokens {
            buffer: a,
            tokens: vec![a_token()],
        }),
    }]));

    // A late reply from a server generation that has since been superseded.
    let stale = NonEmptyAnchor::on(Axis::Session(
        SessionKind::Lsp,
        escriba_shirube::SessionGen(999),
    ))
    .into_anchor();
    st.interpret(Outcome::did(vec![Negai::ErrandReply {
        anchor: stale,
        then: Box::new(Negai::PublishSemanticTokens {
            buffer: a,
            tokens: vec![],
        }),
    }]));

    assert_eq!(
        st.semantic_spans(a),
        &[a_token()][..],
        "a superseded reply must not blank a live paint",
    );
}

/// With no server at all, the face is told nothing — which is what makes it
/// fall back to hikari's lexer rather than paint an empty partition.
#[test]
fn an_editor_that_never_heard_from_a_server_reports_no_colour() {
    let (st, a, _b) = two_buffers();
    assert!(st.semantic_spans(a).is_empty());
}
