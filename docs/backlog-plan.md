# The escriba backlog — an implementation plan

**Status:** REVISED 2026-08-07 after a four-dimension recon, each sweep
adversarially verified and then re-checked by hand. **§V Phases 0, 1 and 2
have SHIPPED.** Three of this document's load-bearing premises were not
merely stale but **false** — they named code that does not exist. §0 below
leads with those, because a plan's premises failing is worse than its
ambition failing, and it happened here three times.

**Framing (operator instruction, 2026-08-07):** *"it doesn't matter how long
it is, we do it piece by piece and only care about delivering quality that
compounds — abort all care of timelines."*

This document therefore carries **no schedule, no effort tier, and no cut
line.** The ordering is by **what makes the next piece safer or cheaper**,
never by what is quickest.

---

## 0. What this plan got wrong

### 0.0 Tier A was 8 actions on paper and 5 in fact — all five landed (2026-08-08)

`trouble.{toggle,document,workspace}` and `files.{open,open-parent}` are
wired and out of `action_resolution.rs`'s INERT set (64 → 59), with
`alias_revival.rs` updated for the two that became dispatchable aliases.
Both are picker sources over machinery that already shipped:
`PickerSource::Findings { workspace }` reads `shirube`'s `ListRegistry`
through `ResultList::fresh(&world)` — so a stale list contributes nothing
rather than offering a row whose line has moved — and
`PickerSource::FilesUnder(root)` reuses the same bounded walk, now
`walk_from(root, limit)` with two callers instead of a second copy.

**The other three were mis-sized as Tier A, and this document already knew
it** (§VII): `illuminate.{next,prev}` is subsumed by LSP, and `snacks.zen`
wants a `kasane` surface plus a `shikiri` dock — neither of which is built.
The summary that called Tier A "~8 actions needing nothing new" was
counting them; §VII was right and the summary was wrong. **Tier A is
finished at five.**

Three things the tooling caught rather than the author: adding
`Source::Findings` broke `Source::title`'s exhaustive match (no wildcard,
so a new source cannot render as the wrong label); both ratchets fired in
the correct direction and named their own fix; and clippy's line-count
lint on `open_picker` was pointing at real duplication — the `Files` and
`FilesUnder` arms were one body with a different root — which is why the
fix was extraction rather than a threshold bump.

### 0.1 Phase 3's blocker never existed

The plan said to fix `Surface::on_key`'s stringly-typed `KeyCombo` **before**
escriba consumes egaku. **There is no `Surface` trait anywhere in egaku.**
`FuzzyPicker::on_event` takes a typed `PickerEvent` (`egaku/src/picker.rs`),
and `KeyCombo` appeared only in `egaku/src/keymap.rs` plus a re-export. It was
never on the picker's path. Phase 3 carried a prerequisite that blocked
nothing.

The stringly-typed defect was real — just elsewhere. `KeyCombo` is a `HashMap`
key that stored raw strings, so `"Ctrl"`/`"ctrl"`, `["ctrl","ctrl"]`/`["ctrl"]`
and `"Escape"`/`"esc"` were each two unmatchable values, every miss silent.
Fixed upstream (egaku `c502920`, `7785f9b`): both constructors canonicalise
through `awase`, no API change.

### 0.2 "Two of the five primitives are upstream extensions of egaku" — false

- `egaku::Modal` is a struct of `visible: bool, title: String`. No content, no
  keys, no z-order, no stack, no occlusion. Nothing to extend for `kasane`.
- `egaku::SplitPane` is exactly two panes — a ratio and an orientation. No
  tree, no nesting, no N-way. Nothing to extend for `shikiri`.

**Both are net-new wherever they land.** The *(upstream: egaku)* tags are
removed. Upstreaming the result is still right; the false discount that made
those phases look cheap is gone.

### 0.3 "egaku has no consumers" — false, and the correction inverts the risk

egaku is consumed by `madori`, `moldura`, `banken`, `banken-spec` and
`egaku-term`. What is true is narrower: **`FuzzyPicker` specifically has zero
consumers.** Adoption risk is per-widget, not crate-wide.

