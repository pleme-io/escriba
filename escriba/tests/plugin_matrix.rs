//! Plugin catalog verification matrix — the forcing function that keeps
//! the blnvim-parity plugin substrate honest (★★ CLOSED-LOOP
//! MASS-SYNTHESIS rule #1 + ★★ CATALOG REFLECTION).
//!
//! Every `catalog/<name>.escribaplugin.lisp` source must:
//!   1. forge into a re-parseable `caixa.lisp` + an applying entry;
//!   2. carry a manifest whose `:name` matches its filename stem;
//!   3. use a canonical category;
//!   4. declare only parseable `:ativar-em` triggers;
//!   5. appear in the binary's baked [`BUNDLED`] table — and the table
//!      must have NO row without a file (dir ↔ table bijection).
//!
//! Failures aggregate before the assert so one run reports EVERY broken
//! plugin, not just the first. A new catalog file with no `BUNDLED` row
//! (or a row with no file) fails the build — the table can never
//! silently drift from the directory.

use std::collections::BTreeSet;
use std::path::PathBuf;

use escriba::catalog_bundle::BUNDLED;

/// The minimum size of the parity catalog. Guards against an accidental
/// mass-deletion silently passing the per-file checks.
const MIN_PLUGINS: usize = 45;

fn catalog_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("catalog")
}

/// `(filename_stem, source)` for every catalog file on disk.
fn catalog_files() -> Vec<(String, String)> {
    let dir = catalog_dir();
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("catalog dir exists") {
        let path = entry.unwrap().path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if let Some(stem) = name.strip_suffix(".escribaplugin.lisp") {
            let src = std::fs::read_to_string(&path).expect("read catalog source");
            out.push((stem.to_string(), src));
        }
    }
    out.sort();
    out
}

#[test]
fn every_catalog_plugin_forges_and_applies() {
    let mut failures: Vec<String> = Vec::new();

    for (stem, src) in catalog_files() {
        // 1+2. Forge + name matches filename.
        match escriba_plugin::forge_plugin(&src) {
            Ok((spec, art)) => {
                if spec.name != stem {
                    failures.push(format!(
                        "{stem}: manifest :name `{}` != filename stem",
                        spec.name
                    ));
                }
                // caixa.lisp re-parses.
                if tatara_lisp::read(&art.caixa_lisp).is_err() {
                    failures.push(format!("{stem}: generated caixa.lisp does not re-parse"));
                }
                // 3. category canonical.
                if !escriba_lisp::category_is_canonical(&spec) {
                    failures.push(format!(
                        "{stem}: non-canonical category `{}`",
                        spec.category
                    ));
                }
                // 4. triggers parse.
                for t in &spec.ativar_em {
                    if escriba_plugin::ActivationTrigger::parse(t).is_none() {
                        failures.push(format!("{stem}: unparseable :ativar-em trigger `{t}`"));
                    }
                }
                // entry applies (the meta form is ignored by apply_source).
                if let Err(e) = escriba_lisp::apply_source(&art.entry_lisp) {
                    failures.push(format!("{stem}: entry does not apply: {e}"));
                }
            }
            Err(e) => failures.push(format!("{stem}: forge failed: {e}")),
        }
    }

    assert!(
        failures.is_empty(),
        "{} catalog plugin(s) failed:\n  - {}",
        failures.len(),
        failures.join("\n  - "),
    );
}

#[test]
fn catalog_dir_and_bundled_table_are_a_bijection() {
    let on_disk: BTreeSet<String> = catalog_files().into_iter().map(|(s, _)| s).collect();
    let in_table: BTreeSet<String> =
        BUNDLED.iter().map(|b| b.name.to_string()).collect();

    let missing_row: Vec<_> = on_disk.difference(&in_table).cloned().collect();
    let missing_file: Vec<_> = in_table.difference(&on_disk).cloned().collect();

    assert!(
        missing_row.is_empty(),
        "catalog files with NO BUNDLED row (add them to catalog_bundle.rs): {missing_row:?}",
    );
    assert!(
        missing_file.is_empty(),
        "BUNDLED rows with NO catalog file (stale rows): {missing_file:?}",
    );
}

