# memori (目盛) — the positioning vocabulary

**Status: M2 — three axes typed and proven; TWO of the three wired.**

Corrected 2026-08-04 after an adversarial review measured the claim. The
previous wording said "wired to real consumers" of all three axes, and a
reviewer disproved it by deleting `Bytes, Chars, Offset, Ruler, Scale,
Utf16Units` from `escriba-core`'s re-export and watching the workspace still
compile clean.

**Corrected again the same day:** that correction ITSELF cited
`escriba-lsp-client`-style UTF-16 conversion "in `analysis.rs`". **There is no
`analysis.rs` in this repo, and no UTF-16 conversion anywhere in the
workspace** — `escriba-lsp-client` is 182 lines of `ServerConfig` /
`ServerRegistry` / `detect_root` with no `Position` type and no offsets at all.
The citation was carried in from another repo's LSP crate. A file path in a
doc is exactly the kind of claim that gets checked, which is why it is worth
correcting twice rather than leaving a plausible-looking pointer to nothing.

So, stated at the tier each half actually earns:

- **`Bound`** — load-bearing. `engine.rs::step_bounded` is the single
  implementation `step`/`step_inclusive` delegate to.
- **`Anchored`** — load-bearing. `EditorState::search_at` is
  `Anchored<usize, TextRev>` and the manual invalidation was deleted.
- **`Chars`/`Bytes`/`Ruler`** — **two consumers, one of them the hot path.**
  `CaretLine::byte_of_caret` (chars→bytes, now serving BOTH the search prompt
  and the ex-line — see `CaretLine` below) and
  `engine::find_all` (every match offset, bytes→chars, via
  `Ruler::ascending`). Verified for each by reverting the call site and
  watching the imports die. Captured for the third, 2026-08-04:

  ```
  warning: unused imports: `Chars`, `Offset`, and `Ruler`
    --> escriba-mode/src/lib.rs:36:33
     |
  36 | use escriba_memori::{CaretMove, Chars, Offset, Ruler};
  ```
- **`CaretLine`** — load-bearing, and the sharpest instance of what this crate
  is FOR. A line of text plus the caret editing it, private fields, invariant
  `caret <= text.chars().count()` maintained by the type. It was written twice
  before it lived here: `escriba-search`'s `Prompt` held the two as sibling
  fields and paired them correctly at six mutation sites by convention, and
  `escriba-mode`'s ex-line held the identical pair and got it wrong at the
  seventh. Both crates now hold one, and `escriba-search` lost its private
  `byte_of_caret` in the process — which is why the Scale consumer count went
  back DOWN to two while the number of call sites served went up.
- **`CaretMove`** — load-bearing, and the reason it lives here. It started in
  `escriba-search` as the search prompt's private business; when the ex-line
  grew a caret, `escriba-mode` needed the same four moves. Two crates that
  cannot see each other both needing the same positioning verb is the
  definition of something that belongs below both.
- **`Utf16Units`** — **no consumer, and none is possible yet.** It unblocks
  only with LSP phase 2, which does not exist here.

**The third consumer arrived by way of the mistake it exists to prevent**, and
that is worth recording rather than tidying away. The ex-line's `byte_of_caret` was
first written as a local `text.char_indices().nth(caret)` — a *fourth*
hand-rolled copy of chars→bytes, authored inside the crate that had, in the
same commit, taken a dependency on the vocabulary built to hold it. Nothing
caught it; the tests passed, because a hand-rolled conversion that happens to
be correct is indistinguishable from the shared one until someone edits it.
The lesson is not "be more careful": it is that a vocabulary only stops
duplication at the sites that *reach for it*, and reaching is still a habit,
not an invariant.

Note the falsification that DOESN'T work, since the previous correction used
it: deleting the re-export from `escriba-core` proves nothing, because
`escriba-search` imports `escriba_memori` directly. The honest test is to
revert the call site.

