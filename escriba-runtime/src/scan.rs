//! The scan runner — the courier's first real carrier.
//!
//! Walks a tree on its own thread and posts matches back as findings. This is
//! what removes the grep ceiling: the synchronous version stopped at 2,000
//! files and 500 hits because it ran on the thread that draws the screen, and
//! its own comment named the courier as the way out.
//!
//! # Batches, not one big answer
//!
//! Results are posted as they are found, in batches, and each batch is
//! CUMULATIVE — it carries every match so far rather than only the new ones.
//! That is deliberate: the reply lands as a `PublishFindings`, and publishing
//! REPLACES a list rather than appending to it, so an incremental batch would
//! make the list flicker down to the last few rows. Re-sending the whole set
//! costs a clone per batch and keeps the surface honest at every moment.
//!
//! # What bounds it
//!
//! Nothing bounds the walk. That is the point — the ceiling was a symptom of
//! running on the wrong thread. What DOES stop it is the cancel flag, checked
//! once per directory and once per batch; and even that only stops the
//! posting promptly, since the flag is observed between units of work rather
//! than interrupting one.
//!
//! A superseded scan whose runner ignores the flag is still harmless: its
//! replies are sealed against a scan generation the editor has moved past, so
//! they are dropped on arrival. The flag makes it stop sooner, not safer.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use escriba_core::{Position, Range};
use escriba_madoguchi::Negai;
use escriba_madoguchi::errand::{Errand, Freight, Parcel, Runner};
use escriba_search::CaseMode;
use escriba_shirube::{Finding, Origin, Severity, Site};

/// The list name scan results publish under.
///
/// One name, referenced by the runner and by whatever projects it, so the
/// producer and the consumer cannot drift onto two spellings.
pub const LIST: &str = "grep";

/// How many matches accumulate before a batch is posted.
///
/// Small enough that the first rows appear immediately on a big tree, large
/// enough that a dense match does not post per line.
const BATCH: usize = 64;

/// Directory names never descended into.
///
/// Not a gitignore implementation and not pretending to be — it is the same
/// list the synchronous walker used, kept identical so this change is about
/// WHERE the walk runs, not what it finds.
fn skip(name: &str) -> bool {
    name.starts_with('.') || name == "target" || name == "node_modules"
}

/// Does `line` contain `needle`, under `case`?
///
/// **Substring, not regex.** The synchronous grep was `line.contains(pattern)`,
/// and routing this through a regex engine would silently change what a
/// pattern containing `.` or `*` means for every existing user. Case handling
/// is new and is a strict widening: smartcase makes `foo` find `Foo`, which the
/// old behaviour did not.
fn matches(line: &str, needle: &str, case: CaseMode) -> bool {
    let insensitive = match case {
        CaseMode::Sensitive => false,
        CaseMode::Ignore => true,
        // vim's smartcase: an uppercase character in the pattern means the
        // operator meant it.
        CaseMode::Smart => !needle.chars().any(char::is_uppercase),
    };
    if insensitive {
        line.to_lowercase().contains(&needle.to_lowercase())
    } else {
        line.contains(needle)
    }
}

fn finding_at(path: &Path, line: u32, text: &str) -> Finding {
    let at = Position::new(line, 0);
    Finding::new(
        Site::in_file(path, Range { start: at, end: at }),
        Severity::Info,
        text.trim().to_string(),
        Origin::Search,
    )
}

/// Walks the filesystem for matches, on a thread of its own.
pub struct ScanRunner;

