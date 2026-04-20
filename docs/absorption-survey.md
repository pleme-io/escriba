# escriba absorption survey

Catalog of what the editor category has built that escriba can absorb, and
what Rust libraries (inside and outside pleme-io) are available to power
those absorptions.

The absorption pattern: identify a capability → express its declarative
surface as an escriba-lisp `def*` form → implement the runtime in Rust,
typed from the Lisp spec → push the result to the authored surface
declared in `blnvim-defaults.lisp`.

---

## Tree-sitter — the substrate every modern editor absorbs

`escriba-ts` today hosts `tree-sitter-rust` only. The absorption envelope
covers five concentric layers — escriba is inside layer 1.

| Layer | Capability | Status |
|-------|------------|--------|
| 1 | **Multi-grammar parser host.** `GrammarRegistry` keyed on language name, `BufferParser` per-buffer that holds a `Tree` and re-parses on edit. | ✅ scaffold (rust only) |
| 2 | **Highlight-query capture.** `tree_sitter_highlight::HighlightConfiguration` → canonical `Semantic` buckets (16 enum values) → `HighlightSpan` (start/end/semantic). | ✅ rust only |
| 3 | **Injections.** Grammars that reference other grammars — e.g., `<script>` in HTML, JSX in TS, code fences in Markdown, SQL in Rust strings. Tree-sitter supports via `INJECTIONS_QUERY`. | ⏳ wired for rust grammar, needs per-grammar hookup |
| 4 | **Textobjects.** "inside function", "around class", "next parameter". Absorbs nvim-treesitter/nvim-treesitter-textobjects. Composes with vim operators (`vif`, `dap`, `cin`). Tree-sitter supports via textobjects queries. | ❌ unstarted |
| 5 | **Folds, tags, locals.** Scope-aware folding, symbol outlines, scope-aware highlight. Each is its own `.scm` query file. | ❌ unstarted |

### Grammar-dependency landscape

Every grammar is a Rust crate (`tree-sitter-$LANG`) that bundles a C
generated parser. Adding one = one `cargo add`. Key consideration: version
alignment. `tree-sitter-rust 0.21` is what escriba pins; the companion
grammars must target the same `tree-sitter` runtime version. Grammars
newer than 0.23 typically require a newer `tree-sitter` runtime.

| Grammar crate | Latest | Use case in fleet |
|---------------|--------|-------------------|
| tree-sitter-rust | 0.21 (pinned) | daily driver |
| tree-sitter-nix | 0.3.0 | every pleme-io flake |
| tree-sitter-markdown | 0.7.1 | docs, CLAUDE.md |
| tree-sitter-bash | 0.25.1 | scripts, direnv hooks |
| tree-sitter-json | 0.24.8 | configs |
| tree-sitter-python | 0.25.0 | fleet tooling |
| tree-sitter-typescript | 0.23 | lilitu frontend |
| tree-sitter-yaml | ext community | tend / gha |
| tree-sitter-toml | ext community | Cargo.toml |
| tree-sitter-commonlisp | 0.3 | tatara-lisp rc files |
| tree-sitter-go | 0.21 | kenshi, shinka |
| tree-sitter-lua | 0.2 | blnvim interop |

**Absorption plan**: bump tree-sitter runtime to 0.24.x, add 8 grammars
(nix, markdown, bash, json, python, typescript, lisp, go). Pin versions
so Nix builds stay reproducible. Expose each through the same
`GrammarRegistry::builtin()` entry point.

### defmode ↔ GrammarRegistry

Today `MajorModeSpec` carries a `tree_sitter: String` field but it doesn't
plug into `escriba-ts`. The glue: when the rc is applied, iterate every
defmode and register its `extensions` list with the named grammar so
`GrammarRegistry::from_extension(ext)` resolves correctly.

Requires: change `Grammar::extensions` from `Vec<&'static str>` to
`Vec<String>` so runtime-added extensions are possible. Then add
`apply_plan_to_grammar_registry(plan, &mut registry)` in `escriba-lisp`.

---

