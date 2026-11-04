use escriba_buffer::BufferSet;
use escriba_keymap::Key;
use escriba_lisp::{FORM_KEYWORDS, apply_forms, apply_plan_to_keymap};
use escriba_runtime::EditorState;
use escriba_vm::init::serve;
use escriba_vm::{EditorSnapshot, Reply, Request};
use std::time::Instant;

fn state() -> EditorState {
    let mut buffers = BufferSet::new();
    let id = buffers.scratch("first\nsecond\n");
    EditorState::new_with_buffer(buffers, id)
}

fn booted(source: &str) -> (EditorState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("init.b");
    std::fs::write(&path, source).unwrap();
    let request = Request::Load {
        path: path.clone(),
        keywords: FORM_KEYWORDS.iter().map(|k| (*k).to_owned()).collect(),
        snapshot: EditorSnapshot::default(),
    };
    let mut load = serve(&request).unwrap_or_else(|e| Reply {
        refusals: vec![e.to_string()],
        ..Reply::default()
    });
    let (plan, refusals) = apply_forms(
        load.forms
            .iter()
            .map(|f| (f.name.as_str(), f.lisp.as_str())),
    );
    load.refusals.extend(refusals);
    let mut s = state();
    escriba_lisp::apply_plan_to_commands(&plan, &mut s.commands);
    escriba_lisp::apply_plan_to_options(&plan, &mut s.options);
    apply_plan_to_keymap(&plan, s.keymap_mut());
    s.install_blue_init(path, load);
    (s, dir)
}

fn run(s: &mut EditorState, name: &str, args: &[&str]) {
    let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    s.apply_host_effects(vec![escriba_madoguchi::Negai::RunCommand {
        name: name.to_string(),
        args,
    }]);
}

const WORLD: &str = "use(\"escriba\", %i[alias bind command option])\n\nset_option(\"at_top\", \"yes\")\n\ndef escriba()\n  [option(\"from_world\", 7), command(\"hello\", \"Say hello\", :greet), alias(\"hi\", \"Alias\", \"hello\"), bind(\"normal\", \"gh\", :hello)]\nend\n\ndef greet(args)\n  message(\"hello #{join(args, \"+\")} at #{cursor_line}\")\n  set_option(\"greeted\", \"yes\")\nend\n\ndef spin(args)\n  while(true, 1)\nend\n\ndef boom(args)\n  raise(:no, \"nope\")\nend\n\ndef helper()\n  \"from init\"\nend\n";

#[test]
fn a_top_level_effect_and_a_world_option_both_apply() {
    let (s, _d) = booted(WORLD);
    assert_eq!(s.options.get("at_top").map(String::as_str), Some("yes"));
    assert_eq!(s.options.get("from_world").map(String::as_str), Some("7"));
    assert!(s.messages.is_empty(), "{:?}", s.messages);
}

#[test]
fn a_blue_command_runs_with_its_arguments_and_reads_the_editor() {
    let (mut s, _d) = booted(WORLD);
    run(&mut s, "hello", &["a", "b"]);
    assert_eq!(s.messages.last().unwrap(), "hello a+b at 0");
    assert_eq!(s.options.get("greeted").map(String::as_str), Some("yes"));
}

#[test]
fn a_lisp_alias_reaches_a_blue_command() {
    let (mut s, _d) = booted(WORLD);
    run(&mut s, "hi", &[]);
    assert_eq!(s.messages.last().unwrap(), "hello  at 0");
}

#[test]
fn a_blue_binding_reaches_a_blue_command_through_the_keymap() {
    let (mut s, _d) = booted(WORLD);
    s.on_key(&Key::Char('g'));
    s.on_key(&Key::Char('h'));
    assert_eq!(s.messages.last().unwrap(), "hello  at 0");
}

#[test]
fn the_ex_line_runs_blue_with_its_spaces_kept() {
    let (mut s, _d) = booted(WORLD);
    s.on_key(&Key::Char(':'));
    for c in "blue message(\"a  b\")".chars() {
        s.on_key(&Key::Char(c));
    }
    s.on_key(&Key::Enter);
    assert_eq!(s.messages.last().unwrap(), "a  b");
}

