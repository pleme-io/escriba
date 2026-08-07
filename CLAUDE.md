# escriba

## Start screen — `(defsplash …)` (2026-08-07, shipped)

`escriba` with no file argument opens on a start screen: the wordmark,
a tagline, a five-entry menu, and a footer strip. `escriba <file>` never
shows it — a welcome screen in front of a file you asked for is a
keystroke tax.

**One model, three faces.** `escriba_ui::splash::Splash` owns ALL the
layout (centering, block grouping, degradation); the faces only color it.
Two projections, both derived from `rows()` so they cannot disagree:
`rows()` for a face that positions widgets (ratatui), `screen_chunks()`
for the two that emit a character stream (the ANSI dump, and the GPU's
`set_rich_text`, which needs a coverage-complete partition). Colors are
ROLES resolved through `ChromePalette` — the same seam the rest of the
chrome uses — so the screen tracks a theme change for free.

**Authored, not hardcoded.** There is deliberately no
`Splash::default()`. The content is `(defsplash …)` in
`configs/blnvim-defaults.lisp`, lowered by
`escriba_lisp::apply_plan_to_splash`. Entry `:action` strings go through
`escriba_lisp::resolve_action` — the SAME table `defkeybind` uses (made
`pub` for this), so a menu entry and a key bound to the same string
dispatch identically. The shipped menu lists only actions wired TODAY
(`normal` / `insert` / `search-forward` / `command` / `quit`); a pressed
key that did nothing would be worse than a shorter menu.

**Two traps worth remembering:**

- **`:disabled #t`, not `:enable #f`.** A key absent from a tatara-lisp
  form does NOT reach serde's `#[serde(default = …)]` — it takes the
  field type's zero value. An `:enable: bool` therefore made a bare
  `(defsplash …)` parse cleanly and never appear. Verified, not assumed:
  the first cut shipped with `enable = false`. Polarity is inverted here
  relative to `defeffect` on purpose, to put the safe state on the zero
  value.
- **The screen owns the FIRST keypress only.** A menu key runs its
  entry; any other key dismisses and is then handled normally, so the
  first thing you type is never eaten. It is `Option<Splash>` on
  `EditorState`, deliberately NOT a `Mode` variant — a mode is a state
  keys are interpreted *in*, and this interprets exactly one key.

`--no-splash` / `$ESCRIBA_NO_SPLASH` skip it for one run; `--list-rc`
reports it in the wiring-status block. Footer facts are computed from the
plan the editor actually booted with, and the theme name comes from
`chrome::prescribed_theme_name()` (what is PAINTED) rather than
`plan.theme` (what was DECLARED) — see the theming gap below.

## Search reports as SEARCH, not COMMAND (2026-08-07, fixed)

vim's `/` is the command line, so escriba's search genuinely runs in
`Mode::Command`. The status line reported the raw mode, which meant
`/foo` rendered `: COMMAND` — character-for-character the line `:`
produces — with the pattern parked at the far RIGHT, past the match
count. Search was fully working and read as "pressing `/` put me in `:`
mode". **The model was right and the report was wrong.**

Fixed at the shared model so both faces inherit it:
`StatusModel::mode_label()` says `SEARCH` when a search prompt is open,
and `pill_sigil()` gives the pill `/` or `?` instead of `:`. Both derive
from the same typed `PromptKind` as the sigil, so label and sigil cannot
disagree. The TUI also moved the prompt to the LEFT, where vim puts the
cmdline. Pinned by `escriba-tui/tests/status_line_frame.rs`, which
asserts on rendered CELLS — the unit tests all passed throughout the
period this was broken.

`resolve_action` also gained `search-forward` / `search-backward` /
`search-next` / `search-prev`, which were missing: `:action
"search-forward"` silently became a lookup for a command that does not
exist.

## Test coverage of the GPU face — what is and isn't claimed

`GpuRenderer::render` needs a live `madori::RenderContext` (wgpu device +
surface view), so it cannot run under `cargo test`. Rather than leave the
whole face uncovered, the parts that can actually be WRONG were extracted
into pure functions and tested in `escriba-render/tests/gpu_logic.rs`:

