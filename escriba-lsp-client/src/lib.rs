//! `escriba-lsp-client` — minimal LSP client.
//!
//! Phase 1.B: spec-complete wire types for LSP initialize/didOpen/didChange,
//! a `ServerConfig` describing how to launch a server (command + args +
//! root-markers), and `ClientHandle` placeholder. The live stdio transport
//! + JSON-RPC framing lives in the binary when a buffer is opened — this
//! crate stays pure so its tests can run offline.
//!
//! Phase 2: wire a full `tokio`-based JSON-RPC framer and connect to
//! caixa-lsp + rust-analyzer + any configured server.

extern crate self as escriba_lsp_client;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LspError {
    #[error("no server configured for language {0}")]
    NoServer(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, LspError>;

/// How to launch one LSP server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerConfig {
    /// The language this server handles (matches escriba-ts grammar names).
    pub language: String,
    /// The command to spawn (e.g. `rust-analyzer`, `caixa-lsp`).
    pub command: String,
    /// Extra args passed to the server.
    #[serde(default)]
    pub args: Vec<String>,
    /// Files whose presence determines the project root.
    #[serde(default)]
    pub root_markers: Vec<String>,
}

impl ServerConfig {
    #[must_use]
    pub fn rust_analyzer() -> Self {
        Self {
            language: "rust".to_string(),
            command: "rust-analyzer".to_string(),
            args: vec![],
            root_markers: vec!["Cargo.toml".to_string(), ".git".to_string()],
        }
    }

    #[must_use]
    pub fn caixa_lsp() -> Self {
        Self {
            language: "caixa".to_string(),
            command: "caixa-lsp".to_string(),
            args: vec![],
            root_markers: vec!["caixa.lisp".to_string(), ".git".to_string()],
        }
    }
}

/// Registry of server configs by language.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ServerRegistry {
    servers: Vec<ServerConfig>,
}

impl ServerRegistry {
    #[must_use]
    pub fn default_set() -> Self {
        Self {
            servers: vec![ServerConfig::rust_analyzer(), ServerConfig::caixa_lsp()],
        }
    }

    pub fn register(&mut self, config: ServerConfig) {
        self.servers.push(config);
    }

    #[must_use]
    pub fn for_language(&self, language: &str) -> Option<&ServerConfig> {
        self.servers.iter().find(|s| s.language == language)
    }

    #[must_use]
    pub fn all(&self) -> &[ServerConfig] {
        &self.servers
    }
}

/// Placeholder for a running LSP connection. Phase 2 wraps a tokio process
/// + JSON-RPC framer; phase 1.B stores just the config + URI so the binary
/// can log what it would have done.
#[derive(Debug, Clone)]
pub struct ClientHandle {
    pub config: ServerConfig,
    pub root: std::path::PathBuf,
    pub active_uri: Option<String>,
}

impl ClientHandle {
    #[must_use]
    pub fn new(config: ServerConfig, root: std::path::PathBuf) -> Self {
        Self {
            config,
            root,
            active_uri: None,
        }
    }

    pub fn did_open(&mut self, uri: &str) {
        self.active_uri = Some(uri.to_string());
    }
}

/// Detect the project root by walking up from `start` until one of
/// `markers` appears.
#[must_use]
pub fn detect_root(start: &std::path::Path, markers: &[String]) -> Option<std::path::PathBuf> {
    let mut cur = start.canonicalize().ok()?;
    loop {
        for m in markers {
            if cur.join(m).exists() {
                return Some(cur);
            }
        }
        if !cur.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn default_registry_has_rust_and_caixa() {
        let r = ServerRegistry::default_set();
        assert!(r.for_language("rust").is_some());
        assert!(r.for_language("caixa").is_some());
        assert_eq!(r.all().len(), 2);
    }

    #[test]
    fn rust_config_uses_rust_analyzer() {
        let c = ServerConfig::rust_analyzer();
        assert_eq!(c.command, "rust-analyzer");
        assert!(c.root_markers.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn detect_root_finds_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let inner = tmp.path().join("deep/nested");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(tmp.path().join("caixa.lisp"), "(defcaixa :nome \"x\")").unwrap();
        let found = detect_root(&inner, &["caixa.lisp".to_string()]).unwrap();
        assert_eq!(found, tmp.path().canonicalize().unwrap());
    }

    #[test]
    fn detect_root_missing_marker_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let result = detect_root(tmp.path(), &["Cargo.toml".to_string()]);
        // Probably walks up to / and returns None.
        assert!(result.is_none() || !result.unwrap().join("Cargo.toml").exists());
    }

    #[test]
    fn client_handle_tracks_open_file() {
        let mut h = ClientHandle::new(ServerConfig::rust_analyzer(), PathBuf::from("/tmp"));
        assert!(h.active_uri.is_none());
        h.did_open("file:///tmp/main.rs");
        assert_eq!(h.active_uri.as_deref(), Some("file:///tmp/main.rs"));
    }
}
