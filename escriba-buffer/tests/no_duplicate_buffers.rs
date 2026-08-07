//! Opening one file twice must yield one buffer.
//!
//! ## The class this seals
//!
//! `BufferSet::open` used to mint a new id unconditionally. Open the same
//! file twice and you got two independent `Buffer`s over the same path, each
//! with its own rope, its own undo stack and its own `modified` flag. Edits
//! split between them; whichever saved last silently won and the other's work
//! was gone.
//!
//! It was latent only because nothing but the initial CLI argument ever
//! called `open`. Every plausible next feature — a file picker, `files.open`,
//! goto-definition, a session restore, a git conflict view — makes it
//! reachable on the first use, which is why it is sealed in Phase 0 rather
//! than discovered in Phase 6.
//!
//! ## Honest tier: only-mitigated
//!
//! Duplicate-by-`open` is unreachable. `BufferSet` does not own path
//! assignment, so a caller holding two `get_mut` handles can still `save_as`
//! both onto one path. Closing that means moving path mutation behind this
//! type, which belongs with the `madoguchi` dispatch seam
//! (`docs/backlog-plan.md` §V Phase 1). Stated here so nobody reads this file
//! as a stronger guarantee than it is.

use escriba_buffer::BufferSet;

/// A real file on disk — `open` reads it, so a fixture must exist.
fn fixture(name: &str, body: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("escriba-no-dup-buffers");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let p = dir.join(name);
    std::fs::write(&p, body).expect("write fixture");
    p
}

#[test]
fn opening_the_same_path_twice_returns_the_same_buffer() {
    let path = fixture("same.txt", "hello\n");
    let mut set = BufferSet::new();
    let a = set.open(&path).expect("first open");
    let b = set.open(&path).expect("second open");
    assert_eq!(a, b, "the same file must not become two buffers");
    assert_eq!(set.ids().len(), 1, "and must not grow the set");
}

#[test]
fn edits_cannot_be_split_across_two_views_of_one_file() {
    // The consequence that actually costs work: with two buffers, an edit
    // made through one is invisible to the other, and the loser's changes
    // vanish on save.
    let path = fixture("split.txt", "original\n");
    let mut set = BufferSet::new();
    let first = set.open(&path).expect("open");
    set.get_mut(first).expect("buffer").modified = true;

    let second = set.open(&path).expect("re-open");
    assert!(
        set.get(second).expect("buffer").modified,
        "re-opening must return the DIRTY buffer, not a clean second copy \
         that would silently discard the edit on save",
    );
}

#[test]
fn different_spellings_of_one_path_are_one_file() {
    // `./foo` and `foo` are the same file, and a picker will hand over
    // whichever the source produced. Comparison is canonical where the
    // filesystem allows it.
    let path = fixture("spelling.txt", "x\n");
    let dir = path.parent().expect("parent");
    let dotted = dir.join(".").join("spelling.txt");

    let mut set = BufferSet::new();
    let a = set.open(&path).expect("plain");
    let b = set.open(&dotted).expect("dotted");
    assert_eq!(a, b, "./x and x are one file: {path:?} vs {dotted:?}");
}

#[test]
fn distinct_files_still_get_distinct_buffers() {
    // The obvious way to "fix" duplication wrongly: collapse everything.
    let one = fixture("one.txt", "1\n");
    let two = fixture("two.txt", "2\n");
    let mut set = BufferSet::new();
    let a = set.open(&one).expect("one");
    let b = set.open(&two).expect("two");
    assert_ne!(a, b, "two files must be two buffers");
    assert_eq!(set.ids().len(), 2);
}

#[test]
fn scratch_buffers_are_never_deduplicated() {
    // Scratch buffers have no path; they must stay independent however many
    // are created, or `:enew` would keep handing back the same buffer.
    let mut set = BufferSet::new();
    let a = set.scratch("a");
    let b = set.scratch("b");
    assert_ne!(a, b);
    assert_eq!(set.ids().len(), 2);
}

#[test]
fn find_by_path_reports_honestly() {
    let path = fixture("find.txt", "y\n");
    let mut set = BufferSet::new();
    assert_eq!(set.find_by_path(&path), None, "nothing open yet");
    let id = set.open(&path).expect("open");
    assert_eq!(set.find_by_path(&path), Some(id));
    assert_eq!(
        set.find_by_path(path.parent().expect("parent").join("absent.txt")),
        None,
        "a path nobody opened must not match",
    );
}
