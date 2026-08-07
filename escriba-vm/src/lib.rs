//! `escriba-vm` — escriba's embedded tatara-lisp runtime.
//!
//! Hosts a [`tatara_lisp_eval::Interpreter`] parameterized over
//! [`EscribaHost`]. Lisp code runs the full language (arithmetic,
//! lists, `if`/`let`/`lambda`/`begin` via `install_primitives`) and
//! interacts with the editor through **native functions**:
//!
//! - **reads** (`cursor-line`, `current-line`, `editor-mode`, …) answer
//!   from a [`EditorSnapshot`] captured *before* eval, so the host owns
//!   no borrows and satisfies `Interpreter<H>`'s `H: 'static` bound;
//! - **writes** (`message`, `insert`, `set-option`, `run-command`) push
//!   typed [`Negai`](escriba_madoguchi::Negai) slips onto an accumulating log.
//!
//! The effect boundary is the **sandbox seam** (the "terreiro"): Lisp
//! can never corrupt editor state directly — it can only request typed,
//! validated mutations that the runtime applies after eval. It is also
//! the seam through which polyglot **WASM/WASI** plugins are hosted: a
//! plugin authored in any language is driven by the same tatara-lisp
//! host and emits the same typed slips, so the editor's apply path is
//! identical regardless of plugin language.
//!
//! This is the imperative tier of escriba's two-tier programmability
//! model — the declarative tier is `escriba-lisp`'s def-forms.

use escriba_madoguchi::Negai;
use serde::{Deserialize, Serialize};
use tatara_lisp_eval::{Arity, Interpreter, Value, install_full_stdlib_with};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VmError {
    #[error("tatara-lisp read error: {0}")]
    Read(#[from] tatara_lisp::LispError),
    #[error("tatara-lisp eval error: {0}")]
    Eval(#[from] tatara_lisp_eval::EvalError),
}

// NOTE: `HostEffect` lived here — Message / RunCommand / SetOption /
// InsertText. It was a THIRD mutation vocabulary beside the Action executor
// and the slip interpreter, with its own `apply_host_effects` in the runtime
// re-implementing message-push, option-insert and insert-text. That is the
// exact shape that let `u` and `:undo` drift apart in M3, waiting to happen
// again. The VM now emits `escriba_madoguchi::Negai` and the interpreter is
// the single implementation.

/// Read-side snapshot of editor state, captured before eval so Lisp can
/// query without borrowing live state. Integer fields are `i64` to
/// marshal directly to the Lisp `Int` value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorSnapshot {
    pub cursor_line: i64,
    pub cursor_column: i64,
    pub current_line: String,
    pub mode: String,
    pub buffer_name: String,
}

/// The Lisp host: a read snapshot + an accumulating effect log. Owned
/// (no borrows) so it satisfies `Interpreter<H>`'s `H: 'static`; passed
/// by `&mut` per eval call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EscribaHost {
    pub snapshot: EditorSnapshot,
    pub effects: Vec<Negai>,
}

impl EscribaHost {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A host seeded with a read snapshot (and an empty effect log).
    #[must_use]
    pub fn with_snapshot(snapshot: EditorSnapshot) -> Self {
        Self {
            snapshot,
            effects: Vec::new(),
        }
    }

    /// Drain the accumulated effects, leaving the log empty — the
    /// runtime calls this after eval to apply them.
    pub fn take_effects(&mut self) -> Vec<Negai> {
        std::mem::take(&mut self.effects)
    }
}

/// escriba's embedded tatara-lisp runtime. Build once, eval many times;
/// the registered native fns (the editor capability surface) persist
/// across calls.
pub struct EscribaVm {
    interp: Interpreter<EscribaHost>,
}

impl Default for EscribaVm {
    fn default() -> Self {
        Self::new()
    }
}

impl EscribaVm {
    #[must_use]
    pub fn new() -> Self {
        let mut interp: Interpreter<EscribaHost> = Interpreter::new();
        // Install the FULL language stdlib — primitives + higher-order
        // fns (map/filter/fold) + maps + channels + fibers + type-check
        // + the Lisp-authored stdlib. The bootstrap host is a throwaway:
        // stdlib install runs before the editor fns are registered, so
        // it emits no editor effects.
        let mut bootstrap = EscribaHost::new();
        install_full_stdlib_with(&mut interp, &mut bootstrap);
        register_editor_fns(&mut interp);
        Self { interp }
    }

    /// Evaluate Lisp `src` against `host`. Returns the last form's
    /// value; any effects the program requested accumulate on
    /// `host.effects` (drain with [`EscribaHost::take_effects`]).
    pub fn eval(&mut self, src: &str, host: &mut EscribaHost) -> Result<Value, VmError> {
        let forms = tatara_lisp::read_spanned(src)?;
        Ok(self.interp.eval_program(&forms, host)?)
    }
}

