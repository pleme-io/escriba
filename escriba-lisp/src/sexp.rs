//! Typed s-expression emitter — the typed render surface escriba-lisp
//! uses to EMIT tatara-lisp source (plugin `caixa.lisp` manifests,
//! generated rc fragments). Mirrors the org-wide ★★ TYPED EMISSION
//! discipline: a generated lisp form is built as a typed [`Sexp`] value
//! and rendered through one canonical writer, never assembled by
//! string concatenation that future edits can silently malform. String
//! atoms are escaped exactly once, here, so an emitted `:descricao`
//! containing a quote or backslash can never produce un-parseable
//! output downstream.
//!
//! This is the EMIT half of the bridge; the PARSE half is
//! `tatara_lisp::compile_typed`. The forge ([`crate::catalog`]) builds
//! `Sexp` values from a typed [`crate::EscribaPluginSpec`] and renders
//! them with [`Sexp::render`]; the matrix test re-parses the output to
//! prove the round-trip closes.

use std::fmt::{self, Write as _};

/// A typed tatara-lisp s-expression. The four shapes the emitter needs:
/// a bare symbol (`Biblioteca`, `#t`), a keyword (`:nome`), a quoted
/// string (escaped), and a list. Numbers render through [`Sexp::sym`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sexp {
    /// A bare symbol rendered verbatim — `Biblioteca`, `#t`, `42`.
    Sym(String),
    /// A `:keyword` — rendered with the leading colon.
    Kw(String),
    /// A `"string"` — rendered quoted, with `\` and `"` escaped.
    Str(String),
    /// A `(… …)` list — rendered space-separated inside parens.
    List(Vec<Sexp>),
}

impl Sexp {
    /// A bare symbol.
    pub fn sym(s: impl Into<String>) -> Self {
        Self::Sym(s.into())
    }
    /// A `:keyword`.
    pub fn kw(s: impl Into<String>) -> Self {
        Self::Kw(s.into())
    }
    /// A quoted, escaped string.
    pub fn str(s: impl Into<String>) -> Self {
        Self::Str(s.into())
    }
    /// A list of children.
    pub fn list(items: impl IntoIterator<Item = Sexp>) -> Self {
        Self::List(items.into_iter().collect())
    }
    /// A list of quoted strings — the common `("a" "b" "c")` shape used
    /// for `:etiquetas`, `:filetypes`, `:keybinds`.
    pub fn str_list(items: impl IntoIterator<Item = String>) -> Self {
        Self::List(items.into_iter().map(Sexp::Str).collect())
    }

    /// Render this s-expression to canonical, flat tatara-lisp.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        // Display can't fail on a String writer; the `let _` is the
        // idiomatic discard.
        let _ = self.write_into(&mut out);
        out
    }

    fn write_into(&self, out: &mut String) -> fmt::Result {
        match self {
            Sexp::Sym(s) => out.write_str(s),
            Sexp::Kw(k) => write!(out, ":{k}"),
            Sexp::Str(s) => {
                out.write_char('"')?;
                for ch in s.chars() {
                    match ch {
                        '\\' => out.write_str("\\\\")?,
                        '"' => out.write_str("\\\"")?,
                        _ => out.write_char(ch)?,
                    }
                }
                out.write_char('"')
            }
            Sexp::List(items) => {
                out.write_char('(')?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.write_char(' ')?;
                    }
                    item.write_into(out)?;
                }
                out.write_char(')')
            }
        }
    }
}

impl fmt::Display for Sexp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The Display impl IS the typed render surface — flat canonical form.
        let mut s = String::new();
        self.write_into(&mut s)?;
        f.write_str(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_atoms() {
        assert_eq!(Sexp::sym("Biblioteca").render(), "Biblioteca");
        assert_eq!(Sexp::kw("nome").render(), ":nome");
        assert_eq!(Sexp::str("hello").render(), "\"hello\"");
    }

    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(
            Sexp::str(r#"a "quote" and \ slash"#).render(),
            r#""a \"quote\" and \\ slash""#,
        );
    }

    #[test]
    fn renders_nested_list() {
        let form = Sexp::list([
            Sexp::sym("defcaixa"),
            Sexp::kw("nome"),
            Sexp::str("escriba-oil"),
            Sexp::kw("kind"),
            Sexp::sym("Biblioteca"),
            Sexp::kw("etiquetas"),
            Sexp::str_list(["escriba-plugin".into(), "files".into()]),
        ]);
        assert_eq!(
            form.render(),
            r#"(defcaixa :nome "escriba-oil" :kind Biblioteca :etiquetas ("escriba-plugin" "files"))"#,
        );
    }

    #[test]
    fn emitted_form_re_parses() {
        // The round-trip contract: anything the emitter renders must be
        // legal tatara-lisp that the reader accepts.
        let form = Sexp::list([
            Sexp::sym("defcaixa"),
            Sexp::kw("nome"),
            Sexp::str("x"),
            Sexp::kw("descricao"),
            Sexp::str(r#"has a "quote""#),
        ]);
        let rendered = form.render();
        let parsed = tatara_lisp::read(&rendered).expect("emitted form must re-parse");
        assert_eq!(parsed.len(), 1, "exactly one top-level form");
    }
}
