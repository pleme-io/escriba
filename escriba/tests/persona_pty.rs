//! Persona PTY harness — the REAL `escriba` TUI binary under a typed fake
//! terminal. The "apart" leg of the fleet terminal-conformance matrix:
//! composition row `(:host :pty-persona :guest :escriba-tui)` in espelho's
//! `specs/term-conformance.lisp`. (espelho is unpublished — its GUEST
//! invariants are mirrored locally here, exactly as the frost L0 harness
//! does; when espelho lands on crates.io this file consumes it instead.)
//!
//! GUEST invariants exercised (espelho vocabulary):
//! - **never-fatal** — no persona behavior may kill the editor. A mute
//!   host (never answers any VT query) must not crash or wedge boot.
//! - **no-freeze** — liveness is proven by a ROUND-TRIP (keystroke in →
//!   repaint out), never by aliveness alone (the 2026-06-10
//!   false-verification rule). Under the mute persona the round-trip is
//!   the `:`-keypress → COMMAND-statusline repaint.
//! - **mode-restore** — escriba-tui is a FULL-SCREEN guest
//!   (`EnterAlternateScreen` + `enable_raw_mode` in escriba-tui/src/run.rs):
//!   on exit it must leave the alt screen (`ESC[?1049l`) and hand back a
//!   cooked terminal (ICANON+ECHO restored on the pty termios).
//!
//! Pattern mirrored from `frost/crates/frost/tests/persona_pty.rs` (the
//! `(:host :pty-persona :guest :frost)` row): a typed persona owns the
//! master side of a real PTY, answers (or refuses to answer) VT queries
//! per a typed `DsrPolicy`, scans for `ESC[6n` with a ROLLING cursor
//! (per-chunk scans miss queries split across reads — the E1-harness
//! fragility), and injects a timed keystroke script. Differences forced
//! by the full-screen guest class:
//! - the script clock starts at observed alt-screen ENTRY, not at spawn
//!   (boot time varies; the contract clock starts when the guest claims
//!   the screen);
//! - the persona can inject a winsize shrink (`TIOCSWINSZ` → SIGWINCH →
//!   ratatui full repaint) — diff-renderers only emit changed cells, so
//!   typed text never appears contiguously in the incremental stream;
//!   demanding a complete frame is how a persona reads the screen;
//! - mode-restore is asserted from the pty termios via `tcgetattr` on
//!   the master (master and slave share one line discipline), with a
//!   tier-honest fallback to the leave-alt sequence when the post-exit
//!   read is unavailable.

use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use nix::sys::signal::{kill, Signal};
use nix::sys::termios::{tcgetattr, LocalFlags};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

/// crossterm 0.28's `EnterAlternateScreen` / `LeaveAlternateScreen` wire
/// bytes — the mode-restore observables.
const ALT_ENTER: &[u8] = b"\x1b[?1049h";
const ALT_LEAVE: &[u8] = b"\x1b[?1049l";

/// The persona's terminal geometry. 150×40 per the conformance row; the
/// shrink injection drops one column to force a full repaint.
const WS_ROWS: u16 = 40;
const WS_COLS: u16 = 150;

/// How the persona treats `ESC[6n` (DSR-6 / CPR) queries. escriba-tui
/// emits ZERO VT queries today (crossterm raw-mode + ratatui never ask),
/// so `Answer` is dormant and `Mute` passes trivially — the rows exist
/// to hold the invariant the moment a query-emitting change lands
/// (synchronized-output probes, kitty-keyboard detection, OSC color
/// queries are all one crossterm call away). frost's `SplitReply` arm is
/// deliberately NOT mirrored yet: with no query on the wire there is no
/// reply to fragment (add it with escriba-tui's first real query).
#[derive(Clone, Copy, Debug)]
enum DsrPolicy {
    /// Answer every query with `ESC[{rows};1R` after `latency` — a REAL
    /// terminal answers with latency (mado's engate→VT→send_keys loop
    /// measured ~tens of ms; 50ms is the proven race window from the
    /// frost harness).
    Answer { latency: Duration },
    /// Never answer — the hostile-terminal class (E2/E4).
    Mute,
}

