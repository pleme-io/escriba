# The escriba backlog — an implementation plan

**Status:** DESIGNED. Nothing in §V–§VI is written. Every claim marked
VERIFIED in §VIII was checked against the source; everything else is design.

**Framing (operator instruction, 2026-08-07):** *"it doesn't matter how long
it is, we do it piece by piece and only care about delivering quality that
compounds — abort all care of timelines."*

This document therefore carries **no schedule, no effort tier, and no cut
line.** An earlier draft did. That draft ordered the work by cost and
proposed deleting seven capabilities for being poor value — reasoning the
★★★ COMPOUNDING DIRECTIVE names as the cardinal sin, and which produced a
visibly worse plan (see §VII, where the cost framing had it exactly
backwards). The ordering here is by **what makes the next piece safer**.

---

## I. The destination

escriba stops being an editor with 85 dead keybindings and becomes a
substrate that *generates* editor features.

Concretely, when this is done:

- Authored behaviour **never mutates the editor**. It reads through a
  capability-narrowed window and returns typed request slips. Exactly one
  interpreter in the whole codebase holds `&mut EditorState`.
- Everything that **floats** — picker, which-key, completion, hover,
  diagnostics, DAP panes, zen mode, toasts — is one `Surface` in one
  z-ordered stack.
- Everything **spatial** is one leaf in one container tree whose geometry is
  a pure function of the frame, never stored.
- Everything **located** — a diagnostic, a git hunk, a test failure, a grep
  hit, a TODO, a conflict, a reference — is one `Finding` in one named list,
  walked by the same stepper that already drives `n`/`N`.
- Everything **out-of-process** is an errand with a revision-anchored reply,
  and `tick()` remains a pure synchronous reducer testable with no runtime
  and no socket.

At that point "add go-to-definition" is a config entry plus a producer. Not
an engineering project. That is the compounding claim, and it is the only
reason this plan is worth its length.

## II. The insight

**The backlog is not 22 subsystems. It is 5 missing primitives — and 2 of
them already exist in a fleet library escriba does not yet depend on.**

The evidence is a confession already in the source. `escriba-runtime`'s
`run_command` special-cases `:noh` *inside the runtime*, with this comment:

