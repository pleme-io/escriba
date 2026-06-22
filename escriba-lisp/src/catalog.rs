//! `defescribaplugin` — the catalog/authoring form for an escriba plugin
//! caixa, and the typed emitter that derives a plugin caixa's published
//! artifacts from it.
//!
//! # The compounding move (Pillar 12 — generation over composition)
//!
//! A plugin caixa is **not** hand-authored across four files. It is
//! *emitted* from ONE typed catalog source. A catalog source
//! (`catalog/<name>.escribaplugin.lisp`) is a single tatara-lisp file:
//!
//! ```lisp
//! (defescribaplugin
//!   :name         "escriba-oil"
//!   :version      "0.1.0"
//!   :category     "files"
//!   :description  "Edit the filesystem like a buffer"
//!   :blnvim-origin "stevearc/oil.nvim"
//!   :ativar-em    ("Command: Oil" "Startup")
//!   :priority     0)
//!
//! ;; ── escriba entry: the def-forms this plugin contributes ──
//! (defkeybind :mode "normal" :key "<leader>e" :action "files.open"
//!             :description "open file explorer")
//! (defcmd :name "Oil" :description "open the file explorer"
//!         :action "files.open")
//! ```
//!
//! The first form ([`EscribaPluginSpec`]) is the manifest; everything
//! after it is the *escriba entry* — plain escriba-lisp, the same
//! def-forms a user writes in their rc. [`apply_source`](crate::apply_source)
//! deliberately IGNORES the `defescribaplugin` form (it is not one of
//! the editor's ApplyPlan keywords), so the **same file** is both the
//! authoring spec AND a valid plugin entry. The forge then emits:
//!
//! - `caixa.lisp` — the `:kind Biblioteca` caixa manifest (typed, via
//!   [`emit_caixa_lisp`]).
//! - `escriba/plugin.lisp` — the entry escriba LOADS + APPLIES.
//! - `flake.nix` — minimal nix packaging (in `escriba-plugin`).
//! - the persisted catalog source itself — so re-rendering is
//!   idempotent + drift-detectable (CLOSED-LOOP MASS-SYNTHESIS rule #3).

use serde::{Deserialize, Serialize};
use tatara_lisp::DeriveTataraDomain;

use crate::plugin::is_known_category;
use crate::sexp::Sexp;

/// One catalog entry — the manifest of a single escriba plugin caixa.
///
/// Authored as `(defescribaplugin :name … :category … :ativar-em …)`.
/// The forge reads the FIRST such form in a catalog source; the rest of
/// the file is the plugin's escriba entry.
#[derive(DeriveTataraDomain, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tatara(keyword = "defescribaplugin")]
pub struct EscribaPluginSpec {
    /// Caixa name — the plugins-dir directory + the `defplugin :name`
    /// descriptor it generates. Conventionally `escriba-<thing>`.
    pub name: String,
    /// Semver. Empty → `0.1.0` at emit time (see [`Self::effective_version`]).
    #[serde(default)]
    pub version: String,
    /// One-line purpose, becomes `:descricao`.
    #[serde(default)]
    pub description: String,
    /// Feature group mirroring blnvim's nine groups + escriba additions.
    /// See [`KNOWN_CATEGORIES`](crate::KNOWN_CATEGORIES).
    #[serde(default)]
    pub category: String,
    /// The upstream blnvim / Neovim plugin this caixa mirrors, e.g.
    /// `"folke/trouble.nvim"`. Empty for escriba-original capability.
    /// Recorded in the caixa `:etiquetas` for provenance + the parity
    /// audit.
    #[serde(default)]
    pub blnvim_origin: String,
    /// lazy.nvim-style activation triggers, in `escriba-plugin`'s
    /// `:ativar-em` vocabulary: `"Startup"` / `"eager"`,
    /// `"FileType: rust"`, `"Event: LspAttach"`, `"Command: Trouble"`.
    /// Empty ⇒ eager (apply at startup).
    #[serde(default)]
    pub ativar_em: Vec<String>,
    /// Extra caixa tags beyond the auto-added `"escriba-plugin"` +
    /// category + origin.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Load-order hint (colorschemes want high). Emitted onto the
    /// generated `(defplugin … :priority N)` descriptor.
    #[serde(default)]
    pub priority: i32,
}

