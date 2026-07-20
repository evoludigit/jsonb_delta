# Phase 4: Re-measure and Correct Performance Claims

Resolves: #15

## Objective

Establish what jsonb_delta's actual speedup is, per operation and scenario, and
rewrite every published claim to match the measurements — including revising
downward where the data says so.

## Framing — read this before starting

This phase is **not** "defend the 2–7× claim." Evidence gathered so far points
the other way:

1. **No artifact in the repo substantiates the published table.** `benchmarks/*.txt`
   contains single-shot `EXPLAIN ANALYZE` runs against `tv_company` / `tv_feed`.
   The README/PERFORMANCE.md sweep (10/100/1000 elements → 2.0×/3.2×/3.6×) appears
   nowhere in any recorded output.
2. **The stated methodology does not match the harness.** `docs/PERFORMANCE.md:7-11`
   claims "warm cache, average of 10 runs, median reported." The scripts it points
   at do no warm-up, no repetition, no averaging — `benchmark_array_update_where.sql`
   wraps a single `UPDATE` in `EXPLAIN ANALYZE`, despite a header reading
   "1000 iterations."
3. **The harness has been unrunnable since the `jsonb_ivm` rename** (Phase 1), so
   the numbers cannot have been regenerated recently.
4. **The reporter's hypothesis is mechanically plausible.** On a whole-document
   update, JSONB parse + serialize + detoast + row write dominate, and both sides
   pay it. Faster element *matching* (the v0.2.0 SIMD work) cannot move a total it
   is a small fraction of. Their measured parity is the expected result under that
   model; a 2.9× would need the matching to dominate.

Treat #15's author as a collaborator who did work we should have done. They
explicitly withheld a "contradicted" verdict pending our own harness — the
honourable response is a real measurement and a corrected claim, promptly.

## Success Criteria

- [ ] A repeatable harness with warm-up, N≥10 trials, median + p95 reported
- [ ] Results for both scenarios the reporter distinguishes: **stored-table UPDATE**
      and **in-memory expression**
- [ ] Coverage per operation: update / delete / insert / batch / multi-row —
      the delete claim (5–7×) is separate from update and may well hold
- [ ] Array-size sweep 10 / 50 / 100 / 1000; both integer-id and text-id match
- [ ] Output correctness asserted byte-identical to the native baseline per scenario
- [ ] Measured on a **rentable, named machine type** so a third party can reproduce
      the run rather than take our hardware on trust
- [ ] Results committed as a dated artifact keyed to version + machine profile
- [ ] Measurement host destroyed once results are pulled down, and its teardown
      confirmed
- [ ] Every claim in README.md and docs/PERFORMANCE.md either matches the artifact
      or is removed
- [ ] #15 answered with numbers and the reporter's three questions addressed directly

## Measurement environment

Measurements are taken on a short-lived Hetzner Cloud instance, not on a developer
machine. Three reasons, in order of importance:

1. **Reproducibility is the actual deliverable.** #15 asked for numbers "keyed to
   version + machine profile." "Hetzner CCX33, PG 17, jsonb_delta v0.2.0" is a spec
   anyone can rent for a few euros and re-run. A laptop is not.
2. **The local server is disqualified.** `/opt/postgresql17` has pg_tviews in
   `shared_preload_libraries`, whose `ProcessUtility` hook fires on the `tv_`-prefixed
   fixtures (see Phase 1). An extension hooking utility statements is not something
   to have in the loop while timing utility statements.
3. **Isolation.** No editor, no browser, no compile jobs competing for cache and
   scheduler.

**Dedicated vCPU (CCX line), not shared (CX/CPX).** Shared-vCPU instances have
noisy neighbours, and steal time at the ~1 ms scale we are measuring is not a
rounding error — it is the whole signal. This costs more per hour, which is a
further reason the instance should be short-lived.

**Sequencing.** The harness is built and validated locally *first* (Cycle 1); the
instance is provisioned only once there is something worth running on it (Cycle 2),
and destroyed as soon as results are retrieved (Cycle 3). Provisioning early would
bill an idle box and would not accelerate any of the work that actually blocks.

**Cost discipline.** Provisioning and teardown live in one scripted path
(`scripts/provision_bench_vps.sh`) so teardown cannot be forgotten, and the run is
expected to be under an hour of billing. Record the instance type, region, image,
and PG build flags in the results artifact — an unnamed machine profile makes the
numbers as unverifiable as the ones we are replacing.

## TDD Cycles

### Cycle 1: Honest harness (local) — [x] COMPLETE
- **RED**: `test/bench/harness.sql` — a driver taking (setup, native SQL,
  jsonb_delta SQL, N), running warm-up then N trials via `clock_timestamp()`,
  emitting median/p95/min. Assert it detects a known-slower control.
- **GREEN**: Implement. Assert byte-identical output between arms before timing;
  refuse to report a speedup for arms that disagree.
- **REFACTOR**: Table-driven scenario definitions so adding a case is one row.
- **CLEANUP**: Replace the `EXPLAIN ANALYZE` bodies in the existing benchmark
  scripts with harness calls.