#[test]
fn a_runaway_command_stops_at_its_budget_and_the_editor_goes_on() {
    let (mut s, _d) = booted(
        "use(\"escriba\", [:command])\n\ndef escriba()\n  [command(\"spin\", \"\", :spin), command(\"boom\", \"\", :boom)]\nend\n\ndef spin(args)\n  while(true, 1)\nend\n\ndef boom(args)\n  raise(:no, \"nope\")\nend\n",
    );
    let started = Instant::now();
    run(&mut s, "spin", &[]);
    assert!(started.elapsed().as_secs() < 30, "{:?}", started.elapsed());
    assert!(
        s.messages.last().unwrap().contains("spin"),
        "{:?}",
        s.messages
    );
    run(&mut s, "boom", &[]);
    assert!(
        s.messages.last().unwrap().contains("nope"),
        "{:?}",
        s.messages
    );
    run(&mut s, "boom", &[]);
    assert_eq!(s.messages.len(), 3);
}

#[test]
fn the_blue_command_runs_a_line_with_the_world_in_scope_and_no_repeat_of_its_top() {
    let (mut s, _d) = booted(WORLD);
    s.options.remove("at_top");
    run(&mut s, "blue", &["message(helper)"]);
    assert_eq!(s.messages.last().unwrap(), "from init");
    assert_eq!(s.options.get("at_top"), None);
}

#[test]
fn a_bad_line_is_a_message_and_no_line_is_usage() {
    let (mut s, _d) = booted(WORLD);
    run(&mut s, "blue", &["message("]);
    assert!(!s.messages.is_empty());
    let before = s.messages.len();
    run(&mut s, "blue", &["while(true, 1)"]);
    assert_eq!(s.messages.len(), before + 1);
    run(&mut s, "blue", &[]);
    assert!(
        s.messages.last().unwrap().starts_with("usage"),
        "{:?}",
        s.messages
    );
}

#[test]
fn the_blue_line_works_with_no_world_file() {
    let mut s = state();
    run(&mut s, "blue", &["message(\"plain\")"]);
    assert_eq!(s.messages.last().unwrap(), "plain");
}

#[test]
fn a_command_naming_no_function_is_refused_alone() {
    let (s, _d) = booted(
        "use(\"escriba\", %i[command option])\n\ndef escriba()\n  [command(\"gone\", \"\", :nothing), option(\"kept\", 1)]\nend\n",
    );
    assert_eq!(s.options.get("kept").map(String::as_str), Some("1"));
    assert!(s.messages[0].contains("gone"), "{:?}", s.messages);
    assert!(!s.commands.contains("gone"));
}

#[test]
fn every_plan_field_has_its_keyword_and_each_form_is_read() {
    let plan = escriba_lisp::ApplyPlan::default();
    assert_eq!(plan.counts().len(), FORM_KEYWORDS.len());
    let cases = [
        "(defkeybind :mode \"normal\" :key \"x\" :action \"y\")",
        "(defcmd :name \"n\" :action \"a\")",
        "(defoption :name \"n\" :value \"v\")",
        "(deftheme :preset \"nord\")",
        "(defhighlight :group \"Comment\" :fg \"#000000\")",
    ];
    for form in cases {
        let (plan, refused) = apply_forms([("x", form)]);
        assert!(refused.is_empty(), "{form}: {refused:?}");
        assert!(plan.counts().iter().any(|(_, n)| *n > 0), "{form}");
    }
    let (_, refused) = apply_forms([("x", "(defnothing :a 1)")]);
    assert_eq!(refused.len(), 1);
}

#[test]
fn a_workflow_naming_an_undeclared_tool_is_refused_alone() {
    let wf = "(defworkflow :name \"w\" :steps (\"mcp:s.t\"))";
    let tool = "(defmcp :name \"t\" :server \"s\" :tool \"t\")";
    let (plan, refused) = apply_forms([("w", wf), ("o", "(defoption :name \"n\" :value \"v\")")]);
    assert_eq!(plan.workflows.len(), 0);
    assert_eq!(plan.options.len(), 1);
    assert_eq!(refused.len(), 1, "{refused:?}");
    let (plan, refused) = apply_forms([("w", wf), ("t", tool)]);
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(plan.workflows.len(), 1);
    assert!(escriba_lisp::apply_source(wf).is_err());
}
