# doma (土間) — the terminal as a first-class citizen of escriba's pane tree

> **Status: DESIGN.** Nothing in this document is shipped. Every claim about
> what exists today is dated and was read from source on **2026-08-13**; every
> claim about what we will build is marked with its tier. Read §3 before
> trusting anything in §5–§8.

---

## 1. The destination, unhedged

**escriba and mado are two faces over one pane substrate.**

Not "escriba can open a terminal". Not "escriba shells out to mado". A pane in
escriba's container tree holds *either* a text buffer *or* a live terminal, the
two are the same kind of citizen, and every verb the editor already has —
`:sp`, `:vsp`, window motion, the theme seam, the keymap, the tatara-lisp
authoring surface, the picker, the status line — reaches both without knowing
which it is holding.

The terminal is not a guest. It is a **naturalized citizen**: rebuilt on fleet
substrate we own, typed the way the rest of escriba is typed, with its bad
states unrepresentable rather than mitigated.

And the substrate underneath is **not a copy**. It is the same `tear-core`
that `tear-daemon` drives headlessly and that mado is scheduled to rebase onto.
When both landings are done there is exactly **one** authoritative VT grid in
the fleet, with three faces over it: a daemon, a terminal, and an editor.

---

## 2. The reframe: you cannot embed mado, and you should not want to

The request was "fully embed mado in escriba". Grounding it against source
dissolves it, and the dissolution is the most valuable thing in this document.

**mado is a binary.** Its `[lib]` target exists (`mado/src/lib.rs`) and exposes
exactly two pure modules — `motion` (the animation algebra, for benches) and
`float` (the browser-surface geometry substrate). The file carries an explicit
instruction: *"Do not widen this surface ad hoc."* The terminal itself —
`terminal.rs`, `render.rs`, `render_graph.rs`, `pty.rs`, `session.rs` — is in
the **bin**. There is no `mado::Terminal` to link against, and widening the lib
to create one would be taking a rule out of the way rather than solving the
problem.

