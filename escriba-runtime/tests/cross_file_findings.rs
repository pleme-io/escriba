//! A located finding's location is (buffer, position). Code kept dropping the
//! buffer half.
//!
//! Every one of these was latent: the first shirube producer scans a single
//! buffer, so nothing crossed files. Every producer AFTER it — diagnostics,
//! git hunks, grep hits, test failures — is cross-file by nature, which is
//! why these are pinned before that producer exists rather than after it
//! surfaces as "the gutter is lying".

use escriba_buffer::BufferSet;
use escriba_core::{Position, Range};
use escriba_madoguchi::{Negai, Outcome};
use escriba_runtime::EditorState;
use escriba_shirube::{Finding, Origin, Severity, Site};

fn two_buffers() -> (EditorState, escriba_core::BufferId, escriba_core::BufferId) {
    let mut bufs = BufferSet::new();
    let a = bufs.scratch("a0\na1\na2\na3\na4\n");
    let b = bufs.scratch("b0\nb1\nb2\nb3\nb4\n");
    let mut st = EditorState::new_with_buffer(bufs, a);
    st.dismiss_splash();
    (st, a, b)
}

fn at(buffer: escriba_core::BufferId, line: u32, msg: &str) -> Finding {
    Finding::new(
        Site::in_buffer(
            buffer,
            Range::new(Position::new(line, 0), Position::new(line, 1)),
        ),
        Severity::Info,
        msg.to_string(),
        Origin::Text("test"),
    )
}

#[test]
fn walking_to_a_finding_in_another_buffer_switches_to_that_buffer() {
    // The defect: `walk_list` clamped against `self.active`, so a finding in
    // buffer B landed on that LINE NUMBER in buffer A. Right line, wrong file.
    let (mut st, a, b) = two_buffers();
    st.interpret(Outcome::did(vec![Negai::PublishFindings {
        list: "x".to_string(),
        findings: vec![at(b, 3, "in the other file")],
    }]));
    assert_eq!(st.active, a, "precondition: we start in A");

    st.interpret(Outcome::did(vec![Negai::WalkList {
        list: "x".to_string(),
        forward: true,
    }]));

    assert_eq!(st.active, b, "the walk must switch to the finding's buffer");
    assert_eq!(st.cursor().line, 3, "and land on its line");
}

#[test]
fn a_cross_file_walk_is_still_a_jump() {
    // `<C-o>` must return from it, exactly as from an `n`.
    let (mut st, a, b) = two_buffers();
    st.interpret(Outcome::did(vec![Negai::PublishFindings {
        list: "x".to_string(),
        findings: vec![at(b, 2, "elsewhere")],
    }]));
    st.interpret(Outcome::did(vec![Negai::WalkList {
        list: "x".to_string(),
        forward: true,
    }]));
    assert_eq!(st.active, b);
    st.on_key(&escriba_keymap::Key::Ctrl('o'));
    assert_eq!(st.active, a, "<C-o> must come back to the buffer we left");
}

#[test]
fn same_line_in_two_files_are_two_distinct_stops() {
    // Ordering by bare line number made these indistinguishable: the walk
    // landed on whichever the sort put first and could never reach the other.
    let (mut st, a, b) = two_buffers();
    st.interpret(Outcome::did(vec![Negai::PublishFindings {
        list: "x".to_string(),
        findings: vec![at(a, 2, "in A"), at(b, 2, "in B")],
    }]));
    let mut seen = Vec::new();
    for _ in 0..2 {
        st.interpret(Outcome::did(vec![Negai::WalkList {
            list: "x".to_string(),
            forward: true,
        }]));
        seen.push((st.active, st.cursor().line));
    }
    seen.sort_by_key(|(id, _)| id.0);
    seen.dedup();
    assert_eq!(
        seen.len(),
        2,
        "both line-2 findings must be reachable, got {seen:?}",
    );
}

#[test]
fn a_list_anchored_on_the_index_axis_is_not_born_stale() {
    // `world()` emitted only `Axis::Text`, and `is_fresh` treats an ABSENT
    // axis as stale — so a hunk list would answer "out of date" forever.
    let (st, a, _b) = two_buffers();
    let anchor = escriba_shirube::Anchor::new()
        .on(escriba_shirube::Axis::Text(a, {
            st.buffers.get(a).expect("buffer").text_rev()
        }))
        .on(escriba_shirube::Axis::Index(
            escriba_shirube::IndexRev::default(),
        ));
    assert!(
        anchor.is_fresh(&st.world()),
        "an index-anchored list must be FRESH against a world that tracks the \
         index axis",
    );
}

#[test]
fn bumping_the_index_generation_stales_an_index_anchored_list() {
    // And the axis must actually MOVE, or it is decoration.
    let (mut st, a, _b) = two_buffers();
    let anchor = escriba_shirube::Anchor::new()
        .on(escriba_shirube::Axis::Text(a, {
            st.buffers.get(a).expect("buffer").text_rev()
        }))
        .on(escriba_shirube::Axis::Index(
            escriba_shirube::IndexRev::default(),
        ));
    assert!(anchor.is_fresh(&st.world()));
    st.bump_index_rev();
    assert!(
        !anchor.is_fresh(&st.world()),
        "a `git add` must invalidate an index-anchored list",
    );
}
