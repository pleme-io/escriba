//! The external formatter route: run a `defformatter` command over the BUFFER.
//!
//! [`crate::format`] asks a language server. This module runs the command a
//! `(defformatter … :external #t)` names, which is how the fleet's formatter
//! registry (substrate `lib/formatters.nix`) reaches escriba: the HM module
//! renders one such form per language with the registry's store-path command
//! and argv, the same argv CI's canonical checks run. A buffer formatted here
//! is therefore byte-for-byte what CI accepts.
//!
//! Two modes, chosen by the declaration, never guessed from the args:
//!
//! - stdin: the buffer goes in on stdin, the result comes back on stdout.
//!   `$FILE` in the args is the buffer's path, a hint only (prettier picks its
//!   parser from the name).
//! - file mode: the buffer is written to a scratch copy with the same file
//!   name, `$FILE` names that copy, the formatter rewrites it in place, and
//!   the copy is read back. The file on disk is never touched, so "format"
//!   never becomes "save".
//!
//! A formatter that outlives its deadline is killed; a non-zero exit is an
//! error carrying the formatter's stderr, and the buffer is left alone.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// One external formatter, as the catalog declared it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct External {
    pub command: String,
    pub args: Vec<String>,
    pub file_mode: bool,
    /// The edition used when no `Cargo.toml` above the file names one. Set,
    /// it makes the runner append `--edition <found>`, as `cargo fmt` does.
    pub cargo_edition: Option<String>,
    pub timeout: Duration,
}

/// The default deadline, matching the server route's.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

static SCRATCH: AtomicU64 = AtomicU64::new(0);

/// The finished text, or why there is none.
pub fn format(spec: &External, path: &Path, text: &str) -> Result<String, String> {
    let mut args = spec.args.clone();
    if let Some(default) = &spec.cargo_edition {
        let edition = path
            .parent()
            .and_then(cargo_edition)
            .unwrap_or_else(|| default.clone());
        args.push("--edition".into());
        args.push(edition);
    }
    if spec.file_mode {
        format_file(spec, &args, path, text)
    } else {
        let hint = path.to_string_lossy();
        let args = substitute(&args, &hint);
        let (stdout, _) = run(spec, &args, Some(text))?;
        Ok(stdout)
    }
}

fn format_file(
    spec: &External,
    args: &[String],
    path: &Path,
    text: &str,
) -> Result<String, String> {
    let name = path
        .file_name()
        .map_or_else(|| "buffer".into(), |n| n.to_string_lossy().into_owned());
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).map_err(|e| describe("format: no scratch dir", &e))?;
    let copy = dir.join(name);
    let result = (|| {
        std::fs::write(&copy, text).map_err(|e| describe("format: scratch write", &e))?;
        let args = substitute(args, &copy.to_string_lossy());
        run(spec, &args, None)?;
        std::fs::read_to_string(&copy).map_err(|e| describe("format: scratch read", &e))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn scratch_dir() -> PathBuf {
    let n = SCRATCH.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("escriba-fmt-{}-{n}", std::process::id()))
}

fn substitute(args: &[String], file: &str) -> Vec<String> {
    args.iter()
        .map(|a| {
            if a == "$FILE" {
                file.to_string()
            } else {
                a.clone()
            }
        })
        .collect()
}

fn run(spec: &External, args: &[String], input: Option<&str>) -> Result<(String, String), String> {
    let mut child = Command::new(&spec.command)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| describe(&format!("format: could not start {}", spec.command), &e))?;

    let writer = input.map(|text| {
        let mut stdin = child.stdin.take();
        let text = text.to_owned();
        std::thread::spawn(move || {
            if let Some(s) = stdin.as_mut() {
                let _ = s.write_all(text.as_bytes());
            }
        })
    });
    let mut stdout = child.stdout.take();
    let out = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = stdout.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let mut stderr = child.stderr.take();
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });

    let deadline = Instant::now() + spec.timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "format: {} gave no answer within {:?}",
                    spec.command, spec.timeout
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(describe("format: wait", &e)),
        }
    };
    if let Some(w) = writer {
        let _ = w.join();
    }
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        Ok((stdout, stderr))
    } else {
        Err(format!(
            "format: {} exited {}: {}",
            spec.command,
            status
                .code()
                .map_or_else(|| "by signal".into(), |c| c.to_string()),
            stderr.trim()
        ))
    }
}

