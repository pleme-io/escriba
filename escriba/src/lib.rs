//! `escriba` — editor library + binary entry point.
//!
//! TUI (ratatui + crossterm) is the default render — escriba runs
//! inside your terminal, nvim-style. `--render=gpu` opens a
//! madori + garasu window; `--render=text` emits a one-shot ANSI
//! snapshot (CI / headless). `$ESCRIBA_RENDER` overrides the
//! default so home-manager can pick per-node.
//!
//! Every escriba-* crate composes here; the [`run`] function is
//! the shared entry point both `escriba` and `escriba-gpu`
//! binaries call into.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use escriba_buffer::BufferSet;
use escriba_command::CommandRegistry;
use escriba_core::Position;
use escriba_keymap::Keymap;
use escriba_render::{GpuRenderer, Renderer, TextRenderer};
use escriba_runtime::EditorState;
use madori::App;

pub mod catalog_bundle;

/// The "blnvim-parity" BASELINE rc, baked into the binary. Carries the
/// non-plugin substrate (theme, options, leader, major-modes, base
/// highlights, escriba inventions). The plugin caixas in
/// [`catalog_bundle`] are merged on top. Users get both unless they
/// pass `--no-defaults`. A user rc (via `--rc` / `$ESCRIBARC`) merges
/// last — user declarations win per plan-merge semantics.
const DEFAULT_RC: &str = include_str!("../configs/blnvim-defaults.lisp");