- **`cell_grid(w_px, h_px, font_size, line_height)`** — the pixel→character
  conversion. This was duplicated: `resize` divided by line-height then
  subtracted a row, `render` subtracted a line-height then divided. Same
  intent, two spellings, two chances to reserve the status row wrong. Now
  one function, covering degenerate surfaces (0×0 on minimise), degenerate
  font metrics (`with_font_size` is public and unvalidated), and
  monotonicity under resize.
- **`splash_runs(chunks, palette)`** — the role→colour mapping. A mis-mapped
  role paints menu keys as body text and renders perfectly; glyphon cannot
  notice, so this is where the check has to live. Public *so the test calls
  the real function* — a test that rebuilt the mapping would keep passing
  after the renderer stopped calling it.

**The honest claim is "the GPU face's LOGIC is tested", not "the GPU face is
tested".** What remains uncovered is glyphon/wgpu call sequencing — buffer
sizing, shaping, encoder submission — which is upstream's contract. If you
add branching logic to `render()`, extract it rather than growing the
untestable region.

## Theming — how escriba paints (2026-07-26)

**One seam: `escriba_ui::chrome::ChromePalette`.** Both faces (the ratatui
TUI chrome and the GPU backend) resolve every color through it, and it
resolves through `ishou_tokens::SemanticRoles` — the closed ROLE vocabulary —
never through a theme's own token spelling. Consequences worth knowing:

- **Colors are named by role, not hue.** `c.info`, not "cyan". On Nord
  `info` is frost blue; on Vellum it was ice cyan; no call site knows or
  cares. This is what makes a theme change a value change instead of a
  fleet-wide rename.
- **`ChromePalette::for_theme` is total over `FleetTheme`** — no wildcard
  arm, so adding a theme upstream fails `chrome.rs` to compile rather than
  silently painting the wrong thing.
- **There are no hex literals in the paint path.** The last one
  (`Color::Rgb(0xCD, 0xC7, 0xB6)`, "statusline_fg") is gone.
- **Both convergence guards assert `FleetTheme::prescribed_default()`**, not
  a hand-written constant, so they cannot be satisfied by a stale literal.

**Why this landed:** the renderers were hardwired to
`VellumPalette::vellum()`. When the fleet moved its prescribed theme from
Vellum to PlemeDark (Nord — what mado ships), escriba kept painting Vellum
and both guards went RED (`theme drift — actual Vellum != fleet PlemeDark`).
`(deftheme :preset …)` had the same defect from the other side: it resolved
to a real `FleetTheme` that **nothing outside tests consumed**.

**Closed 2026-08-07 — `(deftheme :preset …)` now reaches the pixels.**
The paint path used to read `ChromePalette::prescribed()` at every site, so
an operator could author `(deftheme :preset "vellum")`, watch `--list-rc`
report it, and see Nord on screen. The fix, in three parts:

- **`EditorState` owns the theme** (`theme` + resolved `chrome`), so there
  is ONE answer to "what colour is this editor" and every face reads it.
  `set_theme` bumps the refresh generation — without that the GPU face
  keeps its cached shaped buffer and the old colours stay up until an
  unrelated edit invalidates it.
- **Every paint site takes the palette as an ARGUMENT.** The TUI style
  helpers and the GPU's `ground_bg` / `mode_color` no longer reach for the
  default themselves. A palette that arrives as a parameter cannot be
  ignored; one fetched inside the function always could be.
- **The binary applies `plan.theme.resolve()`** before anything paints.

`clear_frame` still paints the prescribed ground, correctly: it runs when
state could not be read, which is exactly when the operator's theme is
unknowable.

