# escriba: the destination

escriba is the editor that is blue all the way down (operator, 2026-11-07):

- **Embedded.** The blue runtime runs inside escriba's process. A buffer, a
  window, a key and the event loop are blue values and hooks; a keypress calls
  a blue word; a live REPL and eval of a line, a region or a buffer run against
  the editor as it is.
- **Programmable.** Every command, motion, operator, keymap and extension is a
  blue word. Rust keeps the host primitives only: the terminal and the GPU, file
  and process I/O, the language-server transport.
- **Configurable.** Configuration is blue declaration entries in the shikumi
  tiers (defaults, file, environment, overrides). Each entry is typed, has a
  default, is refused by name when malformed while its siblings apply, and is
  turned off rather than deleted.
- **Bounded.** An extension runs in its own capability frame (blue's waku, HORIZON
  H13): a plugin with no `filesystem` cannot write a file.
- **At parity where it matters.** Parity is measured against blnvim, the
  operator's neovim (`blackmatter-nvim`), not against all of vim.

## Where escriba stands (measured 2026-11-07, `main` at 4027014)

- blue is linked (`blue-lang-host` 0.0.151, `escriba-vm`), and `~/.config/escriba/init.b`
  runs in a supervised child, `escriba blue-worker` (`docs/blue.md`). Scripting
  defaults to tatara-lisp.
- The base vim keymap (`escriba-keymap/src/lib.rs`, `Keymap::default_vim`), the
  action table (`escriba-lisp/src/apply.rs`, `resolve_action`) and the ex alias
  table (`escriba-command/src/ex.rs`) are Rust literals. Only plugin and user
  bindings are data.
- Config: tatara-lisp defaults (`escriba/configs/blnvim-defaults.lisp`) and an rc
  search path beside a thin shikumi `EscribaConfig`.
- The GPU face hard-codes its font size and line height (`escriba-render/src/gpu.rs`).
- CI excludes `escriba-render` and `escriba-tui` from tests, and the flake has no
  `checks`.

## Two blue primitives come first

Both block embedding, and both are blue's to build, not escriba's:

1. **A persistent embedded session.** `blue-lang-host` cannot extend a loaded
   `Program` with new source, so a `def` made in one eval is gone in the next,
   and `session::Session` is fixed to blue's own host type. Needed: a session
   generic over the host, that takes new source into the same program.
2. **A stoppable native call.** A step budget does not stop one long native
   call, which is why user config runs in a child process. Needed: an
   interruptible native (a deadline the host checks), or a capability frame
   that withholds the long natives from config.

## Parity, as data

`docs/parity.tsv` holds one row per feature: area, feature, the blnvim key or
mechanism, the state (built, partial, absent), the test that holds a built row,
and the rung that closes an open one. `escriba/tests/parity_matrix.rs` gates it:
a built row with no test, a cited test that is not a `#[test] fn`, an absent row
that claims a test, an open row with no rung, or a value outside its set is red.
Its second test feeds ten bad rows and requires each to be refused.

Measured: 50 rows, 17 built, 9 partial, 24 absent.

## Rungs

| Rung | Closes | Done when |
|---|---|---|
| R1 | embedding: the two blue primitives; eval of a line, region and buffer; the REPL; the keymap as blue data; options as shikumi-tiered blue entries; blue as the default script language | the `blue` and `config` rows are built and the base keymap is a blue table |
| R2 | blue.nvim parity: the kiwari grammar, the `blue lsp` session, run, test, check and fmt of the buffer | every `blue` row is built |
| R3 | vim core: named registers, macros, visual block, `:s`, `:sp`/`:vsp` in the ex table, the persistent undo tree, and blnvim's edits (`A-j`/`A-k`, `C-s`, `jk`, trailing whitespace) | every `core` and `editing` row is built |
| R4 | finding and language: files, live grep, the explorer, projects; completion, hover, definition, rename, code action, references, outline, diagnostics; tree-sitter objects, motions and swap | every `find`, `lsp` and `syntax` row is built |
| R5 | git hunks and blame, the statusline, the Nord and Vellum theme with its 256-colour fallback, the terminal | every row is built |

House rules for the work: new code carries no comments (intent goes in
`@doc`); a fix lands with its test and a red run; each rung moves the code it
touches to the highest rung of the ladder that can hold it (blue, then
tatara-lisp, then Rust).