/// The composite default plan: the baseline rc plus every bundled
/// plugin caixa's escriba entry (and a generated `defplugin` descriptor
/// per plugin). The single source of truth for "what escriba boots with
/// by default" — the editor boot path, `--commands`, `--list-rc`, and
/// `plugin list` all build from here so they never drift. `no_defaults`
/// yields an empty plan (the bare `--no-defaults` editor).
fn default_plan(no_defaults: bool) -> Result<escriba_lisp::ApplyPlan> {
    if no_defaults {
        return Ok(escriba_lisp::ApplyPlan::default());
    }
    let mut plan =
        escriba_lisp::apply_source(DEFAULT_RC).context("parsing baseline blnvim-defaults")?;
    // Honor per-plugin toggles via `$ESCRIBA_DISABLED_PLUGINS` (the HM
    // module's `programs.escriba.plugins.<name>.enable = false`).
    let disabled = catalog_bundle::disabled_from_env();
    plan.merge(
        catalog_bundle::bundled_plan_excluding(&disabled)
            .context("applying bundled plugin catalog")?,
    );
    Ok(plan)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum RenderMode {
    /// Open a GPU window via madori + garasu.
    Gpu,
    /// Interactive TUI via ratatui + crossterm — works over SSH, inside ghostty.
    Tui,
    /// Render once to stdout as ANSI-colored text and exit.
    Text,
}

#[derive(Parser)]
#[command(name = "escriba", version, about = "the Rust + tatara-lisp editor")]
struct Args {
    /// Path to open. If omitted, a scratch buffer is used.
    file: Option<PathBuf>,
    /// Dump the OpenAPI 3.1 spec and exit.
    #[arg(long)]
    spec: bool,
    /// When used with --spec, emit YAML instead of JSON.
    #[arg(long)]
    yaml: bool,
    /// List every registered command and exit.
    #[arg(long)]
    commands: bool,
    /// List every default keybinding and exit.
    #[arg(long)]
    keymap: bool,
    /// Compile config / open buffer / print summary; do not render.
    #[arg(long)]
    dry_run: bool,
    /// Path to the Tatara-Lisp rc file. Falls back to `$ESCRIBARC`,
    /// then `$XDG_CONFIG_HOME/escriba/rc.lisp`, then
    /// `$HOME/.escribarc.lisp`. Missing files are silently skipped;
    /// parse errors fail fast.
    #[arg(long, env = "ESCRIBARC")]
    rc: Option<PathBuf>,
    /// Parse the rc file, print the apply-plan summary, and exit.
    /// Mirrors `frost --doctor` — useful for CI / config validation.
    #[arg(long)]
    list_rc: bool,
    /// Skip the bundled blnvim-parity defaults. By default, escriba
    /// boots with the same plugin/keybinding/theme surface area as
    /// `pleme-io/blackmatter-nvim`; `--no-defaults` yields a bare
    /// vim-ish editor with only the user rc applied. Also honoured
    /// via `ESCRIBA_NO_DEFAULTS=1` so home-manager can toggle it.
    #[arg(long, env = "ESCRIBA_NO_DEFAULTS")]
    no_defaults: bool,
    /// Render backend. Default is `tui` so `escriba` behaves like
    /// nvim — runs inside your terminal. `gpu` opens a madori+garasu
    /// window; `text` emits a one-shot ANSI dump (for CI / headless).
    /// Honours `$ESCRIBA_RENDER` so HM can set a per-node default.
    #[arg(long, value_enum, env = "ESCRIBA_RENDER", default_value_t = RenderMode::Tui)]
    render: RenderMode,
    /// Viewport height in lines for the text renderer.
    #[arg(long, default_value_t = 20)]
    height: u32,
    /// Window title.
    #[arg(long, default_value = "escriba")]
    title: String,
    /// Initial window width.
    #[arg(long, default_value_t = 1200)]
    width: u32,
    /// Initial window height (pixels).
    #[arg(long, default_value_t = 800)]
    win_height: u32,
    /// Operator subcommand. When set, dispatches to the subcommand
    /// instead of opening the editor. Today the only subcommand is
    /// `config-show`, the shikumi-canonical tier-model inspection
    /// surface every TieredConfig consumer ships.
    #[command(subcommand)]
    command: Option<EscribaCommand>,
}

/// Operator-facing subcommands. Optional — when omitted, escriba boots
/// the editor as before. Pillar 12: the `ConfigShow` variant is the
/// reusable shikumi factory; new operator surfaces land here, not
/// scattered across one-off `--flag` knobs.
#[derive(Subcommand)]
enum EscribaCommand {
    /// Show the materialized config at a tier (bare/default/discovered/custom/env).
    ConfigShow(shikumi::cli::ConfigShowCommand),
    /// Evaluate tatara-lisp source against a fresh editor and print the
    /// effects it produced — the user-facing entry to escriba's
    /// imperative programmability tier (`escriba-vm`). Example:
    /// `escriba eval '(set-option "number" "true")'`.
    Eval(EvalArgs),
    /// Inspect and load caixa-native plugins. An escriba plugin IS a
    /// caixa (a dir with `caixa.lisp` + an `escriba/plugin.lisp` entry).
    Plugin(PluginArgs),
}

/// Args for `escriba plugin <cmd>`.
#[derive(clap::Args)]
struct PluginArgs {
    #[command(subcommand)]
    cmd: PluginCmd,
}

#[derive(Subcommand)]
enum PluginCmd {
    /// List the bundled catalog caixas + any rc-declared plugins, and
    /// whether each is installed in the plugins dir.
    List,
    /// Load a plugin caixa from a directory, apply its escriba entry to
    /// a fresh editor, and report what it registered.
    Load(PluginLoadArgs),
    /// Forge the bundled catalog (or a catalog dir) into complete,
    /// installable caixa directories — caixa.lisp + escriba/plugin.lisp
    /// + flake.nix + the persisted spec. The generation half of the
    /// plugin substrate.
    Forge(PluginForgeArgs),
    /// Materialize the bundled catalog into the plugins dir, so the
    /// default plugins are present on disk for forking / overriding.
    InstallBundled(PluginInstallArgs),
}

/// Args for `escriba plugin load <path>`.
#[derive(clap::Args)]
struct PluginLoadArgs {
    /// Path to the plugin caixa directory (contains `caixa.lisp` +
    /// `escriba/plugin.lisp`).
    path: PathBuf,
}

/// Args for `escriba plugin forge`.
#[derive(clap::Args)]
struct PluginForgeArgs {
    /// Output directory for the forged caixa directories.
    #[arg(long, default_value = "plugins-out")]
    out: PathBuf,
    /// Forge from this catalog dir's `*.escribaplugin.lisp` sources
    /// instead of the baked bundle.
    #[arg(long)]
    catalog: Option<PathBuf>,
}

/// Args for `escriba plugin install-bundled`.
#[derive(clap::Args)]
struct PluginInstallArgs {
    /// Plugins dir to install into. Defaults to the resolved plugins
    /// dir (`$ESCRIBA_PLUGINS_DIR` / XDG / `~/.local/share/escriba/plugins`).
    #[arg(long)]
    out: Option<PathBuf>,
}

/// Args for `escriba eval` — the imperative tatara-lisp entry point.
#[derive(clap::Args)]
struct EvalArgs {
    /// Tatara-lisp source to evaluate, e.g. `'(message "hi")'`.
    source: String,
    /// Optional file to open as the active buffer before evaluating, so
    /// buffer-mutating effects (`insert`, `run-command "save"`) have a
    /// real target.
    #[arg(long)]
    file: Option<PathBuf>,
}

/// Public entry point — both `escriba` and `escriba-gpu` binaries
/// call this. The GPU sibling just sets `ESCRIBA_RENDER=gpu` before
/// invoking.
pub fn run() -> Result<()> {
    init_tracing();
    let args = Args::parse();

    // ── Subcommand dispatch (shikumi-canonical operator surface) ──
    // When a subcommand is present, dispatch and exit. Editor flags
    // (--spec / --commands / --keymap / --list-rc / file) are ignored
    // for the subcommand path — operators reach the typed config tier
    // model via `escriba config-show ...` without booting the editor.
    if let Some(command) = args.command {
        match command {
            EscribaCommand::ConfigShow(cmd) => {
                return cmd
                    .run::<escriba_config::EscribaConfig>("ESCRIBA_TIER")
                    .map_err(|e| anyhow::anyhow!("config-show failed: {e}"));
            }
            EscribaCommand::Eval(cmd) => return run_eval(&cmd),
            EscribaCommand::Plugin(cmd) => return run_plugin(&cmd),
        }
    }

    if args.spec {
        let spec = escriba_spec::build_spec();
        if args.yaml {
            print!("{}", spec.to_yaml());
        } else {
            println!("{}", spec.to_json_pretty());
        }
        return Ok(());
    }

    if args.commands {
        // Show the real command surface the editor boots with: built-ins
        // plus every `(defcmd …)` from the bundled defaults and user rc.
        let mut reg = CommandRegistry::default_set();
        let plan = default_plan(args.no_defaults)?;
        escriba_lisp::apply_plan_to_commands(&plan, &mut reg);
        if let Some((_, plan)) = load_rc_optional(args.rc.as_deref())? {
            escriba_lisp::apply_plan_to_commands(&plan, &mut reg);
        }
        println!("escriba commands ({}):", reg.specs().len());
        for c in reg.specs() {
            println!("  {:<18} {}", c.name, c.description);
        }
        return Ok(());
    }

    if args.keymap {
        let k = Keymap::default_vim();
        println!("escriba keymap ({} binding(s)):", k.len());
        for (mode, key, binding) in k.entries_sorted() {
            println!(
                "  [{:<8}] {:>12}  →  {:<22}  {}",
                mode.as_str(),
                format!("{key:?}"),
                format!("{:?}", binding.action),
                binding.description,
            );
        }
        return Ok(());
    }

    if args.list_rc {
        return run_list_rc(args.rc.as_deref());
    }

    escriba_config::EscribaConfig::register_all();

    let mut buffers = BufferSet::new();
    let active_id = if let Some(path) = &args.file {
        buffers
            .open(path)
            .with_context(|| format!("opening {}", path.display()))?
    } else {
        buffers.scratch(
            ";; escriba scratch buffer\n;; try `escriba --help`\n(message \"hello, escriba\")\n",
        )
    };

    // Build the composite plan: baseline + bundled plugin catalog
    // (unless `--no-defaults`) + user rc on top. User declarations
    // override defaults thanks to the plan's merge semantics.
    let mut plan = default_plan(args.no_defaults)?;
    let user_rc = load_rc_optional(args.rc.as_deref())?;
    if let Some((_, user_plan)) = &user_rc {
        plan.merge(user_plan.clone());
    }

    // Build the initial state, then apply the plan to its keymap so
    // `defkeybind` forms have the chance to override vim defaults
    // before the first frame renders.
    let mut state = EditorState::new_with_buffer(buffers, active_id);
    // Register `defcmd` forms into the live command registry FIRST, so
    // the deferred-keybind dispatch path (`Action::Command { name }`)
    // that the keymap apply produces has real commands to resolve
    // against instead of dead-ending at the five built-ins.
    let cmd_report = escriba_lisp::apply_plan_to_commands(&plan, &mut state.commands);
    let opt_report = escriba_lisp::apply_plan_to_options(&plan, &mut state.options);
    // Resolve the leader from the option store BEFORE binding keys —
    // `<leader>` is resolved at bind time, so the leader must be set
    // first or every `<leader>…` sequence binds under the wrong prefix.
    apply_leader_option(&mut state);
    let report = escriba_lisp::apply_plan_to_keymap(&plan, &mut state.keymap);
    tracing::info!(
        "plan applied: {}; keymap={}; {}; {}",
        plan.summary(),
        report.summary(),
        cmd_report.summary(),
        opt_report.summary(),
    );
    for w in &report.warnings {
        tracing::warn!("rc: {w}");
    }
    for name in &cmd_report.overridden_names {
        tracing::warn!("rc: defcmd `{name}` overrode an existing command");
    }

    // Tree-sitter grammar extensions — wire every `(defmode …)`
    // with a `:tree-sitter` name into the registry so
    // `GrammarRegistry::from_extension` resolves the right grammar
    // for the user's ftdetect set.
    let mut grammar_registry = escriba_ts::GrammarRegistry::builtin()
        .context("building built-in tree-sitter grammar registry")?;
    let known_langs: Vec<String> = grammar_registry
        .languages()
        .map(String::from)
        .collect();
    let ts_report = escriba_lisp::apply_plan_to_grammar_extensions(
        &plan,
        |name| known_langs.iter().any(|l| l == name),
        |lang, ext| {
            grammar_registry.add_extension(lang, ext);
        },
    );
    if ts_report.extensions_registered > 0
        || ts_report.extensions_skipped_unknown_language > 0
    {
        tracing::info!(
            "ts apply: extensions_registered={} skipped_unknown_language={} unknown={:?}",
            ts_report.extensions_registered,
            ts_report.extensions_skipped_unknown_language,
            ts_report.unknown_languages,
        );
    }

    // Wire user-installed caixa plugins from the plugins dir: eager ones
    // apply now; lazy ones register for trigger-gated activation. (The
    // bundled default catalog is already in `plan` — baked, eager.)
    activate_user_plugins(&plan, &mut state);

    if args.dry_run {
        let buf = state.buffers.get(active_id).context("active buffer missing")?;
        println!(
            "buffer {active_id}: {} line(s), {} chars",
            buf.line_count(),
            buf.char_count(),
        );
        println!("plan: {}", plan.summary());
        if let Some((path, user_plan)) = &user_rc {
            println!("user rc {}: {}", path.display(), user_plan.summary());
        }
        return Ok(());
    }

    match args.render {
        RenderMode::Text => run_text(state, args.height),
        RenderMode::Gpu => run_gpu(state, &args),
        RenderMode::Tui => run_tui(state),
    }
}

fn run_tui(state: EditorState) -> Result<()> {
    escriba_tui::run(state).context("tui loop exited")
}

fn run_text(state: EditorState, height: u32) -> Result<()> {
    // Override viewport height from CLI.
    let mut layout = state.layout.clone();
    if let Some(w) = layout.windows.iter_mut().find(|w| w.id == layout.active) {
        w.viewport.visible_lines = height;
    }
    let mut renderer = TextRenderer;
    let frame = renderer.render_frame(&layout, &state.buffers, Position::ZERO);
    print!("{frame}");
    Ok(())
}

fn run_gpu(mut initial: EditorState, args: &Args) -> Result<()> {
    // Tighten the initial viewport to something reasonable for a GPU window.
    if let Some(w) = initial
        .layout
        .windows
        .iter_mut()
        .find(|w| w.id == initial.layout.active)
    {
        w.rect.width = args.width;
        w.rect.height = args.win_height;
        w.viewport.visible_lines = (args.win_height / 20).max(10);
    }
    let state = Arc::new(Mutex::new(initial));
    let event_state = state.clone();
    let render_state = state.clone();

    let renderer = GpuRenderer::new(render_state);

    App::builder(renderer)
        .title(args.title.clone())
        .size(args.width, args.win_height)
        .on_event(move |event, _renderer| {
            let mut s = event_state.lock().unwrap_or_else(|e| e.into_inner());
            s.tick(event);
            let mut resp = madori::EventResponse::default();
            if s.quit_requested {
                resp.exit = true;
                resp.consumed = true;
            }
            resp
        })
        .run()
        .map_err(|e| anyhow::anyhow!("madori app exited: {e}"))?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};
    // Default to `warn` so plain `escriba` boots silently like nvim.
    // The plan summary + ts-apply lines are `info`-level; users who
    // want them set `RUST_LOG=info escriba` or `--list-rc`.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().compact().with_writer(std::io::stderr))
        .try_init();
}