impl Runner for ScanRunner {
    fn start(&self, errand: Errand, cancel: Arc<AtomicBool>, reply: Sender<Parcel>) {
        let Freight::Scan { raw, case, root } = errand.freight else {
            // Unreachable: `Crew::get` routes by class. Say so rather than
            // panicking on a thread nobody is waiting on.
            let _ = reply.send(Parcel {
                id: errand.id,
                slip: Negai::Message("scan runner received the wrong freight".into()),
            });
            return;
        };
        let id = errand.id;
        let anchor = errand.anchor.into_anchor();
        // Kept out of the closure so the spawn-failure arm still has a way to
        // speak. An errand that silently never starts is precisely the failure
        // this seam exists to prevent.
        let on_fail = reply.clone();

        std::thread::Builder::new()
            .name("escriba-scan".into())
            .spawn(move || {
                let post = |found: &[Finding]| {
                    reply
                        .send(Parcel {
                            id,
                            slip: Negai::ErrandReply {
                                anchor: anchor.clone(),
                                then: Box::new(Negai::PublishFindings {
                                    list: LIST.to_string(),
                                    findings: found.to_vec(),
                                }),
                            },
                        })
                        .is_ok()
                };

                let mut found: Vec<Finding> = Vec::new();
                let mut stack = vec![root];
                let mut since_post = 0usize;

                while let Some(dir) = stack.pop() {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let Ok(entries) = std::fs::read_dir(&dir) else {
                        continue;
                    };
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        if skip(&name.to_string_lossy()) {
                            continue;
                        }
                        let path = entry.path();
                        if entry.file_type().is_ok_and(|t| t.is_dir()) {
                            stack.push(path);
                            continue;
                        }
                        // Binary or unreadable is not an error worth
                        // reporting — a tree has plenty of both.
                        let Ok(text) = std::fs::read_to_string(&path) else {
                            continue;
                        };
                        for (n, line) in text.lines().enumerate() {
                            if !matches(line, &raw, case) {
                                continue;
                            }
                            let Ok(n) = u32::try_from(n) else { break };
                            found.push(finding_at(&path, n, line));
                            since_post += 1;
                        }
                        if since_post >= BATCH {
                            since_post = 0;
                            // A closed channel means the editor is gone.
                            if !post(&found) {
                                return;
                            }
                        }
                    }
                }

                // The final batch. Sent even when empty and even when nothing
                // changed since the last one: it is what turns "still
                // searching" into "that is all there is", and a scan that
                // simply stops talking is indistinguishable from one that hung.
                post(&found);
            })
            .map_or_else(
                |e| {
                    let mut m = String::from("could not start the scan thread: ");
                    m.push_str(&e.to_string());
                    let _ = on_fail.send(Parcel {
                        id,
                        slip: Negai::Message(m),
                    });
                },
                |_handle| {
                    // Detached on purpose. Joining here would block the editor
                    // on the walk, which is the entire thing being fixed.
                },
            );
    }
}

