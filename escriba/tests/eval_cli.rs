//! End-to-end tests for `escriba eval` — the CLI face of the
//! imperative tatara-lisp programmability tier. Runs the real binary
//! so the whole path (clap → run_eval → EditorState::run_lisp →
//! EscribaVm → apply effects → report) is exercised.

use std::process::Command;

fn eval(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_escriba"))
        .arg("eval")
        .args(args)
        .output()
        .expect("run `escriba eval`");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn eval_message_reports_effect() {
    let (ok, stdout, stderr) = eval(&[r#"(message "from-cli")"#]);
    assert!(ok, "eval should succeed: {stderr}");
    assert!(stdout.contains("message: from-cli"), "got: {stdout:?}");
}

#[test]
fn eval_set_option_reports_option() {
    let (ok, stdout, stderr) = eval(&[r#"(set-option "number" "true")"#]);
    assert!(ok, "eval should succeed: {stderr}");
    assert!(stdout.contains("option: number = true"), "got: {stdout:?}");
}

#[test]
fn eval_insert_reports_buffer() {
    let (ok, stdout, stderr) = eval(&[r#"(insert "hello")"#]);
    assert!(ok, "eval should succeed: {stderr}");
    assert!(stdout.contains("buffer:"), "got: {stdout:?}");
    assert!(stdout.contains("hello"), "got: {stdout:?}");
}

#[test]
fn eval_computes_and_branches() {
    // Reads live cursor state (line 0 on a fresh scratch) and an `if`
    // chooses the effect — proves genuine evaluation from the CLI.
    let (ok, stdout, stderr) =
        eval(&[r#"(if (= (cursor-line) 0) (message "top") (message "mid"))"#]);
    assert!(ok, "eval should succeed: {stderr}");
    assert!(stdout.contains("message: top"), "got: {stdout:?}");
}

#[test]
fn eval_reports_parse_error_nonzero() {
    let (ok, _stdout, stderr) = eval(&["((("]);
    assert!(!ok, "malformed lisp should exit non-zero");
    assert!(
        stderr.contains("tatara-lisp eval failed") || stderr.contains("eval failed"),
        "stderr should explain the failure: {stderr:?}",
    );
}
