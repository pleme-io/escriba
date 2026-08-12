//! The diagnostics runner — the courier's language-server carrier.
//!
//! Spawns a server, opens a document, waits for the first `publishDiagnostics`
//! about it, converts it to findings and posts them back. Everything about the
//! conversation stays inside this crate.
//!
//! # Where tokio is, and why it is only here
//!
//! `escriba-runtime` has no async runtime and must not gain one — it is the
//! interpreter, and an editor whose core needs a reactor to apply a keystroke
//! is a different program. This crate already depends on tokio because an LSP
//! server is a child process spoken to over its stdio, and that is a genuinely
//! async problem.
//!
//! So the runtime lives **inside the runner's own thread**: a
//! `current_thread` reactor, built per errand, driving one conversation and
//! ending with it. The `Runner` signature it satisfies is
//! `std::sync::mpsc`-shaped, which is the containment seam — nothing async
//! crosses it, and the interpreter never learns that a reactor was involved.
//!
//! # What this deliberately is NOT, yet
//!
//! **One server per errand.** Each diagnostics request spawns a language
//! server, initializes it, asks about one file and lets it die. That is
//! genuinely wasteful and it is the honest M0: a persistent session is a
//! different lifetime — a server outlives any one errand, needs `didChange` as
//! the buffer moves, and needs restart handling — and building it inside a
//! one-shot runner would be the wrong shape wearing the right signature.
//!
//! The cost is bounded and visible: a spawn per open, not per keystroke,
//! because the only dispatcher today fires on opening a file. What makes the
//! upgrade safe rather than urgent is that the seam does not change — a
//! session-holding runner satisfies the same trait and the interpreter cannot
//! tell the difference.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use escriba_madoguchi::Negai;
use escriba_madoguchi::errand::{Errand, Freight, Parcel, Runner};

use crate::conn::Connection;
use crate::findings::{PublishDiagnostics, to_findings};
use crate::wire::Incoming;
use crate::{ServerConfig, ServerRegistry, detect_root};

/// The list name diagnostics publish under, per language.
///
/// Per-language rather than one shared list so a Rust server's results cannot
/// silently replace a Nix server's — `publish` REPLACES a list, and two
/// producers sharing a name would take turns erasing each other.
#[must_use]
pub fn list_for(language: &str) -> String {
    let mut s = String::with_capacity(language.len() + 12);
    s.push_str("diagnostics:");
    s.push_str(language);
    s
}

/// How long to wait for a server to say something about the file.
///
/// A server that never answers must not hold a thread forever. This bounds
/// the WAIT, not the server — the child is killed by dropping the
/// `Connection`, which owns it.
const REPLY_TIMEOUT: Duration = Duration::from_secs(20);

/// Which language a path belongs to, by extension.
///
/// Deliberately tiny and local. escriba has a real filetype table in the
/// runtime, but the runner cannot see it — it receives owned data, which is
/// the property that keeps a runner from reaching into the editor. When the
/// dispatcher needs to disagree with this, it should send the language rather
/// than teach this function more extensions.
#[must_use]
pub fn language_of(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()? {
        "nix" => Some("nix"),
        "rs" => Some("rust"),
        _ => None,
    }
}

/// Runs one diagnostics conversation per errand.
///
/// See [`root_for`] for the empty-parent trap that made every language's
/// diagnostics silently dead for a relative path outside a project.
pub struct LspRunner {
    registry: ServerRegistry,
}

impl LspRunner {
    #[must_use]
    pub fn new(registry: ServerRegistry) -> Self {
        Self { registry }
    }
}

impl Default for LspRunner {
    fn default() -> Self {
        Self::new(ServerRegistry::default_set())
    }
}

/// Build a `file://` URI for a path.
fn uri_of(path: &Path) -> String {
    let mut s = String::from("file://");
    s.push_str(&path.to_string_lossy());
    s
}

