# memori (目盛) — the positioning vocabulary

**Status: M1 — typed border, laws proven, and the search engine's endpoint
rule folded onto it.** One architectural finding remains open (§4.2); read it
before citing freshness as solved.

`memori` is the graduation marks on a ruler. An offset is a count of marks, and
the marks differ — which is the entire problem.

---

## 1. Why this exists — the recurrence, not a theory

Five distinct bugs fixed in one session (2026-08-04) were one class:

| # | Bug | Where |
|---|---|---|
| 1 | commit stepped from the live cursor, which preview had already moved → landed on the match AFTER the previewed one | `submit_search` |
| 2 | `saturating_sub(1)` cannot back past 0, so a match at offset 0 was unreachable | `submit_search` |
| 3 | match offsets used against text that had since been edited (`refresh` had ZERO callers) | the mutation funnel |
| 4 | byte→char conversion correct only because one function guarded it | `engine.rs:107-133` |
| 5 | exclusive-vs-inclusive endpoint chosen by picking a function NAME | `step` / `step_inclusive` |

The fleet rule is that the third hand-wiring of a class is a primitive, not a
task. This is the fifth.

**What they share:** a position is a `usize`, and a `usize` cannot say which
ruler it was counted on, whether its endpoint counts itself, or which text it
was true of. All three facts live in the programmer's head, and all three were
wrong at least once.

---

## 2. The three orthogonal axes

### Scale — `Offset<S>`

Three rulers, disagreeing on every non-ASCII character:

| scale | `"héllo"` end | `"🔥"` end | who demands it |
|---|---|---|---|
| `Bytes` | 6 | 4 | `regex`, every `&str` index |
| `Chars` | 5 | 1 | escriba's `Position`, match offsets |
| `Utf16Units` | 5 | **2** | LSP, and nothing else |

`Offset<S>` is phantom-tagged, so the substitution is a **type error**.
Conversion is possible only through a `Ruler`, which cannot be built without
the text — the structural statement that there is no such thing as converting
scales "in general". `"héllo".len()` is 6 or 5 and only the string knows.

**This is the failure profile that makes it worth a type:** mixing scales
compiles, and is wrong *only for users with non-ASCII text*. It survives every
test written in English.

### Bound — `Bound::{Inclusive, Exclusive}`

Was a choice between two function names, plus one call site trying to convert
between them with arithmetic. `Bound::first_matching` never subtracts, so the
"back up one to include the anchor" trick — which saturates at 0 and therefore
hid any match at the start of the file — has nowhere to live.

### Freshness — `Anchored<T>`

An offset is a claim about text; when the text changes the claim expires, and
the expiry is invisible because the number still indexes *something*. `Anchored`
carries the generation, and `get(now)` returns `None` on a mismatch.

Note `law_freshness_is_not_ordering`: a value from a *later* generation is just
as unusable as an older one. Treating "newer, so fine" as acceptable is how a
stale read sneaks back in.

---

## 3. The ledger

<!-- tier-ledger -->

| bad state | how the vocabulary corners it | tier |
|---|---|---|
| a byte offset used where chars/UTF-16 are expected | `Offset<Bytes>` and `Offset<Chars>` are distinct types; no coercion exists | truly-unrep |
| inventing a fourth scale that conversion does not handle | `Scale`'s only members are constants and the three impls are the whole set | truly-unrep |
| a scale conversion done without the text | `Ruler` has no constructor that omits `&str` | truly-unrep |
| a mid-codepoint or past-the-end byte offset | `Ruler::snap` is total and idempotent; every conversion snaps first | parse-time-rejected |
| an inclusive search spelled as `step(from - 1)`, hiding a match at 0 | `Bound` is a value and `first_matching` contains no subtraction | parse-time-rejected |
| reading an offset computed against older text | `Anchored::get(now)` returns `None` on a generation mismatch | parse-time-rejected |
| a `Ruler` built for the WRONG text | nothing prevents it — the text is a plain `&str` argument | only-mitigated (C1: no text identity exists to bind a Ruler to) |
| `escriba-search`'s `step`/`step_inclusive` drifting apart | CLOSED — both are one-line delegations to `step_bounded`, which asks `Bound`; there is one implementation to drift | parse-time-rejected |

**Proof of the top row**, captured 2026-08-04 by compiling a deliberate
violation (`r.to_bytes(b)` where `b: Offset<Bytes>`):

