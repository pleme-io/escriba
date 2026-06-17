# Fleet tatara-lisp lineage unification — migration plan

**Directive (user, 2026-06-14):** one tatara-lisp lineage —
`pleme-io/tatara-lisp` (the WASM/WASI-capable platform: host-embeddable
`tatara-lisp-eval`, `tatara-lisp-script`, `compiler_spec` polyglot seam).
Retire the `tatara-lisp` / `tatara-lisp-derive` crates published from the
old `pleme-io/tatara` (macro-farm) repo.

## Why

Two lineages can't coexist in one cargo graph: a binary that pulls
`tatara-lisp` from both sources gets two incompatible `Sexp`/`Value`
types. escriba already migrated (clean — lineage B is an API superset).
Until the fleet unifies, escriba (lineage B) cannot link `caixa-core` /
`caixa-resolver` (lineage A) — which blocks the first-class
`CaixaKind::Extensao` + `feira escriba install` plugin-installer work.

## Blast radius (measured 2026-06-14)

12 repos consume `tatara-lisp` from the OLD lineage (`git
pleme-io/tatara`). `dependents` = pleme-io repos that git-depend on it:

| Repo | kind | dependents | notes |
|---|---|---|---|
| alicerce | workspace | 0 | leaf |
| arnes | workspace | 0 | leaf |
| caixa | workspace | 0 | **unblocks Phase 3 (feira/Extensao)** |
| estante | workspace | 0 | leaf |
| kura | workspace | 0 | leaf |
| passaporte | leaf | 0 | leaf |
| repo-forge | workspace | 0 | leaf |
| promessa | workspace | 1 | |
| frost | workspace | 1 | the running shell — migrate carefully |
| nami-core | leaf | 2 | |
| moldura | leaf | 5 | |
| shikumi | leaf | **52** | shared lib; tatara-lisp behind OPTIONAL `lisp` feature (off by default), so blast is limited to lisp-feature consumers |

## Conflict rule

A repo can migrate **independently** iff it does NOT expose `tatara_lisp`
types in its PUBLIC API (internal-only use). If it does, all its
dependents must co-migrate (they'd otherwise see two `Sexp` types). Leaf
repos (0 dependents) are always safe. shikumi exposes tatara-lisp only
under `--features lisp`, so its 52 dependents are unaffected unless they
opt in.

## Per-repo recipe

1. In the repo's workspace `Cargo.toml`, change the `tatara-lisp` +
   `tatara-lisp-derive` git source from
   `https://github.com/pleme-io/tatara` → `…/tatara-lisp`; add
   `tatara-lisp-eval` if the repo wants the runtime evaluator.
2. `cargo build --workspace` → fix any API drift (lineage B is a
   superset; escriba needed zero fixes).
3. `cargo test --workspace` → green.
4. `gen build .` → regenerate `Cargo.gen.lock` (+ `Cargo.build-spec.json`);
   `gen check-spec .` → `fresh`. (Delta-only-spec repos commit
   `Cargo.gen.lock`.)
5. Commit only when the operator asks; branch off the default branch.

## Phased order

- **Phase A — zero-dependent leaves (any order, lowest risk):** alicerce,
  arnes, estante, kura, passaporte, repo-forge, **caixa** (do caixa first
  — it unblocks the escriba plugin-installer).
- **Phase B — low-dependent, verify public-API tatara-lisp exposure:**
  promessa (1), frost (1, the shell), nami-core (2), moldura (5). For each
  with public-API exposure, co-migrate its dependents.
- **Phase C — shikumi (52 dependents) last:** migrate the optional `lisp`
  feature's source to lineage B; verify shikumi+lisp consumers (escriba
  intentionally keeps the feature OFF). Then retire lineage A's
  `tatara-lisp`/`tatara-lisp-derive`.

## Crux findings (2026-06-14) — deeper than a git-source swap

1. **The two lineages have DIVERGED** (not one-ahead-of-the-other):
   different `reader.rs`, different file counts (A=12, B=13), different
   public APIs. Lineage A (`pleme-io/tatara`) uniquely exports
   `closed_set` / `diagnostic` / `DomainHandler` / `DeriveClosedSet`;
   lineage B uniquely has the eval / script / `compiler_spec` (WASM/WASI)
   surface. **Unification = reconcile both into one superset crate**, not
   just repoint deps. A's consumers that use A-unique APIs must have those
   ported into B (or drop them) before they can migrate.

2. **3 repos are ENTANGLED with the `pleme-io/tatara` macro farm's
   non-lisp crates** (which transitively carry lineage-A tatara-lisp):
   - caixa → `tatara-process`  (← THIS blocks escriba's feira/Extensao)
   - nami-core → `tatara-eval`
   - moldura → `tatara-ui`
   So caixa can't migrate until `tatara-process` (a macro-farm crate) is
   lineage-B-compatible. The hard core of unification is the macro-farm
   repo's OWN internal tatara-lisp dependency.

3. **Open-source / crates.io end-state (user directive):** lineage B is
   ALREADY open-source (MIT, public `github.com/pleme-io/tatara-lisp`) and
   ALREADY has the standard release flows (`auto-release.yml` + substrate
   `rust-workspace-release-flake.nix`; path-deps carry `version=`). The
   END-STATE of unification is: reconcile divergence → publish the unified
   `tatara-lisp` + `tatara-lisp-derive` (+ `-eval`) to **crates.io** →
   fleet consumes the PUBLISHED versioned crate (no git-dep lineage forks
   possible). Prereqs before publish: (a) reconcile divergence; (b) fix
   the path-dep version pins (sit at 0.2.2, workspace is 0.2.4); (c)
   crates.io token + explicit operator go-ahead (publishing is an
   outward, effectively-irreversible action — do NOT auto-run it).

## Status

- ✅ escriba (lineage B) — done (this initiative).
- ✅ Plan + blast-radius + crux — done.
- ⏸ caixa — BLOCKED on `tatara-process` (macro-farm entanglement); not a
  clean Phase-A leaf as first assumed.
- ⌀ 9 clean leaves (alicerce/arnes/estante/kura/passaporte/repo-forge/
  promessa/frost/shikumi) — mechanically migratable (escriba-proven
  recipe), but only AFTER divergence reconciliation if they use A-unique
  APIs; verify each.
- ⌀ crates.io open-source publish of unified tatara-lisp — pending
  reconciliation + operator go-ahead (outward action).
