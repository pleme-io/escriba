use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const WORLD: &str = r##"use("escriba", %i[alias bind command highlight hook off option theme tune])

def escriba()
  [
    option("relativenumber", true),
    option("wrap", "true"),
    off(:defoption, "wrap"),
    theme("vellum"),
    highlight("Comment", {fg: "#616E88", italic: true}),
    alias("w2", "Write", "buffer.save"),
    bind("normal", "gh", :hello),
    command("hello", "Say hello", :hello),
    hook("BufWritePost", "hello")
  ]
end

def hello(args)
  message("hello #{buffer_name} #{length(args)}")
end
"##;

fn scratch(name: &str, source: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, source).unwrap();
    (dir, path)
}

fn escriba(init: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_escriba"))
        .env_remove("ESCRIBARC")
        .env_remove("ESCRIBA_NO_INIT")
        .env("ESCRIBA_INIT", init)
        .args(args)
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    let mut all = String::from_utf8_lossy(&out.stdout).into_owned();
    all.push_str(&String::from_utf8_lossy(&out.stderr));
    all
}

#[test]
fn a_world_reaches_the_plan_and_the_command_registry() {
    let (_d, init) = scratch("init.b", WORLD);
    let out = escriba(&init, &["--dry-run"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(
        t.contains("keybinds=1 cmds=1 options=1 theme=1 hooks=1"),
        "{t}"
    );
    assert!(t.contains("commands=1; off=1; refused=0"), "{t}");

    let listed = text(&escriba(&init, &["--commands"]));
    assert!(
        listed.contains("hello") && listed.contains("Say hello"),
        "{listed}"
    );
    assert!(listed.contains("w2"), "{listed}");
    assert!(listed.contains("blue "), "{listed}");
}

#[test]
fn a_bad_entry_is_refused_alone_and_named_in_the_editor() {
    let (_d, init) = scratch(
        "init.b",
        "use(\"escriba\", %i[bind form hook option theme])\n\ndef escriba()\n  [option(\"keep\", 1), theme(\"no-such-theme\"), bind(\"nomode\", \"x\", :y), hook(\"NoEvent\", \"x\"), form(:defnothing, \"n\", {}), form(:defoption, \"typo\", {nam: 1})]\nend\n",
    );
    let t = text(&escriba(&init, &["--dry-run"]));
    assert!(t.contains("options=1"), "{t}");
    assert!(t.contains("refused=5"), "{t}");
    assert!(t.contains("message: init.b: theme"), "{t}");
    assert!(t.contains("message: init.b: unknown_kind"), "{t}");
    assert!(t.contains("message: init.b: 5 refused"), "{t}");
}

#[test]
fn every_way_a_world_fails_is_reported_and_the_editor_still_opens() {
    let cases: [(&str, &str, &str); 8] = [
        (
            "one native call that outruns fuel",
            "def escriba()\n  xs = range(0, 100000000000000)\n  [length(xs)]\nend\n",
            "blue",
        ),
        (
            "the same call at the top",
            "xs = range(0, 100000000000000)\nmessage(to_s(length(xs)))\n",
            "blue",
        ),
        ("parse", "def escriba(\n", "init.b:"),
        ("raise at the top", "raise(:no, \"boom\")\n", "boom"),
        (
            "raise in the world",
            "def escriba()\n  raise(:no, \"boom\")\nend\n",
            "boom",
        ),
        ("a loop at the top", "while(true, 1)\n", "init.b:"),
        (
            "a loop in the world",
            "def escriba()\n  while(true, 1)\nend\n",
            "init.b:",
        ),
        (
            "recursion",
            "def f(n)\n  1 + f(n + 1)\nend\n\ndef escriba()\n  f(0)\nend\n",
            "init.b:",
        ),
    ];
    let (_b, buffer) = scratch("note.txt", "the buffer text\n");
    for (name, source, says) in cases {
        let (_d, init) = scratch("init.b", source);
        let started = Instant::now();
        let out = escriba(
            &init,
            &["--render=text", "--no-splash", buffer.to_str().unwrap()],
        );
        let t = text(&out);
        assert!(out.status.success(), "{name}: {t}");
        assert!(t.contains("he buffer text"), "{name}: {t}");
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "{name} took {:?}",
            started.elapsed()
        );
        let dry = text(&escriba(&init, &["--dry-run", buffer.to_str().unwrap()]));
        assert!(dry.contains("message: init.b:"), "{name}: {dry}");
        assert!(dry.contains(says), "{name}: {dry}");
    }
}

#[test]
fn a_missing_named_file_is_reported_and_no_file_at_all_says_nothing() {
    let missing = Path::new("/nonexistent/init.b");
    let t = text(&escriba(missing, &["--dry-run"]));
    assert!(t.contains("message: init.b:"), "{t}");

    let home = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_escriba"))
        .env_remove("ESCRIBA_INIT")
        .env_remove("ESCRIBARC")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("cfg"))
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(!text(&out).contains("init.b"), "{}", text(&out));
}

