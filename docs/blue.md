# blue in escriba — two integrations, both shipped

`theory/BLUE.md` §VII.5 names escriba the strongest of blue's three
candidate consumers: *"an editor needs a user-facing language, escriba
already speaks the AST, and blue is the ergonomic surface over it. Emacs'
lesson, without Emacs Lisp."*

That argument is about blue as escriba's **extension language**. It is a
different thing from blue as a language escriba **edits**, and conflating
the two is how a well-scoped filetype addition gets described as though it
shipped a programmable editor. This document keeps them apart.

---

## 1. blue as an EDITED language — SHIPPED

Five legs, matched to the shape every other language in escriba already
has. `escriba/tests/blue_language.rs` is the forcing function: drop any leg
and the build goes red naming the leg.

| Leg | Where | Tier |
|---|---|---|
| Highlighting | `escriba-render/src/langs.rs` → registered in `gpu::build_ecosystem` | **LIVE** — a `.b` buffer is painted today |
| Major mode | `(defmode :name "blue")` in `configs/blnvim-defaults.lisp` | **LIVE** — fills the filetype table the session reads |
| Language server | `(deflsp :name "blue")` in `catalog/escriba-lspconfig…` | **LIVE** — the session below |
| Formatter | `(defformatter :filetype "blue")` in `catalog/escriba-conform…` | **LIVE** — `lsp.format` through `FormatRunner` |
| Canonicality gate | `(defgate :name "blue-canonical")` in `catalog/escriba-conform…` | **declaration only** |

### The live session

`escriba_lsp_client::session::SessionRunner` holds one server per command and
root. It lives on its own thread with its own reactor. Nothing in it names a
language, and it does not parse blue: names, scope and tiers are what the server
answers. A new `use` spelling or a nested namespace needs no change here.

What a person sees while typing a `.b` file:

| Thing | Where it shows | How |
|---|---|---|
| Completion | a popup at the caret | a word of `cmp.keyword_length` chars, a server trigger char (`:` for blue), or `<C-n>` |
| Why a name is offered | the popup row: `needs a use`, `imported`, `builtin` | the server's `labelDetails` |
| Diagnostics | the gutter, and the first line beside the code | `publishDiagnostics` per revision |
| Live values | `=> 3` beside the line | `textDocument/inlayHint` |
| Doctest results | diagnostics | blue's second push |
| Hover | a card at the caret, `K`; any key closes it | `textDocument/hover` |
| Definition | `gd`, on the jumplist | `textDocument/definition` |
| Server state | `[blue]`, `[blue:restarting 1/5]`, `[blue:DEAD …]`, `[blue:MISSING blue]` on the status line | `Negai::ServerStatus` |

Popup keys: `<C-n>`/`<C-p>` and the arrows move. `<Tab>` or `<C-y>` accepts.
`<C-e>` closes. `<Enter>` accepts only after you moved in the popup, so a word
you typed is never replaced by a newline. Any other key types. Accepting applies
the server's `additionalTextEdits`, which is how `retsu::x` gains its `use`.

| Option | Default | Meaning |
|---|---|---|
| `lsp.session` | `true` in the catalog, off with `--no-defaults` | `false` keeps the one-shot errand per open |
| `lsp.debounce_ms` | `80` | quiet time before a full-sync `didChange` |
| `lsp.request_timeout_ms` | `10000` | a question this old shows `slow` |
| `lsp.hang_after_ms` | `20000` | a write blocked this long kills the server |
| `lsp.restart_limit` | `5` | restarts with backoff (2250 ms to 8 s) before `DEAD`; `:LspRestart` retries |
| `lsp.max_document_chars` | `8388608` | a larger file gets no session, and a message says so |
| `cmp.enabled`, `cmp.sources` | `true`, `lsp,…` | off, or without `lsp`, no completion is asked |
| `cmp.autocomplete` | `true` | `false` leaves only `<C-n>` |
| `cmp.keyword_length` | `2` | chars of a word before an automatic ask |

All are `option("lsp.session", false)` in `init.b` or `(defoption …)` in the rc.

### Why the editor cannot hang on a server

- A keystroke does an `mpsc` send at most. The text copy runs once per quiet
  period, not per key, and `lsp.max_document_chars` bounds its size.
- `Runner::start` only posts to the session thread. Reads, writes, the handshake
  and every wait run there.