/// Resolve the `mapleader` option from the applied option store and set
/// it on the keymap. MUST run after `apply_plan_to_options` and BEFORE
/// `apply_plan_to_keymap`, because `<leader>` is resolved at bind time.
/// An unparseable value keeps the keymap's default leader and warns —
/// partial application beats a dead leader surface.
fn apply_leader_option(state: &mut EditorState) {
    if let Some(value) = state.options.get("mapleader") {
        match escriba_lisp::parse_leader_key(value) {
            Some(key) => state.keymap.set_leader(key),
            None => tracing::warn!(
                "rc: mapleader value {value:?} is not a parseable key — keeping default leader"
            ),
        }
    }
}

/// Apply a loaded plugin caixa's escriba entry to live editor state —
/// the activation seam. The entry is plain escriba-lisp, so a plugin's
/// keybinds / commands / options land through the SAME apply paths a
/// user rc uses. Returns a one-line summary of what it registered.
fn activate_plugin(
    plugin: &escriba_plugin::PluginCaixa,
    state: &mut EditorState,
) -> Result<String> {
    let plan = escriba_lisp::apply_source(&plugin.entry_src)
        .with_context(|| format!("parsing plugin `{}` entry", plugin.name))?;
    let cmd = escriba_lisp::apply_plan_to_commands(&plan, &mut state.commands);
    let opt = escriba_lisp::apply_plan_to_options(&plan, &mut state.options);
    apply_leader_option(state);
    let km = escriba_lisp::apply_plan_to_keymap(&plan, &mut state.keymap);
    Ok(format!(
        "commands +{} keybinds +{} (sequences {}) options +{}",
        cmd.registered, km.keybinds_applied, km.keybinds_sequences, opt.set,
    ))
}

