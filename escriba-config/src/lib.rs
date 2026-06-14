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

// ── shikumi::TieredConfig — fleet-wide tier model (M-166 backfill) ──
//
// Operators reach via:
//   ESCRIBA_TIER=bare escriba ...
//   ESCRIBA_TIER=default escriba ...
//
// Prior migrations: tatara, zoekt-mcp, kindling, ayatsuri, kenshi,
// taimen. See `shikumi/src/tiered.rs` for the trait contract.
//
// EscribaConfig is currently all-Option: today bare() and
// prescribed_default() are byte-identical (no curated defaults are
// shipped yet — the operator's lisp file IS the prescription). The
// impl pins the structural contract; once defaults arrive (eg.
// largura_tab: Some(4)) the diff fans out from one place.

impl shikumi::TieredConfig for EscribaConfig {
    /// Tier 0 — bare: zero-opinion floor. Every field None.
    fn bare() -> Self {
        Self {
            tema: None,
            numeros_linha: None,
            numeros_relativos: None,
            largura_tab: None,
            quebra_suave: None,
            mostrar_statusline: None,
            mostrar_tabbar: None,
        }
    }

    /// Tier 2 — prescribed: the curated defaults that ship today.
    /// The fleet theme `vellum` (warm aged-paper Nord-matte) is the
    /// prescribed `:tema`, matching `ishou_tokens::FleetTheme::Vellum`
    /// — so an operator who sets no theme lands on the fleet look. The
    /// remaining fields stay `None` (the operator's lisp prescribes them).
    fn prescribed_default() -> Self {
        Self {
            tema: Some("vellum".into()),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tiered_tests {
    use super::*;
    use shikumi::{ConfigTier, TieredConfig};

    #[test]
    fn escriba_config_bare_is_zero_opinion() {
        let b = <EscribaConfig as TieredConfig>::bare();
        assert!(b.tema.is_none());
        assert!(b.numeros_linha.is_none());
        assert!(b.numeros_relativos.is_none());
        assert!(b.largura_tab.is_none());
        assert!(b.quebra_suave.is_none());
        assert!(b.mostrar_statusline.is_none());
        assert!(b.mostrar_tabbar.is_none());
    }

    #[test]
    fn escriba_config_prescribed_pins_vellum_theme() {
        // The prescribed default carries the fleet theme; every other
        // field stays None (the operator's lisp prescribes them).
        let p = <EscribaConfig as TieredConfig>::prescribed_default();
        assert_eq!(p.tema.as_deref(), Some("vellum"));
        assert!(p.numeros_linha.is_none());
        assert!(p.largura_tab.is_none());
        // Prescribed differs from the all-None bare floor only by the theme.
        let bare = <EscribaConfig as TieredConfig>::bare();
        assert_ne!(p, bare);
    }

    #[test]
    fn escriba_config_resolve_tier_dispatches() {
        // Bare is zero-opinion; Default pins the vellum theme.
        let bare = <EscribaConfig as TieredConfig>::resolve_tier(ConfigTier::Bare);
        let default = <EscribaConfig as TieredConfig>::resolve_tier(ConfigTier::Default);
        assert_eq!(bare, <EscribaConfig as TieredConfig>::bare());
        assert_eq!(default, <EscribaConfig as TieredConfig>::prescribed_default());
        assert_eq!(default.tema.as_deref(), Some("vellum"));
    }

    #[test]
    fn escriba_config_diff_against_self_is_empty() {
        // The diff machinery: a value diffed against itself produces
        // an empty diff.
        let p = <EscribaConfig as TieredConfig>::prescribed_default();
        assert!(p.diff_against(&p).is_empty_diff());
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