- A write runs in its own task, so a server that stops reading blocks that task
  only. Queued changes collapse to the newest per document, and a newer question
  replaces an older one of its kind.
- Replies re-enter through `Negai::ErrandReply`, sealed with the text revision
  and the session generation. A reply about older text is dropped.
- A dead server fails its waiters through `pending`, bumps `lsp_gen` so its
  findings go stale, and restarts with backoff. On return every open document is
  sent again from the editor's unsaved text.
- A binary not on `PATH` is `MISSING`, once, with no retry loop.

`escriba/tests/live_session.rs` holds these against a real `blue lsp`, with
`SIGKILL` for a death and `SIGSTOP` for a frozen server, asserting no key takes
250 ms. `blue` absent fails with that cause, except on a GitHub runner, which has
no `blue`: there the test says it did not run, in the step summary and on stderr.
`escriba-runtime/tests/session_dispatch.rs` holds the keyboard's half with no
server, on every machine.

### What is not done

- `cmp.sources` has one producer, `lsp`. `buffer`, `path` and `snippet` are
  still declarations.
- The GPU face paints the popup, the hover card and the side text as text over
  block glyphs, because it has no quad pipeline. Its placement logic is tested
  and shared with the terminal face. No test sees those pixels.
- An inlay hint shows at the end of its line, not at its column. blue's are at
  the end of the line anyway.
- A diagnostic about a file with no open buffer is dropped.
- Findings and tokens are sealed to a revision, so they blank while you type
  and return on the next answer. `lsp_gen` is one counter, so a restart stales
  every server's results until the re-send answers.
- `didChange` is full text for every server, also one that offers incremental.

### Highlighting: a table, not a grammar

There is no `tree-sitter-blue`. hikari's table backend is what serves every
non-tree-sitter language escriba supports (nix, yaml, lua, toml, …), and
blue joins them: `escriba_render::langs::BLUE_TABLE` is *data*, read by
hikari's one `TableLexer`. Nothing new was written — a language here is a
`static`, which is the entire point of that backend.

The `defmode` deliberately declares **no** `:tree-sitter`. Naming a
fictional grammar would make `apply_grammars` count `.b` as an extension
skipped for an unknown language, and would be a claim about the world that
is false.

Known limits, so nobody reads more into it: string interpolation
(`"n=#{x}"`) paints as one string span; the hash-literal label `name:` is
not a symbol and paints as identifier-plus-punctuation (the symbol form
`:name` *is* covered).

### Upstream now paints blue too — and the two will have to agree

**2026-08-04, blue @ `d276578`:** `blue lsp` gained a `semanticTokensProvider`
(`blue-lang-lsp/src/tokens.rs`), classifying the token stream from
`blue_lang_syntax::lex` — the same lexer the compiler runs. blue took that
path rather than a tree-sitter grammar for the reason its own
`docs/NATURALIZE-TREESITTER.md` §2 gives: a second definition of blue's syntax
drifts invisibly from the first.

escriba has **two** sources of colour for `.b`: this table, and the server's
tokens. The table works with no `blue` on `$PATH`. The server's tokens are more
precise (they know `def foo` declares a function, and they fix the `name:` label
limit noted above). Server tokens win while they are fresh for this revision;
the table paints otherwise. The terminal face reads only the table.

blnvim reached the same conclusion from the other side and needed no wiring at
all — nvim requests semantic tokens from any server advertising them, so blue
buffers there went from monochrome to coloured with no edit in that repo.

### Why the keyword table is a copy

The table is transcribed from `blue_lang_syntax::parse` (`SURFACE_KEYWORDS` plus
the four block words `is_reserved_word` adds). `escriba-vm` now links blue, so
`escriba-render` could read it from the crate. That move is open.

---

## 2. blue as escriba's EXTENSION language — SHIPPED

New editor configuration is written in blue. The lisp rc stays, and it keeps
working. Measured on `blue-lang-host` 0.0.151.

### The file

`~/.config/escriba/init.b`. The search order is `--init`, `$ESCRIBA_INIT`,
`$XDG_CONFIG_HOME/escriba/init.b`, then `$HOME/.config/escriba/init.b`. A relative
`XDG_CONFIG_HOME` or `HOME` is ignored. `--no-init` or `ESCRIBA_NO_INIT=1` keeps the
file out. `escriba/examples/sample-init.b` is a whole example, and a test loads it.

