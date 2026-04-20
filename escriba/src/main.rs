//! `escriba` — editor binary, TUI by default.
//!
//! Thin wrapper around `escriba::run()`. The sibling `escriba-gpu`
//! binary shares the same entry point but flips
//! `$ESCRIBA_RENDER=gpu` before calling in.

fn main() -> anyhow::Result<()> {
    escriba::run()
}