impl EscribaPluginSpec {
    /// The version string to emit — `version` if set, else `0.1.0`.
    #[must_use]
    pub fn effective_version(&self) -> &str {
        if self.version.is_empty() {
            "0.1.0"
        } else {
            &self.version
        }
    }

    /// Is this plugin eager (applied at startup)? Eager iff there are no
    /// triggers, or any trigger is `Startup`/`eager`. Mirrors
    /// `escriba_plugin::PluginCaixa::is_eager` without depending on it
    /// (lineage: escriba-plugin depends on escriba-lisp, not the
    /// reverse).
    #[must_use]
    pub fn is_eager(&self) -> bool {
        self.ativar_em.is_empty()
            || self
                .ativar_em
                .iter()
                .any(|t| t.trim().eq_ignore_ascii_case("startup") || t.trim().eq_ignore_ascii_case("eager"))
    }

    /// The caixa `:etiquetas` set: always `"escriba-plugin"`, then the
    /// category (if any), then the blnvim origin (if any), then extra
    /// tags. Deduplicated, order-stable.
    #[must_use]
    pub fn etiquetas(&self) -> Vec<String> {
        let mut out = vec!["escriba-plugin".to_string()];
        let mut push = |s: &str| {
            if !s.is_empty() && !out.iter().any(|x| x == s) {
                out.push(s.to_string());
            }
        };
        push(&self.category);
        push(&self.blnvim_origin);
        for t in &self.tags {
            push(t);
        }
        out
    }
}

/// Render the `caixa.lisp` manifest for this plugin — a `:kind
/// Biblioteca` caixa whose identity, version, and provenance come
/// entirely from the typed spec. Built as typed [`Sexp`] values, so
/// strings are escaped exactly once and the output always re-parses.
#[must_use]
pub fn emit_caixa_lisp(spec: &EscribaPluginSpec) -> String {
    let nome = Sexp::str(spec.name.clone());
    let versao = Sexp::str(spec.effective_version().to_string());
    let descricao = Sexp::str(spec.description.clone());
    let etiquetas = Sexp::str_list(spec.etiquetas());

    // Lay the form out one keyword per line for readable diffs — each
    // `:kw value` pair rendered through the typed emitter, never
    // string-concatenated.
    let header = "(defcaixa";
    let lines = [
        format!("          {} {}", Sexp::kw("nome"), nome),
        format!("          {} {}", Sexp::kw("versao"), versao),
        format!("          {} {}", Sexp::kw("kind"), Sexp::sym("Biblioteca")),
        format!("          {} {}", Sexp::kw("descricao"), descricao),
        format!("          {} {}", Sexp::kw("etiquetas"), etiquetas),
    ];
    let mut out = String::from(
        ";; GENERATED by `escriba plugin forge` from the catalog source.\n\
         ;; Edit the `.escribaplugin.lisp` catalog source, never this file.\n",
    );
    out.push_str(header);
    out.push(' ');
    // First pair on the same line as `(defcaixa`.
    out.push_str(lines[0].trim_start());
    for line in &lines[1..] {
        out.push('\n');
        out.push_str(line);
    }
    out.push_str(")\n");
    out
}

