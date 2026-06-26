# escriba

skip-fleet-convergence-guard: EscribaConfig (escriba-config/src/lib.rs)
exposes NO `font_family` / `font_size` / `cursor` fields that participate
in `ishou_tokens::FleetDefaults::prescribed()` — those visual primitives
live in escriba-render's GPU target, not in EscribaConfig — so the
ishou_tokens convergence Guard has nothing to assert against. (Since
2026-06-26, `prescribed_default()` DOES pin scalar defaults — `tema`
"vellum" + line numbers + tab-width 2 + statusline — mirroring the
load-bearing `configs/blnvim-defaults.lisp` so `config-show default`
is honest; but none of those are FleetDefaults font/cursor primitives,
so the waiver stands.) Reconsider if a future EscribaConfig revision
introduces typed `font_family` / `font_size` / `cursor` fields.

## Default config (the shipped blnvim-parity baseline)

`nix run .#escriba` boots a baked-in curated default —
`escriba/configs/blnvim-defaults.lisp` (`include_str!` at
`escriba/src/lib.rs:40`) + the 45-entry bundled plugin catalog
(`catalog_bundle.rs`) — applied at boot unless `--no-defaults`. The
default mirrors blackmatter-nvim (blnvim): leader `,` (blnvim parity),
tab-width 2, ~19 `:set` options, `<C-s>` save, 14 tree-sitter major
modes, the Vellum (Nord-matte) theme, ~30 highlights. **Tier-honest
parity gap:** the WIRED def-form set is `defcmd`/`defoption`/`defkeybind`
/`defmode` only — so the ~40 catalog plugins that declare picker/LSP/git/
completion/formatter/diagnostic keybinds, and the operator-edit verbs
(`dw`/`ciw`/paste/search), are **bound-but-inert** until their subsystem
waves land (a running LSP client, a picker, a git layer, a `tema`→palette
renderer for live theme-switch). Closing those = escriba feature-work,
not a config change. See `theory`-side analysis or run
`escriba config-show default`.

**Operator-over-motion engine (2026-06-26, shipped):** the vim
`{operator}{motion}` verbs (`dw`/`c$`/`y0`) execute in `escriba-runtime`
— `Action::ApplyOperator { op, motion }` is no longer a no-op.
Composition stands on a pure `resolve_motion(from, motion) -> Position`
(extracted so the cursor-move path AND the operator-range path share one
motion-resolution source of truth); Delete/Change/Yank act over the
resolved `[cursor, target)` range with undo, Change enters Insert, a
single unnamed `register` captures deleted/yanked text. Indent/Format/
structural operators are named-but-unwired. The keymap **operator-pending
FSM** is now wired: `d`/`c`/`y` are bound to `Action::Operator(_)` and a
small `(State,Event)->(State,effects)` machine (`operator_pending.rs`)
holds the pending operator until the next motion composes
`Action::ApplyOperator`. That FSM **stands on the fleet `zenmai`
primitive** (escriba is zenmai's 3rd cross-repo consumer, after bolso +
gaveta) — the editor doesn't re-roll a bespoke `Option<Operator>` + dispatch
`if let`s. So typing `d` then `w` deletes a word from the keyboard today.
**Counts compose correctly** (`3dw` = 3 words, `2d3l` = 6 chars): the FSM's
event/effect is `(Action, count)` and it owns count composition (operator
count × motion count), so a bare `5j` still passes through unchanged — the
prior naive outer repeat-loop, which silently broke `3dw`, is gone.
**Remaining:** `dd` linewise (a doubled operator currently cancels),
text-objects (`ciw`/`diw`), a combined register for counted deletes (the
register currently holds only the last sub-delete), and the named-but-unwired
operators (Indent/Format/structural).