```
error[E0308]: mismatched types
   --> escriba-core/src/memori.rs:514:28
    |
514 |         let _ = r.to_bytes(b);
    |                   -------- ^ expected `Offset<Chars>`, found `Offset<Bytes>`
    = note: expected struct `memori::Offset<memori::Chars>`
               found struct `memori::Offset<memori::Bytes>`
```

A gate that has never been red is a declaration. This one has been.

**13 laws**, each run over a corpus that includes `""`, `"héllo"`, `"日本語 foo"`
and `"🔥🔥🔥"` — so a law that holds only for ASCII cannot pass:

- `law_byte_char_roundtrip_is_identity_on_boundaries`
- `law_conversion_is_monotonic` (a later byte never maps to an earlier char —
  violating it is how highlights cross over each other)
- `law_snap_is_total_and_idempotent_over_every_byte`
- `law_snap_never_moves_forward`
- `law_an_inclusive_forward_search_finds_a_match_at_zero` ← bug #2, sealed
- `law_the_two_bounds_differ_only_at_the_anchor`
- `law_no_bound_ever_underflows`
- `law_a_value_from_an_older_generation_reads_as_absent`
- `law_freshness_is_not_ordering`
- + 4 more

---

## 4. What is NOT wired, and why — two architectural findings

Both are real constraints found while wiring, not remaining effort. Stating
them is the point; a vocabulary that claims coverage it does not have is worse
than one that does not exist.

### 4.1 RESOLVED — memori is a leaf crate

`escriba-core/src/action.rs:1` is `use escriba_search::Direction`, so **core
depends on search**, and a primitive living in core was invisible to the search
engine whose twins `Bound` exists to replace.

memori is therefore its own crate, `escriba-memori`, with a **deliberately
empty `[dependencies]`** — it sits below core AND search, which is the only
position from which a positioning vocabulary can serve everything that
manipulates a position. `escriba-core` re-exports it so position code there
reads naturally without naming a second crate.

`Anchored<T, G>` is generic over the generation type as a consequence: memori
cannot name `EditGen` (that lives in core, above it), and does not need to —
it requires only that two generations can disagree.

`engine.rs::step_bounded` is now the single implementation; `step` and
`step_inclusive` are one-line delegations that pass `Bound::Exclusive` /
`Bound::Inclusive`. They stay because they are published API with their own
tests and read better at call sites with no other reason to name a bound
(★★ MODULARIZE, DON'T DELETE).

**Proven 2026-08-04:** swapping the bound in `step`'s delegation turns four
existing engine tests red — `forward_advances_past_a_match_the_cursor_sits_on`,
`a_lone_match_resolves_to_itself_by_wrapping`,
`backward_wraps_at_the_top_and_says_so`,
`step_inclusive_backward_also_accepts_the_cursor_position` — so the fold is
load-bearing rather than cosmetic.

### 4.2 `EditGen` is a REFRESH generation, not a TEXT generation

`Anchored` wants to key on "has the text changed". escriba's only generation
counter, `EditGen`, is bumped by `apply_resolved` on **every action** —
including pure cursor moves — because its job is telling the renderer when to
repaint.

Keying `Anchored` on it would mark every offset stale after any keypress,
which is not staleness but noise. So `EditorState::search_at` still uses manual
invalidation (re-derived from the cursor after a text-mutating action) rather
than `Anchored`.

**The missing primitive is a text-revision counter on the buffer**, distinct
from the refresh generation. That is `escriba-buffer`'s to own, and once it
exists `Anchored<Offset<Chars>>` replaces the manual clear and the
`refresh`-had-no-callers class closes by construction rather than by a
classifier that a future action could still forget.

---

## 5. Named follow-ups

1. ~~Extract to `escriba-memori` → fold the twins.~~ **DONE.**
2. Text-revision counter in `escriba-buffer` → `Anchored` becomes usable for
   real, replacing manual invalidation. Closes the C1-adjacent staleness class.
3. Retrofit `escriba-search::engine`'s `byte_to_char` map and
   `escriba-lsp-client`-style UTF-16 conversion onto `Ruler`, deleting the
   hand-rolled copies.
4. `(defmemori …)` tatara-lisp surface + `#[derive(DeriveTataraDomain)]`.
   **Not shipped** — M0 is the typed Rust border only.