/// The edition the nearest `Cargo.toml` above `dir` names, or the workspace's
/// when a crate inherits it. The same reading conform.nvim's
/// `parse_rust_edition` does, so blnvim and escriba pick the same edition.
pub fn cargo_edition(dir: &Path) -> Option<String> {
    for ancestor in dir.ancestors() {
        let Ok(manifest) = std::fs::read_to_string(ancestor.join("Cargo.toml")) else {
            continue;
        };
        for line in manifest.lines() {
            let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
            if compact.starts_with("edition.workspace=true")
                || compact.starts_with("edition={workspace=true")
            {
                break;
            }
            if let Some(rest) = compact.strip_prefix("edition=\"")
                && let Some(edition) = rest.split('"').next()
                && !edition.is_empty()
                && edition.chars().all(|c| c.is_ascii_digit())
            {
                return Some(edition.to_string());
            }
        }
    }
    None
}

fn describe(what: &str, e: &impl std::fmt::Display) -> String {
    format!("{what}: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(command: &str, args: &[&str], file_mode: bool) -> External {
        External {
            command: command.into(),
            args: args.iter().map(|s| (*s).into()).collect(),
            file_mode,
            cargo_edition: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    #[test]
    fn stdin_mode_returns_stdout() {
        let s = spec("tr", &["a-z", "A-Z"], false);
        assert_eq!(format(&s, Path::new("/x/y.txt"), "abc\n").unwrap(), "ABC\n");
    }

    #[test]
    fn stdin_mode_passes_the_buffer_path_for_file() {
        let s = spec("echo", &["$FILE"], false);
        assert_eq!(
            format(&s, Path::new("/x/y.yaml"), "").unwrap(),
            "/x/y.yaml\n"
        );
    }

    #[test]
    fn file_mode_rewrites_a_scratch_copy_and_leaves_the_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.b");
        std::fs::write(&path, "on disk\n").unwrap();
        let s = spec("sh", &["-c", "printf formatted > \"$0\"", "$FILE"], true);
        assert_eq!(format(&s, &path, "in the buffer\n").unwrap(), "formatted");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");
    }

    #[test]
    fn a_failing_formatter_is_an_error_not_an_empty_buffer() {
        let s = spec("sh", &["-c", "echo nope >&2; exit 3"], false);
        let e = format(&s, Path::new("/x"), "keep me").unwrap_err();
        assert!(e.contains("exited 3") && e.contains("nope"), "{e}");
    }

    #[test]
    fn a_formatter_past_its_deadline_is_killed() {
        let mut s = spec("sleep", &["5"], false);
        s.timeout = Duration::from_millis(100);
        let start = Instant::now();
        assert!(
            format(&s, Path::new("/x"), "")
                .unwrap_err()
                .contains("no answer")
        );
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn the_crate_edition_wins_and_a_workspace_crate_inherits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace.package]\nedition = \"2024\"\n",
        )
        .unwrap();
        let member = dir.path().join("m");
        std::fs::create_dir_all(member.join("src")).unwrap();
        std::fs::write(
            member.join("Cargo.toml"),
            "[package]\nedition.workspace = true\n",
        )
        .unwrap();
        assert_eq!(cargo_edition(&member.join("src")).as_deref(), Some("2024"));
        std::fs::write(member.join("Cargo.toml"), "[package]\nedition = \"2021\"\n").unwrap();
        assert_eq!(cargo_edition(&member.join("src")).as_deref(), Some("2021"));
    }

    #[test]
    fn the_edition_is_appended_from_cargo_or_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = spec("echo", &[], false);
        s.cargo_edition = Some("2015".into());
        assert_eq!(
            format(&s, &dir.path().join("x.rs"), "").unwrap(),
            "--edition 2015\n"
        );
        std::fs::write(dir.path().join("Cargo.toml"), "edition = \"2024\"\n").unwrap();
        assert_eq!(
            format(&s, &dir.path().join("x.rs"), "").unwrap(),
            "--edition 2024\n"
        );
    }
}
