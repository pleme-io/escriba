//! `escriba-config` — tatara-lisp editor config. Every top-level config
//! form is a TataraDomain: `defescriba` / `defkeymap` / `defcommand` /
//! `defplugin` / `defmajor-mode` / `defminor-mode`.

extern crate self as escriba_config;

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

#[derive(
    DeriveTataraDomain,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    Debug,
    Clone,
    PartialEq,
    Default,
)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defescriba")]
pub struct EscribaConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeros_linha: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeros_relativos: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub largura_tab: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quebra_suave: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mostrar_statusline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mostrar_tabbar: Option<bool>,
}

#[derive(
    DeriveTataraDomain, Serialize, Deserialize, schemars::JsonSchema, Debug, Clone, PartialEq,
)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defkeymap")]
pub struct KeymapDecl {
    pub modo: String,
    pub tecla: String,
    pub comando: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descricao: Option<String>,
}

#[derive(
    DeriveTataraDomain, Serialize, Deserialize, schemars::JsonSchema, Debug, Clone, PartialEq,
)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defcommand")]
pub struct CommandDecl {
    pub nome: String,
    pub descricao: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(
    DeriveTataraDomain, Serialize, Deserialize, schemars::JsonSchema, Debug, Clone, PartialEq,
)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defplugin")]
pub struct PluginDecl {
    pub caixa: String,
    pub versao: String,
    #[serde(default)]
    pub ativar_em: Vec<String>,
}

#[derive(
    DeriveTataraDomain, Serialize, Deserialize, schemars::JsonSchema, Debug, Clone, PartialEq,
)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defmajor-mode")]
pub struct MajorMode {
    pub nome: String,
    #[serde(default)]
    pub extensoes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estrutural_lisp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tamanho_indent: Option<i64>,
}

#[derive(
    DeriveTataraDomain, Serialize, Deserialize, schemars::JsonSchema, Debug, Clone, PartialEq,
)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defminor-mode")]
pub struct MinorMode {
    pub nome: String,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descricao: Option<String>,
}

impl EscribaConfig {
    pub fn from_lisp(src: &str) -> Result<Self, tatara_lisp::LispError> {
        use tatara_lisp::domain::TataraDomain;
        let forms = tatara_lisp::read(src)?;
        let first = forms
            .first()
            .ok_or_else(|| tatara_lisp::LispError::Compile {
                form: "defescriba".into(),
                message: "empty config".into(),
            })?;
        Self::compile_from_sexp(first)
    }

    pub fn register_all() {
        tatara_lisp::domain::register::<Self>();
        tatara_lisp::domain::register::<KeymapDecl>();
        tatara_lisp::domain::register::<CommandDecl>();
        tatara_lisp::domain::register::<PluginDecl>();
        tatara_lisp::domain::register::<MajorMode>();
        tatara_lisp::domain::register::<MinorMode>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defescriba() {
        let src = r#"(defescriba :tema "nord" :numeros-linha #t :largura-tab 2)"#;
        let c = EscribaConfig::from_lisp(src).unwrap();
        assert_eq!(c.tema.as_deref(), Some("nord"));
        assert_eq!(c.numeros_linha, Some(true));
        assert_eq!(c.largura_tab, Some(2));
    }

    #[test]
    fn parses_defkeymap() {
        use tatara_lisp::domain::TataraDomain;
        let forms = tatara_lisp::read(
            r#"(defkeymap :modo "Normal" :tecla "<leader>w" :comando "save" :descricao "save")"#,
        )
        .unwrap();
        let k = KeymapDecl::compile_from_sexp(&forms[0]).unwrap();
        assert_eq!(k.modo, "Normal");
        assert_eq!(k.comando, "save");
    }

    #[test]
    fn parses_defmajor_mode_with_structural_lisp() {
        use tatara_lisp::domain::TataraDomain;
        let forms = tatara_lisp::read(
            r#"(defmajor-mode :nome "lisp" :extensoes ("lisp" "el" "clj") :estrutural-lisp #t)"#,
        )
        .unwrap();
        let m = MajorMode::compile_from_sexp(&forms[0]).unwrap();
        assert_eq!(m.nome, "lisp");
        assert_eq!(m.estrutural_lisp, Some(true));
    }

    #[test]
    fn register_all_populates_registry() {
        EscribaConfig::register_all();
        let kws = tatara_lisp::domain::registered_keywords();
        for keyword in [
            "defescriba",
            "defkeymap",
            "defcommand",
            "defplugin",
            "defmajor-mode",
            "defminor-mode",
        ] {
            assert!(kws.contains(&keyword), "missing keyword: {keyword}");
        }
    }
}