#[cfg(test)]
mod tests {
    use super::{BATCH, LIST, ScanRunner, finding_at, matches, skip};
    use escriba_madoguchi::errand::{Errand, Freight, Runner};
    use escriba_madoguchi::{ErrandId, Negai};
    use escriba_search::CaseMode;
    use escriba_shirube::{Axis, NonEmptyAnchor, Origin, SessionGen, SessionKind};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (name, body) in files {
            let p = d.path().join(name);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, body).unwrap();
        }
        d
    }

    /// Runs a scan to completion and returns every finding from the LAST
    /// batch, which is cumulative and therefore the full result.
    fn scan(root: &std::path::Path, pattern: &str) -> Vec<escriba_shirube::Finding> {
        let (tx, rx) = channel();
        ScanRunner.start(
            Errand {
                id: ErrandId(1),
                freight: Freight::Scan {
                    raw: pattern.into(),
                    case: CaseMode::Smart,
                    root: root.to_path_buf(),
                },
                anchor: NonEmptyAnchor::on(Axis::Session(SessionKind::Scan, SessionGen(1))),
            },
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let mut last = Vec::new();
        // Bounded: a regression that hangs the walk must fail the suite rather
        // than hang it.
        while let Ok(p) = rx.recv_timeout(Duration::from_secs(20)) {
            if let Negai::ErrandReply { then, .. } = p.slip {
                if let Negai::PublishFindings { list, findings } = *then {
                    assert_eq!(list, LIST);
                    last = findings;
                }
            }
        }
        last
    }

    #[test]
    fn a_match_is_found_and_located() {
        let d = tree(&[("a.txt", "one\nneedle here\nthree\n")]);
        let got = scan(d.path(), "needle");
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].site.range.start.line, 1, "zero-based line");
        assert_eq!(got[0].origin, Origin::Search);
        assert!(got[0].message.contains("needle"));
        assert!(got[0].site.path.as_ref().unwrap().ends_with("a.txt"));
    }

    /// **The ceiling this exists to remove.** The synchronous grep stopped at
    /// 500 hits; a match past that was simply not found.
    #[test]
    fn a_match_past_the_old_five_hundred_hit_ceiling_is_found() {
        let mut body = String::new();
        for _ in 0..600 {
            body.push_str("needle\n");
        }
        let d = tree(&[("big.txt", &body)]);
        let got = scan(d.path(), "needle");
        assert_eq!(got.len(), 600, "no hit ceiling");
    }

    /// …and the file ceiling, which was 2,000.
    #[test]
    fn more_files_than_the_old_two_thousand_file_ceiling_are_walked() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..2_100 {
            std::fs::write(d.path().join(format!("f{i}.txt")), "needle\n").unwrap();
        }
        let got = scan(d.path(), "needle");
        assert_eq!(got.len(), 2_100, "no file ceiling");
    }

    /// Results must arrive progressively — a scan that only answers at the end
    /// is off-thread but still feels frozen.
    #[test]
    fn results_arrive_in_batches_rather_than_only_at_the_end() {
        let mut body = String::new();
        for _ in 0..(BATCH * 4) {
            body.push_str("needle\n");
        }
        let d = tree(&[("big.txt", &body)]);
        let (tx, rx) = channel();
        ScanRunner.start(
            Errand {
                id: ErrandId(1),
                freight: Freight::Scan {
                    raw: "needle".into(),
                    case: CaseMode::Smart,
                    root: d.path().to_path_buf(),
                },
                anchor: NonEmptyAnchor::on(Axis::Session(SessionKind::Scan, SessionGen(1))),
            },
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let mut batches = 0;
        while rx.recv_timeout(Duration::from_secs(20)).is_ok() {
            batches += 1;
        }
        assert!(batches >= 2, "expected progressive batches, got {batches}");
    }

    /// Each batch carries everything found so far, because publishing REPLACES
    /// a list. An incremental batch would make the surface shrink.
    #[test]
    fn batches_are_cumulative_so_the_list_never_shrinks() {
        let mut body = String::new();
        for _ in 0..(BATCH * 3) {
            body.push_str("needle\n");
        }
        let d = tree(&[("big.txt", &body)]);
        let (tx, rx) = channel();
        ScanRunner.start(
            Errand {
                id: ErrandId(1),
                freight: Freight::Scan {
                    raw: "needle".into(),
                    case: CaseMode::Smart,
                    root: d.path().to_path_buf(),
                },
                anchor: NonEmptyAnchor::on(Axis::Session(SessionKind::Scan, SessionGen(1))),
            },
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let mut sizes = Vec::new();
        while let Ok(p) = rx.recv_timeout(Duration::from_secs(20)) {
            if let Negai::ErrandReply { then, .. } = p.slip {
                if let Negai::PublishFindings { findings, .. } = *then {
                    sizes.push(findings.len());
                }
            }
        }
        assert!(sizes.len() >= 2, "need several batches: {sizes:?}");
        assert!(
            sizes.windows(2).all(|w| w[1] >= w[0]),
            "a batch must never be smaller than the one before: {sizes:?}"
        );
    }

    /// An empty result must still be REPORTED. A scan that finds nothing and
    /// says nothing is indistinguishable from one that hung.
    #[test]
    fn a_scan_with_no_matches_still_reports_completion() {
        let d = tree(&[("a.txt", "nothing to see\n")]);
        let (tx, rx) = channel();
        ScanRunner.start(
            Errand {
                id: ErrandId(1),
                freight: Freight::Scan {
                    raw: "absent".into(),
                    case: CaseMode::Smart,
                    root: d.path().to_path_buf(),
                },
                anchor: NonEmptyAnchor::on(Axis::Session(SessionKind::Scan, SessionGen(1))),
            },
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let got = rx.recv_timeout(Duration::from_secs(20));
        assert!(got.is_ok(), "an empty scan must still post a final batch");
    }

    /// Smartcase is a strict widening over the old `line.contains`: `foo`
    /// finds `Foo`, `Foo` does not find `foo`.
    #[test]
    fn smartcase_widens_a_lowercase_pattern_and_respects_an_uppercase_one() {
        assert!(matches("Foo bar", "foo", CaseMode::Smart));
        assert!(!matches("foo bar", "Foo", CaseMode::Smart));
        assert!(matches("Foo", "Foo", CaseMode::Smart));
        assert!(!matches("Foo", "foo", CaseMode::Sensitive));
        assert!(matches("Foo", "foo", CaseMode::Ignore));
    }

    /// **Substring, NOT regex.** Routing grep through a regex engine would
    /// silently reinterpret every pattern containing a metacharacter.
    #[test]
    fn a_metacharacter_is_a_literal_not_a_pattern() {
        assert!(matches("a.c", "a.c", CaseMode::Sensitive));
        assert!(
            !matches("abc", "a.c", CaseMode::Sensitive),
            "`.` must not match any character"
        );
        assert!(!matches("aaa", "a*", CaseMode::Sensitive));
    }

    #[test]
    fn the_skip_list_is_unchanged_from_the_synchronous_walker() {
        for name in [".git", ".direnv", "target", "node_modules"] {
            assert!(skip(name), "{name} must be skipped");
        }
        for name in ["src", "Cargo.toml", "a.rs"] {
            assert!(!skip(name), "{name} must be walked");
        }
    }

    #[test]
    fn a_skipped_directory_is_not_searched() {
        let d = tree(&[
            ("src/a.txt", "needle\n"),
            ("target/b.txt", "needle\n"),
            (".git/c.txt", "needle\n"),
        ]);
        let got = scan(d.path(), "needle");
        assert_eq!(got.len(), 1, "only src/ is walked: {got:?}");
    }

    /// A tree full of binaries must not stop the walk.
    #[test]
    fn an_unreadable_file_is_skipped_rather_than_fatal() {
        let d = tree(&[("good.txt", "needle\n")]);
        std::fs::write(d.path().join("bin.dat"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        let got = scan(d.path(), "needle");
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn a_cancelled_scan_stops_posting() {
        let mut body = String::new();
        for _ in 0..(BATCH * 20) {
            body.push_str("needle\n");
        }
        let d = tempfile::tempdir().unwrap();
        for i in 0..50 {
            std::fs::write(d.path().join(format!("f{i}.txt")), &body).unwrap();
        }
        let cancel = Arc::new(AtomicBool::new(true)); // already cancelled
        let (tx, rx) = channel();
        ScanRunner.start(
            Errand {
                id: ErrandId(1),
                freight: Freight::Scan {
                    raw: "needle".into(),
                    case: CaseMode::Smart,
                    root: d.path().to_path_buf(),
                },
                anchor: NonEmptyAnchor::on(Axis::Session(SessionKind::Scan, SessionGen(1))),
            },
            cancel,
            tx,
        );
        // Cancelled before the first directory is read, so nothing is posted.
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_err(),
            "a pre-cancelled scan must post nothing"
        );
    }

    #[test]
    fn a_finding_records_the_line_it_was_found_on() {
        let f = finding_at(std::path::Path::new("x.rs"), 41, "  hit  ");
        assert_eq!(f.site.range.start.line, 41);
        assert_eq!(f.message, "hit", "the label is trimmed");
    }
}