**What wiring it surfaced:** the shipped rc declared `(deftheme :preset
"vellum")` — stale since the fleet moved to Nord, and invisible for as long
as nothing consumed it. It now says `nord`, and
`theme_declaration_agrees_with_the_fleet_default` asserts the declaration
against `FleetTheme::prescribed_default()` rather than against a literal,
so a fleet re-point fails in the one file that decides it. Vellum remains
opt-in via `--rc escriba/configs/vellum.lisp`.

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
modes, the fleet-prescribed theme (Nord), ~30 highlights, and the start
screen. **Tier-honest parity gap:** the WIRED def-form set is
`defcmd`/`defoption`/`defkeybind`/`defmode`/`deftheme`/`defsplash` — so
the ~40 catalog plugins that declare picker/LSP/git/completion/formatter/
diagnostic keybinds, and the operator-edit verbs (`dw`/`ciw`/paste), are
**bound-but-inert** until their subsystem waves land (a running LSP
client, a picker, a git layer). Closing those = escriba feature-work, not
a config change.

**That inert set is no longer a prose claim.** `escriba/tests/action_resolution.rs`
pins the exact 85 shipped keybind actions that resolve to neither a typed
`Action` nor a registered command, and asserts SET EQUALITY — so a typo
fails (it is not in the list) and wiring a subsystem also fails until its
names are removed. The `:action` fallback to `Action::Command` is
load-bearing (it is how a plugin command resolves later) and is exactly
what makes a typo and a not-yet-wired subsystem indistinguishable at
parse, apply and dispatch time; the list is what tells them apart. Start
screen entries are held to the stricter bar of zero unresolvable, because
an inert keybind is invisible until pressed while an inert menu entry is
printed as an offer.

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
cargo run -- --no-splash            # skip the start screen for one run
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
| `escriba-ui` | Viewport, Window, Layout, Splash — pure layout math | `Viewport`, `Window`, `Rect`, `Layout`, `Splash` |
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

## Implementation plan for the backlog

**[`docs/backlog-plan.md`](./docs/backlog-plan.md)** is the ordered plan for
the 85 inert actions plus Waves 1.5–4. It carries **no schedule** by operator
instruction (2026-08-07): the work goes piece by piece, ordered by what makes
the next piece safer, not by what is cheapest.

Three things from it that change how you should read the rest of this file:

- **The backlog is 5 missing primitives, not 22 subsystems.** `madoguchi` 窓口
  (dispatch seam), `shirube` 標 (located findings), `kasane` 重ね (floating
  surfaces), `shikiri` 仕切り (container tree), `denrei` 伝令 (the courier).
- **Two of them land upstream in `egaku`**, which already ships
  `FuzzyPicker<T>`, `SplitPane`, `Modal`, `FocusManager` with 239 tests and
  zero rendering deps — and is already in our `Cargo.lock` with no consumers.
- **Phase 0 is three verified defects** that make every later claim
  unfalsifiable until fixed: `run_action` reports success for unknown actions
  (`escriba-command:232`), `run_command` discards `NotFound`, and
  `BufferSet::open` silently duplicates an already-open file
  (`escriba-buffer:349`).

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
| Command palette | `:` | `:` + picker | `:` | `M-x` | Cmd-Shift-P | Cmd-Shift-P | Cmd-Shift-P | Cmd-Shift-P | `escriba-command` registry | wire palette UI + `egaku::fuzzy_score` |
| Fuzzy picker | telescope, fzf-lua | built-in | fzf plugin | helm, vertico | built-in | quick open | goto anything | built-in | — | **absorb: escriba-picker (wraps `egaku::FuzzyPicker<T>`)** |
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
   Anything). **The fleet's answer is `egaku::FuzzyPicker<T>`** — 864
   lines, a typed `PickerEvent`→`PickerEffect<T>` machine with its own
   `fuzzy_score`, already in escriba's `Cargo.lock` as a transitive dep and
   with zero rendering dependencies. Ship `escriba-picker` as an adapter
   over it. (This previously said `skim`, "already in the fleet via
   frostmourne" — VERIFIED FALSE 2026-08-07: no fleet lockfile contains
   skim and frostmourne has no Rust crate. Building on that claim would
   have added a dependency to replace a library we already ship.)
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
2. **escriba-picker.** An adapter over `egaku::FuzzyPicker<T>` (NOT
   skim — see the Conclusions note above). Files, buffers, commands,
   symbols are one adapter over different `T`.
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