impl Runner for LspRunner {
    fn start(&self, errand: Errand, cancel: Arc<AtomicBool>, reply: Sender<Parcel>) {
        let Freight::Diagnostics {
            path,
            text,
            language,
            ..
        } = errand.freight.clone()
        else {
            let _ = reply.send(Parcel {
                id: errand.id,
                slip: Negai::Message("lsp runner received the wrong freight".into()),
            });
            return;
        };
        let id = errand.id;
        let anchor = errand.anchor.into_anchor();

        // The EDITOR's answer first — it owns the filetype table the catalog
        // populates. `language_of` is the fallback for a dispatcher that had no
        // opinion, which is what keeps a bare `--no-defaults` boot (empty
        // filetype table) diagnosing Rust and Nix as it always did.
        let Some(language) = language.or_else(|| language_of(&path).map(str::to_owned)) else {
            // Not an error. Most files have no server, and saying so on every
            // open would be noise — the errand simply produces nothing.
            return;
        };
        let Some(cfg) = self.registry.for_language(&language).cloned() else {
            return;
        };

        let on_fail = reply.clone();
        let spawned = std::thread::Builder::new()
            .name("escriba-lsp".into())
            .spawn(move || {
                // One reactor, one conversation, one thread. `current_thread`
                // because there is exactly one child to talk to and a
                // work-stealing pool would be machinery with no work.
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = reply.send(Parcel {
                            id,
                            slip: Negai::Message(describe("could not start the lsp reactor", &e)),
                        });
                        return;
                    }
                };

                let outcome = rt.block_on(converse(&cfg, &path, &text, &cancel));

                let slip = match outcome {
                    Ok(published) => {
                        if cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        let findings = to_findings(&published, &text, None);
                        Negai::ErrandReply {
                            anchor,
                            then: Box::new(Negai::PublishFindings {
                                list: list_for(&language),
                                findings,
                            }),
                        }
                    }
                    // A server that failed is worth ONE line, not silence: an
                    // editor that quietly shows no diagnostics because the
                    // server never started is indistinguishable from a clean
                    // file, which is the worse of the two lies.
                    Err(m) => Negai::Message(m),
                };
                let _ = reply.send(Parcel { id, slip });
            });

        if let Err(e) = spawned {
            let _ = on_fail.send(Parcel {
                id,
                slip: Negai::Message(describe("could not start the lsp thread", &e)),
            });
        }
    }
}

/// `what (which)` — the one place a server name is joined to a message, so
/// the phrasing has a single spelling.
fn named(what: &str, which: &str) -> String {
    let mut m = String::with_capacity(what.len() + which.len() + 3);
    m.push_str(what);
    m.push_str(" (");
    m.push_str(which);
    m.push(41 as char);
    m
}

fn describe(what: &str, e: &impl std::fmt::Display) -> String {
    let mut m = String::from(what);
    m.push_str(": ");
    m.push_str(&e.to_string());
    m
}