And the reading inverts once you see where the widgets came from: egaku's
picker is described in its own source as the generic version of mado's
Ctrl-S session picker, and its scroll integrator was lifted from mado too.
**These are proven designs whose migration never happened** — not speculative
code nobody wanted.

### 0.4 The plan describes work that has already shipped

Phase 0's three defects are fixed, `madoguchi` is wired and dispatching,
`shirube` is wired with all three faces painting through it, axis-set
freshness landed, and the `Bound` stepper moved into `memori`. The inert
count is **78**, not 85; egaku has **247** tests, not 239.

Three crates were still describing themselves as unbuilt — corrected in
`e29f0ee`. `escriba-keymap`'s **published crates.io description** claimed it
was "built on awase key parsing"; it has no awase dependency.

### 0.5 Phase 4 mis-targeted the thing it wanted to delete

`Window.rect` was dead state: six writes, **zero reads**, and its writers
disagreed on units (pixels in the GPU path, cells elsewhere) with no reader to
notice. It needed none of `shikiri`. **Deleted in `e29f0ee`.**

The geometry the faces actually read is `Window.viewport`, whose `top_line`
and `left_column` are **scroll position** — retained state that *cannot* be a
pure function of `(tree, frame)`. A `solve()` that claims to own it will break
cursor visibility on resize. Any `shikiri` design must split `Viewport` into
derived-vs-retained first.

### 0.6 The splash is the wrong first surface

Phase 3 wanted to retrofit `escriba_ui::splash` onto `kasane` and delete
`screen_chunks` because "one consumer today is precisely when that change is
cheap". There are **three production faces** between `rows()` and
`screen_chunks()`. And `screen_chunks` is *derived from* `rows()` — deleting
it pushes the same flatten into two faces separately, which is the exact
three-face drift this repo has been burned by twice. **Strike the deletion.**

The splash also *replaces* the buffer pane rather than floating over it, so it
exercises none of occlusion, z-order, or two-surfaces-claiming-one-key. A
picker, which really does float over live text, is the honest first surface.

### 0.7 What the recon could NOT verify

**One of the four sweeps died** (the dimension auditing Phases 4–6 and §VIII
systematically). Its ground was covered incidentally by the others, but **the
remaining claims in Phases 5, 6 and §VIII have NOT had the treatment §0.1–0.6
got.** Treat them as unaudited. Given that three of three audited premises
were false, assume more are.

---

## 0.8 The load-bearing architectural answer

**`madoguchi` and egaku widgets are not directly compatible, and that is
fine.** `madoguchi` is a *command* seam: a handler reads a read-only
`Snapshot` and returns `Outcome { slips }`. An egaku widget is a *key* seam
needing `&mut` widget state across many presses. **A handler cannot drive a
picker.**

But this does not mean a second dispatch system. Both crates already carry the
same pure/engine split. The picker lives on `EditorState`, is driven from
`on_key`, and its `PickerEffect::Accepted` is lowered into a `Negai` handed to
`interpret()` — so the *effect* still goes through the one seam.

**escriba already has this exact precedent:** the start screen consumes a key
ahead of the keymap through a total three-arm enum. A picker is the same shape
with a wider outcome — it holds keys for many presses rather than one. Copy
that; do not design `kasane` to get it.

The real gap is vocabulary, and it is small: `Negai` has no variant for a
floating surface, and `Snapshot` exposes no view of whether one is open. Since
`Negai` is deliberately **not** `#[non_exhaustive]`, adding a variant fails
`honour_one` to compile until every case is decided — the seal working in our
favour.

---

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
- Everything **spatial** is one leaf in one container tree. Its PANE geometry
  is derived from the frame; its SCROLL position is not. `top_line` and
  `left_column` are retained state that no `solve(tree, frame)` can compute —
  see §0.5. This bullet said "a pure function of the frame, never stored"
  until 2026-08-08, which §0.5 refuted without amending the destination it
  contradicted.
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

## V. The build order (REVISED)

Phases 0, 1 and 2 have shipped. What follows replaces the old §V from Phase 3
onward, and the phases are **no longer a chain** — the dependencies that
sequenced them were largely imaginary (§0.1, §0.5).

### Next — the picker, without any new primitive