/// Resolve the plugins directory: `$ESCRIBA_PLUGINS_DIR`, else
/// `$XDG_DATA_HOME/escriba/plugins`, else `$HOME/.local/share/escriba/plugins`.
fn plugins_dir() -> PathBuf {
    if let Ok(p) = std::env::var("ESCRIBA_PLUGINS_DIR") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("escriba").join("plugins");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("escriba")
            .join("plugins");
    }
    PathBuf::from("plugins")
}

/// Map an `escriba-lisp` `PluginSpec` (the lazy.nvim-shaped `defplugin`)
/// to caixa-plugin `:ativar-em` trigger strings. Covers all three
/// trigger kinds `ActivationTrigger::parse` understands — event,
/// command, AND filetype. (`:keybinds` triggers are not yet a lazy
/// trigger kind — there is no `Keybind` variant in `ActivationTrigger` /
/// `LazyTrigger` — so they are intentionally not emitted here.)
fn plugin_triggers(spec: &escriba_lisp::PluginSpec) -> Vec<String> {
    let mut t = Vec::new();
    if !spec.on_event.is_empty() {
        t.push(format!("Event: {}", spec.on_event));
    }
    if !spec.on_command.is_empty() {
        t.push(format!("Command: {}", spec.on_command));
    }
    if !spec.on_filetype.is_empty() {
        t.push(format!("FileType: {}", spec.on_filetype));
    }
    t
}