/// Generate the `(defplugin …)` descriptor (escriba-lisp's lazy.nvim
/// shape) that represents this catalog entry in the bundled defaults
/// plan — so the rc's plugin list never drifts from the catalog. Built
/// as a typed [`Sexp`].
#[must_use]
pub fn emit_defplugin_descriptor(spec: &EscribaPluginSpec) -> String {
    let mut items = vec![
        Sexp::sym("defplugin"),
        Sexp::kw("name"),
        Sexp::str(spec.name.clone()),
    ];
    if !spec.description.is_empty() {
        items.push(Sexp::kw("description"));
        items.push(Sexp::str(spec.description.clone()));
    }
    if !spec.category.is_empty() {
        items.push(Sexp::kw("category"));
        items.push(Sexp::str(spec.category.clone()));
    }
    // Lower the typed triggers to the defplugin on-* / lazy fields.
    for trig in &spec.ativar_em {
        if let Some((kind, arg)) = trig.split_once(':') {
            let arg = arg.trim();
            match kind.trim().to_ascii_lowercase().as_str() {
                "event" => {
                    items.push(Sexp::kw("on-event"));
                    items.push(Sexp::str(arg.to_string()));
                }
                "command" | "cmd" => {
                    items.push(Sexp::kw("on-command"));
                    items.push(Sexp::str(arg.to_string()));
                }
                "filetype" | "ft" => {
                    items.push(Sexp::kw("on-filetype"));
                    items.push(Sexp::str(arg.to_string()));
                }
                _ => {}
            }
        }
    }
    if spec.priority != 0 {
        items.push(Sexp::kw("priority"));
        items.push(Sexp::sym(spec.priority.to_string()));
    }
    if !spec.is_eager() {
        items.push(Sexp::kw("lazy"));
        items.push(Sexp::sym("#t"));
    }
    Sexp::list(items).render()
}

/// Validation outcome for a catalog source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// No `(defescribaplugin …)` form found.
    MissingMeta,
    /// More than one `(defescribaplugin …)` form — a catalog source is
    /// exactly one plugin.
    MultipleMeta(usize),
    /// `:name` is empty.
    EmptyName,
    /// `:name` is not a clean slug — it must be a directory-safe,
    /// nix-string-safe identifier (`[A-Za-z0-9_-]+`). This makes a
    /// quote / path-separator / whitespace-bearing name *unrepresentable*
    /// downstream (it can't break the emitted `caixa.lisp`, `flake.nix`,
    /// or the on-disk directory path).
    InvalidName(String),
    /// Parse error from the underlying tatara-lisp reader.
    Parse(String),
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::MissingMeta => {
                f.write_str("catalog source has no (defescribaplugin …) manifest form")
            }
            CatalogError::MultipleMeta(n) => {
                write!(f, "catalog source has {n} (defescribaplugin …) forms — expected exactly 1")
            }
            CatalogError::EmptyName => f.write_str("(defescribaplugin …) has empty :name"),
            CatalogError::InvalidName(n) => write!(
                f,
                "(defescribaplugin …) :name `{n}` is not a clean slug \
                 (allowed: letters, digits, `-`, `_`)"
            ),
            CatalogError::Parse(e) => write!(f, "catalog parse error: {e}"),
        }
    }
}

impl std::error::Error for CatalogError {}

/// Read the single [`EscribaPluginSpec`] manifest out of a catalog
/// source. Errors if there are zero or more than one. The rest of the
/// source (the escriba entry) is the caller's business — it is the same
/// text, applied via [`apply_source`](crate::apply_source).
pub fn read_catalog_meta(src: &str) -> Result<EscribaPluginSpec, CatalogError> {
    let specs: Vec<EscribaPluginSpec> =
        tatara_lisp::compile_typed(src).map_err(|e| CatalogError::Parse(e.to_string()))?;
    match specs.len() {
        0 => Err(CatalogError::MissingMeta),
        1 => {
            let spec = specs.into_iter().next().expect("len == 1");
            if spec.name.is_empty() {
                return Err(CatalogError::EmptyName);
            }
            if !spec
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(CatalogError::InvalidName(spec.name));
            }
            Ok(spec)
        }
        n => Err(CatalogError::MultipleMeta(n)),
    }
}