/// One persona action against the guest.
enum Inject {
    /// Keystroke bytes onto the master (the persona "types").
    Keys(&'static [u8]),
    /// Shrink the winsize by one column (`TIOCSWINSZ`). The kernel sends
    /// SIGWINCH to the guest's foreground process group; crossterm emits
    /// `Event::Resize`; ratatui's autoresize clears + repaints EVERY
    /// cell on the next draw — the only way typed buffer text appears
    /// contiguously in a diff-renderer's output stream.
    ShrinkOneColumn,
}

/// A timed injection. `after_alt` is measured from observed alt-screen
/// entry, not from spawn.
struct Step {
    after_alt: Duration,
    inject: Inject,
}

/// Everything observed from one driven session.
struct Outcome {
    transcript: Vec<u8>,
    cpr_queries: usize,
    /// When (since spawn) `ESC[?1049h` was first observed.
    alt_entered_at: Option<Duration>,
    /// Whether `ESC[?1049l` appeared anywhere in the transcript.
    alt_left: bool,
    /// Whether the pty termios was observed with ICANON cleared while
    /// the guest held the screen — proves raw mode was genuinely
    /// entered, so the cooked-at-exit assertion has teeth.
    raw_mode_observed: bool,
    /// Post-exit pty termios: `Some(true)` = ICANON+ECHO restored
    /// (cooked), `Some(false)` = still raw, `None` = tcgetattr
    /// unavailable after the slave closed (tier-honest fallback).
    final_termios_cooked: Option<bool>,
    /// `Some(code)` when the guest exited on its own.
    exit_code: Option<i32>,
    /// Guest still running when the window closed (the expected
    /// end-state for hostile-persona rows; the harness then reaps it).
    alive_at_end: bool,
}

/// `KEY=VALUE` env entry from typed pieces (no `format!` — TYPED
/// EMISSION; `OsString::push` is the path-safe composer).
fn env_kv(key: &str, value: &std::path::Path) -> CString {
    let mut s = std::ffi::OsString::from(key);
    s.push("=");
    s.push(value);
    CString::new(s.as_encoded_bytes()).expect("no NUL in env entry")
}

/// Shrink the pty to `WS_COLS - 1` columns — see [`Inject::ShrinkOneColumn`].
fn shrink_one_column(master_fd: std::os::fd::RawFd) {
    let ws = nix::pty::Winsize {
        ws_row: WS_ROWS,
        ws_col: WS_COLS - 1,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: master_fd is the live pty master owned by `drive`'s File;
    // TIOCSWINSZ only reads the winsize struct.
    unsafe {
        libc::ioctl(master_fd, libc::TIOCSWINSZ, std::ptr::from_ref(&ws));
    }
}

/// Drive the real `escriba` binary (TUI default render) on a real PTY
/// under `policy` for at most `total`, injecting `script` steps at their
/// offsets from alt-screen entry. Returns `None` (SKIP) when forkpty is
/// unavailable in the environment. Never panics on session mechanics —
/// assertions live in the tests.
fn drive(policy: DsrPolicy, script: &[Step], total: Duration) -> Option<Outcome> {
    use nix::pty::ForkptyResult;

    let home = tempfile::tempdir().expect("tempdir");
    let exe = CString::new(env!("CARGO_BIN_EXE_escriba")).unwrap();
    let argv = [exe.clone()];
    // Hermetic env: tempdir HOME so no operator rc.lisp leaks in
    // ($ESCRIBARC unset; default_rc_path resolves under HOME and won't
    // exist). The bundled blnvim defaults still apply — that IS the
    // shipped boot path.
    let envp: Vec<CString> = vec![
        CString::new("TERM=xterm-256color").unwrap(),
        env_kv("HOME", home.path()),
        CString::new("PATH=/usr/bin:/bin").unwrap(),
    ];

    let ws = nix::pty::Winsize {
        ws_row: WS_ROWS,
        ws_col: WS_COLS,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: the child performs only async-signal-safe operations
    // between fork and exec (execve / _exit), per fork-in-threaded-
    // process rules.
    let fork = unsafe { nix::pty::forkpty(Some(&ws), None) };
    let (child, master) = match fork {
        Ok(ForkptyResult::Parent { child, master }) => (child, master),
        Ok(ForkptyResult::Child) => {
            let _ = nix::unistd::execve(&exe, &argv, &envp);
            unsafe { libc::_exit(127) };
        }
        Err(e) => {
            eprintln!("SKIP persona_pty: forkpty unavailable in this environment: {e}");
            return None;
        }
    };

    // Non-blocking master so the poll loop owns all timing.
    nix::fcntl::fcntl(
        master.as_raw_fd(),
        nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
    )
    .expect("set O_NONBLOCK on pty master");
    let mut master = std::fs::File::from(master);

    let mut transcript: Vec<u8> = Vec::new();
    let mut scan = 0usize; // rolling ESC[6n scan cursor (never per-chunk)
    let mut cpr_queries = 0usize;
    let mut sent = vec![false; script.len()];
    let mut alt_entered_at: Option<Duration> = None;
    let mut raw_mode_observed = false;
    let mut exit_code: Option<i32> = None;
    let mut signaled = false;
    let mut saw_eof = false;
    let start = Instant::now();

    while start.elapsed() < total {
        let mut buf = [0u8; 4096];
        match master.read(&mut buf) {
            Ok(0) => {
                saw_eof = true;
                break;
            }
            Ok(n) => transcript.extend_from_slice(&buf[..n]),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {
                saw_eof = true;
                break;
            }
        }
        // Rolling scan: answer every complete ESC[6n found so far.
        while let Some(rel) = find(&transcript[scan..], b"\x1b[6n") {
            scan += rel + 4;
            cpr_queries += 1;
            match policy {
                DsrPolicy::Answer { latency } => {
                    std::thread::sleep(latency);
                    let _ = master.write_all(b"\x1b[40;1R");
                }
                DsrPolicy::Mute => {}
            }
        }
        // Alt-screen entry starts the script clock.
        if alt_entered_at.is_none() && find(&transcript, ALT_ENTER).is_some() {
            alt_entered_at = Some(start.elapsed());
        }
        // Raw-mode witness: master and slave share one termios, so the
        // guest's enable_raw_mode is visible from the persona's side.
        if alt_entered_at.is_some() && !raw_mode_observed {
            if let Ok(t) = tcgetattr(&master) {
                if !t.local_flags.contains(LocalFlags::ICANON) {
                    raw_mode_observed = true;
                }
            }
        }
        if let Some(alt_at) = alt_entered_at {
            let since_alt = start.elapsed().saturating_sub(alt_at);
            for (i, s) in script.iter().enumerate() {
                if !sent[i] && since_alt >= s.after_alt {
                    match s.inject {
                        Inject::Keys(bytes) => {
                            let _ = master.write_all(bytes);
                        }
                        Inject::ShrinkOneColumn => shrink_one_column(master.as_raw_fd()),
                    }
                    sent[i] = true;
                }
            }
        }
        match waitpid(child, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => {
                exit_code = Some(code);
                break;
            }
            Ok(WaitStatus::Signaled(..)) => {
                signaled = true;
                break;
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    // ── Teardown: reap WHILE draining; never block in waitpid ──────────
    // macOS pty teardown wedge (observed 2026-06-10, this harness): a
    // SIGKILLed session leader can sit in the kernel's tty output-drain
    // exit path for MINUTES while the master goes unread (`ps` shows the
    // guest in `E` state) — a blocking `waitpid` then hangs the whole
    // suite. So the master keeps being consumed while reaping with
    // WNOHANG, and an unreapable child is abandoned (the test process's
    // exit closes the master, which unwedges the drain and lets launchd
    // reap it) rather than hanging the run.
    let mut alive_at_end = false;
    let mut reaped = exit_code.is_some() || signaled;
    let mut killed = false;
    if !reaped && !saw_eof {
        // Window closed with the guest running — the expected end-state
        // for hostile-persona rows. Record it, then tear down. (The
        // saw_eof path skips the record: EOF means the guest closed its
        // slave fds on the way out; it gets a grace period below.)
        alive_at_end = true;
        let _ = kill(child, Signal::SIGKILL);
        killed = true;
    }
    let reap_deadline = Instant::now() + Duration::from_secs(5);
    while !reaped && Instant::now() < reap_deadline {
        drain(&mut master, &mut transcript);
        match waitpid(child, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => {
                exit_code = Some(code);
                reaped = true;
            }
            // Signal-death (or ECHILD): reaped, but neither alive nor a
            // clean exit — rows asserting either fail with the right
            // evidence.
            Ok(WaitStatus::Signaled(..)) | Err(_) => reaped = true,
            _ => {
                // saw_eof guests get a 2s grace to finish their own exit
                // (EOF normally precedes it by microseconds) before the
                // kill.
                if !killed
                    && reap_deadline.saturating_duration_since(Instant::now())
                        < Duration::from_secs(3)
                {
                    let _ = kill(child, Signal::SIGKILL);
                    killed = true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    if !reaped {
        eprintln!(
            "NOTE persona_pty: guest unreapable 5s after SIGKILL (macOS pty \
             drain wedge) — abandoning it to launchd"
        );
    }

    // Final drain — LeaveAlternateScreen lands in the pty buffer after
    // the exit observation. Deadline-bounded, never EOF-driven.
    let drain_deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < drain_deadline {
        if drain(&mut master, &mut transcript) == 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // Post-exit termios: the pty (and its line discipline) lives while
    // the master is open, so this works after a clean exit on macOS +
    // Linux; `None` falls back to the leave-alt-sequence tier.
    let final_termios_cooked = tcgetattr(&master).ok().map(|t| {
        t.local_flags.contains(LocalFlags::ICANON) && t.local_flags.contains(LocalFlags::ECHO)
    });

    let alt_left = find(&transcript, ALT_LEAVE).is_some();
    Some(Outcome {
        transcript,
        cpr_queries,
        alt_entered_at,
        alt_left,
        raw_mode_observed,
        final_termios_cooked,
        exit_code,
        alive_at_end,
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// One non-blocking read off the master into the transcript. Returns the
/// byte count (0 on EOF / WouldBlock / error — callers are
/// deadline-bounded, never EOF-driven).
fn drain(master: &mut std::fs::File, transcript: &mut Vec<u8>) -> usize {
    let mut buf = [0u8; 4096];
    match master.read(&mut buf) {
        Ok(n) => {
            transcript.extend_from_slice(&buf[..n]);
            n
        }
        Err(_) => 0,
    }
}

/// Healthy persona, full session: boot → alt-screen within 3s → `:q⏎` →
/// clean exit with the terminal handed back. This is the mode-restore
/// row: exit 0 AND leave-alt-screen on the wire AND cooked termios
/// (ICANON+ECHO back) — with the mid-session raw-mode witness proving
/// the final check isn't vacuous.
///
/// The quit chord is the canonical modal path (escriba-keymap
/// `default_vim`): `:` (Normal→Command) + `q` (minibuffer) + Enter
/// (SubmitCommand → `:q` → the `quit` command → quit_requested).
#[test]
fn healthy_persona_renders_and_quits() {
    let script = [Step {
        after_alt: Duration::from_millis(1000),
        inject: Inject::Keys(b":q\r"),
    }];
    let Some(o) = drive(
        DsrPolicy::Answer {
            latency: Duration::from_millis(50),
        },
        &script,
        Duration::from_secs(10),
    ) else {
        return;
    };
    assert!(
        o.alt_entered_at.is_some_and(|at| at <= Duration::from_secs(3)),
        "TUI must claim the alt screen within 3s of spawn: alt_entered_at={:?} cpr={} tail={:?}",
        o.alt_entered_at,
        o.cpr_queries,
        tail(&o.transcript),
    );
    assert!(
        o.raw_mode_observed,
        "raw mode (ICANON cleared) must be observable on the pty while the TUI holds the screen",
    );
    assert!(
        o.exit_code == Some(0),
        ":q must exit the editor cleanly: exit_code={:?} alive_at_end={} tail={:?}",
        o.exit_code,
        o.alive_at_end,
        tail(&o.transcript),
    );
    assert!(
        o.alt_left,
        "full-screen guest must LEAVE the alt screen on exit (ESC[?1049l missing): tail={:?}",
        tail(&o.transcript),
    );
    match o.final_termios_cooked {
        Some(cooked) => assert!(
            cooked,
            "mode-restore: pty termios must be cooked (ICANON+ECHO) after exit",
        ),
        // Tier-honest fallback: when the post-exit tcgetattr is
        // unavailable, the leave-alt assertion above is the proof tier
        // — note it instead of silently passing a weaker check as the
        // strong one.
        None => eprintln!(
            "TIER NOTE persona_pty: post-exit tcgetattr unavailable — \
             cooked-mode restore verified only via ESC[?1049l this run"
        ),
    }
}

/// Mute persona: a host that never answers ANY VT query must neither
/// kill nor freeze the full-screen guest (never-fatal + no-freeze).
/// Today escriba-tui emits zero queries, so this row passes trivially —
/// it exists to hold the invariant the moment a query-emitting change
/// lands (the class that killed frost on 2026-06-10). Liveness is
/// proven by round-trip, not aliveness: at alt+4s the persona presses
/// `:` and requires the COMMAND-mode statusline repaint on the wire.
#[test]
fn mute_persona_never_fatal() {
    let script = [Step {
        after_alt: Duration::from_millis(4000),
        inject: Inject::Keys(b":"),
    }];
    let Some(o) = drive(DsrPolicy::Mute, &script, Duration::from_secs(7)) else {
        return;
    };
    assert!(
        o.alt_entered_at.is_some(),
        "TUI must enter the alt screen under a mute host: tail={:?}",
        tail(&o.transcript),
    );
    assert!(
        o.alive_at_end,
        "mute host must not kill the TUI (never-fatal): exit_code={:?} tail={:?}",
        o.exit_code,
        tail(&o.transcript),
    );
    assert!(
        find(&o.transcript, b"COMMAND").is_some(),
        "TUI must still round-trip input under a mute host (no-freeze): \
         `:` pressed at alt+4s but no COMMAND statusline repaint: tail={:?}",
        tail(&o.transcript),
    );
}

/// Insert-mode round-trip: `i` → type a marker → Esc → force a FULL
/// repaint (winsize shrink → SIGWINCH → ratatui autoresize clears and
/// redraws every cell) → the marker must appear contiguously in the
/// frame output → `:q⏎` still exits cleanly. The shrink is load-bearing:
/// ratatui diffs per keystroke, so the incremental stream only ever
/// carries one new char (plus the shifted tail) per frame — the full
/// repaint is how a persona reads the composed screen.
#[test]
fn insert_roundtrip() {
    // Insert-mode chars are taken literally (Mode::Insert dispatch), so
    // any letter run works; pick one that collides with nothing the TUI
    // renders on its own (scratch text, statusline, mode names).
    const MARKER: &[u8] = b"ZQINSERTMARKERXJ";
    let script = [
        Step {
            after_alt: Duration::from_millis(600),
            inject: Inject::Keys(b"i"),
        },
        Step {
            after_alt: Duration::from_millis(1200),
            inject: Inject::Keys(MARKER),
        },
        // Lone ESC: the 800ms quiet gap on both sides lets crossterm's
        // disambiguation window resolve it as the Esc KEY, not the
        // start of a sequence.
        Step {
            after_alt: Duration::from_millis(2000),
            inject: Inject::Keys(b"\x1b"),
        },
        Step {
            after_alt: Duration::from_millis(2800),
            inject: Inject::ShrinkOneColumn,
        },
        Step {
            after_alt: Duration::from_millis(3800),
            inject: Inject::Keys(b":q\r"),
        },
    ];
    let Some(o) = drive(
        DsrPolicy::Answer {
            latency: Duration::from_millis(50),
        },
        &script,
        Duration::from_secs(12),
    ) else {
        return;
    };
    assert!(
        o.alt_entered_at.is_some(),
        "TUI must enter the alt screen: tail={:?}",
        tail(&o.transcript),
    );
    assert!(
        find(&o.transcript, MARKER).is_some(),
        "typed insert-mode text must appear contiguously in the forced full repaint: tail={:?}",
        tail(&o.transcript),
    );
    assert!(
        o.exit_code == Some(0) && o.alt_left,
        "editor must still quit cleanly after the insert round-trip: \
         exit_code={:?} alt_left={} tail={:?}",
        o.exit_code,
        o.alt_left,
        tail(&o.transcript),
    );
}

/// Last 300 transcript bytes, lossy — assertion-failure context.
fn tail(transcript: &[u8]) -> String {
    let from = transcript.len().saturating_sub(300);
    String::from_utf8_lossy(&transcript[from..]).into_owned()
}
