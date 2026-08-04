# blue in escriba — two integrations, only one of them landable today

`theory/BLUE.md` §VII.5 names escriba the strongest of blue's three
candidate consumers: *"an editor needs a user-facing language, escriba
already speaks the AST, and blue is the ergonomic surface over it. Emacs'
lesson, without Emacs Lisp."*

That argument is about blue as escriba's **extension language**. It is a
different thing from blue as a language escriba **edits**, and conflating
the two is how a well-scoped filetype addition gets described as though it
shipped a programmable editor. This document keeps them apart, records what
landed, and records the two independent reasons the second one did not.

---

## 1. blue as an EDITED language — SHIPPED

Five legs, matched to the shape every other language in escriba already
has. `escriba/tests/blue_language.rs` is the forcing function: drop any leg
and the build goes red naming the leg.

| Leg | Where | Tier |
|---|---|---|
| Highlighting | `escriba-render/src/langs.rs` → registered in `gpu::build_ecosystem` | **LIVE** — a `.b` buffer is painted today |
| Major mode | `(defmode :name "blue")` in `configs/blnvim-defaults.lisp` | declaration; consumed by `apply_grammars` |
| Language server | `(deflsp :name "blue")` in `catalog/escriba-lspconfig…` | **declaration only** |
| Formatter | `(defformatter :filetype "blue")` in `catalog/escriba-conform…` | **declaration only** |
| Canonicality gate | `(defgate :name "blue-canonical")` in `catalog/escriba-conform…` | **declaration only** |

### The tier line, stated plainly

Only highlighting is live. `ApplyPlan::lsp_servers`, `::formatters` and
`::gates` have **no consumer in `escriba-runtime`** — nothing spawns a
server, runs a formatter or fires a gate, for *any* language. blue is not
behind here; it is level with rust, and it becomes live for free when those
subsystem waves land. Until then "escriba has a blue LSP binding" means the
binding is present and coherent in the boot plan, and nothing more.

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

Nothing in escriba changes today, because escriba spawns no language server
for any language. It matters for **when the LSP wave lands**, and it is worth
writing down now rather than rediscovering it then:

- escriba will then have **two** sources of colour for `.b` — this table, and
  the server's tokens — where every other language has one. They must agree, or
  a buffer repaints differently the moment the server attaches. The table is
  the fallback that works with no `blue` on `$PATH`; the server's is the more
  precise one (it knows `def foo` declares a function, and it fixes the `name:`
  label limit noted above, which a table lexer structurally cannot).
- So the wave's blue-specific question is *precedence*, not *capability*:
  server tokens over table when a server is attached, table otherwise.
- This does **not** change the tier line above. The `(deflsp :name "blue")` leg
  is still a declaration with no consumer, and blue is still level with rust.
  What changed is upstream: the thing that leg will eventually spawn now has
  something more to say.

blnvim reached the same conclusion from the other side and needed no wiring at
all — nvim requests semantic tokens from any server advertising them, so blue
buffers there went from monochrome to coloured with no edit in that repo.

### Why no blue crate dependency

The keyword table is transcribed from `blue_lang_syntax::parse`
(`SURFACE_KEYWORDS` + the four block words `is_reserved_word` adds) rather
than imported. Reading it from the crate would be the drift-proof spelling,
and was rejected on three measurements:

- `blue-lang-syntax` is **0.0.12** on crates.io. Under cargo semver a
  `0.0.x` version is its own compatibility range, so `"0.0.12"` resolves to
  *exactly* 0.0.12 — the dependency would not follow blue at all. Eleven
  releases went out in the three hours after first publish. The
  "cannot drift" benefit is illusory at this version.
- It would unify `tatara-lisp` 0.3.3 → 0.3.21 across all twenty-one escriba
  crates, to import fifteen strings.
- The rest of blue is not published, and **escriba is** (`escriba` 0.1.20).
  A `git =` dependency would freeze escriba's own publishing.

The blue *toolchain* is wired the other way — `blue lsp` and `blue fmt` are
subcommands of one binary resolved off `$PATH`. That needs no registry and
follows blue automatically, which is why it is the half that gets to be
version-agnostic.

---

## 2. blue as escriba's EXTENSION language — NOT LANDED

### The constraint that governs it

> *"The moment blue is EVALUATED rather than PARSED, the security boundary
> moves: a session preset that can branch is a session preset that can run
> code."* — `theory/BLUE.md`

For a config format, shikumi's seam deliberately takes **only the parser**.
An editor extension language is a genuinely different case — an editor
extension *is* meant to run code — but that has to be a decision someone
makes on purpose, with the reach named. It must never arrive as a side
effect of "we added a language". Nothing in this change evaluates blue.
`escriba` links no blue crate at all; the only blue code that runs is
`blue lsp` / `blue fmt`, in their own processes, launched explicitly.

blue's `waku` posture lattice (REACH/WHEN/WHERE, narrow-cannot-widen) is the
mechanism designed for exactly this gate. It is also young: `check_reach`
got its first non-test caller in `7ce9cee`. It is the right destination for
bounding an escriba extension's reach; it is not yet evidence that the
bound holds.

### The blocker is in blue's surface, and it is measured

Independent of any security or dependency question, blue **cannot currently
express an escriba def-form**. Every escriba def-form — and every
`#[derive(DeriveTataraDomain)]` form in the fleet — is flat kwargs:

```lisp
(defkeybind :mode "normal" :key "<C-s>")
```

Probed against the built `blue` binary (`blue ast`, 2026-08-02):

| blue source | lowers to |
|---|---|
| `defkeybind(mode: "normal", key: "<C-s>")` | **parse error** — `expected an expression, found Label("mode")` |
| `defkeybind({mode: "normal", key: "<C-s>"})` | `(defkeybind (hash-map :mode "normal" :key "<C-s>"))` |

The second parses, but it is a *different tree*: one nested `hash-map`
argument, not alternating keyword/value. `compile_typed` will not read it.

So even the conservative, evaluation-free version of integration 2 — a
`.b` rc parsed to `Vec<Sexp>` and handed to the existing applier, exactly
the shape of the shikumi seam — does not work today. It is blocked on blue
growing a labelled-argument call form that lowers to flat kwargs.

The tempting local fix is for escriba to unwrap
`(defX (hash-map …))` → `(defX :k v …)` on the way in. That is refused
here: it would invent a private escriba-only calling convention for blue,
put the fleet's def-form ABI in two places, and hide a language gap behind
an editor workaround. The load-bearing fix is in blue.

### What has to be true before integration 2 is reconsidered

1. blue's surface lowers a call with labels to flat kwargs (blue-side).
2. blue's runtime crates are on crates.io, so escriba can depend on them
   without a `git =` that would freeze escriba's own publishing.
3. If — and only if — the step past parsing is taken, the reach an escriba
   extension gets is named explicitly and bounded by `waku`, with the
   threat model written down before the first evaluation call site.

Steps 1 and 2 are blue's. Step 3 is a decision, not a task.