/// The directory to start the server in: a project root if one can be found,
/// else the file's own directory, else the working directory.
///
/// # The empty-parent trap
///
/// **Every step of this chain must yield a directory that EXISTS**, because the
/// result becomes `Command::current_dir`, and spawning into a directory that is
/// not there fails with `ENOENT` — reported as `io: No such file or directory`,
/// which reads as "the SERVER BINARY is missing" and sends you looking in
/// entirely the wrong place.
///
/// The trap is `Path::parent`. For a bare relative filename it returns
/// `Some("")`, not `None` — so the old chain's `.unwrap_or(".")` could never
/// fire, and `""` went straight through to `current_dir`. Measured 2026-08-12:
/// `escriba t.b` in a directory with no `.git` and no `Bluefile` reported
/// `lsp: could not start (blue): io: No such file or directory` while `blue` was
/// on `PATH` and working. `escriba $PWD/t.b` in the same directory formatted
/// fine. It was never about the binary, and it was never specific to blue —
/// every language's diagnostics were dead under the same two conditions.
fn root_for(path: &Path, markers: &[String]) -> std::path::PathBuf {
    detect_root(path, markers)
        .or_else(|| path.parent().map(Path::to_path_buf))
        // `Some("")` is the trap; treat it as "no answer" so the fallback runs.
        // RED RUN 2026-08-12: removing this line fails both
        // `the_root_is_never_the_empty_path` and
        // `a_bare_filename_roots_at_the_working_directory`.
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// Spawn, handshake and open one document — the opening every conversation
/// shares.
///
/// Extracted when formatting became the second consumer. Root detection, spawn,
/// handshake and `didOpen` are identical for both errands; only the question
/// asked afterwards differs, and duplicating four steps to ask a different
/// fifth is how the two would have drifted (the `initialized` notification in
/// particular is easy to omit in a copy, and a server that never sees it
/// accepts documents and then does no work).
///
/// Returns the live connection, the capabilities it advertised, and the
/// document's URI. **The caller owns the `Connection` from here on, and
/// dropping it is what reaps the child** — no path may return without leaving
/// the caller's scope.
pub(crate) async fn open_document(
    cfg: &ServerConfig,
    path: &Path,
    text: &str,
) -> Result<(Connection, crate::conn::ServerCaps, String), String> {
    let root = root_for(path, &cfg.root_markers);

    let conn = Connection::spawn(cfg, &root)
        .map_err(|e| describe(&named("lsp: could not start", &cfg.command), &e))?;

    let caps = conn
        .handshake(&root)
        .await
        .map_err(|e| describe(&named("lsp: could not initialize", &cfg.command), &e))?;

    let uri = uri_of(path);
    conn.notify(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": cfg.language,
                "version": 1,
                "text": text,
            }
        }),
    )
    .await
    .map_err(|e| describe("lsp: didOpen failed", &e))?;

    Ok((conn, caps, uri))
}