## Editor-ecosystem capabilities to absorb (beyond what's already authored)

Every row below is a capability that **has a clear declarative spec
shape** escriba could absorb. The rightmost column names the form or
system it would slot into.

### Authoring surface (more def-forms)

| Capability | Category reference | Fits as |
|------------|-------------------|---------|
| Custom motions (vim `map <expr>`) | vim keymap | `defmotion` (name + body/expression) |
| Custom operators | vim operator-pending | `defoperator` (name + action on range) |
| Text objects | vim + nvim-treesitter-textobjects | `deftextobject` (name + tree-sitter query) |
| Registers (pre-populate `"a..."z`) | vim registers | `defregister` (name + value) |
| Marks | vim `m[a-zA-Z]` | `defmark` (name + position / pattern) |
| Sessions / workspaces | session.vim, projections.nvim | `defsession` (name + buffers + layout) |
| Projects | nvim-project, workspace-folders | `defproject` (name + root markers + env) |
| Tasks (runnable commands) | asynctasks, vscode tasks | `deftask` (name + command + ft-scope) |
| Debugger adapters | nvim-dap | `defdap-adapter` (name + command + launch) |
| Test runners | neotest | `deftest-adapter` (name + command + parse) |
| Dashboard | dashboard.nvim, alpha.nvim | `defdashboard` (greet text + quick actions) |
| Which-key mappings (explicit descriptions) | which-key.nvim | already covered by `:description` on defkeybind — deepen with prefix groups |
| Completion sources | nvim-cmp sources | `defcmp-source` (name + ft + priority) |
| Spell dictionaries | vim spell, nvim spell | `defspell` (lang + path) |
| Terminal profiles | nvim `:terminal` | `defterm-profile` (shell + env + rows) |
| File tree config | nvim-tree, neotree | `deftree` (root + filters + sort) |

### Runtime behaviours (not yet a def-form, but escriba needs)

| Behaviour | Category reference | Escriba crate |
|-----------|-------------------|---------------|
| Multi-selection | Helix, Kakoune, Sublime, VSCode | promote `Selection` → `Selections` in `escriba-core` |
| Pending-stroke state | vim (`gh`, `gg`, `dd`) | add `pending_key: Vec<Key>` to `ModalState` |
| Which-key popup | which-key.nvim | new `escriba-popup` crate |
| Fuzzy picker | telescope, helix picker | new `escriba-picker` crate (uses `hayai` or `nucleo`) |
| Registers / kill-ring | vim / emacs | `escriba-core::Registers` |
| Marks + jumplist | vim | `escriba-core::{Marks, Jumps}` |
| Undo tree | undotree.vim | `escriba-buffer::UndoTree` (persistent branch) |
| Snippet expansion | LuaSnip | `escriba-snippet` crate (LSP snippet grammar) |
| LSP completion | nvim-cmp | `escriba-cmp` crate |
| LSP hover / code actions / rename | lspsaga | `escriba-lsp-client` methods |
| DAP client | nvim-dap | `escriba-dap-client` new crate |
| Git gutter | gitsigns | `escriba-git` new crate (uses `git2`) |
| Git blame | fugitive | `escriba-git::Blame` |
| Terminal pane | `:terminal` | `escriba-term` new crate (embed `mado`) |
| File tree pane | nvim-tree | `escriba-tree` new crate |
| Notification popup | nvim-notify | use pleme-io `tsuuchi` |
| Status line render | lualine | new `escriba-statusbar` crate (Lisp-driven) |
| Tab line render | bufferline | new `escriba-tabline` crate |
| Tree-sitter folds | tree-sitter folds query | `escriba-ts::folds` module |
| AI inline assist | copilot / cursor / windsurf | `escriba-mcp` (existing; needs client half) |

---

## Rust library evaluation

### pleme-io-native (sibling crates) — already built + proven

