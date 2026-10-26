# unsoku is extracted, unwired, and quietly diverging (2026-08-13, found)

`unsoku` (運足) is a workspace member of THIS repo, published in
`[workspace.dependencies]`, and has **zero consumers** — nothing in escriba
says `use unsoku`. It was lifted out of escriba's semantics so a picker filter
box or a command palette could have `w`/`b`/`dw`/`ciw` without carrying a rope,
an LSP client and a plugin host. The lift happened; the reintegration never
did.

**This is not simple duplication, and calling it that would be wrong.** unsoku
resolves against **a single line** (`&str` + a `usize` caret); escriba-runtime
resolves against a **rope** (`Position { line, column }`). Those cannot share a
resolver, and unsoku's header says so. They already share the *vocabulary* —
both use `escriba_core::Motion` / `Operator` / `TextObject`, and unsoku
declares none of its own precisely to avoid that.

**What they should not have is two answers to one question, and they do.**
`unsoku::is_inclusive(motion)` and `escriba_core::Motion::is_inclusive()` are
two functions over the same enum, and they disagree on three arms:

| motion | `escriba_core` | `unsoku` |
|---|---|---|
| `MatchPair` (`%`) | inclusive | **not** |
| `LineEnd` (`$`) | not | **inclusive** |
| `DocEnd` (`G`) | not | **inclusive** |

Each disagreement is *defensible* on the representation: on one line `DocEnd`
IS `LineEnd`, and escriba reaches `d$`'s inclusive behaviour by resolving
`LineEnd` one-past-the-end instead of by flagging it. **None of it is written
down anywhere**, so the next reader finds two same-named functions over one
enum and no way to tell a deliberate divergence from a stale copy. That is the
defect — not the divergence itself.

**Before reaching for either:** `escriba_core::Motion::is_inclusive` is the
one the editor's operator path uses (`operated_end`). unsoku's is for
single-line consumers. If you change one, decide about the other in the same
commit, or write down why not.

**What today's work means for it.** The typed `Register` + `RegisterKind`
(charwise/linewise) landed in `escriba-runtime`, and unsoku's `Register` is
still `{ text: String }` with a `paste(after, count)` that has no linewise
notion. That is **correct as it stands** — a single-line target has no lines,
so linewise is meaningless there — and it is recorded here so the absence reads
as a decision rather than as lag.