The axis is not vestigial — its laws are proven and its compile error is
recorded below — but "proven" and "load-bearing" are different tiers and this
file said the stronger one.

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
| reading an ordinal computed against older text | `Anchored<usize, TextRev>` — `get(current_rev)` returns `None`; the manual invalidation was DELETED | parse-time-rejected |
| a `Ruler` built for the WRONG text | nothing prevents it — the text is a plain `&str` argument | only-mitigated (C1: no text identity exists to bind a Ruler to) |
| `escriba-search`'s `step`/`step_inclusive` drifting apart | CLOSED — both are one-line delegations to `step_bounded`, which asks `Bound`; there is one implementation to drift | parse-time-rejected |
| a caret desynchronized from the text it indexes (`clear()` emptying the line and stranding the caret past the end) | `CaretLine` holds both as PRIVATE fields; every mutation is a method that maintains the pair, so the fifteen former call sites across two crates cannot touch either half (`E0616`) | only-mitigated (C: unrepresentable for CONSUMERS, but still hand-maintained inside `CaretLine`'s own impl block — a type does not audit itself) |
| the SAME caret-line logic written twice, once per prompt | CLOSED — `CaretLine` is in memori, below both `escriba-search` and `escriba-mode`, which cannot see each other. There is one implementation to drift | parse-time-rejected |
| a DOCUMENT asserting a caret past the end of its line | `CaretLine::new` is the only door a caret enters through from outside, and it clamps; `escriba-mode`'s `ExLineWire` deserialization shadow routes through it. Note it CLAMPS rather than erroring — the bad value cannot survive the boundary, but it is normalized away rather than reported | parse-time-rejected |

**Proof of the top row**, captured 2026-08-04 by compiling a deliberate
violation (`r.to_bytes(b)` where `b: Offset<Bytes>`):

```
error[E0308]: mismatched types
   --> escriba-memori/src/lib.rs (then escriba-core/src/memori.rs, pre-extraction)
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

### 4.2 RESOLVED — `TextRev`, distinct from `EditGen`

`Anchored` needs to key on "has the TEXT changed". escriba's only counter,
`EditGen`, is bumped by `apply_resolved` on **every action** including pure
cursor moves, because its job is telling the renderer when to repaint. Keying
staleness on it would blank every cached offset after any keypress — noise, not
staleness.

`escriba_buffer::TextRev` is the missing primitive: a private field on
`Buffer`, bumped **if and only if the rope changed**. Private so it can only
advance through a real mutation — a settable revision is a revision that lies —
and bumped after the fallible prelude, so a REJECTED edit does not invalidate
offsets that are still correct (`a_rejected_edit_does_not_advance_the_revision`).

`EditorState::search_at` is now `Option<Anchored<usize, TextRev>>`, and **the
manual invalidation line was deleted**. That line is the whole point: it was a
thing that had to be remembered, and `SearchState::refresh` — which existed for
exactly this purpose and had zero callers for its entire life — is the
cautionary tale for what happens when it is not.

**Proven 2026-08-04, both directions:**

- freezing `text_rev()` so it never changes turns
  `an_edit_expires_the_match_ordinal_without_anyone_clearing_it` red — the
  anchor is doing the work, not a leftover clear;
- `a_pure_cursor_move_does_not_expire_the_ordinal` pins the distinction that
  makes it useful rather than noisy, and would fail immediately if anyone
  re-keyed it on `EditGen`.

---

## 5. Named follow-ups

1. ~~Extract to `escriba-memori` → fold the twins.~~ **DONE.**
2. ~~Text-revision counter in `escriba-buffer`.~~ **DONE** — `TextRev`.
3. ~~Retrofit `find_all` onto `Ruler`.~~ **DONE**, via `Ruler::ascending`.

   Worth recording WHY it needed a new API rather than the obvious
   substitution: `Ruler::to_chars` is **O(n) per call** (it re-walks from the
   start, because it must be total and random-access), so calling it per match
   is O(n·m) — a performance REGRESSION, and precisely the cost the old code's
   own comment called "the whole frame budget". `AscendingScan` is the third
   option and beats both: O(n + m) total, O(1) extra memory, versus a dense map
   that allocated and zeroed EIGHT BYTES PER DOCUMENT BYTE on every keystroke
   of an incremental search.

   Licensed by `the_ruler_scan_agrees_with_the_hand_rolled_map`, a differential
   test whose oracle is the deleted algorithm reconstructed verbatim.

4. ~~`CaretMove` belongs below both prompt surfaces.~~ **DONE.** It moved from
   `escriba-search`'s private prompt code into memori when `escriba-mode`'s
   ex-line grew a caret and needed the same four moves. `escriba-search` and
   `escriba-mode` cannot see each other, so the only place the shared verb can
   live is beneath both — which is exactly what a leaf positioning crate is
   for.

5. **UTF-16 is BLOCKED, not queued.** `Utf16Units` has no consumer and cannot
   have one until LSP position conversion exists — `escriba-lsp-client` has no
   `Position` type. Giving it a consumer means building that feature for its
   own reasons; manufacturing one to make the axis look load-bearing would be
   the same over-claim this file has already had to correct twice.

6. **Make reaching for the vocabulary an invariant rather than a habit** —
   opened by the ex-line's `byte_of_caret` slip in §2 above. A
   fourth hand-rolled `char_indices().nth()` landed in a crate that already
   depended on memori, and nothing failed. Candidate mechanisms, none built: a
   clippy `disallowed_methods` entry for `char_indices().nth()` outside
   `escriba-memori`, or a grep-shaped CI gate. Both are *only-mitigated* by
   construction — a lint catches the spelling, not the intent — so this is
   named as an open problem, not a queued fix.

7. `(defmemori …)` tatara-lisp surface + `#[derive(DeriveTataraDomain)]`.
   **Not shipped** — M0 is the typed Rust border only.
