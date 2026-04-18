//! `escriba` — the editor binary.
//!
//! Phase 1 composes every escriba-* crate into a working end-to-end demo.
//! The GPU window (via madori + garasu) lands in phase 1.B; this binary
//! exercises the full stack in a terminal renderer so every invariant is
//! observable before the GUI.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use escriba_buffer::BufferSet;
use escriba_command::CommandRegistry;
use escriba_core::{Mode, Position, WindowId};
use escriba_keymap::Keymap;
use escriba_mode::ModalState;
use escriba_render::{Renderer, TextRenderer};
use escriba_ui::{Layout, Rect, Viewport, Window};

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
    /// Viewport height in lines.
    #[arg(long, default_value_t = 20)]
    height: u32,
}

fn main() -> Result<()> {
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

    let window = Window {
        id: WindowId(1),
        buffer_id: active_id,
        viewport: Viewport {
            top_line: 0,
            left_column: 0,
            visible_lines: args.height,
            visible_columns: 120,
        },
        rect: Rect {
            x: 0,
            y: 0,
            width: 1200,
            height: 800,
        },
    };
    let layout = Layout::single(window);

    let mut modal = ModalState::new();
    modal.enter(Mode::Normal);

    if args.dry_run {
        let buf = buffers.get(active_id).context("active buffer missing")?;
        println!(
            "buffer {active_id}: {} line(s), {} chars, mode={}",
            buf.line_count(),
            buf.char_count(),
            modal.mode.as_str()
        );
        return Ok(());
    }

    let mut renderer = TextRenderer;
    let frame = renderer.render_frame(&layout, &buffers, Position::ZERO);
    print!("{frame}");
    Ok(())
}