/// Spawn, handshake, open, and wait for the first word about this document.
///
/// Returns the published diagnostics, or a message describing why there are
/// none. The `Connection` is dropped on every path out, which kills the child
/// — that is the only thing reaping the server, and it is why no path here may
/// return without leaving this scope.
async fn converse(
    cfg: &ServerConfig,
    path: &Path,
    text: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<PublishDiagnostics, String> {
    let (mut conn, caps, uri) = open_document(cfg, path, text).await?;

    // READ, not assumed. A server advertising no `textDocumentSync` does not
    // track documents, so it will never publish anything about one — waiting
    // the full timeout for a diagnostic it was never going to send would look
    // like a hang, and would be reported as one.
    //
    // Checked AFTER the open rather than before: the open is what obtains the
    // capabilities, and refusing to open in order to read them is circular.
    // An extra `didOpen` to a server that ignores documents costs nothing.
    if caps.text_document_sync.unwrap_or(0) == 0 {
        return Err(named(
            "lsp: tracks no documents, so it publishes no diagnostics",
            &cfg.command,
        ));
    }

    // Wait for the first publish ABOUT THIS DOCUMENT. A server may say other
    // things first — progress, logs, diagnostics for a file it decided to read
    // on its own — and taking the first notification of any kind would report
    // one file's problems against another's.
    let deadline = tokio::time::sleep(REPLY_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(String::from("lsp: cancelled"));
        }
        tokio::select! {
            () = &mut deadline => {
                return Err(describe("lsp: no diagnostics within", &format_args!("{REPLY_TIMEOUT:?}")));
            }
            msg = conn.inbox.recv() => {
                let Some(Incoming::Notification { method, params }) = msg else {
                    // `None` is the pump exiting — the server died or closed
                    // its stdout, and no diagnostic is ever coming.
                    if msg.is_none() {
                        return Err(String::from("lsp: the server went away before answering"));
                    }
                    continue;
                };
                if method != "textDocument/publishDiagnostics" {
                    continue;
                }
                let Ok(published) = serde_json::from_value::<PublishDiagnostics>(params) else {
                    continue;
                };
                if published.uri == uri {
                    return Ok(published);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LspRunner, REPLY_TIMEOUT, language_of, list_for, root_for, uri_of};
    use crate::{ServerConfig, ServerRegistry};

    /// The root is NEVER the empty path — `current_dir("")` is `ENOENT`.
    ///
    /// The regression gate for the empty-parent trap, and it is stated as
    /// non-emptiness rather than existence deliberately. An earlier draft
    /// asserted `root.exists()` for every shape and failed on `sub/t.b`, whose
    /// parent `"sub"` does not exist relative to THIS test's working directory —
    /// while being exactly right in the editor, where a file at `sub/t.b` means
    /// a `sub` that is really there. `root_for` cannot promise existence for an
    /// arbitrary path it was handed; it can promise never to hand back the one
    /// value that is broken by construction, which is what this checks.
    ///
    /// RED RUN 2026-08-12: dropping `.filter(|p| !p.as_os_str().is_empty())`
    /// from `root_for` fails this on the two bare filenames. Measured in the
    /// live editor before the fix as
    /// `lsp: could not start (blue): io: No such file or directory` — a message
    /// that blamed the server BINARY for a bad working directory, which is why
    /// it took a live probe rather than a test to find.
    #[test]
    fn the_root_is_never_the_empty_path() {
        let markers = vec!["Cargo.toml".to_string(), ".git".to_string()];
        for path in [
            // The trap: a bare relative filename. `Path::parent` answers
            // `Some("")` here, not `None`.
            std::path::Path::new("t.b"),
            std::path::Path::new("main.rs"),
            std::path::Path::new("sub/t.b"),
            std::path::Path::new("/tmp/t.b"),
            std::path::Path::new(file!()),
        ] {
            let root = root_for(path, &markers);
            assert!(
                !root.as_os_str().is_empty(),
                "root for {path:?} is the empty path — `current_dir` would ENOENT"
            );
        }
    }

    /// A bare filename roots at the working directory, not at nothing.
    #[test]
    fn a_bare_filename_roots_at_the_working_directory() {
        let root = root_for(std::path::Path::new("t.b"), &[]);
        assert_eq!(root, std::path::PathBuf::from("."));
    }

    /// A file that really exists roots somewhere that really exists.
    ///
    /// The existence half, on paths where existence is actually `root_for`'s to
    /// deliver: a file in a temp directory (no marker ⇒ the parent), and the
    /// same file under a directory carrying a marker (⇒ the marker's directory).
    #[test]
    fn a_real_file_roots_somewhere_that_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let deep = tmp.path().join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        let file = deep.join("t.b");
        std::fs::write(&file, "x = 1\n").unwrap();

        let no_marker = root_for(&file, &["Bluefile".to_string()]);
        assert!(no_marker.exists(), "{no_marker:?} must exist");
        assert_eq!(no_marker, deep, "no marker ⇒ the file's own directory");

        std::fs::write(tmp.path().join("Bluefile"), "").unwrap();
        let with_marker = root_for(&file, &["Bluefile".to_string()]);
        assert!(with_marker.exists());
        assert_eq!(
            with_marker,
            tmp.path().canonicalize().unwrap(),
            "a marker upward wins over the file's directory"
        );
    }
    use escriba_madoguchi::errand::{Errand, Freight, Runner};
    use escriba_madoguchi::{ErrandId, Negai};
    use escriba_shirube::{Axis, NonEmptyAnchor, SessionGen, SessionKind};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    fn an_errand(path: &str, text: &str) -> Errand {
        Errand {
            id: ErrandId(1),
            freight: Freight::Diagnostics {
                buffer: escriba_core::BufferId(1),
                path: path.into(),
                // `None` on purpose: these tests exercise the FALLBACK, which
                // is what a bare boot with an empty filetype table gets.
                language: None,
                text: text.into(),
            },
            anchor: NonEmptyAnchor::on(Axis::Session(SessionKind::Lsp, SessionGen(1))),
        }
    }

    #[test]
    fn each_language_publishes_under_its_own_list() {
        assert_ne!(list_for("nix"), list_for("rust"));
        assert!(list_for("nix").contains("nix"));
    }

    #[test]
    fn a_language_is_recognised_by_extension() {
        assert_eq!(language_of(std::path::Path::new("a/b.nix")), Some("nix"));
        assert_eq!(language_of(std::path::Path::new("a/b.rs")), Some("rust"));
        assert_eq!(language_of(std::path::Path::new("README.md")), None);
        assert_eq!(language_of(std::path::Path::new("noext")), None);
    }

    #[test]
    fn a_uri_is_a_file_url() {
        assert_eq!(uri_of(std::path::Path::new("/a/b.nix")), "file:///a/b.nix");
    }

    /// Most files have no server. That must be silent — a message on every
    /// open would train the operator to ignore messages.
    #[test]
    fn a_file_with_no_server_produces_nothing_and_says_nothing() {
        let (tx, rx) = channel();
        LspRunner::default().start(
            an_errand("notes.md", "hello"),
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        assert!(
            rx.recv_timeout(Duration::from_secs(2)).is_err(),
            "an unsupported language is not an event"
        );
    }

    /// A server that cannot be spawned must be REPORTED. Silence here looks
    /// exactly like a clean file, which is the worse of the two lies.
    #[test]
    fn a_server_that_will_not_start_is_reported_rather_than_silent() {
        let mut registry = ServerRegistry::default();
        registry.register(ServerConfig {
            language: "nix".into(),
            command: "escriba-no-such-language-server".into(),
            args: vec![],
            root_markers: vec![],
        });
        let (tx, rx) = channel();
        LspRunner::new(registry).start(
            an_errand("a.nix", "{ }"),
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(p) => match p.slip {
                Negai::Message(m) => assert!(m.contains("lsp"), "names the subsystem: {m}"),
                other => panic!("expected a message, got {other:?}"),
            },
            Err(e) => panic!("a failed spawn must be reported, got {e:?}"),
        }
    }

    /// The wait is bounded. A server that answers nothing must not hold a
    /// thread forever.
    #[test]
    fn the_wait_for_diagnostics_is_bounded() {
        assert!(
            REPLY_TIMEOUT <= Duration::from_secs(30),
            "an unbounded wait is a hang with extra steps"
        );
    }

    /// End to end against the REAL sui-lsp, when it is on PATH. Skipped rather
    /// than failed when it is not — a developer without it installed should
    /// not see a red suite — but when it is present this is the only test that
    /// proves the whole path: spawn, initialize, didOpen, publishDiagnostics,
    /// findings.
    #[test]
    fn a_real_server_produces_findings_for_a_broken_file() {
        if std::process::Command::new("sui-lsp")
            .arg("--help")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("sui-lsp not on PATH — skipping the live half");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("broken.nix");
        std::fs::write(&file, "{ x = ; }\n").unwrap();

        let (tx, rx) = channel();
        LspRunner::default().start(
            Errand {
                id: ErrandId(7),
                freight: Freight::Diagnostics {
                    buffer: escriba_core::BufferId(1),
                    path: file.clone(),
                    language: None,
                    text: "{ x = ; }\n".into(),
                },
                anchor: NonEmptyAnchor::on(Axis::Session(SessionKind::Lsp, SessionGen(1))),
            },
            Arc::new(AtomicBool::new(false)),
            tx,
        );

        let parcel = rx
            .recv_timeout(Duration::from_secs(60))
            .expect("the server must answer");
        match parcel.slip {
            Negai::ErrandReply { then, .. } => match *then {
                Negai::PublishFindings { list, findings } => {
                    assert_eq!(list, list_for("nix"));
                    assert!(!findings.is_empty(), "a broken file must produce findings");
                    assert_eq!(
                        findings[0].origin,
                        escriba_shirube::Origin::Lsp("sui".into())
                    );
                }
                other => panic!("expected findings, got {other:?}"),
            },
            Negai::Message(m) => panic!("expected findings, got a message: {m}"),
            other => panic!("unexpected slip {other:?}"),
        }
    }
}