/// Best-effort note on whether a catalog entry's category is one of the
/// canonical groups. Unknown categories are allowed (forward-compat),
/// but the matrix test surfaces them so a typo doesn't silently create
/// an orphan group.
#[must_use]
pub fn category_is_canonical(spec: &EscribaPluginSpec) -> bool {
    spec.category.is_empty() || is_known_category(&spec.category)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        (defescribaplugin
          :name "escriba-oil"
          :version "0.2.0"
          :category "files"
          :description "Edit the filesystem like a buffer"
          :blnvim-origin "stevearc/oil.nvim"
          :ativar-em ("Command: Oil"))

        (defkeybind :mode "normal" :key "<leader>e" :action "files.open")
        (defcmd :name "Oil" :description "file explorer" :action "files.open")
    "#;

    #[test]
    fn reads_meta_and_ignores_entry_forms() {
        let spec = read_catalog_meta(SAMPLE).expect("one meta form");
        assert_eq!(spec.name, "escriba-oil");
        assert_eq!(spec.version, "0.2.0");
        assert_eq!(spec.category, "files");
        assert_eq!(spec.ativar_em, vec!["Command: Oil"]);
        assert!(!spec.is_eager(), "a Command-triggered plugin is lazy");
    }

    #[test]
    fn apply_source_ignores_the_meta_form() {
        // The same catalog source is a valid escriba entry: apply_source
        // skips defescribaplugin and applies the def-forms.
        let plan = crate::apply_source(SAMPLE).expect("entry applies");
        assert_eq!(plan.keybinds.len(), 1);
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.keybinds[0].action, "files.open");
    }

    #[test]
    fn missing_meta_errors() {
        let err = read_catalog_meta("(defkeybind :mode \"normal\" :key \"x\" :action \"y\")")
            .expect_err("no meta");
        assert_eq!(err, CatalogError::MissingMeta);
    }

    #[test]
    fn rejects_non_slug_name() {
        // A quote / slash / space-bearing name is rejected at the parse
        // boundary, so it can never reach the caixa.lisp / flake.nix
        // emitters or an on-disk directory path (unrepresentability).
        for bad in [
            r#"(defescribaplugin :name "bad name")"#,
            r#"(defescribaplugin :name "bad/slash")"#,
            r#"(defescribaplugin :name "x.y")"#,
        ] {
            let err = read_catalog_meta(bad).expect_err("non-slug name rejected");
            assert!(matches!(err, CatalogError::InvalidName(_)), "for {bad}");
        }
        // A clean slug is accepted.
        assert!(read_catalog_meta(r#"(defescribaplugin :name "escriba-ok_1")"#).is_ok());
    }

    #[test]
    fn multiple_meta_errors() {
        let src = r#"
            (defescribaplugin :name "a")
            (defescribaplugin :name "b")
        "#;
        let err = read_catalog_meta(src).expect_err("two metas");
        assert!(matches!(err, CatalogError::MultipleMeta(2)));
    }

    #[test]
    fn emit_caixa_lisp_re_parses_as_defcaixa() {
        let spec = read_catalog_meta(SAMPLE).unwrap();
        let manifest = emit_caixa_lisp(&spec);
        let forms = tatara_lisp::read(&manifest).expect("emitted caixa.lisp re-parses");
        // One (defcaixa …) form (comments are not forms).
        assert_eq!(forms.len(), 1);
        assert!(manifest.contains("Biblioteca"));
        assert!(manifest.contains("escriba-oil"));
        // Provenance lands in the tags.
        assert!(manifest.contains("stevearc/oil.nvim"));
    }

    #[test]
    fn etiquetas_dedup_and_order() {
        let spec = EscribaPluginSpec {
            name: "x".into(),
            version: String::new(),
            description: String::new(),
            category: "files".into(),
            blnvim_origin: "stevearc/oil.nvim".into(),
            ativar_em: vec![],
            tags: vec!["files".into(), "extra".into()],
            priority: 0,
        };
        // "files" appears in both category + tags but is deduped.
        assert_eq!(
            spec.etiquetas(),
            vec!["escriba-plugin", "files", "stevearc/oil.nvim", "extra"],
        );
    }

    #[test]
    fn emit_defplugin_descriptor_lowers_triggers() {
        let spec = read_catalog_meta(SAMPLE).unwrap();
        let desc = emit_defplugin_descriptor(&spec);
        // Re-parses as a defplugin and carries the lazy + on-command shape.
        let plan = crate::apply_source(&desc).unwrap();
        assert_eq!(plan.plugins.len(), 1);
        assert_eq!(plan.plugins[0].name, "escriba-oil");
        assert_eq!(plan.plugins[0].on_command, "Oil");
        assert!(plan.plugins[0].lazy);
    }

    #[test]
    fn effective_version_defaults() {
        let mut spec = read_catalog_meta(SAMPLE).unwrap();
        spec.version = String::new();
        assert_eq!(spec.effective_version(), "0.1.0");
    }
}