```blue
use("escriba", %i[bind command option theme])

def escriba()
  [
    theme("nord"),
    option("number", true),
    bind("normal", "gh", :whereami),
    command("whereami", "Say where the cursor is", :whereami)
  ]
end

def whereami(args)
  message("#{buffer_name} #{cursor_line + 1}")
end
```

### The order

1. The bundled defaults.
2. The lisp rc.
3. `init.b`, so a blue value wins.

### The words

The `escriba` bidama ships inside the binary (`escriba-vm/bidamas/escriba`). It
is the same shape as `motenashi`: pure constructors over `kousei` entries.

| Word | Lisp twin |
|---|---|
| `bind(mode, key, action)` | `defkeybind` |
| `alias(name, description, action)` | `defcmd` |
| `command(name, description, call)` | none: the body is a blue function |
| `option(name, value)` | `defoption` |
| `theme(preset)` | `deftheme` |
| `highlight(group, fields)` | `defhighlight` |
| `hook(event, command)` | `defhook` |
| `form(keyword, name, fields)` | every def-form in `escriba_lisp::FORM_KEYWORDS` |
| `off(kind, name)`, `tune(kind, name, fields)` | none |

A def-form's fields are checked by its Rust spec, through the validators the lisp
rc goes through (`escriba_lisp::apply_forms`). Blue holds no second field table.
A blue map becomes the lisp form with `blue_lang_host::domain_form`, so the two
spellings cannot disagree. This removes the old blocker, that blue could not
express a def-form.

Inside a function, these editor words work: `message`, `insert`, `set_option`,
`run_command`, `cursor_line`, `cursor_column`, `current_line`, `editor_mode` and
`buffer_name`. Reads answer from a snapshot, and writes apply after the function
returns.

### Commands

`:blue <source>` runs one line of blue, with the functions of `init.b` in scope.
The top level of `init.b` does not run again for its effects. A definition made
on the line does not persist, so `script_language` stays `lisp` by default.

### The budget and the fault model

Blue never runs in the editor process. The binary starts itself as
`escriba blue-worker` for each load, command and line. It sends a request on stdin
and reads one reply with the effects to apply.

| Bound | Load | Command or line |
|---|---|---|
| Steps (`LOAD_FUEL`, `CALL_FUEL`) | 20,000,000 | 5,000,000 |
| Wall clock, then the worker is killed | 3 s | 2 s |
| Call depth | `DEFAULT_MAX_DEPTH` | `DEFAULT_MAX_DEPTH` |
| Address space, Linux only | 1 GiB | 1 GiB |
| Host effects (`process`, `filesystem`, …) | none | none |

The first version ran in process, under steps alone. A probe proved that wrong:
`range(0, 100000000000000)` is one native call, so it spends one step, and it ran
for more than 180 s while it allocated. The test
`every_way_a_world_fails_is_reported_and_the_editor_still_opens` holds this case
through the real binary.

| Fault | The answer |
|---|---|
| A parse or check error, or a raise at the top | a message; nothing from the file applies |
| A raise or a loop in `escriba()` | a message; the top-level effects stand |
| A bad entry | a message for that entry; its siblings stand |
| A native call that hangs or allocates | killed at the deadline, a message |
| An abort in the worker | a message; the editor does not notice |

The editor waits for the worker, so a runaway command holds a keypress for up to
2 s. It does not hang.

Measured on cid, release without LTO, with `examples/sample-init.b`: start-up goes
from 180 ms to 220 ms, a worker holds 15 MB and lives for the call, a one-line
script takes 10 ms, 5,000,000 steps run out in 0.35 s, and 20,000,000 in 1.4 s, so
the steps end a plain loop before the clock does.

**In-process use stays for tests.** `EditorState` serves blue in process until
the binary calls `set_blue_exec`, so `escriba-runtime` tests need no binary. Those
tests have only the step budget.

### What a failure looks like

`init.b:` messages reach the message line and `:messages`. `--dry-run` prints
them, and `--list-rc` prints the refusals.

### What is not done

- `init.b` loads once. There is no hot reload.
- A hook entry reaches the plan, and nothing fires hooks yet, for lisp or blue.
- `run_lisp` stays in process, with no budget.
