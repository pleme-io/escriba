# escriba plugin substrate — caixa-native, generation-driven, blnvim-parity

> **Destination first.** This document leads with the long-term shape
> (Operating Principle #0), then the path that got here. The destination
> is: **every escriba editor capability is a tatara-lisp plugin caixa,
> emitted from one typed catalog, installed by default, reconciled with
> Nix + module support, and proven by a verification matrix.** This is
> Pillar 12 (generation over composition) + ★★ CLOSED-LOOP
> MASS-SYNTHESIS + ★★ CATALOG REFLECTION applied to the editor-plugin
> domain.

## The one-sentence model

A plugin is **authored once** as a `(defescribaplugin …)` catalog source
(`escriba/catalog/<name>.escribaplugin.lisp`); a **forge** mechanically
emits its complete, installable caixa (`caixa.lisp` + `escriba/plugin.lisp`
+ `flake.nix` + the persisted spec); the catalog is **baked into the
binary** so the plugins are installed-by-default by construction; a
**verification matrix** fails the build if any catalog file is malformed
or missing from the baked table.

```
catalog/<name>.escribaplugin.lisp        ← THE SPEC (one authored file)
  │   (defescribaplugin :name … :category … :ativar-em …)   ← manifest
  │   (defkeybind …) (defcmd …) (deflsp …) (defhighlight …) ← escriba entry
  ▼
escriba_plugin::forge_plugin(source)     ← typed emitter (Sexp, no string concat)
  ├── caixa.lisp            (:kind Biblioteca manifest)
  ├── escriba/plugin.lisp   (the entry escriba LOADS + APPLIES)
  ├── flake.nix             (per-plugin nix packaging)
  └── <name>.escribaplugin.lisp  (the spec, persisted next to output)
  ▼
catalog_bundle::BUNDLED      ← baked into the binary (include_str!)
  ▼   bundled_plan()  — eager: every entry merged into the boot plan
EditorState                  ← keybinds/commands/options/highlights/lsp live
```

## Why a plugin IS a caixa (and authored in tatara-lisp)

Per the org `Rust + Lisp` doctrine: **Rust owns invariants + execution,
Lisp owns authoring.** A plugin's authoring surface is exactly the
escriba-lisp def-forms a user already writes in their rc
(`defkeybind` / `defcmd` / `defoption` / `defhighlight` / `deflsp` /
`defformatter` / `deftextobject` / `deffold` / `defdap` / `deficon` /
`defmcp` / `defsnippet` / …). So "a plugin is just authored editor
config, shipped as a caixa." The `:kind Biblioteca` caixa gives it fleet
identity (git-as-registry publish/consume via `feira`); escriba loads the
*entry* at `escriba/plugin.lisp`. The `defescribaplugin` manifest form is
inert at apply time (`escriba_lisp::apply_source` ignores it), so the
**same file** is both the authoring spec and a valid plugin entry.

## The 45-caixa parity catalog

Every default-on blnvim capability has an escriba plugin caixa, grouped
by blnvim's feature groups:

| group | caixas |
|---|---|
| common | which-key, comment, todo-comments, surround, autopairs, leap, overseer |
| files | oil, tree |
| git | gitsigns, fugitive, git-conflict |
| lsp | lspconfig, lspsaga, trouble, lsp-signature, tiny-inline-diagnostic, illuminate, mason, conform, dap, neotest, helm |
| completion | cmp, lspkind, luasnip |
| telescope | telescope, compass (absorbs vim-tmux-navigator) |
| treesitter | treesitter (core), treesitter-textobjects, treesitter-fold, treesitter-context, ts-autotag, ts-context-commentstring, render-markdown |
| theming | nord, lualine, bufferline, noice, snacks, notify, indent-blankline, colorizer, devicons |
| ai | mcp-assist |

The composite plan carries **12 LSP servers, 11 formatters, 17 tree-sitter
text objects, 5 DAP adapters, 23 icons, 5 MCP-tool bindings, 5 fold
rules, 58 highlight groups, 93 keybinds, 42 commands** — sourced entirely
from the caixas, not the monolithic defaults.

> **Parity note.** vim-tmux-navigator is absorbed by escriba-compass
> (the `<C-hjkl>` pane bindings ARE the seamless vim↔tmux navigation), so
> it needs no separate caixa. Infrastructure plugins (plenary, lazy.nvim,
> nui.nvim) are not user-facing capabilities and have no caixa.

## "Solve once" — the baseline / plugin split

`escriba/configs/blnvim-defaults.lisp` is now the **baseline only**: theme,
core `:set` options, the leader, baseline motion keybinds, the language
major-modes, the base syntax + UI highlight groups, the fleet palette,
and escriba's convergence-layer inventions (gates, workflows, sessions,
effects, terms, marks, schedules, kmacros, attests, rulers). Every
plugin-owned form (`deflsp`, `defformatter`, `deftextobject`, `deffold`,
`defdap`, `deficon`, the Nord palette, the statusline, the bufferline,
the plugin keybinds, the GitSigns/Diagnostic/TODO highlights, the tasks,
the MCP bindings) **moved into its caixa** — one concern, one home. The
`bundled_composite_preserves_full_capability` matrix test proves the
migration lost nothing.