/// Map an `escriba-plugin` activation trigger to the runtime's lazy
/// trigger. `Startup` plugins have no lazy trigger (they activate
/// eagerly), so they map to `None`.
fn map_lazy_trigger(
    t: &escriba_plugin::ActivationTrigger,
) -> Option<escriba_runtime::LazyTrigger> {
    use escriba_plugin::ActivationTrigger as A;
    use escriba_runtime::LazyTrigger as L;
    match t {
        A::Startup => None,
        A::FileType(f) => Some(L::FileType(f.clone())),
        A::Event(e) => Some(L::Event(e.clone())),
        A::Command(c) => Some(L::Command(c.clone())),
    }
}

/// Wire USER plugins installed in the plugins dir (declared via
/// `(defplugin :name … :on-* …)` in the user rc): EAGER ones have their
/// entry applied now; LAZY ones (an on-event / on-command / on-filetype
/// trigger, or `:lazy`) are registered with the runtime's
/// [`PluginHost`](escriba_runtime::PluginHost) for trigger-gated
/// activation later. The BUNDLED default catalog is NOT touched here —
/// it is baked into the binary and already merged into the boot plan; a
/// bundled descriptor whose name isn't present in the plugins dir is
/// skipped. Best-effort: a declared-but-not-installed plugin is skipped
/// silently; a malformed plugin warns but never aborts startup.
fn activate_user_plugins(plan: &escriba_lisp::ApplyPlan, state: &mut EditorState) {
    let dir = plugins_dir();
    if !dir.exists() {
        return;
    }
    for spec in &plan.plugins {
        let root = dir.join(&spec.name);
        if !root.exists() {
            continue;
        }
        let triggers = plugin_triggers(spec);
        match escriba_plugin::PluginCaixa::load(&spec.name, "*", &triggers, &root) {
            Ok(plugin) if plugin.is_eager() && !spec.lazy => match activate_plugin(&plugin, state) {
                Ok(summary) => tracing::info!("plugin `{}` activated: {summary}", plugin.name),
                Err(e) => tracing::warn!("plugin `{}` failed to activate: {e}", plugin.name),
            },
            Ok(plugin) => {
                let lazy_triggers: Vec<_> =
                    plugin.triggers.iter().filter_map(map_lazy_trigger).collect();
                tracing::debug!(
                    "plugin `{}` is lazy — registered for {} trigger(s)",
                    plugin.name,
                    lazy_triggers.len(),
                );
                state.register_lazy_plugin(plugin.name.clone(), lazy_triggers, plugin.entry_src.clone());
            }
            Err(e) => tracing::warn!("plugin `{}`: {e}", spec.name),
        }
    }
}

/// `escriba plugin <cmd>` dispatch.
fn run_plugin(args: &PluginArgs) -> Result<()> {
    match &args.cmd {
        PluginCmd::List => run_plugin_list(),
        PluginCmd::Load(a) => run_plugin_load(&a.path),
        PluginCmd::Forge(a) => run_plugin_forge(a),
        PluginCmd::InstallBundled(a) => run_plugin_install_bundled(a),
    }
}

/// Resolve the `(name, source)` pairs to forge: either every
/// `*.escribaplugin.lisp` under `--catalog <dir>`, or the baked bundle.
fn forge_sources(catalog: Option<&std::path::Path>) -> Result<Vec<(String, String)>> {
    if let Some(dir) = catalog {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("reading catalog dir {}", dir.display()))?
        {
            let path = entry?.path();
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.ends_with(".escribaplugin.lisp"))
            {
                let src = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                out.push((path.display().to_string(), src));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    } else {
        Ok(catalog_bundle::BUNDLED
            .iter()
            .map(|b| (b.name.to_string(), b.source.to_string()))
            .collect())
    }
}

