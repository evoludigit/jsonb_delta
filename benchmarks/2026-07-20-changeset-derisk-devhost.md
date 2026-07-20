# jsonb_apply_changeset — de-risking run (developer host, NOT publishable)

**Status: not a publishable artifact.** Taken on a developer workstation, which
Phase 4 explicitly disqualifies as a measurement host. It exists to answer one
question before any money is spent on a rented instance: does the coalescing
story survive a release build and a calibrated instrument? It does, with one
important correction to how it should be described.

Everything below is a **ratio measured between two arms in the same run on the
same host**, which is the only quantity a dev machine can report honestly.
Absolute milliseconds are recorded for completeness and should not be quoted.

## Environment

| | |
|---|---|
| Host | 13th Gen Intel Core i7-13700K, 24 cores, governor `powersave` |
| PostgreSQL | 18.1 (pgrx-managed scratch cluster, `shared_preload_libraries=''`) |
| Extension | jsonb_delta 0.2.0 @ `bbb6181`, **release profile** (`opt-level=3`) |
| Harness | `test/bench/harness.sql`, 3 warm-up + 10 measured trials, interleaved arms |
| Scenarios | `test/bench/scenarios_changeset.sql` |

The cluster is `pg_tviews`-free by construction: that extension's `ProcessUtility`
hook fires on `tv_`-prefixed fixtures, and an extension hooking utility statements
has no business being in the loop while utility statements are timed.

All 14 non-control scenarios reported `outputs_match = true`. Every ratio below is
between two arms that produced a byte-identical document; the harness withholds a
ratio otherwise.

## Result 1 — the withdrawn "4.8×–40×" reproduces in release

The figure removed in `68ff374` as unsourced was re-measured at its own four
configurations, under the harness, in a release build:

| array | N ops | withdrawn (debug, single-shot) | measured (release, median of 10) |
|---|---|---|---|
| 500 | 5 | 4.8× | **4.50×** |
| 500 | 20 | 17.6× | **17.99×** |
| 500 | 50 | 38.3× | **41.82×** |
| 5000 | 50 | 40.3× | **41.81×** |

**The standing hypothesis was wrong.** The handoff expected the 40× to shrink
materially in release, on the theory that debug builds inflate serde
disproportionately. It does not shrink — it holds, and at high N it is slightly
larger. Release made both arms faster in absolute terms (the 5000/50 chain went
1153.8 ms → 761.0 ms) but it sped up both arms by about the same factor, and a
ratio is indifferent to that.

The 40× was never a debug artifact. It was simply N=50: the chain pays whole-
document (de)serialization once per edit, the changeset pays it once in total, so
the ratio tracks N.

## Result 2 — the ratio is a straight line in N, and is 1.0 at N=1

200-post document, chained `jsonb_smart_patch_array` vs one changeset:

| N ops | chain (ms) | changeset (ms) | ratio |
|---|---|---|---|
| 1 | 0.613 | 0.617 | **0.99** |
| 2 | 1.167 | 0.643 | 1.82 |
| 4 | 2.307 | 0.631 | 3.66 |
| 8 | 4.500 | 0.652 | 6.90 |

The changeset arm is flat in N (0.617 → 0.652 ms) while the chain is linear. The
N=1 row is the load-bearing control: at one op both arms do exactly one
parse/serialize, and the measured ratio is 0.99. An instrument that reported the
changeset as fast there would be measuring something other than coalescing, and
nothing else in this file could be trusted.

Holding N=4 and growing the document (10 → 100 → 1000 posts) moves the ratio
2.17 → 3.29 → 3.69, approaching N as fixed per-call overhead is diluted. Both
sweeps agree with the same one-line model: ratio → N as the document grows.

## Result 3 — against the strongest baseline, it is parity

This is the result that should govern how the feature is described.

For the shape of edit used above — N updates, one array, integer match key —
`jsonb_array_update_where_batch` already does the job in a single pass, and is on
paper the better algorithm (one HashMap-driven pass over the array, where a
changeset rescans the array once per op).

| array | N ops | batch (ms) | changeset (ms) | ratio |
|---|---|---|---|---|
| 500 | 5 | 1.516 | 1.475 | **1.03** |
| 500 | 50 | 1.603 | 1.701 | **0.94** |
| 5000 | 50 | 15.922 | 14.816 | **1.07** |

Parity, and the theoretical O(N × array) disadvantage never shows up — serde still
dominates matching at these sizes.

So `jsonb_apply_changeset` is **not faster than the best tool already in the box**
for homogeneous integer-keyed array updates. Its advantage over `batch` is
coverage, not speed:

- heterogeneous ops (`set`, `merge`, `increment`, `array_delete`, …) in one pass
- several different paths in one pass
- non-integer match keys — `batch` reads `match_value` via `as_i64` and silently
  skips anything else, so UUID and text keys cannot use it at all

## What this means for the published claim

A "41× faster" headline would be true against the chain and misleading as a
description of the feature, because a reader doing exactly the benchmarked
operation should reach for `batch` and would measure parity. Quoting the chained
ratio alone would reproduce, in a new place, the overclaiming that issue #15 is
about.

The defensible statement is about the mechanism and the conditions:

> Replacing a chain of N `jsonb_smart_patch_*` calls with one
> `jsonb_apply_changeset` amortizes whole-document (de)serialization across the
> changeset, so the speedup grows with N — measured ≈N× on this host, e.g. 41×
> at N=50. Where a single-pass alternative already exists
> (`jsonb_array_update_where_batch`, integer keys, one array), the changeset is
> at parity; its gain there is that it also covers heterogeneous ops, multiple
> paths, and UUID/text match keys.

Both families must be quoted together, with N and array size attached. Numbers
fit to publish still require the rented-host run (Phase 4 Cycle 2/3).

## Reproducing

```sh
initdb -D "$SCRATCH" -U postgres
pg_ctl -D "$SCRATCH" -o "-p 28900 -c shared_preload_libraries= \
  -c unix_socket_directories=/tmp/jdb-sock -c listen_addresses=" start
cargo pgrx install --release --no-default-features --features pg18 \
  --pg-config ~/.pgrx/18.1/pgrx-install/bin/pg_config
createdb bench
psql -d bench -c 'CREATE EXTENSION jsonb_delta;' \
  -f test/bench/harness.sql -f test/bench/harness_test.sql \
  -f test/bench/scenarios_changeset.sql
psql -d bench -c 'SELECT count(*) FROM bench.run_all();'
psql -d bench -c 'SELECT * FROM bench.report;'
```

`harness_test.sql` must pass first; it is the calibration gate on the instrument,
and its eight controls establish that the harness can detect a slowdown, detect a
speedup, and withhold a ratio when the arms disagree.