#[test]
fn the_default_path_is_under_the_config_home_and_a_relative_one_is_ignored() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".config/escriba");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("init.b"), WORLD).unwrap();
    let run = |xdg: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_escriba"))
            .env_remove("ESCRIBA_INIT")
            .env_remove("ESCRIBARC")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", xdg)
            .arg("--dry-run")
            .output()
            .unwrap();
        text(&out)
    };
    assert!(run("relative/dir").contains("refused=0"));
    assert!(!run(home.path().join("elsewhere").to_str().unwrap()).contains("blue init"));
}

#[test]
fn no_init_keeps_the_file_on_disk_out_of_the_editor() {
    let (_d, init) = scratch("init.b", WORLD);
    let t = text(&escriba(&init, &["--dry-run", "--no-init"]));
    assert!(!t.contains("blue init"), "{t}");
}

#[test]
fn blue_values_win_over_the_lisp_rc_and_the_lisp_rc_still_works() {
    let (_d, init) = scratch(
        "init.b",
        "use(\"escriba\", [:theme])\n\ndef escriba()\n  [theme(\"nord\")]\nend\n",
    );
    let (_r, rc) = scratch(
        "rc.lisp",
        "(deftheme :preset \"vellum\")\n(defoption :name \"from-lisp\" :value \"1\")\n",
    );
    let out = escriba(
        &init,
        &["--rc", rc.to_str().unwrap(), "--render=text", "--no-splash"],
    );
    assert!(out.status.success(), "{}", text(&out));
    let both = text(&escriba(
        &init,
        &["--rc", rc.to_str().unwrap(), "--dry-run"],
    ));
    assert!(both.contains("user rc"), "{both}");
    assert!(both.contains("blue init"), "{both}");
    let lisp = text(&escriba(
        &init,
        &["--rc", rc.to_str().unwrap(), "--no-init", "--dry-run"],
    ));
    assert!(
        lisp.contains("user rc") && !lisp.contains("blue init"),
        "{lisp}"
    );
}

#[test]
fn the_shipped_sample_loads_with_nothing_refused() {
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/sample-init.b");
    let t = text(&escriba(&sample, &["--dry-run"]));
    assert!(t.contains("commands=1; off=1; refused=0"), "{t}");
}

#[test]
fn eval_in_blue_runs_in_the_worker_and_a_native_runaway_is_stopped() {
    let run = |src: &str| {
        Command::new(env!("CARGO_BIN_EXE_escriba"))
            .args(["eval", "--lang", "blue", src])
            .output()
            .unwrap()
    };
    let ok = run("message(\"from the worker\")\n");
    assert!(ok.status.success(), "{}", text(&ok));
    assert!(
        text(&ok).contains("message: from the worker"),
        "{}",
        text(&ok)
    );
    let started = Instant::now();
    let bad = run("xs = range(0, 100000000000000)\nmessage(to_s(length(xs)))\n");
    assert!(!bad.status.success());
    let said = text(&bad);
    assert!(
        said.contains("stopped after") || said.contains("worker died"),
        "{said}"
    );
    assert!(started.elapsed() < Duration::from_secs(30));
}
