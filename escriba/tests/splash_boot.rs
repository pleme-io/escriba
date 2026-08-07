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
fn the_shipped_screen_fits_a_classic_80x24_terminal_intact() {
    // Every degradation path in `Splash::rows` is SILENT by design — a
    // too-wide wordmark quietly becomes the word "escriba", a too-short
    // canvas quietly drops the art, and the menu quietly loses its last
    // entries. That is right behaviour for a small terminal and wrong as
    // a description of the shipped screen on an ordinary one: widening
    // the art by ten columns would degrade it for everybody, everywhere,
    // with nothing going red.
    let plan = escriba::default_plan(false).expect("shipped defaults parse");
    let splash = escriba_lisp::apply_plan_to_splash(&plan).expect("a start screen");
    let rendered = splash
        .rows(80, 24)
        .iter()
        .map(escriba_ui::splash::SplashRow::plain)
        .collect::<Vec<_>>()
        .join("\n");

    for line in &splash.art {
        assert!(
            rendered.contains(line.as_str()),
            "art line degraded away at 80x24 — is it wider than 76 cells?\n{line}",
        );
    }
    for entry in &splash.entries {
        assert!(
            rendered.contains(&entry.label),
            "menu entry `{}` truncated away at 80x24 — the screen is too tall",
            entry.label,
        );
    }
}

#[test]
fn the_footer_names_the_theme_that_is_actually_painted() {
    // Read from the EDITOR's theme, not from the fleet default and not
    // from `plan.theme`. Those three agree today — the shipped rc declares
    // the fleet theme and it is wired through — but the footer has to
    // follow the pixels, so it is sourced from the value the faces paint.
    let out = plain(&run(&["--render=text", "--height=30"]));
    let painted = escriba_ui::chrome::prescribed_theme_name();
    let footer = out
        .lines()
        .find(|l| l.contains("plugins"))
        .expect("a footer strip");
    assert!(
        footer.contains(painted),
        "footer must name the painted theme ({painted}): {footer:?}",
    );
}

#[test]
fn a_user_rc_theme_reaches_the_rendered_output() {
    // End to end, through the real binary: an operator authoring a theme
    // must see different bytes come out. This is the whole point of the
    // `(deftheme …)` wiring — before it, both runs produced identical
    // colour sequences because every paint site read the fleet default.
    let dir = empty_rc().with_file_name("theme-rcs");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let shot = |preset: &str| {
        let rc = dir.join(format!("{preset}.lisp"));
        std::fs::write(&rc, format!("(deftheme :preset {preset:?})\n")).expect("write rc");
        let out = escriba()
            .args(["--render=text", "--height=24"])
            .env("ESCRIBARC", &rc)
            .env("ESCRIBA_PLUGINS_DIR", "/nonexistent-escriba-plugins")
            .output()
            .expect("escriba runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let nord = shot("nord");
    let vellum = shot("vellum");
    assert_ne!(
        nord, vellum,
        "two themes produced byte-identical output — (deftheme …) is inert again",
    );
    // It is the SAME screen, recoloured — the layout must not shift. The
    // footer is the one line allowed to differ, because it names the theme.
    let body = |s: &str| {
        plain(s)
            .lines()
            .filter(|l| !l.contains("plugins"))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        body(&nord),
        body(&vellum),
        "a theme change must recolour, not relayout",
    );
    assert!(
        plain(&vellum).contains("vellum") && plain(&nord).contains("nord"),
        "the footer must name the operator's theme",
    );
}

#[test]
fn list_rc_reports_the_start_screen_as_wired() {
    // `--list-rc`'s wiring block is this repo's honesty surface: a
    // def-form that reaches live state says WIRED there, and one that
    // only parses does not. A new form that never appears in it is the
    // failure mode the block exists to prevent.
    let out = plain(&run(&["--list-rc"]));
    let row = out
        .lines()
        .find(|l| l.contains("defsplash"))
        .expect("defsplash must appear in the wiring block");
    assert!(row.contains("WIRED"), "{row:?}");
    assert!(
        row.contains("entries=5"),
        "the row must report the real menu size: {row:?}",
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
