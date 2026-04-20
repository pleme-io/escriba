# escriba

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
cargo test --workspace --lib        # 86 unit tests
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
| `escriba-runtime` | Editor state machine: `tick(event)` orchestration | `EditorState` |
| `escriba-plugin` | Plugin hosting and lifecycle | — |
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
- `escriba-plugin` — WASM plugin host
- `escriba-vm` — embedded Lisp VM
- `escriba-lisp` — Tatara-Lisp authoring bridge (see below)

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

**Wave 1 — Authoring bridge (this PR):**

1. `escriba-lisp` crate with `defkeybind`, `defcmd`, `defoption`,
   `deftheme`, `defhook`, `defft`, `defabbrev`.
2. Binary wires `--rc <path>` + `$ESCRIBARC`, loads at startup.
3. Test harness mirroring `frost/tests/frostmourne_rc.rs`.

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
