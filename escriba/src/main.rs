//! `escriba` — the editor binary.
//!
//! GPU window via madori + garasu is the default. `--render=text` falls back
//! to an ANSI-in-stdout snapshot (used in CI / headless runs). Every escriba-*
//! crate composes here.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use escriba_buffer::BufferSet;
use escriba_command::CommandRegistry;
use escriba_core::Position;
use escriba_keymap::Keymap;
use escriba_render::{GpuRenderer, Renderer, TextRenderer};
use escriba_runtime::EditorState;
use madori::App;

/// The "blnvim-parity" default rc, baked into the binary. Users get
/// this unless they pass `--no-defaults`. A user rc (via `--rc` or
/// `$ESCRIBARC`) merges on top — user declarations win per plan-merge
/// semantics.
const DEFAULT_RC: &str = include_str!("../configs/blnvim-defaults.lisp");

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
    /// Path to the Tatara-Lisp rc file. Falls back to `$ESCRIBARC`, then
    /// `$XDG_CONFIG_HOME/escriba/rc.lisp`, then `$HOME/.escribarc.lisp`.
    /// Missing files are silently skipped; parse errors fail fast.
    #[arg(long)]
    rc: Option<PathBuf>,
    /// Parse the rc file, print the apply-plan summary, and exit.
    /// Mirrors `frost --doctor` — useful for CI / config validation.
    #[arg(long)]
    list_rc: bool,
    /// Skip the bundled blnvim-parity defaults. By default, escriba
    /// boots with the same plugin/keybinding/theme surface area as
    /// `pleme-io/blackmatter-nvim`; `--no-defaults` yields a bare
    /// vim-ish editor with only the user rc applied.
    #[arg(long)]
    no_defaults: bool,
    /// Render backend — gpu (default, interactive window) or text (headless).
    #[arg(long, value_enum, default_value_t = RenderMode::Gpu)]
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
}

fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();

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
        let reg = CommandRegistry::default_set();
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

    // Build the composite plan: bundled blnvim defaults (unless
    // `--no-defaults`) + user rc on top. User declarations override
    // defaults thanks to the plan's merge semantics.
    let mut plan = if args.no_defaults {
        escriba_lisp::ApplyPlan::default()
    } else {
        escriba_lisp::apply_source(DEFAULT_RC).context("parsing bundled blnvim-defaults")?
    };
    let user_rc = load_rc_optional(args.rc.as_deref())?;
    if let Some((_, user_plan)) = &user_rc {
        plan.merge(user_plan.clone());
    }

    // Build the initial state, then apply the plan to its keymap so
    // `defkeybind` forms have the chance to override vim defaults
    // before the first frame renders.
    let mut state = EditorState::new_with_buffer(buffers, active_id);
    let report = escriba_lisp::apply_plan_to_keymap(&plan, &mut state.keymap);
    tracing::info!(
        "plan applied: {}; apply={}",
        plan.summary(),
        report.summary(),
    );
    for w in &report.warnings {
        tracing::warn!("rc: {w}");
    }

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
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().compact().with_writer(std::io::stderr))
        .try_init();
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

/// `--list-rc` handler. Parses the bundled defaults + optional user
/// rc, reports the composite apply plan. Mirrors `frost --doctor`.
fn run_list_rc(explicit: Option<&std::path::Path>) -> Result<()> {
    // Defaults plan (always green unless someone broke the bundled file).
    let defaults = escriba_lisp::apply_source(DEFAULT_RC)
        .context("parsing bundled blnvim-defaults")?;
    println!("escriba defaults (bundled blnvim-parity):");
    println!("  {}", defaults.summary());
    println!("  plugins: {}", defaults.plugins.len());
    let by_cat = group_plugins_by_category(&defaults);
    for (cat, names) in &by_cat {
        println!("    [{cat}] {}", names.join(", "));
    }

    // User rc layered on top.
    let user = load_rc_optional(explicit)?;
    println!();
    match user {
        Some((path, plan)) => {
            println!("user rc: {}", path.display());
            println!("  {}", plan.summary());
            for kb in plan.keybinds.iter().take(10) {
                println!("  keybind  [{}] {:<6} → {}", kb.mode, kb.key, kb.action);
            }
            if plan.keybinds.len() > 10 {
                println!("  (+{} more keybinds)", plan.keybinds.len() - 10);
            }
            for h in plan.hooks.iter().take(5) {
                println!("  hook     {} → {}", h.event, h.command);
            }
            if let Some(t) = &plan.theme {
                println!("  theme    preset={}", t.preset);
            }
        }
        None => {
            println!("user rc: <not found>");
            println!(
                "  search order: $ESCRIBARC  →  $XDG_CONFIG_HOME/escriba/rc.lisp  →  $HOME/.escribarc.lisp"
            );
        }
    }
    Ok(())
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