/// Register the editor capability surface as native Lisp functions.
/// Reads answer from the snapshot; writes push typed effects.
fn register_editor_fns(interp: &mut Interpreter<EscribaHost>) {
    // ── writes — emit typed effects ────────────────────────────────
    interp.register_typed1(
        "message",
        |h: &mut EscribaHost, s: String| -> tatara_lisp_eval::Result<String> {
            h.effects.push(Negai::Message(s.clone()));
            Ok(s)
        },
    );
    interp.register_typed1(
        "insert",
        |h: &mut EscribaHost, s: String| -> tatara_lisp_eval::Result<()> {
            h.effects.push(Negai::InsertText(s));
            Ok(())
        },
    );
    interp.register_typed2(
        "set-option",
        |h: &mut EscribaHost, name: String, value: String| -> tatara_lisp_eval::Result<()> {
            h.effects.push(Negai::SetOption { name, value });
            Ok(())
        },
    );
    // `run-command` is variadic (name + zero-or-more string args), so it
    // uses the raw FFI rather than a fixed-arity typed registration.
    interp.register_fn(
        "run-command",
        Arity::AtLeast(1),
        |args: &[Value], h: &mut EscribaHost, span| {
            let name = value_as_string(&args[0]).ok_or_else(|| {
                tatara_lisp_eval::EvalError::native_fn(
                    "run-command",
                    "first argument must be a string command name",
                    span,
                )
            })?;
            let rest = args[1..].iter().filter_map(value_as_string).collect();
            h.effects.push(Negai::RunCommand { name, args: rest });
            Ok(Value::Nil)
        },
    );

    // ── reads — answer from the pre-eval snapshot ──────────────────
    interp.register_typed0(
        "cursor-line",
        |h: &mut EscribaHost| -> tatara_lisp_eval::Result<i64> { Ok(h.snapshot.cursor_line) },
    );
    interp.register_typed0(
        "cursor-column",
        |h: &mut EscribaHost| -> tatara_lisp_eval::Result<i64> { Ok(h.snapshot.cursor_column) },
    );
    interp.register_typed0(
        "current-line",
        |h: &mut EscribaHost| -> tatara_lisp_eval::Result<String> {
            Ok(h.snapshot.current_line.clone())
        },
    );
    interp.register_typed0(
        "editor-mode",
        |h: &mut EscribaHost| -> tatara_lisp_eval::Result<String> { Ok(h.snapshot.mode.clone()) },
    );
    interp.register_typed0(
        "buffer-name",
        |h: &mut EscribaHost| -> tatara_lisp_eval::Result<String> {
            Ok(h.snapshot.buffer_name.clone())
        },
    );
}

/// Coerce a `Value` to a `String` at the FFI boundary. Accepts `Str`
/// and `Symbol` verbatim, and stringifies `Int`/`Bool` for ergonomics
/// (so `(run-command "goto" 42)` works). Other kinds → `None`.
fn value_as_string(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) | Value::Symbol(s) => Some(s.to_string()),
        Value::Int(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_pure_arithmetic() {
        let mut vm = EscribaVm::new();
        let mut host = EscribaHost::new();
        let v = vm.eval("(+ 1 2)", &mut host).unwrap();
        assert!(matches!(v, Value::Int(3)), "got {v:?}");
        assert!(host.effects.is_empty(), "pure compute emits no effects");
    }

    #[test]
    fn full_stdlib_supports_let_binding() {
        // Beyond bare primitives: `let` + arithmetic confirms the full
        // language stdlib installed and evaluates.
        let mut vm = EscribaVm::new();
        let mut host = EscribaHost::new();
        let v = vm.eval("(let ((x 5)) (* x x))", &mut host).unwrap();
        assert!(matches!(v, Value::Int(25)), "got {v:?}");
    }

    #[test]
    fn message_emits_effect() {
        let mut vm = EscribaVm::new();
        let mut host = EscribaHost::new();
        vm.eval(r#"(message "hello from lisp")"#, &mut host)
            .unwrap();
        assert_eq!(host.effects, vec![Negai::Message("hello from lisp".into())]);
    }

    #[test]
    fn run_command_with_args_emits_effect() {
        let mut vm = EscribaVm::new();
        let mut host = EscribaHost::new();
        vm.eval(r#"(run-command "open" "README.md")"#, &mut host)
            .unwrap();
        assert_eq!(
            host.effects,
            vec![Negai::RunCommand {
                name: "open".into(),
                args: vec!["README.md".into()],
            }]
        );
    }

    #[test]
    fn set_option_and_insert_emit_effects() {
        let mut vm = EscribaVm::new();
        let mut host = EscribaHost::new();
        vm.eval(r#"(set-option "number" "true")"#, &mut host)
            .unwrap();
        vm.eval(r#"(insert "hello")"#, &mut host).unwrap();
        assert_eq!(
            host.effects,
            vec![
                Negai::SetOption {
                    name: "number".into(),
                    value: "true".into(),
                },
                Negai::InsertText("hello".into()),
            ]
        );
    }

    #[test]
    fn reads_snapshot_and_branches() {
        // Proves genuine eval: Lisp READS host state (cursor-line) and an
        // `if` chooses which effect to emit — not a static transform.
        let mut vm = EscribaVm::new();
        let mut host = EscribaHost::with_snapshot(EditorSnapshot {
            cursor_line: 7,
            ..Default::default()
        });
        vm.eval(
            r#"(if (> (cursor-line) 0) (message "below-top") (message "at-top"))"#,
            &mut host,
        )
        .unwrap();
        assert_eq!(host.effects, vec![Negai::Message("below-top".into())]);
    }

    #[test]
    fn multi_form_program_sequences_effects() {
        let mut vm = EscribaVm::new();
        let mut host = EscribaHost::new();
        vm.eval(r#"(message "first") (run-command "save")"#, &mut host)
            .unwrap();
        assert_eq!(
            host.effects,
            vec![
                Negai::Message("first".into()),
                Negai::RunCommand {
                    name: "save".into(),
                    args: vec![],
                },
            ]
        );
    }

    #[test]
    fn take_effects_drains_log() {
        let mut vm = EscribaVm::new();
        let mut host = EscribaHost::new();
        vm.eval(r#"(message "x")"#, &mut host).unwrap();
        let drained = host.take_effects();
        assert_eq!(drained.len(), 1);
        assert!(host.effects.is_empty());
    }

    #[test]
    fn read_error_surfaces_as_vm_error() {
        let mut vm = EscribaVm::new();
        let mut host = EscribaHost::new();
        let err = vm.eval("(((", &mut host).unwrap_err();
        assert!(matches!(err, VmError::Read(_)));
    }
}