## CLI surface (closed-loop primitive composition)

```
escriba plugin list                          # bundled caixas + user installs
escriba plugin forge --out <dir>             # emit every caixa dir from the bundle
escriba plugin forge --catalog <dir> --out … # forge from an external catalog
escriba plugin install-bundled [--out <dir>] # materialize the bundle to disk
escriba plugin load <dir>                    # load + apply one caixa
escriba --list-rc                            # composite plan + wiring status
```

## Activation model

- **Bundled defaults = eager.** The baked catalog is merged into the boot
  plan, so every default plugin's forms are live immediately — the
  "fully loaded" config. This is the primary "default config as powerful
  as blnvim" guarantee.
- **User plugins = eager or lazy.** A user plugin installed in the
  plugins dir (`$ESCRIBA_PLUGINS_DIR` / XDG) and declared via
  `(defplugin :name … :on-event/:on-command/:on-filetype …)` activates
  eagerly, or lazily through `escriba_runtime::PluginHost`: its entry is
  applied the first time its `Command` / `FileType` / `Event` trigger
  fires (the lazy.nvim model, typed). This closed the gap where
  `escriba-plugin`'s loader parsed triggers that nothing ever fired.

## Nix + module support

- **Per-plugin packaging.** Each forged caixa ships a `flake.nix` (a
  pure-source `:kind Biblioteca` package). In the fleet, caixas are
  consumed as `flake = false` source (no per-plugin nixpkgs pin compounds
  into the closure — see the `nix-efficiency` discipline).
- **HM / NixOS / Darwin trio** (substrate `mkModuleTrio`): the existing
  `programs.escriba.{settings,extraKeybinds,extraConfig,noDefaults}` plus
  a new **per-plugin toggle**: `programs.escriba.plugins.<name>.enable`.
  Disabling a plugin sets `$ESCRIBA_DISABLED_PLUGINS`, which the binary
  reads to omit that caixa's forms at boot. The toggle list is
  **auto-discovered** from `escriba/catalog/` in the flake (catalog
  reflection, in Nix), so it never drifts from the files on disk.

## Verification matrix (the forcing function)

`escriba/tests/plugin_matrix.rs` is the mechanical promise:

1. **`every_catalog_plugin_forges_and_applies`** — every catalog file
   forges, its `caixa.lisp` re-parses, its name matches its filename, its
   category is canonical, its triggers parse, and its entry applies.
   Failures aggregate so one run reports every broken plugin.
2. **`catalog_dir_and_bundled_table_are_a_bijection`** — the `catalog/`
   directory and the baked `BUNDLED` table are a bijection: a file with
   no row (or a row with no file) fails the build.
3. **`catalog_covers_the_parity_set`** — at least 45 plugins; every file
   baked exactly once.
4. **`bundled_composite_preserves_full_capability`** — the migration
   invariant (≥11 lsp, ≥11 formatters, ≥17 text objects, …).

## escribamourne (the curated distribution — template)

`escribamourne` (a future `pleme-io/escribamourne` repo, mirroring
`frostmourne`) bundles the escriba binary + the baked catalog + a curated
user-lisp overlay + helper tools (rust-analyzer, the formatters, ripgrep,
fd, skim). Because the catalog is already baked into the binary, the
escribamourne flake is thin: consume `escriba.packages.default`, layer a
curated `rc.lisp` (extra keybinds / theme / private plugins) via the HM
module, and bundle the runtime tools on `$PATH`. The plugin caixas are
consumed as `flake = false` source. Until that repo exists, the escriba
flake's HM module IS the distribution surface (full per-plugin control).

## Adding a plugin

1. Drop `escriba/catalog/escriba-<thing>.escribaplugin.lisp` — one
   `(defescribaplugin …)` manifest + the escriba entry def-forms.
2. Add one row to `catalog_bundle::BUNDLED` (`"escriba-<thing>"`).
3. `cargo test -p escriba --test plugin_matrix` — green proves it forges,
   applies, and is bijective with the table.

That's the whole loop. No hand-authored `caixa.lisp`, no hand-authored
`flake.nix`, no per-plugin module wiring — the substrate emits all of it.
