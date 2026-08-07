//! End-to-end: does a bare `escriba` actually greet you, and does
//! `escriba <file>` actually not?
//!
//! Drives the real binary through `--render=text`, which is escriba's
//! one-shot snapshot face — the same screen an interactive run paints,
//! dumped once and exited.

use std::path::PathBuf;
use std::process::Command;

fn escriba() -> Command {
    // The integration-test binary sits beside the crate binaries.
    let mut exe = PathBuf::from(env!("CARGO_BIN_EXE_escriba"));
    exe.set_file_name("escriba");
    Command::new(exe)
}

/// An EMPTY rc, so these tests describe the SHIPPED defaults rather than
/// whatever the developer running them happens to have in `~`. It has to
/// be a real file: `$ESCRIBARC` is the `--rc` flag's env form, and an
/// explicit rc that does not exist is a hard error by design.
fn empty_rc() -> PathBuf {
    let dir = std::env::temp_dir().join("escriba-splash-boot-test");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let rc = dir.join("empty-rc.lisp");
    std::fs::write(&rc, ";; intentionally empty\n").expect("write empty rc");
    rc
}

fn run(args: &[&str]) -> String {
    let out = escriba()
        .args(args)
        .env("ESCRIBARC", empty_rc())
        // The plugins dir is a live-system input too — point it somewhere
        // that does not exist so user installs cannot colour the result.
        .env("ESCRIBA_PLUGINS_DIR", "/nonexistent-escriba-plugins")
        .output()
        .expect("escriba runs");
    assert!(out.status.success(), "escriba {args:?} failed: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Strip SGR sequences so assertions read about content, not colour.
fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn a_bare_escriba_shows_the_start_screen() {
    let out = plain(&run(&["--render=text", "--height=30"]));
    assert!(
        out.contains("Rust owns the invariants"),
        "no tagline in:\n{out}",
    );
    assert!(out.contains("start editing"), "no menu in:\n{out}");
    assert!(
        out.contains("quit"),
        "the menu must reach its last entry:\n{out}",
    );
}

#[test]
fn the_footer_reports_this_build_not_a_frozen_string() {
    let out = plain(&run(&["--render=text", "--height=30"]));
    let version = env!("CARGO_PKG_VERSION");
    assert!(out.contains(version), "footer must name v{version}:\n{out}");
    assert!(
        out.contains("plugins"),
        "footer must count the plugins actually loaded:\n{out}",
    );
}

#[test]
fn opening_a_file_goes_straight_to_the_file() {
    // A welcome screen in front of a file you explicitly asked for is a
    // keystroke tax, not a welcome.
    let file = empty_rc().with_file_name("hello.txt");
    std::fs::write(&file, "OPENED-FILE-CONTENT\n").expect("write fixture");

    let out = plain(&run(&[
        "--render=text",
        "--height=30",
        file.to_str().expect("utf8 path"),
    ]));
    assert!(out.contains("OPENED-FILE-CONTENT"), "{out}");
    assert!(
        !out.contains("Rust owns the invariants"),
        "no start screen when a file was named:\n{out}",
    );
}

#[test]
fn no_splash_flag_skips_the_screen() {
    let out = plain(&run(&["--render=text", "--height=30", "--no-splash"]));
    assert!(!out.contains("Rust owns the invariants"), "{out}");
    assert!(out.contains("escriba scratch buffer"), "{out}");
}

#[test]
fn no_defaults_means_no_screen_either() {
    // The screen is authored in the shipped rc, so `--no-defaults` — the
    // bare editor — must not conjure one from anywhere else.
    let out = plain(&run(&["--render=text", "--height=30", "--no-defaults"]));
    assert!(!out.contains("Rust owns the invariants"), "{out}");
}
