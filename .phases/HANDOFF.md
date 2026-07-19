# Handoff: open-issue remediation plan (#12–#15)

This branch exists to transfer in-progress work, not to be merged as-is. It
carries the phase plan plus the two pieces of it that are already built, so the
work can be picked up without repeating them.

## What state the work is in

| Phase | Issue | State |
|---|---|---|
| 1 — benchmark reproducibility | #13 | **Done.** 9/9 benchmark scripts run clean via `test/benchmark_smoke.sh`. |
| 2 — version coherence | #14 | Not started. Blocked on a pgrx decision, see below. |
| 3 — `jsonb_smart_patch_array` compat | #12 | Cycle 1 ready; **Cycles 2–3 blocked** on `fraiseql/pg_tviews#50` confirming semantics. Do not guess them. |
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

## What is deliberately NOT on this branch

**The pgrx `0.16.1 → 0.17.0` bump.** It exists but is unresolved, so shipping it
inside a handoff would bake an undecided toolchain change into someone else's
starting point. Two consequences to know about:

- pgrx 0.16.1 does not build against PostgreSQL 18. If you need pg18, the bump
  is a prerequisite, not an optional upgrade.
- 11 places pin `cargo-pgrx 0.16.1` (all workflows, `Dockerfile`, `justfile:init`).
  They must move together with the dependency or the build and the CI toolchain
  disagree.

Phase 2 regenerates SQL via pgrx, so this must be settled **before** Phase 2.

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
