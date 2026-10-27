//! The fleet formatter registry's route into escriba.
//!
//! The HM module renders one `(defformatter … :external #t)` per registry
//! language, with the store-path command and argv CI's canonical checks run.
//! These tests pin the three facts that make that the format command's answer:
//! the form reads into exactly that command and argv, a rendered entry
//! replaces a catalog one for the same filetype, and the bundled catalog alone
//! routes nothing externally (its entries stay declarations).

use std::path::Path;

use escriba_lsp_client::external::{self, External};

const RENDERED: &str = r#"
(defformatter :filetype "nix" :command "/nix/store/x-nixfmt/bin/nixfmt" :args ("--strict") :external #t)
(defformatter :filetype "rust" :command "/nix/store/x-rust/bin/rustfmt" :args ("--emit=stdout") :external #t :cargo-edition "2015")
(defformatter :filetype "blue" :command "/nix/store/x-blue/bin/blue" :args ("fmt" "--write" "$FILE") :external #t :file-mode #t)
"#;

fn externals(src: &str) -> Vec<(String, External)> {
    let plan = escriba_lisp::apply_source(src).expect("the rendered forms read");
    escriba::external_formatters(&plan)
}

#[test]
fn a_rendered_entry_reads_into_its_command_and_argv() {
    let got = externals(RENDERED);
    let nix = &got.iter().find(|(ft, _)| ft == "nix").unwrap().1;
    assert_eq!(nix.command, "/nix/store/x-nixfmt/bin/nixfmt");
    assert_eq!(nix.args, vec!["--strict"]);
    assert!(!nix.file_mode);
    assert_eq!(nix.cargo_edition, None);

    let rust = &got.iter().find(|(ft, _)| ft == "rust").unwrap().1;
    assert_eq!(rust.cargo_edition.as_deref(), Some("2015"));

    let blue = &got.iter().find(|(ft, _)| ft == "blue").unwrap().1;
    assert!(blue.file_mode);
    assert_eq!(blue.args, vec!["fmt", "--write", "$FILE"]);
}

#[test]
fn the_bundled_catalog_alone_routes_nothing_externally() {
    let plan = escriba::default_plan(false).expect("the default boot plan builds");
    assert!(
        escriba::external_formatters(&plan).is_empty(),
        "a catalog defformatter became live without :external"
    );
}

#[test]
fn a_rendered_entry_replaces_the_catalog_one() {
    let src =
        format!("(defformatter :filetype \"nix\" :command \"alejandra\" :external #t)\n{RENDERED}");
    let map: std::collections::HashMap<_, _> = externals(&src).into_iter().collect();
    assert_eq!(map["nix"].command, "/nix/store/x-nixfmt/bin/nixfmt");
}

#[test]
fn the_external_route_formats_the_buffer_text() {
    let src = r#"(defformatter :filetype "txt" :command "tr" :args ("a-z" "A-Z") :external #t)"#;
    let (_, spec) = externals(src).remove(0);
    assert_eq!(
        external::format(&spec, Path::new("/x/a.txt"), "ff\n").unwrap(),
        "FF\n"
    );
}