> **★★★ CSE / Knowable Construction.** This repo operates under
> **Constructive Substrate Engineering** — canonical specification at
> [`pleme-io/theory/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md`](https://github.com/pleme-io/theory/blob/main/CONSTRUCTIVE-SUBSTRATE-ENGINEERING.md).
> The Compounding Directive (operational rules: solve once, load-bearing
> fixes only, idiom-first, models stay current, direction beats velocity)
> is in the org-level pleme-io/CLAUDE.md ★★★ section. Read both before
> non-trivial changes. Rust + tatara-lisp modal editor; the canonical
> `Rust owns invariants, Lisp owns authoring` application — 19 typed
> crates renderable to GPU/TUI/text targets from the same domain types.

Modal text editor written in Rust and authored in tatara-lisp.
pleme-io's canonical `Rust + Lisp` application of the editor category —
the same architectural decision Emacs makes with C+Elisp, Neovim makes
with C+Lua, and Zed makes with Rust+Rust — but targeting the
`Rust owns invariants, Lisp owns authoring` split the rest of the
pleme-io fleet already uses (frost, tatara, sui, etc.).

## Quick Start

```bash
cargo run -- scratch.txt            # open a file (default GPU window)
cargo run -- --render=tui file.rs   # ratatui inside any terminal
cargo run -- --render=text file.rs  # one-shot ANSI dump (CI / headless)
cargo run -- --commands             # list registered commands
cargo run -- --keymap               # list default keybindings
cargo run -- --spec > escriba.json  # dump OpenAPI 3.1 surface
cargo run -- plugin list            # 45 bundled plugin caixas + user installs
cargo run -- plugin forge --out o   # emit every plugin caixa dir (caixa.lisp + entry + flake)
cargo run -- --list-rc              # composite plan summary + wiring status
cargo test --workspace --lib        # workspace unit tests (all green)
```

## Crate Map (19 crates)

| Crate | Purpose | Key types |
|-------|---------|-----------|
| `escriba-core` | Typed primitives — no I/O, no rendering | `Position`, `Range`, `Cursor`, `Selection`, `Mode`, `Motion`, `Operator`, `Edit`, `Action`, `CountedAction`, `BufferId`, `WindowId` |
| `escriba-buffer` | Gap-buffer / rope backed text buffers, edits, undo | `Buffer`, `BufferSet`, `BufferError` |
| `escriba-config` | Config loading via shikumi | — |
| `escriba-mode` | Modal state machine (Normal / Insert / Visual / Command) with pending count + operator | `ModalState` |
| `escriba-keymap` | `(Mode, Key) → Action` binding table | `Key`, `Binding`, `Keymap` |
| `escriba-command` | Command registry + palette entries | `Command`, `CommandSpec`, `CommandRegistry`, `EditContext` |
| `escriba-api` | OpenAPI 3.1 surface generation | `OpenApiSpec`, `build_spec` |
| `escriba-spec` | Thin re-export of `escriba-api` | — |
| `escriba-ui` | Viewport, Window, Layout — pure layout math | `Viewport`, `Window`, `Rect`, `Layout` |
| `escriba-render` | Render backends (GPU via madori/garasu, text) | `Renderer`, `GpuRenderer`, `TextRenderer` |
| `escriba-tui` | ratatui + crossterm TUI backend | — |
| `escriba-input` | Platform-event → escriba-key translation | `InputOutcome`, `translate_app_event` |
| `escriba-runtime` | Editor state machine: `tick(event)` orchestration + lazy plugin host | `EditorState`, `PluginHost`, `LazyTrigger` |
| `escriba-plugin` | Plugin caixa model + **forge** (emit caixa.lisp / entry / flake from a catalog source) | `PluginCaixa`, `ActivationTrigger`, `forge_plugin`, `CaixaArtifacts` |
| `escriba-vm` | Embedded Lisp VM — skeleton for Lisp-authored logic | — |
| `escriba-ts` | Tree-sitter integration (incremental parse + highlight) | — |
| `escriba-lsp-client` | LSP client (tower-lsp-based) | — |
| `escriba-mcp` | MCP server — expose editor state to AI agents | — |
| `escriba` | Binary — wires everything, owns CLI flags + render dispatch | — |

## Architecture

```
madori AppEvent ──► escriba-input ──► escriba-runtime.tick()
                                             ├── escriba-mode (state machine)
                                             ├── escriba-keymap (dispatch)
                                             ├── escriba-command (palette)
                                             ├── escriba-buffer (edits)
                                             └── escriba-ui (layout)
                                                   │
                                                   ▼
                                            EditorState
                                                   │
        ┌──────────────────┬───────────────────────┴───────────────────┐
        ▼                  ▼                                           ▼
   GpuRenderer        ratatui-tui                               TextRenderer
  (madori+garasu)   (crossterm)                             (ANSI-in-stdout)
```

External integrations live in sibling crates:
- `escriba-ts` — tree-sitter (incremental parse, highlight capture queries)
- `escriba-lsp-client` — LSP servers (rust-analyzer, gopls, typescript-language-server, …)
- `escriba-mcp` — AI agents via Model Context Protocol
- `escriba-plugin` — plugin caixa model + forge (see "Plugin Caixa Substrate")
- `escriba-vm` — embedded Lisp VM
- `escriba-lisp` — Tatara-Lisp authoring bridge (32 def-forms + `defescribaplugin` catalog form)

## ★ Plugin Caixa Substrate — generation-driven blnvim parity

**Canonical doc: [`docs/plugin-substrate.md`](./docs/plugin-substrate.md).**
**Every escriba plugin is a tatara-lisp caixa, emitted from one typed
catalog, installed by default, with Nix + per-plugin module support.**
This is Pillar 12 (generation over composition) + ★★ CLOSED-LOOP
MASS-SYNTHESIS + ★★ CATALOG REFLECTION applied to the editor-plugin
domain.

- **Authoring** — one `(defescribaplugin …)` source per plugin under
  `escriba/catalog/<name>.escribaplugin.lisp` (manifest form + the
  escriba entry def-forms). The manifest is inert at apply time, so the
  same file is both the spec and a valid plugin entry.
- **Forge** — `escriba_plugin::forge_plugin` emits each caixa's
  `caixa.lisp` (`:kind Biblioteca`) + `escriba/plugin.lisp` + `flake.nix`
  + the persisted spec, via the typed `escriba_lisp::sexp::Sexp` emitter
  (no string-concatenated lisp). `escriba plugin forge` / `install-bundled`
  materialize them.
- **45-caixa parity catalog** — every default-on blnvim capability is a
  caixa (lspconfig, conform, telescope, gitsigns, trouble, oil, cmp,
  treesitter + textobjects/fold/context/autotag, dap, neotest, illuminate,
  helm, lualine, bufferline, devicons, …; vim-tmux-navigator is absorbed
  by escriba-compass). The composite carries 12 LSP servers, 11
  formatters, 17 text objects, 5 DAP adapters, 23 icons, 58 highlights,
  93 keybinds.
- **Installed by default by construction** — the catalog is baked into
  the binary (`catalog_bundle::BUNDLED`, `include_str!`) and merged into
  the boot plan eagerly. No on-disk install, no network.
- **Solve once** — `configs/blnvim-defaults.lisp` is now the BASELINE
  only (theme, options, leader, major-modes, base highlights, escriba
  inventions). Plugin-owned forms (deflsp / defformatter / deftextobject
  / deffold / defdap / deficon / nord palette / statusline / bufferline /
  tasks / mcp) MIGRATED into their caixas — one concern, one home. A
  matrix test proves the migration lost nothing.
- **Lazy activation** — bundled defaults are eager; USER plugins in the
  plugins dir activate lazily via `escriba_runtime::PluginHost` (their
  entry applies on the first `Command` / `FileType` / `Event` trigger —
  the lazy.nvim model, typed).
- **Nix + modules** — each caixa ships a `flake.nix`; the HM/NixOS/Darwin
  trio exposes `programs.escriba.plugins.<name>.enable` (auto-discovered
  from the catalog dir), which sets `$ESCRIBA_DISABLED_PLUGINS` to omit a
  plugin's forms at boot.
- **Verification matrix** — `tests/plugin_matrix.rs` fails the build if a
  catalog file is malformed, mis-named, or missing from the baked table
  (dir↔table bijection), or if the composite loses capability.

**Adding a plugin** = drop a `catalog/<name>.escribaplugin.lisp` + add one
row to `catalog_bundle::BUNDLED` + `cargo test --test plugin_matrix`. The
substrate emits the caixa.lisp, the entry, and the flake mechanically.

## Absorption Thesis

Escriba is the editor category distilled into typed primitives and
authored declaratively.

Every editor in the category is a solution to the same set of problems:
how to represent text, how to map keys to edits, how to extend the
system, how to integrate with external tooling. They differ in which
abstractions they commit to at each layer.

Escriba's plan is to absorb the best abstractions from each — typed in
Rust, composed in Lisp. The table below is the comparison matrix plus
what escriba does today and what it is absorbing next.

### Category Comparison Matrix

| Capability | vim / neovim | helix | kakoune | emacs | zed | vscode | sublime | cursor / windsurf | **escriba (today)** | **escriba (next)** |
|---|---|---|---|---|---|---|---|---|---|---|
| Text primitive | line-based | selection-first | selection-first | buffer | rope | line | rope | line | typed Position/Range | — |
| Buffer backing | gap buffer | rope (ropey) | rope | gap buffer | rope | array | rope | array | configurable | — |
| Modal editing | yes (vi) | yes (helix) | yes (kak) | no | no | no | no | no | yes (vi-like) | add helix noun-verb option |
| Multi-selection | weak | primary | primary | kill-ring only | primary | primary | primary | primary | single selection | **absorb: Selections (Vec<Selection>)** |
| Registers / kill-ring | `"a…"z` + `"0`…`"9` | `"` + `_` | `"` + `*` | kill-ring | clipboard | clipboard | clipboard | clipboard | — | **absorb: Registers** |
| Marks / jumplist | `m[a-z]` + jumplist | jumplist | marks | marks + registers | — | — | — | — | — | **absorb: Marks + Jumplist** |
| Tree-sitter | yes (plugin) | yes (built-in) | no | tree-sitter.el | yes (built-in) | yes | no | yes | yes (`escriba-ts`) | expand captures + folds |
| LSP client | yes (plugin / built-in 0.5) | yes (built-in) | lsp-kak plugin | lsp-mode / eglot | yes (built-in) | yes (built-in) | LSP plugin | yes | yes (`escriba-lsp-client`) | enrich (inlay hints, semantic tokens, workspace symbols) |
| DAP client | nvim-dap | — | dap-kak plugin | dape.el | debug | yes (built-in) | — | yes | — | **absorb: escriba-dap-client** |
| Scripting language | vimscript + lua | none (yet) | shell | elisp | TS extensions | TS extensions | Python | TS | tatara-lisp (via `escriba-vm`) | **absorb: escriba-lisp authoring bridge** |
| Package / plugin manager | vim-plug, lazy | — | plug.kak | package.el, elpaca | extensions (Rust+WASM) | marketplace | Package Control | marketplace | `escriba-plugin` scaffold | plugin manifest declared in Lisp |
| Command palette | `:` | `:` + picker | `:` | `M-x` | Cmd-Shift-P | Cmd-Shift-P | Cmd-Shift-P | Cmd-Shift-P | `escriba-command` registry | wire palette UI + fuzzy match (skim) |
| Fuzzy picker | telescope, fzf-lua | built-in | fzf plugin | helm, vertico | built-in | quick open | goto anything | built-in | — | **absorb: escriba-picker (uses `skim` crate)** |
| Which-key prompt | which-key.nvim | built-in | — | which-key.el | partial | partial | — | — | — | **absorb: WhichKey popup** |
| Status line | lualine / lightline | built-in | status-line | mode-line | statusbar | status bar | status bar | status bar | basic | customizable via Lisp |
| Tab / buffer line | bufferline.nvim | — | — | tab-bar-mode | tab bar | tabs | tabs | tabs | basic | — |
| File tree | nvim-tree | — | — | dired, treemacs | file tree | explorer | sidebar | explorer | — | **absorb: escriba-tree** |
| Git integration | fugitive, gitsigns | built-in gutter | — | magit | built-in | GitLens | git gutter | built-in | — | **absorb: escriba-git (reuse `git2`)** |
| Integrated terminal | `:terminal` | — | — | ansi-term, eat, vterm | built-in | built-in | — | built-in | — | **absorb: escriba-term (embed mado/frost)** |
| AI inline assist | copilot.lua | — | — | gptel | zed-ai | copilot | LLM plugin | primary | `escriba-mcp` server | **absorb: MCP client + inline completion** |
| Snippets | luasnip | — | — | yasnippet | snippets | snippets | snippets | snippets | — | snippet spec in Lisp |
| Folding | `foldmethod=syntax/treesitter` | — | — | origami.el, hs-mode | built-in | built-in | — | built-in | — | tree-sitter folds |
| Undo tree | undotree.vim | — | — | undo-tree.el | linear | linear | linear | linear | basic undo | persistent undo tree |
| Macros | `q` / `Q` | — | — | kbd-macros | — | — | — | — | — | record keys → action seq |
| Collaboration | — | — | — | crdt.el | primary | Live Share | — | primary | — | — |
| Notebook cells | notebook.nvim | — | — | org-babel, jupyter | repl | Jupyter | — | — | — | — |
| Session / layout persistence | session.vim | — | — | desktop.el | workspaces | workspaces | projects | workspaces | — | layout serializer |
| Minimap | — | — | — | minimap.el | yes | yes | yes (iconic) | yes | — | optional |

### Conclusions

1. **Escriba is on the correct axis for the Rust+Lisp door.** The base
   modal model (`escriba-mode`), typed primitives (`escriba-core`), and
   command/keymap registries match how Emacs, Neovim, and Helix model
   the editor; what's missing is the Lisp authoring bridge that maps
   the Rust state onto Lisp surfaces users write by hand.
2. **Multi-selection is the biggest capability gap vs. modern
   editors** (Helix, Kakoune, Sublime, VSCode). The `Selection` type
   needs to be plural (`Vec<Selection>`), and every motion / operator
   needs to apply to the set.
3. **The picker UI is the second biggest gap.** Every capable editor
   has fuzzy finding (Telescope, Helix picker, Cmd-Shift-P, Goto
   Anything). The `skim` crate already in the pleme-io fleet (used by
   frostmourne) slots in naturally — ship `escriba-picker` on top.
4. **AI pair programming is the clearest moat.** escriba has `escriba-mcp`
   as a server but no MCP *client* inside the editor. An MCP client
   + inline-completion widget would let escriba be a Cursor-style
   agentic editor without leaving the Rust+Lisp pattern.
5. **escriba-term is a composition win.** mado (GPU terminal) and
   frost (zsh-compatible shell) already exist in pleme-io — an editor
   that embeds them as a splittable term pane reuses 100% of that
   work.

## The `escriba-lisp` bridge

**Status: planned — this is the first absorption PR after this
CLAUDE.md.** Mirrors [`frost-lisp`](https://github.com/pleme-io/frost/tree/main/crates/frost-lisp)
one-for-one.

Intent: every piece of editor state that today lives in a `default_*()`
factory should be declarable via Tatara-Lisp. Config composes across
multiple forms the same way frost-lisp composes across multiple
`.lisp` files.

Initial form set:

```lisp
;; bind a key in a mode
(defkeybind :mode "normal" :key "gh" :action "goto-home")
(defkeybind :mode "insert" :key "jk" :action "escape-to-normal")

;; register a command (wired into the command palette)
(defcmd :name "write-all"  :description "Write every modified buffer"
        :action "buffer.write-all")

;; toggle/set an editor option
(defoption :name "number"          :value "true")
(defoption :name "tabstop"         :value "4")
(defoption :name "relativenumber"  :value "true")

;; select a theme (reuses irodzuki palette, mirrors frost-lisp deftheme)
(deftheme :preset "nord")

;; hook on an editor event
(defhook :event "BufWritePost" :command "run-formatter")
(defhook :event "ModeChanged" :to "insert" :command "highlight-cursor-line")

;; filetype routing (extension → mode)
(defft :ext "rs" :mode "rust")

;; abbreviation (insert-mode auto-expand)
(defabbrev :trigger "teh" :expansion "the")

;; snippet
(defsnippet :trigger "fn" :body "fn ${1:name}(${2}) -> ${3} { ${0} }")
```

Every spec is a `#[derive(DeriveTataraDomain)]` struct; the bridge
exposes `load_rc(path, &mut EditorState) -> ApplySummary` the binary
calls at startup. The binary gains a `--rc <path>` flag and honors
`$ESCRIBARC` like frost honors `$FROSTRC`.

## Escribamourne (future — the curated escriba distribution)

Planned analogue of `frostmourne` — a flake that bundles escriba plus
a batteries-included Lisp-authored configuration covering sensible
keybindings, themes, LSP server setup, AI assistant wiring, and
integrated terminal. Out of scope for this session; will be a new
repo (`pleme-io/escribamourne`) once `escriba-lisp` has enough surface
to configure meaningfully.

## Absorption Roadmap

The absorption roadmap is a DAG, not a line — each group can proceed
independently. Ordered by impact × effort ratio:

**Wave 1 — Authoring bridge (landed):**

1. ✅ `escriba-lisp` crate — 8 def-forms (`defkeybind`, `defcmd`,
   `defoption`, `deftheme`, `defhook`, `defft`, `defabbrev`,
   `defsnippet`), each a `#[derive(DeriveTataraDomain)]` spec.
2. ✅ Binary wires `--rc <path>` + `$ESCRIBARC` + `--list-rc`,
   loads and applies at startup.
3. ✅ Apply layer: `apply_plan_to_keymap` resolves the key-string
   grammar (`<Esc>`, `<C-r>`, `<A-f>`, named keys, bare chars),
   maps 26 well-known action strings to typed `Action` variants,
   and falls back to `Action::Command { name, args }` for
   anything else (plugin-registered commands resolve later).
4. ✅ Integration harness (`escriba/tests/rc_integration.rs`) +
   sample fixture (`escriba/examples/sample-rc.lisp`) — 115 tests
   covering parse, apply, and end-to-end binary flow.

**Wave 1.5 — Authoring-bridge polish (queued):**

1. Multi-key sequences (`gh`, `gg`, `dd`) via pending-stroke state
   in `ModalState`. Currently surfaced as a warning from the apply
   layer. Requires keymap + runtime changes.
2. More apply paths:
   `apply_plan_to_commands` (defcmd → CommandRegistry),
   `apply_plan_to_options` (defoption → new `EditorOptions` slot),
   `apply_plan_to_hooks` (defhook → new autocmd dispatcher).
3. `defsource :path "other.lisp"` — rc composition across files,
   mirroring frost-lisp's `defsource`.

**Wave 2 — UX essentials:**

1. **Multi-selection.** Promote `Cursor` → `Cursors` (Vec<Position>)
   and `Selection` → `Selections` (Vec<Selection>) in `escriba-core`.
   Every motion / operator maps over the set. Selection-first mode
   (Helix-style) becomes a Lisp-selectable preset.
2. **escriba-picker.** Fuzzy picker crate built on `skim` (already in
   the fleet via frostmourne). Files, buffers, commands, symbols.
3. **Which-key popup.** Multi-stroke binding preview — render the
   pending key prefix's completions after a configurable delay.
4. **Registers / clipboard.** `"a…"z` registers, kill-ring, system
   clipboard via `hasami`.
5. **Marks / jumplist.** `m[a-z]`, `''`, `C-o`, `C-i` parity.

**Wave 3 — Tooling integrations:**

1. **escriba-dap-client.** DAP parallel to `escriba-lsp-client`.
   Breakpoints, stepping, watches.
2. **escriba-git.** Git integration — `git2` crate, gutter signs,
   blame, log, diff view.
3. **escriba-term.** Embedded terminal pane. Uses mado + frost's
   existing primitives.
4. **escriba-tree.** File tree sidebar.
5. **escriba-lsp-client** enrichment — inlay hints, semantic tokens,
   workspace symbols, code actions, rename, signature help.
6. **AI inline assist** — MCP *client* that calls Claude for inline
   completions, edits, chat. `escriba-mcp` already has the server
   side; pair with the client.

**Wave 4 — Advanced:**

1. **Undo tree** — persistent, branch-able.
2. **Macros** — key-sequence record / replay, storable as Lisp.
3. **Snippets** — declared via `defsnippet`.
4. **Folding** — tree-sitter-aware.
5. **Session persistence** — layout + buffer positions serialized.
6. **Notebook cells** — tree-sitter-driven cell detection.
7. **Collaboration** — CRDT, pair with zed's architecture.

## Conventions

- Edition 2024, Rust 1.89.0+, MIT license.
- `cargo build --workspace` must be warning-free.
- Every new crate wires into `Cargo.toml [workspace.dependencies]` with
  a `version` field so crates.io publishing works later.
- Every `#[tatara(keyword = "…")]` spec needs a dedicated pass in
  `escriba-lisp::apply_source` that mutates `EditorState`.
- Never hand-write a `default_*()` factory that could be a Lisp form.
  If it's configurable, it's a def-form.
