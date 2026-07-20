# Phase 4 appendix: PR #16 (`jsonb_apply_changeset`) is a claim this harness must measure

Cross-reference for whoever executes Phase 4. PR #16 is OPEN as a **draft**,
deliberately not for merge in this window. It adds a new function whose entire
performance justification is a *coalescing* speedup, so it is the first new claim
that Phase 4's harness should validate before any figure ships.

## Why it belongs to Phase 4

`jsonb_apply_changeset(doc, ops)` applies N surgical edits to one document in a
single parse/serialize pass. Its value is entirely in coalescing: for a *single*
edit it is at parity with the dedicated functions — consistent with #15, and with
this phase's Framing note (whole-document serialization dominates and both sides
pay it). So the claim to measure is **not** "X× faster than native"; it is:

> coalescing N edits into one `jsonb_apply_changeset` call vs. chaining N
> `jsonb_smart_patch_*` calls — reported as a ratio that grows with N.

Add to Phase 4 coverage:

- [x] `jsonb_apply_changeset` vs. chained `jsonb_smart_patch_array`, N ∈ {5, 20, 50},
      across the existing array-size sweep — **integer keys done** under the harness
      (2026-07-20, see below). **Text/UUID keys still outstanding.**
- [x] The same run must also show the single-edit case at parity — measured 0.99 at
      N=1, and it is the control that makes the rest of the sweep believable.
- [ ] **Also measure against `jsonb_array_update_where_batch`, not only the chain.**
      Added after the first harness run showed why: see "Result 3" below. Omitting it
      would let the feature be described by its most flattering comparison.

A runnable, self-contained comparison already ships on the PR branch at
`test/benchmark_changeset.sql` (asserts chained ≡ changeset byte-for-byte before
timing). It reports a ratio only; wire it into the Phase 4 harness for the
median/p95, named-machine numbers.

## Preliminary reading — NOT publication-grade (does not meet this phase's criteria)

Measured on the unnamed dev VPS, **release** build (opt-level 3 + fat LTO),
best-of-6, 1000-element doc. This is *not* the N≥10 median/p95 named-machine run
this phase requires; it is recorded only to save re-derivation and to close one
specific worry — that the speedup is a debug-build artifact. It is not:

| N (coalesced edits) | chained smart_patch (ms) | apply_changeset (ms) | ratio |
|--:|--:|--:|--:|
| 5  | 7.0  | 1.47 | 4.8× |
| 20 | 28.8 | 1.59 | 18.1× |
| 50 | 86.6 | 2.16 | 40.1× |

The debug build gave 4.8× / 18.4× / 41.4× on the same script — i.e. the ratio is
essentially build-invariant, because both arms are dominated by the same
whole-document serialization that release speeds up proportionally. Conclusion:
the coalescing advantage survives release; it is the *absolute* milliseconds that
must be regenerated under the real harness, per this phase's rule (argue from
ratios measured in the same run on the same host, never absolute ms).

## Under the harness (2026-07-20) — still dev-host, but now instrument-grade

The run above has now been redone through `test/bench/harness.sql`: 3 warm-up +
10 measured trials, interleaved arms, median and p95, byte-identical output
asserted per arm, calibration gate passed first. Branch `bench/integration`
(PR #16 tip + harness); scenarios in `test/bench/scenarios_changeset.sql`;
artifact `benchmarks/2026-07-20-changeset-derisk-devhost.{md,csv}` with raw
per-trial timings retained.

It **independently reproduces the numbers above** — 4.50× / 17.99× / 41.82× at
N = 5 / 20 / 50 (500-element array), against 4.8× / 18.1× / 40.1× best-of-6. The
build-invariance conclusion stands, now on N≥10 order statistics rather than a
best-of. The ratio is linear in N and 0.99 at N=1.

### Result 3 — against the strongest baseline it is parity, and that changes the claim

`jsonb_array_update_where_batch` already applies N integer-keyed updates to one
array in a single parse/serialize pass, and is on paper the better algorithm (one
HashMap-driven pass, where a changeset rescans the array once per op). Measured
against it, the changeset is at **parity**: 1.03 (500/N=5), 0.94 (500/N=50),
1.07 (5000/N=50).

So the honest framing is narrower than "coalescing is a win." Coalescing is a win
**against a chain**; against the best tool already in the box for that exact
operation it is a wash, and the changeset's real advantage is coverage —
heterogeneous ops, several paths in one pass, and non-integer match keys, which
`batch` cannot express at all (it reads `match_value` via `as_i64` and silently
skips anything else).

Publishing the chained ratio alone would be true and misleading, because a reader
doing precisely the benchmarked operation should reach for `batch` and would
measure parity. That is the same failure mode #15 exists to correct, so both
baselines ship together with N and array size attached, or neither does.

## Do not merge PR #16 until

1. ~~**Phase 2 lands.**~~ **Satisfied 2026-07-20** — Phase 2 merged to `main` as
   PR #21 and #14 is closed. PR #16 now sits directly on top of it, and its diff
   has shrunk to its four feature commits.
2. **Phase 4 produces the real ratios.** The PR's README/CHANGELOG already have the
   unsourced figure removed and point at this methodology; restore a number only
   once it comes from this harness.
3. **pg_tviews#50 confirms the array-match / nested-path semantics.** PR #16 decided
   them provisionally across 11 ops — that is Phase 3 Cycle 2–3 territory and must
   not be treated as settled (per Phase 3's "do not guess" rule).

A running comment on PR #16 (draft) records the same sequencing and the
preliminary numbers above, so the two artifacts do not drift.
