# Phases: Open Issue Remediation (#12–#15)

Plan to resolve the four open GitHub issues, ordered by dependency rather than
issue number.

## Issue → Phase map

| Issue | Title (short) | Phase | Notes |
|---|---|---|---|
| #13 | Benchmark can't run | 1 | Blocks #15 |
| #14 | Version incoherence 0.2.0 / 0.1.0 | 2 | Independent, quick |
| #12 | `jsonb_smart_patch_array` signature mismatch | 3 | Has an open design question |
| #15 | 2–7× claim doesn't reproduce | 4 | Depends on Phase 1; measured on a rented host |
| — | Finalization | 5 | Always last |

## Dependency order

```
Phase 1 (harness runs)  ──┐
Phase 2 (version)         ├──> Phase 4 (re-measure + correct claims) ──> Phase 5
Phase 3 (API compat)    ──┘
```

Phase 1 must land before Phase 4: we cannot answer "under what conditions does
2–7× reproduce?" until our own harness executes. Phases 2 and 3 are independent
of both and can be done in parallel or interleaved.

Within Phase 4 the ordering is **harness first, hardware second**: the measurement
driver is built and validated locally, and only then is a short-lived dedicated-vCPU
Hetzner instance provisioned to take the actual numbers on a machine profile a third
party can rent and reproduce. Provisioning before the harness exists would bill an
idle box while producing nothing, and running the *current* scripts on better
hardware would only regenerate the present problem — single-shot `EXPLAIN ANALYZE`
timings with a fresh date on them.

## Findings that change the issues as filed

Verified against working tree at `d1a395c` + uncommitted `Cargo.toml` pgrx bump.

1. **#13 is right about the outcome, wrong about the cause.** `test/benchmark_baseline.sql:18`
   *does* create `test_tv_network_configuration` (as a `CREATE TABLE … AS SELECT * FROM
   tv_network_configuration`). The real defect is an **undocumented three-step ordering
   dependency** (`generate_cqrs_data.sql` → `benchmark_baseline.sql` → `benchmark_array_update_where.sql`)
   combined with an **error message that names the wrong prerequisite script**. Renaming
   the table as the issue suggests would "fix" the symptom while breaking `benchmark_baseline.sql`.

2. **#13 understates the breakage.** All 7 benchmark scripts in `test/` begin with
   `CREATE EXTENSION IF NOT EXISTS jsonb_ivm;` — the extension's pre-rename name. Even
   with correct fixtures and ordering, every benchmark aborts on a missing extension
   (`\set ON_ERROR_STOP on`). The benchmark suite has been unrunnable since the rename,
   which is consistent with no benchmark artifact in the repo postdating it.

3. **#15's core complaint is very likely correct, and worse than filed.** No artifact in
   `benchmarks/`, `baselines/`, or `test/*.txt` substantiates the headline table. The
   recorded runs (`benchmarks/benchmark_results.txt`) are single-shot `EXPLAIN ANALYZE`
   against `tv_company` / `tv_feed`, not the 10/100/1000-element array sweep the README
   publishes. Meanwhile `docs/PERFORMANCE.md:7-11` states the methodology is "warm cache,
   average of 10 runs, median reported" — the harness it points at (`test/benchmark_*.sql`)
   does no warm-up, no repetition, and no averaging. **The published numbers should be
   treated as unsourced until re-measured.** Plan accordingly: Phase 4 is a re-measurement
   and correction, not a defense.

4. **The pgrx bump is uncommitted but healthy.** `Cargo.toml` has an unstaged
   `pgrx 0.16.1 → 0.17.0` bump; `cargo check --no-default-features --features pg17`
   passes clean. Phase 2 regenerates SQL via pgrx, so this bump must be resolved
   (committed or reverted) *before* Phase 2 to avoid baking an unintended toolchain
   change into the shipped contract.

## Status

- [x] Phase 1: Restore benchmark reproducibility — 9/9 benchmarks run clean
- [ ] Phase 2: Version coherence and upgrade path
- [ ] Phase 3: `jsonb_smart_patch_array` compatibility
- [ ] Phase 4: Re-measure and correct performance claims
- [ ] Phase 5: Finalize
