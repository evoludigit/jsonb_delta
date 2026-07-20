# Handoff: open-issue remediation plan (#12–#15)

This branch exists to transfer in-progress work, not to be merged as-is. It
carries the phase plan plus the two pieces of it that are already built, so the
work can be picked up without repeating them.

## What state the work is in

| Phase | Issue | State |
|---|---|---|
| 1 — benchmark reproducibility | #13 | **Done.** 9/9 benchmark scripts run clean via `test/benchmark_smoke.sh`. |
| 2 — version coherence | #14 | **Done and merged to `main`** (PR #21, 2026-07-20). #14 is closed. |
| 3 — `jsonb_smart_patch_array` compat | #12 | **Cycle 1 done** — signature contract test (`src/contract.rs`), pins all 15 exports; verified it fails on the #12 drift. **Cycles 2–3 blocked** on `fraiseql/pg_tviews#50` confirming semantics. Do not guess them. |
| 4 — re-measure performance claims | #15 | **Cycle 1 done** (harness built and calibrated). Cycles 2–5 open. |
| 5 — finalize | — | Not started. |

## Start here

```bash
just install                 # needs write access to the PostgreSQL share dir
./test/benchmark_smoke.sh    # calibrates the harness, then runs all 9 benchmarks
```

`test/benchmark_smoke.sh` asserts exit status only, never timing. It gates on
`test/bench/harness_test.sql` first: if the measurement instrument fails its own
calibration, no benchmark runs.

## How this branch relates to `main` (read before planning any merge)

**This branch is 7 commits behind `main` and does not contain the toolchain or
version-coherence work.** That work is no longer pending anywhere — it landed on
`main` on 2026-07-20 as PRs #17, #18, #21, #22, with their SHAs preserved:

- **pgrx `0.16.1 → 0.17.0` is resolved and merged** (PR #18). Every one of the
  pins moved with it — all four workflows, `Dockerfile`, `justfile:init`, and the
  docs. There are no `0.16.1` stragglers outside archived phase notes. An earlier
  revision of this file described the bump as deliberately withheld and unresolved;
  that is obsolete, and PG18 builds are unblocked.
- **Phase 2 / #14 is merged** (PR #21): `main` ships `sql/jsonb_delta--0.2.0.sql`
  and `0.1.0--0.2.0.sql` with `default_version = '0.2.0'` and a real UPDATE path.

So rebase this branch onto `main` before building on it. What it still carries
that `main` does not is one commit: the phase plan, the Phase 1 benchmark repair,
the measurement harness, and the Phase 3 Cycle 1 contract test.

## Gotchas that will cost you a day each

1. **Do not benchmark on a cluster with `pg_tviews` in `shared_preload_libraries`.**
   Its `ProcessUtility` hook fires on the `tv_`-prefixed fixtures and panics on
   `DROP TABLE tv_*`. Use a throwaway cluster:
   `initdb -D <scratch> -U postgres`, started with `-c shared_preload_libraries=''`.
   More generally: an extension that hooks utility statements is not something to
   have in the loop while timing utility statements.

2. **The `test_` table prefix is a boundary, not noise.** `test_tv_*` are the
   mutable working copies; `tv_*` are read-only fixtures. Issue #13 proposes
   renaming them — that would fix the symptom and break `benchmark_baseline.sql`.
   See `test/fixtures/setup_benchmark_env.sql`.

3. **Phase 4 is a correction, not a defence.** The evidence says the published
   2–7× does not hold for stored-table updates. Argue from **ratios measured in
   the same run on the same host** — never absolute milliseconds, since the
   existing figures came from unknown hardware and different milliseconds prove
   nothing either way.

4. **Cycle 2 of Phase 4 provisions a cloud host and bills.** It must not run
   without explicit authorization from the repository owner, and teardown is a
   success criterion rather than a courtesy.

## The harness

`test/bench/harness.sql` is the instrument every published number should come
from. Scenarios are rows registered via `bench.define()`; results land in
`bench.result` with **raw per-trial timings retained**, so a reader can recompute
the statistics instead of trusting a summary — that is the substance of #15, and
summarising the evidence away would reproduce it.

It runs warm-up then N≥10 trials, reports median and p95, interleaves the two
arms so host drift cannot be charged to one of them, and **refuses to report a
ratio when the arms produce different output**. `test/bench/harness_test.sql`
holds eight calibration controls covering both directions of that behaviour.

Preliminary local numbers are recorded in `phase-04-performance-claims.md`. They
were taken on a developer machine and are **not publishable** — they exist to
show the instrument works and to indicate direction.

The harness has now been pointed at PR #16's `jsonb_apply_changeset` (release
build, scratch cluster; see `phase-04-performance-claims.md`). Two results worth
knowing before anyone plans Cycle 2:

- The withdrawn "4.8×–40×" **reproduces in release** (4.50× / 17.99× / 41.82× /
  41.81× at its own four configurations). The hypothesis that it was a debug-build
  artifact is refuted — it was simply N=50, and the ratio tracks N because the
  chain pays whole-document serde once per edit while a changeset pays it once.
- Against `jsonb_array_update_where_batch` — the strongest baseline that already
  exists for that operation — the changeset is at **parity** (1.03 / 0.94 / 1.07).
  Its advantage there is coverage, not speed.

Publish both baselines together or not at all. A "41× faster" headline is true
against the chain and misleading as a description of the feature, since a reader
doing exactly the benchmarked operation should use `batch` and would measure
parity — which is the same overclaiming #15 exists to correct.
