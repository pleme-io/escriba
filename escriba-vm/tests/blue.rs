use blue_lang_host::{Error, Host};
use escriba_madoguchi::Negai;
use escriba_vm::{EditorSnapshot, EscribaHost, EscribaVm, ScriptLanguage, VmError, eval_blue};

const SURFACE: [&str; 9] = [
    "message",
    "insert",
    "set_option",
    "run_command",
    "cursor_line",
    "cursor_column",
    "current_line",
    "editor_mode",
    "buffer_name",
];

fn run(src: &str) -> EscribaHost {
    let mut host = EscribaHost::new();
    eval_blue(src, &mut host).unwrap();
    host
}

#[test]
fn the_blue_surface_is_exactly_the_editor_capabilities() {
    let names: Vec<&str> = EscribaHost::SURFACE.iter().map(|f| f.name).collect();
    assert_eq!(names, SURFACE);
    assert!(EscribaHost::SURFACE.iter().all(|f| !f.doc.is_empty()));
}

#[test]
fn every_blue_capability_has_its_lisp_twin() {
    let vm = EscribaVm::new();
    for f in EscribaHost::SURFACE {
        let lisp = f.name.replace('_', "-");
        assert!(vm.binds(&lisp), "{} has no lisp twin {lisp}", f.name);
    }
}

#[test]
fn pure_compute_emits_no_effects() {
    assert!(run("x = 1 + 2\nx * x\n").effects.is_empty());
}

#[test]
fn message_emits_effect() {
    assert_eq!(
        run("message(\"hello from blue\")\n").effects,
        vec![Negai::Message("hello from blue".into())]
    );
}

#[test]
fn run_command_with_args_emits_effect() {
    assert_eq!(
        run("run_command(\"open\", [\"README.md\"])\n").effects,
        vec![Negai::RunCommand {
            name: "open".into(),
            args: vec!["README.md".into()],
        }]
    );
}

#[test]
fn set_option_and_insert_emit_effects_in_order() {
    assert_eq!(
        run("set_option(\"number\", \"true\")\ninsert(\"hello\")\n").effects,
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
    let mut host = EscribaHost::with_snapshot(EditorSnapshot {
        cursor_line: 7,
        current_line: "fn main".into(),
        mode: "normal".into(),
        ..EditorSnapshot::default()
    });
    let src = "\
if cursor_line() > 0
  message(\"below-top\")
else
  message(\"at-top\")
end
message(\"#{editor_mode()}: #{current_line()}\")
";
    eval_blue(src, &mut host).unwrap();
    assert_eq!(
        host.effects,
        vec![
            Negai::Message("below-top".into()),
            Negai::Message("normal: fn main".into()),
        ]
    );
}

#[test]
fn definitions_within_one_script_are_callable() {
    let src = "\
def greet(who)
  message(\"hi #{who}\")
end
greet(buffer_name())
";
    let mut host = EscribaHost::with_snapshot(EditorSnapshot {
        buffer_name: "[scratch]".into(),
        ..EditorSnapshot::default()
    });
    eval_blue(src, &mut host).unwrap();
    assert_eq!(host.effects, vec![Negai::Message("hi [scratch]".into())]);
}

#[test]
fn a_parse_error_is_a_load_error_and_runs_nothing() {
    let mut host = EscribaHost::new();
    let err = eval_blue("message(\"x\")\nif\n", &mut host).unwrap_err();
    assert!(matches!(err, VmError::Blue(Error::Load(_))), "{err}");
    assert!(host.effects.is_empty());
}

#[test]
fn an_unknown_capability_is_refused_before_anything_runs() {
    let mut host = EscribaHost::new();
    let err = eval_blue("message(\"x\")\nsave_buffer()\n", &mut host).unwrap_err();
    assert!(matches!(err, VmError::Blue(Error::Load(_))), "{err}");
    assert!(host.effects.is_empty());
}

#[test]
fn a_wrong_argument_names_the_capability_and_parameter() {
    let mut host = EscribaHost::new();
    let err = eval_blue("run_command(\"open\", \"README.md\")\n", &mut host)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("run_command") && err.contains("(args)"),
        "{err}"
    );
}

#[test]
fn script_language_round_trips_through_its_name() {
    for lang in ScriptLanguage::ALL {
        assert_eq!(lang.as_str().parse::<ScriptLanguage>(), Ok(lang));
    }
    assert!("ruby".parse::<ScriptLanguage>().is_err());
    assert_eq!(ScriptLanguage::default(), ScriptLanguage::Lisp);
}
