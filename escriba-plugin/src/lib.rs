//! `escriba-plugin` — manifest + discovery. Phase 1 skeleton.

extern crate self as escriba_plugin;

use escriba_config::PluginDecl;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedPlugin {
    pub decl: PluginDecl,
    pub activated: bool,
}

#[must_use]
pub fn discover(decls: Vec<PluginDecl>) -> Vec<LoadedPlugin> {
    decls
        .into_iter()
        .map(|decl| LoadedPlugin {
            decl,
            activated: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_lifts_decls() {
        let decl = PluginDecl {
            caixa: "escriba-paredit".into(),
            versao: "^0.1".into(),
            ativar_em: vec!["FileType: lisp".into()],
        };
        let loaded = discover(vec![decl]);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].decl.caixa, "escriba-paredit");
        assert!(!loaded[0].activated);
    }
}