Developed against a throwaway local cluster (`initdb` into a scratch dir, empty
`shared_preload_libraries`) — the same setup Phase 1 used for verification. No
cloud resources are needed for this cycle, and none should be created during it.

#### Outcome

Delivered: `test/bench/harness.sql` (schema `bench`) and `test/bench/harness_test.sql`.

Scenarios are rows in `bench.scenario`, registered via `bench.define()`; results
land in `bench.result` with the **raw per-trial timings retained**, so a reader
can recompute the statistics rather than trust the summary — that is the actual
complaint in #15, and summarising away the evidence would reproduce it.

Eight calibration controls, all passing, gate the instrument in
`test/benchmark_smoke.sh` before any benchmark runs:

| Control | Asserts |
|---|---|
| `control_slower` / `control_faster` | detects a regression *and* a speedup (a sleeping arm cannot be reported as fast) |
| `control_mismatch` / `control_agree` | withholds a ratio for disagreeing arms; does not false-positive on agreeing ones |
| `control_inmem_mismatch` / `control_inmem_agree` | same policing for in-memory expression arms |
| order statistics | `min <= median <= p95` over the requested N, warm-up excluded |
| trial isolation | a mutating scenario leaves the table as it found it |

The gate was verified non-vacuous: sabotaging the correctness guard makes
`benchmark_smoke.sh` fail loudly rather than pass quietly.

#### Deviations from plan (all deliberate)

1. **Arms are interleaved, not run as consecutive blocks.** The deliverable is a
   *ratio*; a host that drifts mid-run would otherwise charge the whole drift to
   whichever arm was running, and that bias lands straight in the published
   number. Arms now alternate trial by trial.
2. **Correctness is established once per arm, outside the timed loop** rather
   than on every trial. It is a property of the SQL, not of the run, and keeping
   it out means the reported milliseconds contain nothing but the arm.
