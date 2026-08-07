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

    /// Claim escriba's `def…` keywords in the process-wide tatara-lisp
    /// registry.
    ///
    /// Returns the FIRST collision rather than swallowing it. tatara-lisp
    /// 0.3.14 made `register` fallible for a reason worth restating: one
    /// keyword belongs to one type per process, and a refusal means some
    /// other type is already answering to a `def…` form escriba believes it
    /// owns. Discarding that result — which this function did — leaves the
    /// editor parsing operator config with the WRONG domain handler and no
    /// indication anywhere that it happened. Re-registering the same type is
    /// idempotent upstream, so a repeat call is still `Ok`.
    pub fn register_all() -> Result<(), tatara_lisp::domain::KeywordCollision> {
        tatara_lisp::domain::register::<Self>()?;
        tatara_lisp::domain::register::<KeymapDecl>()?;
        tatara_lisp::domain::register::<CommandDecl>()?;
        tatara_lisp::domain::register::<PluginDecl>()?;
        tatara_lisp::domain::register::<MajorMode>()?;
        tatara_lisp::domain::register::<MinorMode>()?;
        Ok(())
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
// bare() is the all-None zero-opinion floor. prescribed_default() now
// mirrors the shipped `configs/blnvim-defaults.lisp` baseline (theme +
// numbers + tab-width 2 + statusline) so `escriba config-show default`
// reports what actually boots. The `.lisp` remains the load-bearing
// prescription (it carries the keymaps/modes/highlights this 7-field
// struct cannot express); this struct is the operator-facing summary.

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

    /// Tier 2 — prescribed: the curated defaults that ship today. These
    /// MIRROR the load-bearing `configs/blnvim-defaults.lisp` baseline the
    /// editor actually boots (line numbers + relative numbers on; tab width
    /// 2; no soft wrap; statusline on; tabbar off — `showtabline=0`, blnvim
    /// parity). The `.lisp` remains the load-bearing prescription; this keeps
    /// `escriba config-show default` honest about what ships.
    ///
    /// **The theme is DERIVED, not spelled.** It used to read `"vellum"`
    /// while `configs/blnvim-defaults.lisp` declared `(deftheme :preset
    /// "nord")` and the paint path resolved `FleetTheme::prescribed_default()`
    /// — so `config-show default` named a theme the editor never booted with.
    /// Sourcing it from ishou means the fleet moving its prescribed theme
    /// moves this too, with no edit here and no window where they disagree.
    fn prescribed_default() -> Self {
        Self {
            tema: Some(
                ishou_tokens::FleetTheme::prescribed_default()
                    .preset_name()
                    .to_string(),
            ),
            numeros_linha: Some(true),
            numeros_relativos: Some(true),
            largura_tab: Some(2),
            quebra_suave: Some(false),
            mostrar_statusline: Some(true),
            mostrar_tabbar: Some(false),
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
    fn escriba_config_prescribed_mirrors_blnvim_baseline() {
        // The prescribed default mirrors the shipped blnvim-defaults.lisp so
        // `escriba config-show default` reflects the real boot baseline.
        let p = <EscribaConfig as TieredConfig>::prescribed_default();
        // Asserted against the FLEET, not a literal. A literal here is what
        // let this report `vellum` for as long as it did: the string was
        // pinned by a test, so the drift looked deliberate.
        assert_eq!(
            p.tema.as_deref(),
            Some(ishou_tokens::FleetTheme::prescribed_default().preset_name()),
        );
        assert_eq!(p.numeros_linha, Some(true));
        assert_eq!(p.numeros_relativos, Some(true));
        assert_eq!(p.largura_tab, Some(2));
        assert_eq!(p.quebra_suave, Some(false));
        assert_eq!(p.mostrar_statusline, Some(true));
        assert_eq!(p.mostrar_tabbar, Some(false));
        // Prescribed differs from the all-None bare floor.
        let bare = <EscribaConfig as TieredConfig>::bare();
        assert_ne!(p, bare);
    }

    #[test]
    fn escriba_config_resolve_tier_dispatches() {
        // Bare is zero-opinion; Default pins the fleet-prescribed theme.
        let bare = <EscribaConfig as TieredConfig>::resolve_tier(ConfigTier::Bare);
        let default = <EscribaConfig as TieredConfig>::resolve_tier(ConfigTier::Default);
        assert_eq!(bare, <EscribaConfig as TieredConfig>::bare());
        assert_eq!(
            default,
            <EscribaConfig as TieredConfig>::prescribed_default()
        );
        assert_eq!(
            default.tema.as_deref(),
            Some(ishou_tokens::FleetTheme::prescribed_default().preset_name()),
        );
    }

    /// The three places escriba states a default theme must state the SAME
    /// one: this tiered config, the shipped `configs/blnvim-defaults.lisp`,
    /// and the paint path's `ChromePalette::prescribed()`.
    ///
    /// They disagreed. `config-show default` said `vellum`, the lisp said
    /// `nord`, and the screen showed Nord — a report that was wrong about
    /// the editor it describes. Nothing compared them, so nothing caught it.
    #[test]
    fn every_statement_of_the_default_theme_agrees() {
        let fleet = ishou_tokens::FleetTheme::prescribed_default();
        let from_config = <EscribaConfig as TieredConfig>::prescribed_default()
            .tema
            .expect("the prescribed tier names a theme");
        assert_eq!(
            from_config,
            fleet.preset_name(),
            "the tiered config must name the fleet-prescribed theme",
        );

        // And the shipped lisp — the load-bearing prescription — must declare
        // it too. Read from the file the binary bakes in, so an edit there
        // that forgets this file fails HERE.
        let lisp = include_str!("../../escriba/configs/blnvim-defaults.lisp");
        let declared = lisp
            .lines()
            .find_map(|l| {
                let l = l.trim();
                l.strip_prefix("(deftheme :preset ")
                    .map(|r| r.trim_end_matches(')').trim().trim_matches('"').to_string())
            })
            .expect("the shipped defaults declare a theme");
        assert_eq!(
            declared,
            fleet.preset_name(),
            "configs/blnvim-defaults.lisp declares a different theme than the \
             one escriba reports as its default",
        );
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
        EscribaConfig::register_all().expect("escriba's own keywords must not collide");
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

    #[test]
    fn registering_twice_is_idempotent_not_a_collision() {
        // The registry is process-wide and other tests in this binary also
        // register. If a repeat call reported a collision, startup would fail
        // for a program that merely initialised twice.
        EscribaConfig::register_all().expect("first");
        EscribaConfig::register_all().expect("a repeat call is idempotent");
    }

    /// The RED RUN for the collision path: a deliberately-broken input — a
    /// second type claiming a keyword escriba already owns — must be refused
    /// and NAMED.
    ///
    /// Without this the fallible signature would be decoration: nothing else
    /// in the suite ever produces an `Err`, so a `register` that silently
    /// started returning `Ok` on collision would go unnoticed.
    #[test]
    fn a_second_type_claiming_an_escriba_keyword_is_refused() {
        use tatara_lisp::domain::TataraDomain;

        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
        struct Impostor {
            nome: String,
        }
        impl TataraDomain for Impostor {
            const KEYWORD: &'static str = "defkeymap"; // already KeymapDecl's
            fn compile_from_args(_args: &[tatara_lisp::Sexp]) -> Result<Self, tatara_lisp::LispError> {
                Ok(Self {
                    nome: String::new(),
                })
            }
        }

        EscribaConfig::register_all().expect("escriba's own keywords register");
        let err = tatara_lisp::domain::register::<Impostor>()
            .expect_err("a different type must NOT be allowed to take `defkeymap`");
        assert_eq!(err.keyword, "defkeymap");
        assert!(
            err.challenger.contains("Impostor"),
            "the refusal must name who was turned away: {err}",
        );
    }
}
