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

    if args.dry_run {
        let buf = buffers.get(active_id).context("active buffer missing")?;
        println!(
            "buffer {active_id}: {} line(s), {} chars",
            buf.line_count(),
            buf.char_count(),
        );
        return Ok(());
    }

    match args.render {
        RenderMode::Text => run_text(buffers, active_id, args.height),
        RenderMode::Gpu => run_gpu(buffers, active_id, &args),
        RenderMode::Tui => run_tui(buffers, active_id),
    }
}

fn run_tui(buffers: BufferSet, active_id: escriba_core::BufferId) -> Result<()> {
    let state = EditorState::new_with_buffer(buffers, active_id);
    escriba_tui::run(state).context("tui loop exited")
}

fn run_text(buffers: BufferSet, active_id: escriba_core::BufferId, height: u32) -> Result<()> {
    let state = EditorState::new_with_buffer(buffers, active_id);
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

fn run_gpu(buffers: BufferSet, active_id: escriba_core::BufferId, args: &Args) -> Result<()> {
    let mut initial = EditorState::new_with_buffer(buffers, active_id);
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