/// `escriba plugin forge` — emit every catalog plugin into a complete
/// installable caixa directory under `--out`.
fn run_plugin_forge(args: &PluginForgeArgs) -> Result<()> {
    let sources = forge_sources(args.catalog.as_deref())?;
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating output dir {}", args.out.display()))?;
    let mut forged = 0usize;
    for (label, src) in &sources {
        let (spec, artifacts) = escriba_plugin::forge_plugin(src)
            .with_context(|| format!("forging {label}"))?;
        let root = escriba_plugin::write_plugin_caixa(&spec, &artifacts, &args.out)
            .with_context(|| format!("writing caixa for {}", spec.name))?;
        forged += 1;
        println!("  ✓ {:<32} → {}", spec.name, root.display());
    }
    println!("forged {forged} plugin caixa(s) into {}", args.out.display());
    Ok(())
}

/// `escriba plugin install-bundled` — materialize the baked catalog
/// into the plugins dir so the default plugins are present on disk.
fn run_plugin_install_bundled(args: &PluginInstallArgs) -> Result<()> {
    let dir = args.out.clone().unwrap_or_else(plugins_dir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating plugins dir {}", dir.display()))?;
    let mut n = 0usize;
    for b in catalog_bundle::BUNDLED {
        let (spec, artifacts) = escriba_plugin::forge_plugin(b.source)
            .with_context(|| format!("forging bundled `{}`", b.name))?;
        escriba_plugin::write_plugin_caixa(&spec, &artifacts, &dir)
            .with_context(|| format!("installing `{}`", b.name))?;
        n += 1;
    }
    println!("installed {n} bundled plugin caixa(s) into {}", dir.display());
    Ok(())
}

/// `escriba plugin list` — declared plugins (from bundled defaults + user
/// rc) and whether each is installed in the plugins dir.
fn run_plugin_list() -> Result<()> {
    let mut plan = default_plan(false)?;
    if let Some((_, user)) = load_rc_optional(None)? {
        plan.merge(user);
    }
    let dir = plugins_dir();
    let bundled = catalog_bundle::bundled_specs()?;
    println!(
        "escriba plugins: {} bundled caixas (baked, always-on) + {} declared",
        bundled.len(),
        plan.plugins.len(),
    );
    println!("  plugins dir (user installs): {}", dir.display());
    println!();
    println!("  ── bundled catalog (installed by default) ──");
    for spec in &bundled {
        let trigger = if spec.is_eager() {
            "eager".to_string()
        } else {
            spec.ativar_em.join(", ")
        };
        println!(
            "  ✓ baked   {:<32} [{:<11}] {trigger}",
            spec.name, spec.category,
        );
    }
    // Any plugin declared in the rc but resolved from the plugins dir.
    let dir_declared: Vec<_> = plan
        .plugins
        .iter()
        .filter(|s| dir.join(&s.name).exists())
        .collect();
    if !dir_declared.is_empty() {
        println!();
        println!("  ── user-installed (plugins dir) ──");
        for spec in dir_declared {
            let lazy = if spec.lazy { "lazy" } else { "eager" };
            println!("  ✓ installed {:<30} [{:<11}] {lazy}", spec.name, spec.category);
        }
    }
    Ok(())
}

/// `escriba plugin load <path>` — load a plugin caixa + apply its entry
/// to a fresh editor and report what it registered.
fn run_plugin_load(path: &std::path::Path) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("plugin");
    let plugin = escriba_plugin::PluginCaixa::load(name, "*", &[], path)
        .map_err(|e| anyhow::anyhow!("loading plugin {}: {e}", path.display()))?;

    let mut buffers = BufferSet::new();
    let active = buffers.scratch("");
    let mut state = EditorState::new_with_buffer(buffers, active);
    let summary = activate_plugin(&plugin, &mut state)?;

    println!("✓ loaded plugin caixa `{}` (v{})", plugin.name, plugin.version);
    let triggers = if plugin.triggers.is_empty() {
        "eager".to_string()
    } else {
        format!("{:?}", plugin.triggers)
    };
    println!("  triggers:   {triggers}");
    println!("  registered: {summary}");
    Ok(())
}

/// `escriba eval` — run tatara-lisp against a fresh editor and report
/// the effects it produced. The CLI face of the imperative
/// programmability tier: snapshot → `EscribaVm` eval → apply effects.
fn run_eval(args: &EvalArgs) -> Result<()> {
    let mut buffers = BufferSet::new();
    let active = if let Some(p) = &args.file {
        buffers
            .open(p)
            .with_context(|| format!("opening {}", p.display()))?
    } else {
        buffers.scratch("")
    };
    let mut state = EditorState::new_with_buffer(buffers, active);
    state
        .run_lisp(&args.source)
        .map_err(|e| anyhow::anyhow!("tatara-lisp eval failed: {e}"))?;

    for m in &state.messages {
        println!("message: {m}");
    }
    let mut opts: Vec<(&String, &String)> = state.options.iter().collect();
    opts.sort();
    for (k, v) in opts {
        println!("option: {k} = {v}");
    }
    if let Some(buf) = state.buffers.get(active) {
        let content = buf.to_string();
        if !content.is_empty() {
            println!("buffer:\n{content}");
        }
    }
    Ok(())
}

