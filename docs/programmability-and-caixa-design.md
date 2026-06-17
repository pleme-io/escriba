# escriba — programmability, apply-layer & caixa design

Authoritative plan for making escriba the best highly-pluggable,
highly-configurable, **fully-programmable-in-tatara-lisp**,
**caixa-compatible** modal editor that reproduces the **blnvim** flows +
look-and-feel, with lean effective defaults and a full integration-test
suite. Grounded in a fleet absorption pass (blnvim, frost-lisp/frostmourne,
caixa+feira, tatara-lisp/shikumi) + best-editor research.

> Reference editor = **blnvim** (`blnvim-ng` + `blackmatter-nvim` + active
> `~/.config/nvim`). Proven Lisp-config template = **frost-lisp** /
> **frostmourne** (the shell escriba mirrors one-for-one). Plugin/install
> system = **caixa** + the **feira** installer. Config = **shikumi**.
> Authoring + scripting = **tatara-lisp**.

## 1. Current state (honest, code-verified 2026-06-14)

- `escriba-lisp` has **32 typed def-forms** (keybind, cmd, option, theme,
  hook, ft, abbrev, snippet, mode, plugin, highlight, statusline,
  bufferline, lsp, formatter, palette, icon, dap, gate, textobject,
  workflow, session, effect, term, mark, task, schedule, kmacro, attest,
  ruler, mcp, fold) with rich parse-time validation + 100+ unit tests.
- **Apply-layer gap:** before this work only `apply_plan_to_keymap` and
  `apply_plan_to_grammar_extensions` reached live `EditorState`. The other
  ~30 forms parsed+validated but never mutated state.
- `apply_plan_to_keymap` **defers all multi-key/leader sequences** (`gh`,
  `<leader>ff`) — the bundled blnvim-defaults have **18 dead leader binds**.
- `escriba-vm` is a `SkeletonVm` (parses then `NotImplemented`) — escriba is
  declaratively-authored but **not yet runtime-programmable**.
- `escriba-plugin` scaffolds the **plugin-as-caixa** model
  (`PluginDecl { caixa, versao, ativar_em }`) + `discover()` only — no
  install/load/activate, and references a caixa kind that does not exist.
- Two overlapping def-form systems: `escriba-config` (Portuguese forms +
  `shikumi::TieredConfig` bare/prescribed + `vellum` theme) and
  `escriba-lisp` (English forms → `ApplyPlan`). **Duplication to reconcile.**
- Renderers hardcode `VellumPalette` (theme not yet data-driven).

## 2. Target architecture

### 2.1 Two-tier programmability (the headline)
tatara-lisp ships a **full runtime evaluator** (`tatara-lisp-eval`:
tree-walker + bytecode VM + macros + fibers + host-FFI). So escriba gets
the Neovim/Emacs split:

