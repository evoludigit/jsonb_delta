# Phase 1: Restore Benchmark Reproducibility

Resolves: #13 (and the unfiled `jsonb_ivm` breakage)

## Objective

Make `just bench` run end-to-end from a clean database, so that the performance
claims in #15 can be measured against our own harness.

## Success Criteria

- [x] `just bench` succeeds from a clean database with no manual prerequisite steps
- [x] No `test/*.sql` references the extension name `jsonb_ivm`
- [x] Every benchmark script's prerequisite check names the script that actually
      satisfies it
- [x] `docs/PERFORMANCE.md` reproduction steps match what `just bench` does
- [x] A CI job runs the benchmark suite for exit status (not timing) so this
      cannot silently rot again

## Verification

All 9 benchmark scripts run clean (`test/benchmark_smoke.sh`, 9 passed / 0 failed)
against an isolated PostgreSQL 18.1 cluster with empty `shared_preload_libraries`,
matching CI conditions. Fixture setup and a benchmark were each re-run three times
in the same database to confirm idempotency.

## Design decisions

**Keep both table names.** `tv_network_configuration` is the fixture (the "source"
table); `test_tv_network_configuration` is the per-run mutable copy created by
`benchmark_baseline.sql`. This separation is deliberate and correct — benchmarks
mutate, fixtures should not. We fix the *ordering and messaging*, not the names.
Reply on #13 explaining this, since the suggested one-line rename would break
`benchmark_baseline.sql`.

**Make setup idempotent and self-contained.** Rather than documenting a three-step
sequence users must remember, each benchmark script should ensure its own
preconditions.

## TDD Cycles

### Cycle 1: Extension name
- **RED**: Add `test/benchmark_smoke.sh` asserting each `test/benchmark_*.sql`
  runs to completion against a clean DB. Fails on `extension "jsonb_ivm" is not available`.
- **GREEN**: Replace `jsonb_ivm` → `jsonb_delta` across the 7 benchmark scripts.
- **REFACTOR**: Extract the shared preamble (`\timing`, `ON_ERROR_STOP`,
  `CREATE EXTENSION`) into `test/benchmark_preamble.sql`, `\i`-included by each script.
- **CLEANUP**: Grep for remaining `jsonb_ivm` outside `docs/archive/`; fix
  `fuzz/Cargo.toml` and `.github/workflows/release.yml` hits.

### Cycle 2: Fixture ordering
- **RED**: Smoke test invokes `benchmark_array_update_where.sql` alone against a
  clean DB. Fails with the misleading `Test data not found. Run generate_cqrs_data.sql first.`
- **GREEN**: Add `test/fixtures/setup_benchmark_env.sql` that idempotently runs the
  full chain (generate fixtures → create `test_`-prefixed working copies). Each
  benchmark `\i`-includes it.
- **REFACTOR**: Move the working-copy creation out of `benchmark_baseline.sql`
  into the shared setup so `benchmark_baseline.sql` is a peer of the other
  benchmarks, not a hidden prerequisite.
- **CLEANUP**: Delete the now-dead `DO $$ … RAISE EXCEPTION` precondition guards;
  the setup script makes them unreachable.

### Cycle 3: Entry points and docs
- **RED**: CI job `bench-smoke` runs `just bench` on a clean PG 17 container. Fails.
- **GREEN**: Update `justfile:bench` to run setup then the suite. Add `just bench-all`.
- **REFACTOR**: Align `docs/PERFORMANCE.md:288-300` reproduction steps with the
  justfile recipe; single source of truth.
- **CLEANUP**: Remove the hardcoded "Expected output: … Speedup: 2.9×" block from
  `docs/PERFORMANCE.md:300-306` — it asserts an unverified number as expected
  output. Phase 4 replaces it with real, dated figures.

## Risks

- ~~`generate_cqrs_data.sql` vs `generate_cqrs_data_tables.sql` may build
  `tv_network_configuration` with different shapes.~~ **Resolved:** diffed, and
  they are equivalent (50-element `dns_servers`, integer ids). Phase 4's document
  shape is not in question. The two files remain exact duplicates — remove one in
  Phase 5.

## Defects found and fixed beyond the original scope

The plan assumed the extension name was the only blocker. It was not:

1. **Generators were not re-runnable.** `generate_cqrs_data.sql` dropped 4 tables
   but created 7, so a second run failed on `v_dns_server` already existing. Same
   defect in the UUID generator. The tree generator dropped `v_tree_user_profile`
   as a MATERIALIZED VIEW while creating it as a TABLE. All three fixed.
2. **`benchmark_e2e_cascade.sql:102`** — `column reference "data" is ambiguous` in
   the surgical-merge UPDATE. Qualified as `a.data`.
3. **`benchmark_tree_composition.sql:209`** — created `v_tree_user_report` as a
   MATERIALIZED VIEW, then `UPDATE`d it, which PostgreSQL rejects. Now a table.
4. **`.github/workflows/release.yml`** — built `jsonb_delta-*.tar.gz` but uploaded
   from `jsonb_ivm-*.tar.gz`, so release artifact upload was broken.
5. **Three benchmarks never loaded the extension at all** (`benchmark_baseline`,
   `benchmark_e2e_cascade`, `benchmark_pg_tview_helpers`) — they only worked when
   something else had already run `CREATE EXTENSION` in that database.

**Why CI stayed green:** `benchmark.yml` ran only `benchmark_pg_tview_helpers.sql`
— the single self-contained script — and issued `CREATE EXTENSION` itself before
invoking it. So the one benchmark CI exercised was the one that could not detect
any of the above. The workflow now smoke-tests all nine.

## Deviations from the plan

- **Guards kept, not deleted.** The plan said to delete the now-unreachable
  precondition guards. They were retained and re-pointed at
  `setup_benchmark_env.sql` instead: if someone runs a benchmark directly, a named
  remedy beats a bare `relation does not exist`.
- **Preamble lives in `test/fixtures/preamble.sql`**, not `test/benchmark_preamble.sql`
  — the latter is matched by the `benchmark_*.sql` glob and was being executed as
  if it were a benchmark.

## Environment note (not a repo defect)

On a server with **pg_tviews** preloaded, the `tv_`-prefixed fixtures collide with
its `ProcessUtility` hook and `pg_tviews_convert_table` event trigger; on this
machine the hook panics outright on `DROP TABLE tv_*`. Documented under
Troubleshooting in `docs/PERFORMANCE.md`. Worth filing upstream against pg_tviews —
a hook that panics on a `DROP TABLE IF EXISTS` for a table it does not manage is a
bug in its own right.

## Dependencies

- Requires: none (start here)
- Blocks: Phase 4

## Status
[x] Complete