/// Resolve the rc path (explicit `--rc` > env > xdg > home default) and
/// load it if the file exists. Returns `Ok(None)` when no rc file is
/// present anywhere — that's the default for a fresh install.
fn load_rc_optional(explicit: Option<&std::path::Path>) -> Result<Option<(PathBuf, escriba_lisp::ApplyPlan)>> {
    let path: PathBuf = match explicit {
        Some(p) => p.to_path_buf(),
        None => escriba_lisp::default_rc_path(),
    };
    if !path.exists() {
        if explicit.is_some() {
            anyhow::bail!("--rc path does not exist: {}", path.display());
        }
        return Ok(None);
    }
    let plan = escriba_lisp::load_rc(&path)
        .with_context(|| format!("parsing rc file {}", path.display()))?;
    Ok(Some((path, plan)))
}

// Glyph tables delegate to `escriba_lisp::{form_glyph, category_glyph}`
// so the source of truth stays next to the specs that declare the
// labels. The binary's `--list-rc` banner picks up new glyphs the
// moment a def-form adds one.
use escriba_lisp::{category_glyph, form_glyph};

/// Render the plan summary with per-form glyphs so the counts read
/// at a glance instead of as a flat k=v blob. Walks
/// `ApplyPlan::counts()` (single source of truth) and wraps a soft
/// newline every `WRAP` cells so wide plans don't run off the
/// terminal.
fn glyph_summary(plan: &escriba_lisp::ApplyPlan) -> String {
    const WRAP: usize = 10;
    let mut out = String::new();
    for (i, (name, n)) in plan.counts().iter().enumerate() {
        if i > 0 && i % WRAP == 0 {
            out.push_str("\n  ");
        }
        use std::fmt::Write as _;
        let _ = write!(out, "{} {:<3} ", form_glyph(name), n);
    }
    out.trim_end().to_string()
}

/// `--list-rc` handler. Parses the bundled defaults + optional user
/// rc, reports the composite apply plan. Mirrors `frost --doctor`.
fn run_list_rc(explicit: Option<&std::path::Path>) -> Result<()> {
    // Defaults plan = baseline + bundled plugin catalog (always green
    // unless someone broke a bundled file).
    let defaults = default_plan(false)?;
    println!("✦ escriba defaults (baseline + {} plugin caixas)", catalog_bundle::BUNDLED.len());
    println!("  {}", glyph_summary(&defaults));
    println!();
    println!("{} plugins ({})", form_glyph("plugins"), defaults.plugins.len());
    let by_cat = group_plugins_by_category(&defaults);
    for (cat, names) in &by_cat {
        println!("  {} {:<11} {}", category_glyph(cat), cat, names.join(", "));
    }

    // User rc layered on top.
    let user = load_rc_optional(explicit)?;

    // Composite plan (defaults + user) — the same plan the editor
    // actually boots with. Used below for the wiring-status section.
    let composite = {
        let mut c = defaults.clone();
        if let Some((_, p)) = &user {
            c.merge(p.clone());
        }
        c
    };

    println!();
    match user {
        Some((path, plan)) => {
            println!("✎ user rc: {}", path.display());
            println!("  {}", glyph_summary(&plan));
            println!();
            for kb in plan.keybinds.iter().take(10) {
                println!(
                    "  {} [{}] {:<10} → {}",
                    form_glyph("keybinds"),
                    kb.mode,
                    kb.key,
                    kb.action
                );
            }
            if plan.keybinds.len() > 10 {
                println!("    (+{} more keybinds)", plan.keybinds.len() - 10);
            }
            for h in plan.hooks.iter().take(5) {
                println!("  {} {} → {}", form_glyph("hooks"), h.event, h.command);
            }
            if let Some(t) = &plan.theme {
                println!("  {} preset={}", form_glyph("theme"), t.preset);
            }
        }
        None => {
            println!("✎ user rc: ⌀  not found");
            println!(
                "  search order: $ESCRIBARC  →  $XDG_CONFIG_HOME/escriba/rc.lisp  →  $HOME/.escribarc.lisp"
            );
        }
    }

    print_wiring_status(&composite);
    Ok(())
}