- **Declarative tier** — `escriba-lisp` def-forms → `ApplyPlan` → applied to
  `EditorState`. The rc/config surface. (This doc's apply-layer work.)
- **Imperative tier** — `escriba-vm` hosts an
  `Interpreter<EscribaHost>` from `tatara-lisp-eval`, registering editor
  operations (buffer/cursor/window/option/command) as **native Lisp
  functions** via `register_fn` + the `ffi` types. Users write live Lisp
  commands, plugin logic, and event handlers; an in-editor `ReplSession`
  powers `M-x eval`. **Do NOT build a second VM** — repoint `SkeletonVm`
  onto `tatara-lisp-eval`.

### 2.2 Apply-layer: every def-form → live state
Mirror frost-lisp's proven shape: each form gets an `apply_plan_to_X(plan,
&mut state) -> XApplyReport` pass that mutates `EditorState`, returns
applied/skipped counts + non-fatal warnings, surfaced in `--list-rc`. Add
`defsource`/`defload` rc composition (cycle-detected, `:optional`
overlays) — escriba lacks it; frost-lisp is the template.

### 2.3 Plugins as caixas + feira installer
- New `defcaixa` kind **`Extensao`** in `caixa-core` (escriba-plugin already
  references it). An escriba plugin is an `Extensao` caixa: tatara-lisp +
  optional native (nvim-oxi-style cdylib, per `blnvim-ng`).
- `feira escriba install <caixa>` reuses `caixa-resolver` for dependency
  resolution; configures via shikumi typed-YAML / `defescriba` / tatara-lisp.
- Lazy activation via `PluginDecl.ativar_em` (FileType/Event/Command) —
  the lazy.nvim spec model the `defplugin` form already mirrors.
- Sandbox (`terreiro` / reduced `FnRegistry`) **before** any untrusted
  install.

### 2.4 Look-and-feel = blnvim
Nord default (+ `vellum` warm alt) sourced from **ishou** fleet tokens;
dual gui+cterm baked into the `Color` type day one; lualine-style
statusline, bufferline, which-key (popup OFF by default — blnvim memorizes,
only group-prefix labels), telescope-style picker, oil-style file explorer.
Leader = `,`. Lean defaults: ship only WIRED forms in the bundled rc;
**escribamourne** = curated flake distribution (frostmourne analogue).

## 3. Wave sequence

- **Wave 0 — consolidate + truth-up:** embed `tatara-lisp-eval` (repoint
  `escriba-vm`); resolve the two-Lisp fork; make `Color` dual gui+cterm.
- **Wave 1 — wire core forms:** `defcmd`→registry ✅ (keystone, done);
  multi-key/leader pending-stroke; `defoption`→`EditorOptions`;
  `deftheme`→renderer palette (ishou); `defhook`→autocmd dispatcher;
  `defsource` composition.
- **Wave 2 — UX essentials:** picker (skim), which-key, registers/marks,
  multi-selection, statusline/bufferline render.
- **Wave 3 — caixa plugins + feira:** `Extensao` kind, `feira escriba
  install`, plugin load/activate, sandbox.
- **Wave 4 — escribamourne:** curated flake; blnvim-parity batteries.
- **Cross-cutting:** integration tests per slice; adversarial review loop.

## 3a. Resolved decision — ONE tatara-lisp lineage

The fleet had two tatara-lisp lineages: `pleme-io/tatara` (macro farm +
`tatara-eval`, a Nix-derivation interpreter with **no host-FFI**; frost +
escriba historically) and the standalone **`pleme-io/tatara-lisp`**
workspace (host-embeddable `tatara-lisp-eval` `Interpreter<H>` +
`tatara-lisp-script` + `compiler_spec` polyglot seam + WASM/WASI). The
canonical lineage is **`pleme-io/tatara-lisp`** — it can host any language
via WASM/WASI, so it is the right "fully programmable / polyglot plugin"
platform. escriba is migrated. **Fleet-wide unification is pending** —
~12 repos (incl. frost and the shared `shikumi` `lisp` feature) remain on
the old lineage and need a phased migration (leaf consumers → shared libs
→ deprecate the old Lisp crates). escriba migrated cleanly in isolation
because its `shikumi` `lisp` feature is off.

## 4. Status log

- **2026-06-14 — Phase 3 (escriba-side caixa-plugin system) DONE:** a
  plugin IS a caixa — a dir with `caixa.lisp` + an `escriba/plugin.lisp`
  entry (plain escriba-lisp def-forms). `escriba-plugin` rewritten:
  `ActivationTrigger` (Startup/FileType/Event/Command, parsed from
  `:ativar-em`) + `PluginCaixa::{load,from_decl,is_eager,matches_*}`
  reads the entry with escriba's OWN lineage-B parser (NO `caixa-core`
  dep — that would re-introduce the two-lineage conflict). Binary:
  `activate_plugin` (applies entry def-forms to live state),
  `activate_eager_plugins` at startup (resolve `<plugins_dir>/<name>`,
  apply eager ones), `escriba plugin list` / `escriba plugin load <dir>`.
  Worked example `escriba/examples/plugins/escriba-paredit` (caixa.lisp +
  entry with paredit sexp keybinds + a defcmd + defoption). Tests:
  escriba-plugin 5 unit + 3 integration (load+apply, CLI load, CLI list).
  Full workspace: 48 suites, 365 tests, 0 warnings.
  DEFERRED (lineage-blocked, cross-fleet): the `feira escriba install`
  verb (in caixa-feira) + first-class `CaixaKind::Extensao` (caixa-core,
  226 refs across 15 files) wait for fleet lineage unification — caixa is
  on lineage A, escriba on lineage B, so escriba can't link caixa-core /
  caixa-resolver yet. Install/resolve uses the caixa git model (`feira` /
  git) out of band today; escriba is the consumer (load + activate).

- **2026-06-14 — Adversarial review (32-agent, 25/28 confirmed) + fixes:**
  0 critical. HIGH fixed: (1+2) leader was hardcoded `,` ignoring the
  declared `mapleader` — now the binary resolves `mapleader` from the
  option store via `parse_leader_key` + `keymap.set_leader()` BEFORE
  keymap apply, and `<space>`/`<spc>` are parseable tokens (the whole
  blnvim leader surface now binds correctly); (3) `escriba-vm` is cached
  on `EditorState` (`lisp_vm`) so the full stdlib installs once + env
  persists (REPL semantics, tested); (4) snapshot-isolation semantics
  documented + tested (a program can't read its own writes within one
  `run_lisp`; refreshes across calls); (8) stale `Cargo.gen.lock`
  regenerated via `gen build .` (now new-lineage, `gen check-spec` =
  fresh — Nix freshness gate passes); `defmode` `--list-rc` row now runs
  the real grammar apply + reports counts (was over-claiming WIRED).
  Test gaps closed: visual-mode sequence, count+sequence (`2gj`),
  abort-with-bound-key re-dispatch, multi-line insert cursor, lisp-quit
  sentinel, defcmd command-name alias is inert. Full workspace `cargo
  test`: 47 suites, 357 tests, 0 failures/warnings.
  DEFERRED (with rationale): (9) Cargo.toml stays `branch=main` like
  escriba's other siblings — the committed lock already pins the exact
  rev, so builds are reproducible; (5) the PT `escriba-config` vs EN
  `escriba-lisp` def-form duplication is a larger reconciliation
  (tracked); HostEffect naming + keystroke hot-path micro-opt deferred
  as low-value churn.
- **2026-06-14 — Lineage unified (escriba) + escriba-vm REAL +
  full-stdlib:** escriba's `tatara-lisp`/`tatara-lisp-derive` migrated to
  `git pleme-io/tatara-lisp` (+ `tatara-lisp-eval`); all 32 def-forms +
  escriba-config compiled unchanged (superset API). `escriba-vm` rewritten
  from skeleton to a real `Interpreter<EscribaHost>` host with
  `install_full_stdlib_with` (primitives + hof + map + channels + fibers +
  type-check + lisp stdlib). EscribaHost = pre-eval read snapshot +
  typed effect log (Message/RunCommand/SetOption/InsertText) — the sandbox
  + future WASM/WASI plugin seam. `EditorState::run_lisp(src)`
  (snapshot → eval → apply effects) + new `messages`/`options` fields.
  Two-tier programmability live. Tests: escriba-vm 9, runtime bridge 5
  (set-option/insert/message/snapshot-branch/run-command-undo). Full
  workspace `cargo test` green, warning-free.
- **2026-06-14 — Wave 1 multi-key/leader DONE:** keymap gains a sequence
  table + leader (`,`); `apply.rs` binds sequences (`<leader>ff`, `gg`)
  via `parse_key_sequence` instead of deferring; runtime drives a
  pending-stroke state machine (`pending_keys` + `step_sequence`). Bundled
  defaults' 18 leader binds now live (24/24 keybinds applied). Tests:
  keymap +3, apply +2, runtime +4, integration +1 (leader→command).
- **2026-06-14 — Wave 1 keystone DONE:** `apply_plan_to_commands` wires
  `(defcmd …)` into the live `CommandRegistry`. `escriba-command::Command`
  evolved to owned strings + a `Handler` enum (`Native(fn)` for built-ins,
  `Action(String)` for Lisp `defcmd`, extensible to `Lisp(thunk)`); a
  dotted-action resolver (`buffer.*` / `editor.*`) makes Lisp commands
  genuinely invokable, with unknown symbols inert (never fatal). Binary
  applies commands before keymap; `--list-rc` reports a wiring-status
  section. Tests: escriba-command unit + `escriba/tests/rc_integration.rs`
  (defcmd→registry resolution, deferred-keybind dispatch, negative
  unregistered-command, observable end-to-end file save). `cargo test -p
  escriba -p escriba-command -p escriba-lisp` green.