> handled here rather than in the command registry because it mutates
> SearchState, which EditContext does not expose (and should not — the
> registry's contract is buffers + modal state)

The ceiling was hit once and worked around. Wiring 22 subsystems the same way
means 22 more special cases, each individually reasonable, collectively a
second dispatch system.

And `egaku` v0.1.9 — a pleme-io library, on crates.io, **already in escriba's
`Cargo.lock` as a transitive dependency**, 239 tests, **zero rendering
dependencies** — already ships `FuzzyPicker<T>` (864 lines, a typed
event→effect machine), `SplitPane`, `Modal`, `FocusManager`, `KeyMap<A>`,
`list`, `table`, `scroll`. It has **no consumers**. Two of the five
primitives below are therefore *upstream extensions of a library we already
own*, not new escriba code.

> A prior version of this plan's brief asserted the picker should be built on
> `skim`, "already in the fleet via frostmourne". That is false: no
> `Cargo.lock` in the fleet contains skim, and frostmourne has no Rust crate.
> Building on that claim would have added a dependency to replace a library
> we already ship. Fix the claim in `CLAUDE.md`.

## III. Standing rules for this build

Every piece below obeys all five. A piece that cannot is not ready.

1. **Independently landable.** It compiles, tests green, and ships alone. No
   piece may require a later piece to be correct.
2. **It seals a class.** Each piece makes a category of bad state
   unrepresentable, or — at the honest floor — CI-caught. Never "we'll be
   careful".
3. **A red run is recorded.** Every gate is proven against deliberately
   broken input before it counts. A gate never run red is not known to work.
4. **Tier-honest.** `truly-unrepresentable` > `parse-time-rejected` >
   `only-mitigated`. Never round up. A `Result::Err` is mitigation; a compile
   error is unrepresentability.
5. **Upstream when it is fleet-shaped.** If another pleme-io TUI would want
   it, it lands in `egaku`/`memori`, not in escriba.

## IV. Three corrections that must land before any code

These came from the subsystem designers, who were explicitly asked to report
where a foundation failed them. All three are load-bearing; all three are
cheap now and expensive later.

### IV.1 `Negai::Spawn` and `Errand` are the same thing

`madoguchi` designed process-spawning as `Negai::Spawn(JobSpec)`; `denrei`
designed it as `Errand`. Two job systems, independently derived, in one plan.

**Decision:** `Negai::Spawn` is deleted before it exists. Spawning is a
`denrei` errand. `madoguchi` gets `Negai::Errand(ErrandId)` — behaviour asks
for an errand by identity; the interpreter hands it to the courier. One
supervisor, one cancellation path, one place staleness is decided.

### IV.2 Freshness is two-axis, not one

`denrei` anchors a reply to `Anchor::Text { buffer, rev }`; `shirube` seals a
list with a buffer-only `RevSet`. **A git hunk set is stale when the buffer
moves *or* when the git index moves.** So is a test result (source + binary),
an LSP diagnostic (buffer + server generation), a DAP breakpoint (buffer +
session).

**Decision:** generalise to an axis set from the start —

```rust
enum Axis { Text(BufferId, TextRev), Index(IndexRev), Session(SessionGen) }
struct Anchor(SmallVec<[Axis; 2]>);   // fresh iff EVERY axis still matches
```

Single-axis-then-retrofit means auditing every producer later and finding the
ones that silently kept reporting stale results. This is the difference
between a seal and a bug.

### IV.3 Request slips cannot ask for the next key

`ys{motion}{char}`, `f{char}`, `r{char}`, `m{a-z}`, `"{reg}y` are all
*continuations*: they consume a key that has not been typed yet. Slips as
designed are fire-and-forget, so every one of these is unbuildable.

**Decision:** add `Negai::AwaitKey { then: Continuation }`, and implement it
by **extending the existing `zenmai` operator-pending FSM** rather than
adding a second pending-key mechanism. escriba is already zenmai's third
consumer; a parallel FSM would be the duplication the directive forbids.

## V. The build order

Ordered so that each piece makes the following ones safer to build. Effort is
deliberately absent.

### Phase 0 — Seal the silent-failure class

**Why first:** *nothing after this is verifiable without it.* Today a dead
keybinding is indistinguishable from a working one at runtime, in two
independent ways, and a duplicate buffer is indistinguishable from an edit.
Every later phase's "I wired X" is unfalsifiable until dispatch can fail
loudly.

Three verified defects (`escriba-command:232`, `escriba-runtime`'s
`run_command`, `escriba-buffer:349`):

| Defect | Today | After |
|---|---|---|
| `run_action`'s `_ => Ok(())` | unknown action reports **success** | typed `CommandError::Unhandled(sym)` |
| `let _ = self.commands.run(…)` | `NotFound` **discarded** | surfaced as a user-visible message |
| `BufferSet::open` mints a new id always | same file twice → **two buffers, divergent undo** | `open_or_focus`; a path→id index makes duplicates unrepresentable |

**Seal:** dispatching an unresolvable action produces an observable typed
outcome. **Done-predicate:** a new runtime companion to
`action_resolution.rs` proves an inert action is *reported*, not silent; red
run recorded against a deliberately unregistered name.

### Phase 1 — `madoguchi` 窓口 · the dispatch seam

**Why here:** it is the root. All five subsystem designs name it first, and
the `:noh` special case is its absence made visible.

Behaviour reads through a capability-narrowed `Snapshot` and returns
`Vec<Negai>`. One total interpreter applies them — **the only code in escriba
holding `&mut EditorState`.**

Includes corrections IV.1 (`Negai::Errand`, no `Spawn`) and IV.3
(`Negai::AwaitKey` on zenmai). Adds the `Keymap` view on `Snapshot` that
which-key needs and that the original design omitted.

**Seal (truly-unrepresentable):** a handler bound to `(Search,)` that calls
`.buffers()` **fails to compile**. **Done-predicate:** the `:noh` special
case is deleted; the `INERT` ratchet moves *down* as
`buffer.{next,prev,delete}` and `comment.toggle-*` go live.

> **Amended 2026-08-07 (M3).** The original predicate read "`apply_resolved`
> contains zero `self.` mutations". Implementing it revealed that would force
> a bad design: only 7 of `Action`'s 30 variants have a slip equivalent. The
> other 23 — prompt editing, the operator-pending FSM, motion resolution, the
> jumplist, the dot register — are the KEYMAP's vocabulary, not the AUTHORED
> one. Forcing them into `Negai` would put `PromptClearToStart` and
> `SearchPreviewStep` in front of every plugin author and make the capability
> question meaningless (what capability does a caret move read?). One type
> serving two vocabularies is the mistake.
>
> The invariant actually worth having is **one implementation per mutation**,
> not one vocabulary. The 7 overlapping actions lower onto the interpreter;
> the 23 mechanics stay in the executor.

### Phase 2 — `shirube` 標 · located findings

**Why here:** the highest-compounding piece in the plan. Diagnostics, git
hunks, test results, references, grep hits, TODOs and conflict sites are one
shape. Building it second means every later producer is a *source*, not a
subsystem.

Carries correction IV.2 (axis-set freshness). Its M0 lifts `Wrapped`,
`Landing` and the `Bound` stepper into **memori** — a fleet-level
improvement, and it makes result navigation share one implementation with
`n`/`N` rather than growing a second stepper.

**Seal:** a finding cannot be read without its anchor matching — a stale
result is *absent*, never *wrong*. **Done-predicate:** a producer test lands
a delivery, an edit invalidates it, and the gutter goes empty rather than
lying.

### Phase 3 — `kasane` 重ね · floating surfaces *(upstream: egaku)*

**Why here:** everything interactive from here on is a surface.

Fix `Surface::on_key`'s stringly-typed `KeyCombo` **before** escriba consumes
it — that is an upstream fix to a fleet library, per rule 5, not a shim in
escriba. Retrofit `escriba_ui::splash` onto it and delete `screen_chunks`:
one consumer today is precisely when that change is cheap.

**Seal:** two surfaces cannot both claim one keystroke; occlusion is total
over the stack. **Done-predicate:** deleting the route call fails a test; a
ratatui `TestBackend` snapshot shows correct band-order occlusion.

### Phase 4 — `shikiri` 仕切り · the container tree *(upstream: egaku)*

**Why here — and why it is NOT cut:** the earlier cost-driven draft deferred
this as "16 days for 4 pane actions". That was the wrong measure. `shikiri`
is the spatial algebra that makes the file tree, the terminal pane, the
bottom drawer, and side-by-side conflict resolution *possible at all*, and
`solve(tree, frame)` as a pure function deletes a bug class permanently:
geometry as stored state, which is what `Window.rect` is today.

**Seal (truly-unrepresentable):** a window with no rect, a layout with no
active window, and overlapping panes all cease to be constructible —
`Window.rect` is **deleted**, not maintained. **Done-predicate:** toggling a
dock twice returns `solve()` to a byte-identical layout; the
`scroll_to_contain` cursor-visibility invariant survives resize.

### Phase 5 — `denrei` 伝令 · the courier

**Why here:** everything before it is synchronous and provable. Async arrives
last, and arrives *outside* the core.

The load-bearing promise is not "escriba gets async" — it is **`tick()` never
becomes async.** It stays a pure total synchronous reducer forever; the
supervisor lives outside `EditorState` and its only path back in is a typed,
anchored value. That invariant is what keeps the editor testable for the rest
of its life.

Implements `ErrandKind::Process` (named but unimplemented in the original
design) and consumes the Phase-2 axis set rather than inventing staleness.

**Seal:** a reply whose anchor no longer matches is **dropped**, not applied.
**Done-predicate:** a `deftask` runs off-thread while a PTY test types 20
keystrokes with no dropped input; an edit *during* a format makes the reply
drop.

### Phase 6 — the subsystems

Each is now a producer plus config. Ordered by what each teaches the next.

| Order | Cluster | Why here | Lands |
|---|---|---|---|
| 6a | **Editing / structure** | the `Sintaxe` tree-sitter seam that comment, surround, fold and todo all share — plus text-objects (`ciw`/`diw`), which are *not on the 85 list* and are pure upside | comment ×2, surround ×3, todo ×2, fold ×3, snippet ×2 |
| 6b | **Version control** | conflict markers live in buffer text, so five verbs are pure `&str` work needing no async — it proves `shirube`'s gutter before LSP depends on it | git ×9, conflict ×5, gitbrowse |
| 6c | **Navigation / pickers** | consumes `egaku::FuzzyPicker` as a `kasane` surface; the 7 picker verbs are one adapter over different `T` | picker ×7, files ×2, tree, whichkey, buffer/pane verbs |
| 6d | **Language intelligence** | the longest pole, but by now *only a producer into `shirube`* | lsp ×15, cmp ×2, trouble ×3, illuminate ×2 |
| 6e | **Debug / test** | the edit-surviving anchor table generalises marks, breakpoints and hunks | dap ×7, test ×5 |

## VI. Waves 1.5–4, placed

| Roadmap item | Where it lands |
|---|---|
| Multi-key polish, `defsource` | Phase 1 (`madoguchi` authoring) |
| Multi-selection (`Cursors`) | Phase 6a — `Cursors` already exists and is routed through |
| Registers / clipboard | Phase 1 + `hasami` (has a `MockClipboard` seam already) |
| Marks / jumplist | Phase 2 — a mark **is** a `Finding`; the jumplist already works |
| Picker, which-key | Phase 3 + 6c |
| DAP, git, tree, term | Phase 4 + 5 + 6b/6c/6e |
| Undo tree, macros | Phase 1 — a macro **is** `Vec<Negai>`, which the design already noted |
| Sessions | Phase 4 (`defsession` parses today and is inert) |
| Notebook cells, CRDT | Explicitly deferred — no primitive here serves them; revisit when one does |

## VII. Nothing is deleted — the reversal

The cost-driven draft proposed deleting seven capabilities (`snacks.zen`,
`snacks.gitbrowse`, `leap.*`, `illuminate.*`) as poor value. With cost off the
table, each gets decided on merit — and the answers invert:

- **`illuminate.{next,prev}`** — genuinely subsumed by LSP
  `documentHighlight`. Not deleted, not separately built: **config over the
  Phase-6d producer.**
- **`leap.{backward,forward}`** — genuinely *distinct* from search: a labelled
  two-character jump, not a pattern walk. Build it; it is small and it
  composes with the operator engine.
- **`snacks.zen`** — a `kasane` surface plus a `shikiri` dock configuration.
  Nearly free once both exist.
- **`snacks.gitbrowse`** — one `denrei` errand.

This is the compounding thesis proving itself: given the substrate, the
"low-value" features are nearly free, and deleting them was a judgement about
*our schedule* dressed up as a judgement about *the editor*.

## VIII. Tier-honest ledger

| Claim | Status |
|---|---|
| The five primitive designs | **DESIGNED** — no code written |
| `egaku` ships picker/split/modal/focus; 239 tests; zero render deps; already in `Cargo.lock` | **VERIFIED** — source read |
| The three Phase-0 defects | **VERIFIED** — source read, line numbers cited |
| escriba is 100% synchronous; tokio declared in 2 crates, used in 0 | **VERIFIED** — zero grep matches in any `escriba*/src` |
| All 85 inert actions covered exactly once | **VERIFIED** — reconciled against the pinning test, not against prose |
| `skim` is *not* in the fleet | **VERIFIED** — no lockfile contains it |
| The three corrections in §IV | **DESIGN JUDGEMENT** — mine, from designer-reported gaps |
| §V ordering | **DESIGN JUDGEMENT** — not independently reviewed |
| Adversarial critique | **ABSENT** — the three-lens panel died on an account limit (resets 2026-08-10). §IV is self-critique, which is weaker. **Run the panel before treating §V as settled.** |

## IX. The first move

Phase 0, which depends on nothing:

1. `escriba-command`: `run_action`'s `_ => Ok(())` → `Err(CommandError::Unhandled(sym))`.
2. `escriba-runtime`: `let _ = self.commands.run(…)` → match, and push
   `NotFound`/`Unhandled` into `state.messages` so it reaches the status line.
3. `escriba-buffer`: add a `path → BufferId` index; `open` becomes
   `open_or_focus`.
4. Test: `escriba/tests/action_dispatch.rs` — dispatching an inert action
   yields an observable message. **Record the red run** against a
   deliberately unregistered name before calling it done.

Then Phase 1 M0: `cargo new --lib escriba-madoguchi` — `Negai`, `Outcome`,
`Snapshot`, `FakeSnapshot`, no capabilities yet. First test: a handler
returning `Negai::Message` completes the round trip with **zero
`&mut EditorState` in scope**.