/// Apply the composite plan to a fresh editor's live state and report
/// which def-forms actually reached `EditorState` ("WIRED") versus
/// those still parse-validated only ("parse-only"). This is the honest
/// truth-up surface: as each wave wires another form, it graduates
/// from parse-only to WIRED here. Mirrors `frost --doctor`'s applied
/// counts.
fn print_wiring_status(plan: &escriba_lisp::ApplyPlan) {
    let mut buffers = BufferSet::new();
    let active = buffers.scratch("");
    let mut state = EditorState::new_with_buffer(buffers, active);

    let cmd_report = escriba_lisp::apply_plan_to_commands(plan, &mut state.commands);
    let opt_report = escriba_lisp::apply_plan_to_options(plan, &mut state.options);
    apply_leader_option(&mut state);
    let km_report = escriba_lisp::apply_plan_to_keymap(plan, &mut state.keymap);

    println!();
    println!("⚙ wiring status (def-form → live EditorState)");
    println!(
        "  {} defcmd        WIRED      registered={} overridden={} (registry now {} cmds)",
        form_glyph("cmds"),
        cmd_report.registered,
        cmd_report.overridden,
        state.commands.len(),
    );
    println!(
        "  {} defoption     WIRED      set={} overridden={} (option store now {})",
        form_glyph("options"),
        opt_report.set,
        opt_report.overridden,
        state.options.len(),
    );
    println!(
        "  {} defkeybind    WIRED      applied={} deferred_cmd={} sequences={} warnings={}",
        form_glyph("keybinds"),
        km_report.keybinds_applied,
        km_report.keybinds_deferred_to_commands,
        km_report.keybinds_sequences,
        km_report.warnings.len(),
    );
    match escriba_ts::GrammarRegistry::builtin() {
        Ok(mut reg) => {
            let known: Vec<String> = reg.languages().map(String::from).collect();
            let ts = escriba_lisp::apply_plan_to_grammar_extensions(
                plan,
                |n| known.iter().any(|l| l == n),
                |lang, ext| {
                    reg.add_extension(lang, ext);
                },
            );
            println!(
                "  {} defmode       WIRED      registered={} skipped_unknown_lang={}",
                form_glyph("major_modes"),
                ts.extensions_registered,
                ts.extensions_skipped_unknown_language,
            );
        }
        Err(_) => println!(
            "  {} defmode       parse-only (tree-sitter registry unavailable)",
            form_glyph("major_modes"),
        ),
    }
}

/// Group the plan's plugins by category for the list-rc output so
/// users see which blnvim groups are represented at a glance.
fn group_plugins_by_category(
    plan: &escriba_lisp::ApplyPlan,
) -> Vec<(String, Vec<String>)> {
    use std::collections::BTreeMap;
    let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in &plan.plugins {
        let cat = if p.category.is_empty() {
            "uncategorized".to_string()
        } else {
            p.category.clone()
        };
        by.entry(cat).or_default().push(p.name.clone());
    }
    by.into_iter().collect()
}

#[cfg(test)]
mod theme_tests {
    /// The standalone Vellum theme module, bundled alongside the
    /// blnvim defaults.
    const VELLUM_RC: &str = include_str!("../configs/vellum.lisp");

    #[test]
    fn bundled_default_rc_applies_and_selects_vellum() {
        let plan = escriba_lisp::apply_source(super::DEFAULT_RC)
            .expect("bundled blnvim-defaults.lisp must apply cleanly");
        // The fleet theme is the active preset.
        assert_eq!(
            plan.theme.as_ref().map(|t| t.preset.as_str()),
            Some("vellum"),
            "default rc must select the Vellum fleet theme",
        );
    }

    #[test]
    fn bundled_vellum_rc_applies_and_selects_vellum() {
        let plan = escriba_lisp::apply_source(VELLUM_RC)
            .expect("bundled vellum.lisp must apply cleanly");
        assert_eq!(
            plan.theme.as_ref().map(|t| t.preset.as_str()),
            Some("vellum"),
        );
    }
}

#[cfg(test)]
mod leader_tests {
    use super::*;
    use escriba_keymap::Key;

    fn state() -> EditorState {
        let mut buffers = BufferSet::new();
        let id = buffers.scratch("");
        EditorState::new_with_buffer(buffers, id)
    }

    #[test]
    fn apply_leader_option_sets_space_leader_from_option() {
        let mut s = state();
        s.options.insert("mapleader".into(), "<space>".into());
        apply_leader_option(&mut s);
        assert_eq!(s.keymap.leader(), &Key::Char(' '));
    }

    #[test]
    fn apply_leader_option_keeps_default_on_unparseable() {
        let mut s = state();
        let before = s.keymap.leader().clone();
        s.options.insert("mapleader".into(), "totally-bogus".into());
        apply_leader_option(&mut s);
        assert_eq!(s.keymap.leader(), &before, "bogus mapleader keeps default");
    }

    #[test]
    fn bundled_defaults_resolve_space_leader_end_to_end() {
        // The full glue: apply the bundled blnvim-defaults options, then
        // resolve the leader — proving the shipped
        // `(defoption :name "mapleader" :value "<space>")` actually
        // takes effect on the live keymap.
        let plan = escriba_lisp::apply_source(super::DEFAULT_RC).unwrap();
        let mut s = state();
        escriba_lisp::apply_plan_to_options(&plan, &mut s.options);
        apply_leader_option(&mut s);
        assert_eq!(
            s.keymap.leader(),
            &Key::Char(' '),
            "bundled mapleader=<space> must resolve to the space leader",
        );
    }
}