| Crate | What escriba would use it for |
|-------|-------------------------------|
| `hayai` | Fuzzy matcher engine for `escriba-picker`. Already powers guardrail + skim-tab. |
| `shikumi` | Config discovery + hot-reload for `$ESCRIBARC` and `~/.config/escriba/`. |
| `hasami` | System clipboard access for yank/paste registers. |
| `awase` | Hotkey parsing / manager (`<C-x>`, `<leader>f`, chord sequences). |
| `soushi` | Rhai scripting as a sibling option to tatara-lisp for user-authored commands. |
| `tsunagu` | Daemon / RPC plumbing if escriba grows a long-running process (LSP manager). |
| `tsuuchi` | Desktop notifications for LSP diagnostics / long-running tasks. |
| `kaname` | MCP server scaffolding — `escriba-mcp` already uses `rmcp` but `kaname` adds the pleme-io integration layer (error types, tool registry, response helpers). |
| `egaku` | Widget state machines (focus, scroll, selection) for popup UIs. |
| `mojiban` | Text → styled spans (markdown, syntax). Drives `render-markdown.nvim` parity. |
| `irodori` | Color system (sRGB↔linear, semantic slots). Resolver for `defhighlight`. |
| `irodzuki` | Base16 → GPU uniforms for `defpalette` rendering in the GPU backend. |
| `todoku` | HTTP client if AI/MCP over HTTPS. |
| `garasu` | GPU primitives (already the GPU backend) — for minimap, overview rendering. |
| `madori` | App framework (already the event loop) — for picker popups as sub-windows. |
| `denshin` | WebSocket gateway if escriba ever exposes a remote-edit API. |
| `kenshou` | Auth if escriba talks to a team server. |
| `meimei` | Name-case conversion — useful for code actions that rename. |
| `nami-core` | Browser core (DOM / CSS / layout) for in-editor markdown preview pane. |

### External / crates.io

#### Text & parsing

- **ropey** (already) — rope text storage.
- **tree-sitter** / **tree-sitter-highlight** (already) — syntax.
- **unicode-segmentation** — grapheme iteration.
- **unicode-width** — visual width for alignment.
- **regex** / **fancy-regex** — find-in-buffer.
- **nucleo** — fuzzy matcher Helix uses. Alternative to `hayai`.

#### TUI / rendering

- **ratatui** (already) — TUI framework.
- **crossterm** (already) — terminal abstraction.
- **image** — icon rendering.
- **syntect** — tm-grammar highlighter (fallback if a language has no TS grammar).

#### LSP / DAP / RPC

- **tower-lsp** — LSP server framework (for escriba-lsp *server* mode).
- **lsp-types** — LSP type definitions.
- **async-lsp** — tokio LSP stack.
- **lapce-rpc** — RPC types from Lapce editor.
- **neovim-lib** — nvim RPC client (for opening files in a live nvim).
- **dap-types** — DAP type definitions.
- **rmcp** (already in escriba-mcp) — MCP SDK.

#### Git & FS

- **git2** — libgit2 bindings. For `escriba-git` (gutter, blame, log).
- **gitui-rs** — git ops from GitUI (reusable logic).
- **ignore** / **globset** — gitignore-aware file walk.
- **notify** — filesystem watcher. For auto-reload on external edits.
- **arboard** — OS clipboard. (or use `hasami`.)
- **trash** — safe file deletion.

#### Collaboration / CRDT

- **yrs** — Yjs port for real-time collab. Would power a zed-style
  collaborative editing layer.
- **crdts** — CRDT primitives.
- **automerge** — doc-centric CRDT.

#### Runtime / async

- **tokio** (already) — async runtime.
- **smol** — lightweight alternative.
- **rayon** — data parallelism (for batch formatter runs).

#### Scripting / macros

- **rhai** (via `soushi`) — embedded scripting.
- **mlua** — Lua scripting (for blnvim plugin compat via adapter).
- **pyo3** — Python scripting (for vim plugins that use `+python3`).
- **deno_core** — JavaScript (for nvim plugins that use `+js`).

#### Search / indexing

- **tantivy** — full-text search engine.
- **ignore** — ripgrep-style walker.
- **grep** — ripgrep internals (could drive in-buffer / in-workspace search).

#### Terminal emulation (for embedded term)

