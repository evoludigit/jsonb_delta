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

- [ ] `jsonb_apply_changeset` vs. chained `jsonb_smart_patch_array`, N ∈ {5, 20, 50},
      across the existing array-size sweep, integer and text/UUID keys.
- [ ] The same run must also show the single-edit case at parity — that is the
      honest half of the #15 answer and must be published alongside the win.

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

## Do not merge PR #16 until

1. **Phase 2 lands.** PR #16 regenerates `sql/jsonb_delta--0.1.0.sql` — the file
   whose version incoherence Phase 2 fixes. The regen must be redone on top of
   Phase 2, not before it.
2. **Phase 4 produces the real ratios.** The PR's README/CHANGELOG already have the
   unsourced figure removed and point at this methodology; restore a number only
   once it comes from this harness.
3. **pg_tviews#50 confirms the array-match / nested-path semantics.** PR #16 decided
   them provisionally across 11 ops — that is Phase 3 Cycle 2–3 territory and must
   not be treated as settled (per Phase 3's "do not guess" rule).

A running comment on PR #16 (draft) records the same sequencing and the
preliminary numbers above, so the two artifacts do not drift.