3. **In-memory arms needed a second verification mode** (`verify_sql => ''`,
   capturing the arm's own return value). The plan's shared `verify_sql` reads
   database state, and an in-memory arm writes none — so the success criterion
   "results for both scenarios the reporter distinguishes" was unreachable
   without it. Driven by its own RED test.
4. **Only `benchmark_array_update_where.sql` Benchmark 1 was converted**, not all
   nine scripts. It is the one whose header claimed "1000 iterations" over a
   single `EXPLAIN ANALYZE` — the specific defect #15 names. Benchmarks 2 and 3
   are multi-statement cascades that do not map onto a two-arm scenario; they are
   now labelled indicative-only rather than silently left looking authoritative.
   Converting them is Cycle 3 work, once the matrix defines what they should be.
5. **`docs/PERFORMANCE.md`'s stated methodology is now true of the harness**
   (warm cache, N trials, median) but is *not yet* true of the numbers published
   beside it. Those are Cycle 3/4. The doc was left alone deliberately — editing
   it before re-measuring would just move the unsourced claim to a new sentence.

#### Preliminary local signal — NOT publishable

Developer machine (PG 18.1, pgrx scratch cluster), 25 trials, all 12 scenarios
verified byte-identical. Ratios only; absolute times are meaningless off a named
machine profile.

| Array size | update (stored) | update (in-memory) | delete (in-memory) |
|---|---|---|---|
| 10   | 1.78× | 1.85× | 1.94× |
| 50   | 1.58× | 1.68× | 1.69× |
| 100  | 1.53× | 1.60× | 1.61× |
| 1000 | 1.49× | 1.54× | 1.55× |

Two things to carry into Cycle 3, neither of them settled by this run:

- **The measured ratio does not reach the published 2–7×** anywhere in the sweep.
- **The trend runs the wrong way.** Published claims rise with array size
  (2.0× → 3.6×); measured ratio *falls* (1.9× → 1.5×). That is the direction the
  reporter's mechanical argument predicts — per-element matching is a shrinking
  fraction of a cost dominated by parse/serialize, which both arms pay.

This does not yet confirm the reporter's parity finding: it contradicts *our*
published table but also does not reproduce their 1.0×/0.8×. The gap is most
likely the baseline — theirs and ours may not be the same "native" SQL. Cycle 3
must pin the baseline down explicitly and report it alongside the ratio, or the
two measurements will keep talking past each other.

### Cycle 2: Provision the measurement host
- **RED**: `scripts/provision_bench_vps.sh` with subcommands `up` / `run` / `down`.
  `up` on a nonexistent instance must fail loudly rather than silently continuing;
  `down` must be idempotent and safe to run twice.
- **GREEN**: `up` provisions a **dedicated-vCPU (CCX)** instance via `hcloud`,
  installs PostgreSQL 17 and the pinned jsonb_delta build, and prints the resolved
  machine profile (instance type, region, image, vCPU/RAM, PG version, commit SHA).
- **REFACTOR**: Make the whole path re-entrant, so a failed run can be retried
  without hand-editing cloud state.
- **CLEANUP**: Verify `down` actually destroys the server **and** any volume or
  floating IP created alongside it — `hcloud server list` must come back empty of
  benchmark hosts. Confirm teardown before moving on; an instance left running is a
  standing charge.

Requires explicit go-ahead before first `up`: this is the step that spends money.

#### Outcome — script built and validated, STOPPED before spending (2026-07-20)

`scripts/provision_bench_vps.sh` exists with `up` / `run` / `down`, plus `status`,
`profile`, and `selftest`. Everything that costs nothing has been exercised; **no
billable operation has been performed and `hcloud server list` is empty.**

`selftest` covers the cycle's two RED conditions and passes 8/8:

| Check | Asserts |
|---|---|
| `up` refuses without `BENCH_CONFIRM_SPEND=yes` | no default and no interactive prompt can bill by accident |
| `run` / `profile` with no instance | fails loudly rather than silently measuring the wrong machine |
| `down` with nothing to delete, run twice | idempotent, and does not error on a clean account |
| `up --dry-run` | prints the exact `hcloud server create` call and makes none |
| post-condition | no server was created by the self-test itself |

Cost discipline beyond the plan: `down` sweeps volumes and floating IPs **by
label**, since those survive server deletion and keep billing; it re-checks and
fails loudly if the server is still present afterwards; and `up`, `down` and `run`
all end by printing billing state, so an instance left running is visible rather
than inferred. `--dry-run` is honoured by every mutating call.

Remaining before Cycle 3: the owner's explicit authorization for the first `up`,
and `BENCH_SSH_KEY` naming a key in the `fraisier` context.

### Cycle 3: Measure
- **RED**: Scenario matrix as failing/unpopulated rows: {update, delete, insert,
  batch, multi-row} × {stored table, in-memory} × {10, 50, 100, 1000} × {int-id, text-id}.
- **GREEN**: Run it on the provisioned host. Record raw output to
  `benchmarks/results-v0.2.0-<profile>.txt`, with the machine profile in the header.
- **REFACTOR**: Summarise into a committed markdown table keyed to version, PG
  version, instance type, and date.
- **CLEANUP**: Tear the instance down and confirm. Note which scenarios are *not*
  covered rather than leaving gaps implicit.

Re-run the matrix at least twice on the same host before trusting a result; if
medians move materially between runs, the host is too noisy and the numbers are not
publishable regardless of what they say.

### Cycle 4: Correct the claims
- **RED**: A docs test (or CI grep) asserting no numeric speedup claim exists in
  README.md / docs/PERFORMANCE.md that is absent from the results artifact.
- **GREEN**: Rewrite claims to match measurements. Expect this to mean:
  - Qualifying the headline with the operation and scenario where it holds
  - Dropping the 2–7× banner from README.md:31/152 if update-at-parity is confirmed
  - Stating plainly where jsonb_delta is at parity or slower (the reporter measured
    0.8× in-memory, and slower at 1000 elements)
- **REFACTOR**: Give each claim an inline pointer to the artifact row proving it.
- **CLEANUP**: Delete `docs/PERFORMANCE.md:300-306`'s fabricated "expected output"
  block. Purge unsourced numbers from `docs/implementation/*.md` or mark that
  directory clearly historical.

### Cycle 5: Respond
- **GREEN**: Comment on #15 with: the numbers, answers to their three questions,
  and what we corrected. If update-at-parity confirms, say so directly — a
  maintainer correcting their own claim is worth more than a defended one.
- **CLEANUP**: Offer degustation a versioned machine-readable results file so the
  claim stays checkable over time, as they requested. Consider publishing
  benchmark results per release from CI.

## Risks

- **The SIMD work may have no user-visible effect on this workload.** If so, say
  that; it does not make the work worthless (it may matter for match-heavy or
  batch paths), but it cannot be sold as a whole-document update speedup.
- **Ecosystem coupling.** README.md:44 claims "pg_tviews uses jsonb_delta for
  1.5-3× faster JSONB updates," and README.md:30-34 carries speedup figures for
  three sibling projects. If the update claim is revised, those inherit the
  correction — flag it to those repos rather than leaving stale numbers pointing here.
- **Absolute times will not match the published ones, and that proves nothing.**
  The existing README figures were taken on unknown hardware. A CCX instance will
  produce different milliseconds regardless of who is right. Only the **ratio**
  (jsonb_delta vs the native baseline, measured in the same run on the same host)
  is comparable across machines — so the ratio is what we publish and what we argue
  from. Do not claim the old absolute numbers were wrong; claim only what the
  ratios show.
- **Cloud cost runs while attention is elsewhere.** The instance bills whether or
  not anyone is looking at it. Teardown is a success criterion, not a courtesy, and
  `down` should be run even if the measurement run fails or is abandoned midway.
- **A quiet host is an assumption, not a guarantee.** Even on dedicated vCPU,
  verify stability by re-running the matrix rather than trusting a single pass.

## Dependencies

- Requires: Phase 1 (benchmark suite must run) — **done**
- Requires: Cycle 1 complete before any cloud resource is created
- Requires: `hcloud` CLI authenticated (present locally, context `fraisier`) and
  explicit go-ahead to spend
- Blocks: Phase 5

## Status
[~] In Progress — Cycle 1 complete (harness built, calibrated, gating the suite).
Cycle 2 is next and is the step that spends money: it must not begin without
explicit go-ahead.