#[test]
fn catalog_covers_the_parity_set() {
    let n = catalog_files().len();
    assert!(
        n >= MIN_PLUGINS,
        "catalog has {n} plugins, expected at least {MIN_PLUGINS} for blnvim parity",
    );
    assert_eq!(n, BUNDLED.len(), "every catalog file is baked exactly once");
}

/// The DEFAULT BOOT IS SILENT. `escriba` with no user config applies
/// the baked `blnvim-defaults.lisp` plus the whole bundled catalog; a
/// warning there is escriba complaining about its OWN shipped content,
/// and it is never the operator's to fix.
///
/// This is a forcing function, not a snapshot: it failed when written.
/// The catalog shipped `<C-Space>` (escriba-cmp) and `<S-h>`/`<S-l>`
/// (escriba-bufferline) — three bindings the key parser rejected, so
/// every boot logged three warnings AND silently dropped the bindings.
/// `every_catalog_plugin_forges_and_applies` could not catch it: these
/// surface as `ApplyReport::warnings`, never as an `Err`.
#[test]
fn the_default_boot_applies_without_a_single_warning() {
    let mut failures: Vec<String> = Vec::new();

    // Half 1 — the baked default rc.
    let rc_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("configs/blnvim-defaults.lisp");
    let rc_src = std::fs::read_to_string(&rc_path).expect("default rc readable");
    match escriba_lisp::apply_source(&rc_src) {
        Ok(plan) => {
            let mut keymap = escriba_keymap::Keymap::default_vim();
            let report = escriba_lisp::apply_plan_to_keymap(&plan, &mut keymap);
            for w in report.warnings {
                failures.push(format!("blnvim-defaults.lisp: {w}"));
            }
        }
        Err(e) => failures.push(format!("blnvim-defaults.lisp does not apply: {e}")),
    }

    // Half 2 — the bundled plugin catalog, as one composite plan (the
    // shape the binary actually boots).
    match escriba::catalog_bundle::bundled_plan() {
        Ok(plan) => {
            let mut keymap = escriba_keymap::Keymap::default_vim();
            let report = escriba_lisp::apply_plan_to_keymap(&plan, &mut keymap);
            for w in report.warnings {
                failures.push(format!("bundled catalog: {w}"));
            }
        }
        Err(e) => failures.push(format!("bundled plan does not build: {e}")),
    }

    assert!(
        failures.is_empty(),
        "the default boot emitted {} warning(s) — escriba's own shipped \
         config must apply cleanly:\n  - {}",
        failures.len(),
        failures.join("\n  - "),
    );
}

#[test]
fn bundled_composite_preserves_full_capability() {
    // The migration invariant: moving plugin-owned forms out of the
    // monolithic defaults into caixas must LOSE nothing. The composite
    // bundled plan must still carry the full LSP / formatter / text-object
    // surface the old defaults had.
    let plan = escriba::catalog_bundle::bundled_plan().expect("bundled plan builds");
    assert!(plan.lsp_servers.len() >= 11, "lsp servers: {}", plan.lsp_servers.len());
    assert!(plan.formatters.len() >= 11, "formatters: {}", plan.formatters.len());
    assert!(plan.text_objects.len() >= 17, "text objects: {}", plan.text_objects.len());
    assert!(plan.dap_adapters.len() >= 5, "dap adapters: {}", plan.dap_adapters.len());
    assert!(plan.folds.len() >= 5, "folds: {}", plan.folds.len());
    assert!(plan.icons.len() >= 20, "icons: {}", plan.icons.len());
    assert!(plan.mcp_tools.len() >= 5, "mcp tools: {}", plan.mcp_tools.len());
    // A defplugin descriptor per bundled plugin.
    assert_eq!(plan.plugins.len(), BUNDLED.len(), "one descriptor per plugin");
}
