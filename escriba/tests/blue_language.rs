//! blue is a first-class edited language — the closed leg matrix.
//!
//! Supporting a language in escriba is not one change, it is five, spread
//! across three files and two crates: a highlighter plugin, a major mode, a
//! language server, a formatter, and a canonicality gate. Four of the five are
//! Lisp data, so a delete or a rename is a quiet diff that nothing else
//! notices. This file is the forcing function (★★ CLOSED-LOOP MASS-SYNTHESIS
//! rule #1): drop ANY leg and the build goes red, naming the leg.
//!
//! The last test is the one that earns its keep — the legs are joined by a
//! bare string, `"blue"`, repeated in four places with no type connecting
//! them. It asserts they still agree, so renaming the major mode without
//! renaming the formatter's `:filetype` cannot pass.
//!
//! ## What this file does NOT claim
//!
//! Only the highlighting leg is LIVE. `lsp_servers` / `formatters` / `gates`
//! have no consumer in `escriba-runtime` — nothing spawns a server, runs a
//! formatter, or fires a gate for ANY language yet, blue included. These
//! assertions prove the declarations are present and coherent in the boot
//! plan, which is what a declaration-tier leg can prove; they do not prove a
//! `.b` buffer gets formatted on save, and must not be read that way.

use escriba_lisp::ApplyPlan;

/// The filetype name every leg has to agree on.
const BLUE: &str = "blue";

/// The plan the editor actually boots with — baseline rc + bundled catalog.
fn boot_plan() -> ApplyPlan {
    escriba::default_plan(false).expect("the default boot plan builds")
}

#[test]
fn render_registry_resolves_blue_sources() {
    // The LIVE leg: the registry the GPU renderer holds, asked the same
    // question the render loop asks it.
    let eco = escriba_render::gpu::build_ecosystem();
    assert_eq!(
        eco.resolve("scratch.b"),
        escriba_render::langs::BLUE,
        "`.b` must resolve to blue — escriba_render::langs registers it",
    );
    assert_eq!(
        eco.resolve("Bluefile"),
        escriba_render::langs::BLUE,
        "a Bluefile is a blue program and has no extension to match on",
    );
}

#[test]
fn boot_plan_declares_the_blue_major_mode() {
    let plan = boot_plan();
    let mode = plan
        .major_modes
        .iter()
        .find(|m| m.name == BLUE)
        .expect("blnvim-defaults.lisp must carry `(defmode :name \"blue\" …)`");

    assert!(
        mode.extensions.iter().any(|e| e == "b"),
        "blue's extension is `.b`: {:?}",
        mode.extensions,
    );
    assert_eq!(mode.commentstring, "# %s", "blue comments are `#` to EOL");
    // Deliberately empty: there is no tree-sitter-blue grammar, and naming a
    // fictional one would make `apply_grammars` count `.b` as skipped for an
    // unknown language. Highlighting comes from the hikari table plugin.
    assert!(
        mode.tree_sitter.is_empty(),
        "blue must claim no tree-sitter grammar — none exists; got {:?}",
        mode.tree_sitter,
    );
}

#[test]
fn boot_plan_binds_the_blue_language_server() {
    let plan = boot_plan();
    let lsp = plan
        .lsp_servers
        .iter()
        .find(|s| s.filetypes.iter().any(|f| f == BLUE))
        .expect("escriba-lspconfig must carry a `(deflsp …)` for blue");

    // `blue lsp` is a subcommand of the one blue binary, resolved off $PATH —
    // escriba links no blue crate. If this ever becomes a standalone
    // `blue-language-server`, this is the line that has to change.
    assert_eq!(lsp.command, "blue");
    assert_eq!(lsp.args, vec!["lsp"]);
    assert!(
        lsp.root_markers.iter().any(|m| m == "Bluefile"),
        "a Bluefile roots a blue project: {:?}",
        lsp.root_markers,
    );
    assert!(!lsp.manual_only, "blue should auto-attach like its peers");
}

#[test]
fn boot_plan_binds_the_blue_formatter() {
    let plan = boot_plan();
    let fmt = plan
        .formatters
        .iter()
        .find(|f| f.filetype == BLUE)
        .expect("escriba-conform must carry a `(defformatter :filetype \"blue\" …)`");

    assert_eq!(fmt.command, "blue");
    // `--write` is load-bearing, not decoration: bare `blue fmt FILE` prints
    // to stdout and leaves the file alone, so dropping the flag would format
    // nothing and report success.
    assert!(
        fmt.args.iter().any(|a| a == "--write"),
        "`blue fmt` without --write is a no-op filter: {:?}",
        fmt.args,
    );
    assert!(
        fmt.args.iter().any(|a| a == "$FILE"),
        "`blue fmt` takes a FILE, it does not read stdin: {:?}",
        fmt.args,
    );
}

#[test]
fn boot_plan_gates_blue_on_its_one_formatting() {
    let plan = boot_plan();
    let gate = plan
        .gates
        .iter()
        .find(|g| g.filetype == BLUE)
        .expect("escriba-conform must carry the blue-canonical gate");

    assert_eq!(gate.on_event, "BufWritePre");
    assert_eq!(gate.action, "auto-fix");
    assert!(
        gate.command.contains("--check"),
        "the check half must be `--check`, not a rewrite: {:?}",
        gate.command,
    );
    assert!(
        gate.auto_fix.contains("--write"),
        "the repair half must actually write: {:?}",
        gate.auto_fix,
    );
}

#[test]
fn every_blue_leg_agrees_on_the_filetype_name() {
    // The legs are joined by a bare string with no type behind it. This is
    // the test that makes the join real: rename the major mode and the
    // formatter, server and gate stop matching it, right here.
    let plan = boot_plan();

    let mode = plan
        .major_modes
        .iter()
        .find(|m| m.name == BLUE)
        .expect("blue major mode");

    let mut orphans: Vec<&str> = Vec::new();
    if !plan
        .lsp_servers
        .iter()
        .any(|s| s.filetypes.iter().any(|f| *f == mode.name))
    {
        orphans.push("deflsp");
    }
    if !plan.formatters.iter().any(|f| f.filetype == mode.name) {
        orphans.push("defformatter");
    }
    if !plan.gates.iter().any(|g| g.filetype == mode.name) {
        orphans.push("defgate");
    }

    assert!(
        orphans.is_empty(),
        "these legs no longer reference the `{}` major mode: {:?}",
        mode.name,
        orphans,
    );

    // And the render registry has to claim the same extension the mode
    // declares — the one place the Rust half and the Lisp half touch.
    let eco = escriba_render::gpu::build_ecosystem();
    for ext in &mode.extensions {
        let probe = ["probe.", ext].concat();
        assert_eq!(
            eco.resolve(&probe),
            escriba_render::langs::BLUE,
            "`(defmode :name \"blue\")` claims `.{ext}` but the render \
             registry does not — escriba_render::langs::BLUE_SELECTORS is \
             out of step with blnvim-defaults.lisp",
        );
    }
}
