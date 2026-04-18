//! `escriba-mcp` binary — stdio MCP server.
//!
//! Reads JSON-RPC requests line-delimited on stdin, writes responses on
//! stdout. An LLM pipes into and out of this to drive the editor.

use anyhow::Result;
use std::io::{BufRead, BufReader, Write};
use tracing_subscriber::{EnvFilter, fmt};

fn main() -> Result<()> {
    // Logs to stderr so they don't conflict with the stdio JSON-RPC stream.
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "escriba-mcp ready");

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: escriba_mcp::McpRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, input = %line, "bad JSON-RPC request");
                continue;
            }
        };
        let resp = escriba_mcp::handle(&req);
        let out = serde_json::to_string(&resp)?;
        writeln!(stdout, "{out}")?;
        stdout.flush()?;
    }
    Ok(())
}