A working fuzzy picker is reachable **now**, needing none of `kasane`,
`shikiri` or `denrei`. The shape:

1. `escriba-picker` adapts `egaku::FuzzyPicker<T>` — the whole
   `PickerEvent`/`PickerEffect` machine already exists and is tested.
2. The picker sits on `EditorState` as `Option<…>`, exactly as `splash` does.
3. `on_key` routes to it first when open, using the splash's precedent —
   widened by one variant meaning *"still open, key consumed"*.
4. `PickerEffect::Accepted` lowers to a new `Negai`, so the effect still goes
   through the one interpreter.

**Sources are two tiers, and the plan conflated them.** `buffers`, `commands`
and `help` are derivable from a `Snapshot` and land now. `files`, `grep`,
`project` and `symbols` need I/O, which is the interpreter's job — those wait
on the courier or an interpreter-side producer. The *adapter* is shared; the
*sources* are not one tier.

**Start with `picker.buffers`** specifically: it needs no I/O at all, so it
proves the whole path with nothing mocked. Its done-predicate is free — the
inert ratchet in `action_resolution.rs` goes red until its entry is removed.

### Then — `kasane`, earned rather than assumed

Only once a second surface exists does a surface *abstraction* have anything
to abstract. Build it when the picker and one other overlay both want
z-order — not before, and not out of `egaku::Modal`, which is two fields.

### `shikiri` — blocked on a design question, not on effort

Before any of it: split `Viewport` into derived (`visible_lines`,
`visible_columns`) and retained (`top_line`, `left_column`). A `solve()` that
tries to own scroll position is wrong, and its own done-predicate
(`scroll_to_contain` surviving resize) is what proves it.

### `denrei` — unchanged, and still last

Async arrives last and outside the core. Unaudited (§0.7).

## VI. Waves 1.5–4, placed

| Roadmap item | Where it lands |
|---|---|
| Multi-key polish, `defsource` | **SHIPPED** — `madoguchi` is wired |
| Multi-selection (`Cursors`) | `Cursors` exists and is routed; the operators do not map over the set yet |
| Registers / clipboard | shipped (single unnamed register); `hasami` has a `MockClipboard` seam |
| Marks / jumplist | **SHIPPED** — the jumplist carries `Spot`; a mark IS a `Finding` |
| Picker | **SHIPPED** — six sources; `symbols` blocked on hikari classification |
| which-key | needs the surface (shipped) plus keymap prefix enumeration |
| git, tree, term, DAP | need the courier; git hunks also need `Axis::Index` (now emitted) |
| Undo tree, macros | a macro is NOT `Vec<Negai>` — `lower()` maps 6 of 30 `Action`s, and the comment says the other 23 are deliberately not slips |
| Sessions | `defsession` parses today and is inert |
| Notebook cells, CRDT | Explicitly deferred — no primitive here serves them; revisit when one does |

**The "Phase 3/4/5/6a–6e" numbering this table used until 2026-08-08 no longer
exists.** Rewriting §V replaced those headings with unnumbered pieces and left
every pointer here dangling — an audit found them and they are now stated as
capabilities and blockers rather than as cross-references to deleted text.

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
| `madoguchi`, `shirube` | **SHIPPED AND WIRED** — workspace members, consumed by the runtime |
| `kasane` (surfaces) | **PARTIALLY SHIPPED** — the picker is a working overlay; no z-ordered STACK yet |
| `shikiri` (container tree), `denrei` (courier) | **DESIGNED** — no code written |
| `egaku::FuzzyPicker` is consumed by `escriba-ui::picker` | **SHIPPED** — six sources ride it |
| `egaku` has 247 tests (not 239) and five fleet consumers (not zero) | **CORRECTED** — the original row was wrong in both figures |
| The three Phase-0 defects | **VERIFIED** — source read, line numbers cited |
| escriba is 100% synchronous; tokio declared in 2 crates, used in 0 | **VERIFIED** — zero grep matches in any `escriba*/src` |
| The inert inventory is covered exactly once | **VERIFIED** — reconciled against `action_resolution.rs`, never restated as a number here; a second ratchet (`alias_revival.rs`) covers the 41 `defcmd` actions that test never watched |
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
