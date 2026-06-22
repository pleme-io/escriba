# Changelog

All notable changes to the escriba workspace. Versions are cut
automatically on merge to `main` (★★ AUTO-RELEASE: auto-bump patch →
tag `vX.Y.Z` → publish each member crate to crates.io).

## [Unreleased]

## [0.1.9]

### Added
- **Generation-driven plugin-caixa substrate.** Every editor capability
  is a tatara-lisp plugin caixa, emitted from one typed catalog:
  - `escriba-lisp`: the `(defescribaplugin …)` catalog form + a typed
    `Sexp` s-expression emitter (no string-concatenated lisp). Plugin
    names are validated at the parse boundary.
  - `escriba-plugin::forge`: emits each caixa's `caixa.lisp` +
    `escriba/plugin.lisp` + `flake.nix` + the persisted spec.
  - **45 plugin caixas** under `escriba/catalog/` spanning every
    default-on blnvim group; baked into the binary and applied at boot
    (installed-by-default by construction). Composite carries 12 LSP
    servers, 11 formatters, 17 tree-sitter text objects, 5 DAP adapters,
    23 icons, 58 highlights, 93 keybinds.
  - `escriba_runtime::PluginHost`: lazy activation of user plugins on
    `Command` / `FileType` / `Event` triggers.
  - `escriba plugin list / forge / install-bundled`; per-plugin Nix
    toggle `programs.escriba.plugins.<name>.enable`.
  - `tests/plugin_matrix.rs`: forge+apply every plugin, dir↔table
    bijection, capability preservation.

### Fixed
- **crates.io publish contract.** Every git sibling dep in
  `[workspace.dependencies]` now carries a `version` requirement
  alongside its `git` source, so `cargo publish` can strip the git spec
  and resolve from the registry.

## [0.1.8] — interim auto-release

## [0.1.7] — workspace baseline