This is a textbook [MIRAGEM](https://github.com/pleme-io/theory/blob/main/MIRAGEM.md)
case. "We cannot embed mado" is phrased in terms of *our own abstraction* — the
word "mado" naming a binary we happen to have built — so it is a missing
primitive, not a wall. Phrase it as a fact about the world instead:

> A terminal is a **PTY**, a **VT parser producing a cell grid**, and a
> **registry of panes**.

We ship all three, as libraries, published at `0.1`:

| Piece | Where | State (read 2026-08-13) |
|---|---|---|
| PTY ownership | `tear-core::pty` (`portable-pty`) | shipped |
| VT parse → cell grid | `tear-core::pane_grid` — `impl Perform for GridState`, `PaneGrid`, `Cell`, `PaneSnapshot` | shipped |
| Pane/window/session registry + layout | `tear-core::inproc::InProcess`, `tear-core::registry` | shipped |
| The control vocabulary | `tear_types::MultiplexerControl` (~25 methods) | shipped |

`tear-core`'s own module doc states the destination we are joining:

> *"mado currently owns its own `pane.rs`/`tab.rs` with private state machines.
> At M5 those modules rebase onto `tear-core::InProcess` — both apps then share
> one source of truth for pane semantics. This crate's `InProcess` impl is the
> gravitational center the eventual rebase lands on."*

So the fleet already named this destination — for mado. **escriba embedding a
terminal is the same move, and it must land on the same gravitational center.**
Doing anything else creates a third authoritative grid.

**Corrected claim:** "embed mado in escriba" is not achievable and not
desirable. "escriba and mado become two faces over `tear-core`" is achievable,
is already half-designed by someone else, and is strictly better — it makes
escriba's terminal and mado's terminal *the same terminal*, forever.

---

## 3. Ground truth (read from source, 2026-08-13)

Everything below was verified by reading the file named. Nothing is inferred.

### 3.1 What is genuinely shipped and reusable

| Fact | Evidence | Why it matters here |
|---|---|---|
| `tear-core` exports `PaneGrid`, `Cell`, `PaneSnapshot`, `InProcess` | `tear-core/src/lib.rs` re-exports | the terminal substrate is a library, today |
| `InProcess` is `#![forbid(unsafe_code)]`, `Arc<RwLock<Registry>>` + `BTreeMap<PaneId, PtyHandle>` | `tear-core/src/inproc.rs` | safe to hold inside an editor's state |
| `MultiplexerControl` has `send_keys`, `pane_snapshot`, `split_pane`, `resize_pane`, `kill_pane`, `apply_layout`, … | `tear-types/src/control.rs` | the whole verb set escriba needs already exists |
| **`garasu::TextLayerStack` supports N independent text layers** and its doc names *"terminal grid, overlays"* as the motivating case | `madori/src/render.rs` `RenderContext` | **sharing one GPU device between the editor surface and a terminal grid is the DESIGNED shape, not a hope** |
| `madori::RenderContext` hands out one `GpuContext` + the layer stack + surface view | same | one window, N surfaces, already works |
| `escriba_ui::shikiri` is real and wired — 516 lines, `solve(tree, frame)`, used by `Layout` and by the runtime for `:sp`/`:vsp` | `escriba-ui/src/shikiri.rs`, `escriba-runtime/src/lib.rs:870` | the container tree exists; only its LEAF TYPE is wrong |
| escriba's picker is real and wraps `egaku::FuzzyPicker` | `escriba-ui/src/picker.rs`, 296 lines | pane/session pickers come free |
| Both apps already share `madori`, `garasu`, `ishou-tokens`, `awase`, `shikumi`, `egaku` | both `Cargo.toml`s | the theme, key and config seams are one seam already |

### 3.2 Two corrections to escriba's own CLAUDE.md

The repo's model is stale in two places, and both change this plan materially.

**(a) The courier has LANDED.** `escriba/CLAUDE.md` says the LSP live pump is
*"blocked on the Phase 5 courier"*, that `escriba-runtime` has *"no `tokio`
dependency"*, and that `Negai::Errand(_)` is *"announced-but-unimplemented"*.

Read today: `escriba-runtime/src/courier.rs` is 250 lines of shipped
`denrei` (伝令) — one `std::sync::mpsc` channel, a `Crew`, an errand counter,
per-class cancel flags. `Negai::Errand(freight)` dispatches through it
(`lib.rs:1039`); `Negai::ErrandReply` applies replies gated on
`shirube::Anchor` freshness. The *no-tokio* half is still true and is a
deliberate choice, not a gap — the courier is threads and channels.

So **the async delivery path exists**, which is the single biggest input to
this plan. It also means the LSP pump is unblocked; that `pending-lsp:` line
should be re-checked.

**(b) `shikiri` and the picker are shipped, not backlog.** `escriba/CLAUDE.md`
lists `shikiri` 仕切り (container tree) and `escriba-picker` among *"5 missing
primitives"*. Both exist. The backlog doc needs re-reading against reality
before it is used to plan anything.

### 3.3 The typed hole — stated precisely

```rust
// escriba-ui/src/shikiri.rs
pub enum Shikiri {
    Pane(Window),
    Split(Split),
}

pub struct Window {
    pub id: WindowId,
    pub buffer_id: BufferId,   // ← the hole
    pub viewport: Viewport,
}
```

A leaf **is** a buffer. A terminal pane is not merely unimplemented — it is
**unrepresentable**. Every other gap in this document is downstream of this
one field.

### 3.4 The two facts that will cost us

**(a) `PaneGrid`'s mutators are `pub(crate)`, deliberately.** `PaneGrid::new`,
`::feed`, `::with_scrollback`, `::set_host_role` are all crate-private, sealed
with an authority note: *"no consumer outside this crate can mint a second
authoritative grid"*. An external consumer therefore drives panes through
`MultiplexerControl` — `send_keys` in, `pane_snapshot` out — and never owns a
grid.

This is the **right** constraint and we should not ask for it to be relaxed.
It is also a real cost: escriba's render path will read a `PaneSnapshot` per
frame rather than borrowing a grid. Whether that is affordable at 60fps for a
full-screen pane is **unmeasured** and is M0's first benchmark.

**(b) mado has NOT rebased.** `mado/src/terminal.rs:6113` still carries its own
`impl vte::Perform for Terminal`. tear-core's "M5" is pending. So *today* the
fleet has two authoritative grids, and escriba joining tear-core means escriba
reaches the shared substrate **before mado does**. That is fine — it is in fact
the argument for doing it — but it must not be described as "escriba uses
mado's terminal". It does not. It uses the terminal mado is going to use.

---

## 4. Naming

The family is the Japanese house, which escriba and mado are already deep
inside: **mado** 窓 (window), **madori** 間取り (floor plan), **shikiri** 仕切り
(partition), **engawa** 縁側 (veranda). Two new names, both from the same
family, both with a gloss that lets a reader guess the job:

- **`Nakami` (中身) — "the contents".** The typed enum of what a `shikiri` leaf
  may hold. `Nakami::Buffer(..)` | `Nakami::Doma(..)`. Lives in
  `escriba-ui/src/shikiri.rs` beside the tree it types.
- **`doma` (土間) — the earthen-floor work area of a traditional house.** The
  room in the house where the work and the cooking happen, connected to the
  living space but not the same as it. The terminal citizen: crate
  `escriba-doma`, form `(defdoma …)`.

`tokonoma` 床の間 (the display alcove) stays free for the floating-surface
primitive the backlog calls `kasane`.

This supersedes the roadmap's `escriba-term`, which was English for a concept
that is neither an irreplaceable proper noun nor without a Japanese gloss.

---

## 5. The typescape

### 5.1 `Nakami` — closing the hole

```rust
/// What a shikiri leaf holds.
///
/// Total, and that totality is the point: every renderer, every motion, every
/// `:q`, must decide what a terminal pane means rather than defaulting to the
/// buffer arm. A `Window { buffer_id }` made a terminal unrepresentable; a
/// `Window { nakami }` makes "forgot to handle terminals" a compile error.
pub enum Nakami {
    Buffer(BufferId),
    Doma(DomaId),
}
```

Cost, stated honestly: this is a **breaking change to a shipped, wired type**,
and every consumer of `Window::buffer_id` fails to compile until it decides.
That is the fix working, and it is the largest single mechanical cost in this
plan.

### 5.2 `escriba-doma` — the TYPED-SPEC triplet

Per ★★ TYPED-SPEC + INTERPRETER TRIPLET: a typed Rust border, a `(def…)` Lisp
spec, and an `apply` interpreter behind a mockable `Environment`.

```rust
/// The seam. `InProcess` on the real path; a scripted grid in tests.
///
/// Non-negotiable: escriba's terminal must be testable with zero PTYs, or the
/// pane tests become flaky shell-dependent integration tests — the exact trap
/// the operator-over-motion suite avoided by driving KEYS.
pub trait DomaEnvironment {
    fn spawn(&self, spec: &DomaSpec) -> Result<PaneId, DomaError>;
    fn send(&self, pane: PaneId, bytes: &[u8]) -> Result<(), DomaError>;
    fn snapshot(&self, pane: PaneId) -> Result<PaneSnapshot, DomaError>;
    fn resize(&self, pane: PaneId, cols: u16, rows: u16) -> Result<(), DomaError>;
    fn kill(&self, pane: PaneId) -> Result<(), DomaError>;
}
```

`InProcessEnv` is a thin adapter over `Arc<InProcess>`; every method above
already exists on `MultiplexerControl`. The mock is a `PaneGrid`-shaped script
with no process.

### 5.3 `(defdoma …)` — the authoring surface

```lisp
(defdoma :name    "shell"
         :shell   "frost"
         :cwd     "$PWD"
         :scroll  10000
         :on-exit "close")          ; close | keep | reopen

(defkeybind :mode "normal" :key "<leader>t"  :action "doma.open-below")
(defkeybind :mode "normal" :key "<leader>tv" :action "doma.open-right")
(defkeybind :mode "doma"   :key "<C-\\><C-n>" :action "doma.to-normal")
```

Mechanical given `#[derive(DeriveTataraDomain)]` and the existing apply-path
pattern. **Trap already paid for once:** a key absent from a tatara-lisp form
does *not* reach serde's `#[serde(default)]` — it takes the field type's zero
value. `(defsplash)` shipped with `enable = false` for exactly this reason.
`:on-exit` must therefore make the *safe* behaviour the zero value.

### 5.4 Mode

`Mode::Doma` — a real mode, unlike the splash screen, because a terminal is
precisely "a state keys are interpreted *in*". `Esc` must reach the shell, so
the escape hatch is vim's `<C-\><C-n>`. `Mode` is matched exhaustively in
several places (`chrome.rs`, `highlight_effect`, `StatusModel::mode_label`),
so adding an arm forces each to decide — which is the mechanism working.

---

## 6. The one genuinely hard design call: streaming vs one-shot

This is the part of the plan I am least certain about, and it is worth the
whole rest of the document's care.

The courier as built (`escriba-madoguchi/src/errand.rs`):

```rust
pub enum Freight { Scan {..}, Diagnostics {..}, Format {..} }   // closed
pub struct Crew { scan: Box<dyn Runner>, diagnostics: .., format: .. }  // fixed struct
```

Three named runners, three request shapes, **one reply per errand**, and cancel
flags kept *"newest per class"*.

A PTY pump is a different animal: **long-lived**, **unsolicited** (it posts
when the child writes, not when we ask), and **N-at-once** (one per open pane).
Two of those three collide with the current design:

- *Long-lived + unsolicited* is a **mirage**, not a wall. The `Courier` is an
  mpsc `Sender<Parcel>` drained per tick; nothing forbids a runner posting N
  parcels. This costs a `Runner` shape that may post repeatedly, and it should
  be checked against the trait rather than assumed.
- *N-at-once* is a **real structural fact about the current design**.
  "Newest per class" cancellation means opening pane 2 would cancel pane 1's
  pump. Cancellation must become per-**errand**, not per-class, for at least
  the streaming classes.

That second point is the M0 blocker and the reason the terminal is the
courier's most valuable second consumer: **LSP alone can be faked with polling
and one-shot requests; a PTY cannot.** The terminal is what makes the courier's
design honest.

**Explicitly not decided here.** Whether the answer is a 4th `Freight` variant,
a separate `Stream` freight class, or a distinct channel is a real fork with a
hard-to-reverse choice at the end of it. It deserves `/twin-reasoning` before
M1, not a paragraph in a plan.

---

## 7. Phases

Each phase is a vertical slice that is *provable against a real face*, per
Care #5. No phase is scheduled — ordered by what makes the next one safer.

### M0 — the measurement, before any of it

Three cheap empirical proofs, because the rest of the plan is worthless if any
fails:

1. **Snapshot cost.** Drive an `InProcess` pane through `MultiplexerControl`
   and time `pane_snapshot()` at 200×50 with heavy output. If a per-frame
   snapshot is not affordable, the whole read path changes shape and we should
   know before writing `Nakami`.
2. **Two layers, one device.** Render escriba's text surface and a second
   independent `TextLayerStack` layer in one `madori` frame. The doc says this
   is the designed shape; a red-run proves it.
3. **Streaming through the courier.** One runner that posts three parcels,
   and two concurrent errands of the same class both surviving. This is the §6
   fork, measured rather than argued.

**Deliverable:** a benchmark and three green/red results. **No production code.**

### M1 — `Nakami`, with no terminal in it

Land the enum with exactly one arm populated (`Nakami::Buffer`), fix every
compile error, ship it. A pure refactor with no behaviour change, gated by the
existing suites. This is the largest mechanical change and it deserves to land
*alone*, where a bisect can find it.

### M2 — one terminal, one pane, read-only

`escriba-doma` with the `DomaEnvironment` trait, the mock, and `InProcessEnv`.
`:doma` opens a pane running `frost`; keys reach it; the grid paints through
a second text layer. `--render=text` dumps its cells so it is testable
headlessly. Verified by driving a real pane and *looking at the screen* — the
discipline that found every key defect in this repo.

### M3 — the citizen

`:sp`/`:vsp` mixing buffer and terminal panes; window motions crossing them;
`Mode::Doma` with `<C-\><C-n>`; the status line reporting a doma pane; the
theme seam reaching the grid (`tear_types::theme` ↔ `ChromePalette`, one
resolution through `ishou_tokens::SemanticRoles`, no second palette).

### M4 — authoring + the picker

`(defdoma …)`, `doma.*` actions in `resolve_action`, `$ESCRIBA*` wiring,
`--list-rc` reporting. A pane picker over `egaku::FuzzyPicker`, which is the
same adapter the buffer picker already is.

### M5 — the convergence

Escalate to `tear-core`: propose escriba as its **first GPU consumer**, feed
back whatever the sealed `pub(crate)` surface makes awkward, and support mado's
own rebase. **This is the phase the whole plan exists for** — it is where the
fleet goes from two authoritative grids to one.

### The reverse direction — escriba as mado's editor

Genuinely **aspirational**, and I am not going to dress it up. It is more
tractable than it sounds (escriba is already 19 library crates; mado already
has `shell_seam.rs`), and it is unblocked by the same `Nakami` work — once a
pane's contents are typed, "an editor pane inside a terminal" is the mirror of
"a terminal pane inside an editor". But nothing has been measured, mado's own
pane model has not rebased, and proposing it now would be the round-up this
document exists to avoid. Revisit after M5.

---

## 8. Tier ledger

| Item | Tier | Note |
|---|---|---|
| `tear-core` PTY + `PaneGrid` + `InProcess` | **shipped** | but its production consumer is `tear-daemon` (headless). escriba would be the **first GPU consumer** — unproven at frame rates |
| `MultiplexerControl` verb set | **shipped** | ~25 methods, covers everything escriba needs |
| `garasu::TextLayerStack` multi-layer | **shipped** | doc explicitly names "terminal grid" as the case |
| `escriba_ui::shikiri` container tree | **shipped** | wired; leaf is buffer-typed |
| `escriba-runtime` courier (`denrei`) | **shipped** | **corrects escriba/CLAUDE.md**, which calls it unimplemented |
| Courier *streaming* + per-errand cancel | **design** | §6; the M0 fork, needs `/twin-reasoning` |
| Per-frame `PaneSnapshot` affordability | **unmeasured** | M0.1. If it fails, the read path changes shape |
| `Nakami` | **design** | mechanical but breaking |
| `escriba-doma` + `DomaEnvironment` | **design** | zero code |
| `(defdoma …)` | **design** | mechanical given the derive |
| Theme seam reaching the grid | **extend-existing** | both ends resolve through `ishou_tokens` already |
| mado rebased onto `tear-core` | **not started** | tear-core's own "M5"; `terminal.rs:6113` still owns a `vte::Perform` |
| escriba-as-mado's-editor | **aspirational** | nothing measured |

`pending-doma: M0-measurement` · `pending-denrei: streaming-freight`

---

## 9. What I did not verify

Stated so no one builds on it:

- I did not run any of M0's three measurements. The snapshot-cost and
  two-layer claims are **read from doc comments and type signatures**, not from
  a running frame.
- I did not read `Runner`'s full trait shape, so "a runner may post N parcels"
  is inference from `Courier`'s channel, not a proof.
- I did not check whether `tear-core`'s `InProcess` can be driven without a
  daemon socket in-process end to end; `inproc.rs`'s doc says yes and the type
  is `Arc`-shaped for it, but I did not construct one.
- I did not audit how many call sites read `Window::buffer_id`, so "the
  largest mechanical change" is a judgement, not a count.
- The escriba backlog doc (`docs/backlog-plan.md`) is stale in at least two
  places (§3.2). I did not re-audit the rest of it.