- **alacritty_terminal** — terminal emulator backend.
- **vt100** — VT100 parser.
- **mado** (pleme-io) — GPU terminal.
- **portable-pty** — pty abstraction.

#### Markdown / text rendering

- **pulldown-cmark** — markdown parser.
- **comrak** — CommonMark renderer.
- **mdbook** — book renderer (for in-editor docs browser).

---

## Proposed absorption priority (next three waves)

### Wave 2a — Tree-sitter substrate (biggest visible-impact unit)

1. Bump tree-sitter runtime to 0.24.x.
2. Add grammars: nix, markdown, bash, json, python, typescript, lisp, go.
3. Change `Grammar::extensions` to `Vec<String>` (runtime-mutable).
4. `escriba-lisp::apply_plan_to_grammar_registry` wires defmode → extension table.
5. Injections: enable `INJECTIONS_QUERY` per-grammar (already set for rust).
6. Textobjects: add `deftextobject` form + tree-sitter textobjects query files.
7. Folds: tree-sitter folds query consumption → `BufferParser::folds()`.

### Wave 2b — Interactive UX primitives

1. Multi-selection in `escriba-core::Selections`.
2. `pending_key: Vec<Key>` in `ModalState` (unlocks gh/gg/dd multi-stroke).
3. `escriba-picker` crate using `hayai` or `nucleo` — telescope parity.
4. `escriba-popup` crate using `egaku` widgets — which-key parity.
5. `defregister`, `defmark`, register / jump lists in runtime.

### Wave 2c — Tooling integrations

1. `escriba-git` (git2) — gutter + blame + log.
2. `escriba-dap-client` (DAP parallel to LSP).
3. `escriba-term` (mado + portable-pty).
4. `escriba-tree` (file tree pane — egaku).
5. AI client half of `escriba-mcp` — inline-completion UI.

---

## Editor-ecosystem things worth LEARNING from even if not absorbed

- **Zed's rope + tree-sitter edit batching** — their `Buffer::edit()` is the
  textbook. Study `zed-industries/zed/crates/text/src/text.rs`.
- **Helix's keymap evaluator** — typed token → Action dispatcher without a
  string hop. Study `helix-view/src/keyboard.rs`.
- **Kakoune's selection-first model** — every command operates on
  selections, not cursors. Study `mawww/kakoune/src/normal.cc`.
- **Emacs's completion-at-point-functions** — composable completion
  source protocol. Study `emacs/lisp/minibuffer.el`.
- **Vim's text object grammar** — `viw`, `va{`, `dip` — compositional
  operator + motion + scope. Worth lifting into tatara-lisp macro form.
- **VSCode's extension protocol** — JSON-RPC-only boundary; every
  capability negotiated. Influence the tatara-lisp plugin shape.
- **Cursor / Windsurf's AI UX** — inline diff, accept-or-reject widgets.
  Drive the escriba-mcp client design.

---

## Out-of-scope (deliberately)

- **Nvim plugin API compat**: writing a vim-ish shim so lua plugins
  run as-is inside escriba. Too invasive; better to provide tatara-lisp
  equivalents + import tools.
- **Emacs lisp interpreter**: too large a semantic surface.
- **GUI IDE features** (debugger visualisation, diagram editors): out of
  the text-editor category — defer to specialised tools.

---

## Open design decisions

1. **Fuzzy matcher choice** — `hayai` (pleme-io native) vs `nucleo`
   (Helix's, battle-tested). Leaning `hayai` for fleet cohesion.
2. **Grammar sourcing strategy** — crates.io versions vs. vendor in-tree.
   Crates.io is easier; vendor gives hermetic builds (important for Nix).
3. **LSP manager process model** — in-process (simple) vs out-of-process
   daemon (can survive editor restarts, shared across splits). Helix is
   in-process; zed is daemon. Leaning daemon for fleet integration.
4. **Plugin ABI** — tatara-lisp only (Rust+Lisp pattern) vs WASI (broader
   community). Leaning tatara-lisp with a WASI fallback for untrusted
   community plugins.
