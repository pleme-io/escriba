//! `escriba-gpu` — GPU-default escriba.
//!
//! Same binary as `escriba`, except the render backend defaults to
//! `gpu` (madori + garasu) instead of `tui`. Useful on desktop
//! setups where the editor lives in its own window; the plain
//! `escriba` binary stays terminal-first (nvim-style). `--render`
//! still overrides, and `$ESCRIBA_RENDER` wins over both.
//!
//! Implementation: we set `ESCRIBA_RENDER=gpu` in the process
//! environment if the user hasn't already, then call the main
//! entry point. Zero code duplication — the only difference
//! between the two binaries is this pre-populate.

fn main() -> anyhow::Result<()> {
    // Only default to GPU when the user hasn't specified otherwise.
    // `ESCRIBA_RENDER=tui escriba-gpu` still gives you a TUI — the
    // binary is "GPU by default", not "GPU by force".
    if std::env::var_os("ESCRIBA_RENDER").is_none() {
        // Safety: we're the only thread running at startup, before
        // any async runtime or thread pool spins up.
        unsafe {
            std::env::set_var("ESCRIBA_RENDER", "gpu");
        }
    }
    escriba::run()
}
