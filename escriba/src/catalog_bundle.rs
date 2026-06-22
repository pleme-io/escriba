//! Baked plugin catalog — the blnvim-parity plugin caixas, compiled
//! into the binary so they are **installed by default by construction**.
//!
//! Each entry's source is the catalog `.escribaplugin.lisp` file
//! (`include_str!`), so the default escriba boots with the full plugin
//! set applied — no on-disk install, no network, no plugins dir
//! required. `escriba plugin forge` / `install-bundled` materialize
//! them to disk for users who want to fork or extend a plugin; the
//! escribamourne distribution consumes them as `flake = false` source.
//!
//! **Bundled = eager.** The default set is the "fully loaded" config:
//! every catalog entry's def-forms (keybinds / commands / options /
//! highlights / lsp servers / formatters / text-objects / folds / …)
//! is merged into the default plan at boot, so the editor is fully
//! configured immediately. Per-plugin lazy ACTIVATION (deferring a
//! plugin until its trigger fires) is the runtime path for USER plugins
//! in the plugins dir (see `escriba_runtime::PluginHost`); the bundled
//! defaults are always-on.
//!
//! ## Catalog reflection
//!
//! [`BUNDLED`] is the typed manifest of the catalog. The matrix test
//! (`tests/plugin_matrix.rs`) walks the `catalog/` directory and fails
//! the build if a `.escribaplugin.lisp` file exists with no [`BUNDLED`]
//! row (or vice-versa) — so the table can never silently drift from the
//! files on disk.

use anyhow::{Context, Result};
use escriba_lisp::{ApplyPlan, EscribaPluginSpec};

/// One baked plugin: its caixa name + its catalog source.
pub struct BundledPlugin {
    pub name: &'static str,
    pub source: &'static str,
}

/// Build the [`BUNDLED`] table: one entry per catalog file, where the
/// caixa name and the catalog filename stem are the same literal. The
/// `concat! + include_str!` runs in const context, baking each source.
macro_rules! bundled {
    ($($name:literal),* $(,)?) => {
        pub const BUNDLED: &[BundledPlugin] = &[
            $(
                BundledPlugin {
                    name: $name,
                    source: include_str!(concat!("../catalog/", $name, ".escribaplugin.lisp")),
                }
            ),*
        ];
    };
}

bundled! {
    // ── common ────────────────────────────────────────────────────
    "escriba-which-key",
    "escriba-comment",
    "escriba-todo-comments",
    "escriba-surround",
    "escriba-autopairs",
    "escriba-leap",
    "escriba-overseer",
    // ── files ─────────────────────────────────────────────────────
    "escriba-oil",
    "escriba-tree",
    // ── git ───────────────────────────────────────────────────────
    "escriba-gitsigns",
    "escriba-fugitive",
    "escriba-git-conflict",
    // ── lsp ───────────────────────────────────────────────────────
    "escriba-lspconfig",
    "escriba-lspsaga",
    "escriba-trouble",
    "escriba-lsp-signature",
    "escriba-tiny-inline-diagnostic",
    "escriba-illuminate",
    "escriba-mason",
    "escriba-conform",
    "escriba-dap",
    "escriba-neotest",
    "escriba-helm",
    // ── completion ────────────────────────────────────────────────
    "escriba-cmp",
    "escriba-lspkind",
    "escriba-luasnip",
    // ── telescope ─────────────────────────────────────────────────
    "escriba-telescope",
    "escriba-compass",
    // ── treesitter ────────────────────────────────────────────────
    "escriba-treesitter",
    "escriba-treesitter-textobjects",
    "escriba-treesitter-fold",
    "escriba-treesitter-context",
    "escriba-ts-autotag",
    "escriba-ts-context-commentstring",
    "escriba-render-markdown",
    // ── theming ───────────────────────────────────────────────────
    "escriba-nord",
    "escriba-lualine",
    "escriba-bufferline",
    "escriba-noice",
    "escriba-snacks",
    "escriba-notify",
    "escriba-indent-blankline",
    "escriba-colorizer",
    "escriba-devicons",
    // ── ai ────────────────────────────────────────────────────────
    "escriba-mcp-assist",
}

/// The composite plan of every bundled plugin's escriba entry, plus a
/// generated `(defplugin …)` descriptor per plugin (so `plugin list`
/// and the plan's plugin count reflect the catalog). Merged on top of
/// the baseline rc at boot.
pub fn bundled_plan() -> Result<ApplyPlan> {
    bundled_plan_excluding(&std::collections::HashSet::new())
}

/// [`bundled_plan`] with a set of plugin names to SKIP — the per-plugin
/// toggle surface. A name in `disabled` is omitted entirely (neither its
/// entry nor its descriptor is applied), so the HM module's
/// `programs.escriba.plugins.<name>.enable = false` removes a default
/// plugin's keybinds / commands / highlights cleanly. The binary reads
/// the disabled set from `$ESCRIBA_DISABLED_PLUGINS` (comma-separated).
pub fn bundled_plan_excluding(disabled: &std::collections::HashSet<String>) -> Result<ApplyPlan> {
    let mut plan = ApplyPlan::default();
    for bpl in BUNDLED {
        if disabled.contains(bpl.name) {
            continue;
        }
        let entry = escriba_lisp::apply_source(bpl.source)
            .with_context(|| format!("applying bundled plugin `{}` entry", bpl.name))?;
        plan.merge(entry);
        let spec = escriba_lisp::read_catalog_meta(bpl.source)
            .with_context(|| format!("reading bundled plugin `{}` manifest", bpl.name))?;
        let descriptor = escriba_lisp::emit_defplugin_descriptor(&spec);
        let desc_plan = escriba_lisp::apply_source(&descriptor)
            .with_context(|| format!("applying generated descriptor for `{}`", bpl.name))?;
        plan.merge(desc_plan);
    }
    Ok(plan)
}

/// Parse the `$ESCRIBA_DISABLED_PLUGINS` env var (comma/space separated
/// caixa names) into a set. Empty / unset ⇒ no plugins disabled.
#[must_use]
pub fn disabled_from_env() -> std::collections::HashSet<String> {
    std::env::var("ESCRIBA_DISABLED_PLUGINS")
        .unwrap_or_default()
        .split([',', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Every bundled plugin's typed manifest — for `escriba plugin list`.
pub fn bundled_specs() -> Result<Vec<EscribaPluginSpec>> {
    BUNDLED
        .iter()
        .map(|bpl| {
            escriba_lisp::read_catalog_meta(bpl.source)
                .with_context(|| format!("reading bundled plugin `{}` manifest", bpl.name))
        })
        .collect()
}
