# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.1]

> A packaging and compatibility-matrix release. **The compiled extension does
> not change** — the shared object, every function signature, volatility,
> strictness and parallel-safety are byte-for-byte those of 0.3.0, and `ALTER
> EXTENSION jsonb_delta UPDATE TO '0.3.1'` is a no-op beyond advancing the
> version label. What changes is the set of PostgreSQL versions we build for
> (see _Changed_) and the release automation (see _Fixed_).

### Changed
- **PostgreSQL 13 dropped — jsonb_delta 0.3.x requires PostgreSQL 14+.** The
  0.3.0 binary rewrite uses `JsonbToJsonbValue`, which was added to PostgreSQL's
  C API in version 14, so 0.3.x never compiled on PostgreSQL 13. (This went
  unnoticed because the 0.3.0 release build had already failed for an unrelated
  reason — the `cliff.toml` gap fixed under #24 — masking the PG13 compile
  error underneath.) PostgreSQL 13 reached end-of-life in November 2025. It is
  removed from the CI build and test matrices and from `docs/COMPATIBILITY.md`;
  PostgreSQL 13 users should remain on jsonb_delta 0.2.x.

### Fixed
- **Release automation now produces a GitHub Release with prebuilt packages**
  (#24). The 0.3.0 tag's release workflow failed on a `generate-changelog` job
  that referenced a `cliff.toml` which never existed in the repo, so 0.3.0 had
  to be published by hand and shipped source-only. The workflow now sources
  release notes from the `## [x.y.z]` section of this file and depends only on
  the build job, so the `pg13`–`pg17` tarballs are attached to the Release.
  0.3.1 is cut specifically to exercise that fix and ship the binary packages
  the manually-cut 0.3.0 Release lacked.

### Documentation
- Added a **consumer-compatibility note** for `pg_tviews` to
  `docs/COMPATIBILITY.md`: the only symbol it depends on at runtime is
  `jsonb_smart_patch_scalar(jsonb, jsonb)` (frozen by the contract test in
  `src/contract.rs`), and it degrades gracefully when jsonb_delta is absent.

## [0.3.0]

> The binary-JSONB rewrite. Versioned **0.3.0** — a distinct label from the
> serde **v0.2.0** tag that issue #15 measured — because the implementation
> changed substantially even though the SQL contract did not. See the coherence
> note under _Fixed_.

### Performance
- **Ten functions now operate on PostgreSQL's binary JSONB representation instead of round-tripping the document through `serde_json`.** Measurement showed the round trip *was* essentially the entire cost: on a 1000-element document a no-op parse-and-re-serialize took 2.45 ms while a real update took 2.25 ms, so the matching and mutation these functions perform were not measurable next to the conversion. Worse, the conversion ran through a *text* form — `pgrx::JsonB` renders the document with `jsonb_out`, parses that with `serde_json`, and reverses both steps on return. Walking the binary form lets untouched keys and array elements pass through as pointers into the source document, so cost now tracks what actually changes rather than document size.

  Measured against the native SQL each function is documented as replacing (release build, 1000-element array, medians of 10 trials):

  | Function | Before | After |
  |---|---|---|
  | `jsonb_array_update_where` | 1.34× | **3.40×** |
  | `jsonb_array_delete_where` | 1.35× | **3.55×** |
  | `jsonb_merge_shallow` | 0.31× (slower than `\|\|`) | **1.01× (parity)** |
  | `jsonb_array_contains_id` | — | **6.17×** vs the previous implementation |
  | `jsonb_extract_id` | — | **6.05×** vs the previous implementation |

  The ratio for `update` and `delete` now **rises** with array size (2.6× → 3.4× from 10 to 1000 elements) where previously it fell (2.0× → 1.3×). Falling with size was the substance of the complaint in #15.

  Also ported: `jsonb_smart_patch_scalar`, `jsonb_smart_patch_array`, `jsonb_smart_patch_nested`, `jsonb_merge_at_path`, `jsonb_array_update_where_batch`. The SQL API is unchanged — same names, arguments, volatility and strictness — so no migration is required.

- **Measured on a reproducible machine profile** (issue #15). Rented-host numbers (Hetzner CCX13, PostgreSQL 17.10, release build) keyed to a machine a third party can rent and re-run are in `benchmarks/2026-07-21-issue15-ccx13.md`; `README.md` and `docs/PERFORMANCE.md` now trace to that artifact. The reporter was right about serde v0.2.0 — single-element update/delete was at parity-to-slower and the ratio *fell* with array size. The binary build removes the whole-document round trip the reporter identified: update/delete are now **1.6–3.9×** faster than native re-aggregation and the win **holds or rises** with array size; `jsonb_merge_shallow` is at **parity** with `||` (it is not a speedup over a built-in and is no longer advertised as one); coalescing many edits into one `jsonb_apply_changeset` is a large win (up to ~21×) at high op counts and a small loss at one or two.

### Fixed
- **`jsonb_array_update_where` and friends now match numbers by value rather than by representation.** A `match_value` of `2.0` previously failed to match an element whose id was `2`, because the comparison went through `serde_json::Number`. SQL considers them equal (`'2'::jsonb = '2.0'::jsonb` is true), and so does containment, so a caller passing `to_jsonb(2.0)` or a numeric column silently matched nothing.
- **`jsonb_array_update_where_batch` can now match text and UUID keys.** It read `match_value` with `as_i64` and silently dropped every spec that was not an integer, so non-integer keys could not be batched at all.
- **`jsonb_extract_id` no longer loses numeric precision.** It parsed through `f64`, rendering `1.50` as `1.5`. It now returns `1.50`, agreeing with the `->>` operator.

  All three affect only inputs that previously matched nothing or returned a lossy value; no call that worked before changes behaviour.

### Added
- **`jsonb_apply_changeset(doc, ops)`** — apply an ordered list of surgical edits to a JSONB document in a **single parse/serialize pass**. `ops` is a JSONB array of typed operations: `set`, `remove`, `merge`, `deep_merge`, `increment`, `array_update`, `array_update_all`, `array_replace`, `array_upsert`, `array_delete`, `array_insert`. Paths may be dot-notation strings (`"a.b[0].c"`) or segment arrays (`["a", "b", 0, "c"]`), and array matching works for any key type (int / text / **UUID**). Intended for incremental-view-maintenance callers (e.g. `pg_tviews`) that coalesce many changes to one row per transaction: replacing a chain of N `jsonb_smart_patch_*` calls with a single `jsonb_apply_changeset` amortizes the whole-document (de)serialization across the entire changeset, so the advantage grows with the number of coalesced edits. Quantified speedups are pending measurement under the project's benchmark methodology (release build, median/p95); see `test/benchmark_changeset.sql`.

### Changed
- Toolchain: pgrx 0.16.1 → 0.17.0 (first pgrx with PostgreSQL 18 support);
  all cargo-pgrx pins in CI, Docker and the justfile moved with it.

### Fixed
- **Version coherence** (#14, extended for the rewrite): the crate, the control
  file, and the shipped SQL script all read the same version. #14 fixed the
  original 0.1.0/0.2.0 mismatch; this release advances the label to **0.3.0** so
  the binary rewrite is not conflated with the serde code the **v0.2.0** tag and
  issue #15 refer to.
- Shipped `sql/jsonb_delta--0.3.0.sql` (pgrx-generated) as the canonical install
  script for the crate version. Its SQL contract is byte-identical to
  `--0.2.0.sql` with comments stripped — the rewrite changed the shared object,
  not the catalog.
- Added `sql/jsonb_delta--0.2.0--0.3.0.sql` (and kept `--0.1.0--0.2.0.sql`) so
  any earlier install can `ALTER EXTENSION jsonb_delta UPDATE`. The full
  `0.1.0 → 0.2.0 → 0.3.0` chain is covered by `test/upgrade_path_test.sql` in CI
  and `just test-upgrade` locally.
- `just schema` derives the script name from the crate version instead of
  hardcoding it, and a version-guard test (`tests/version_coherence.rs`) fails
  the build if `Cargo.toml` and the control file ever disagree again.
- **Exported-signature contract test** (#12): `src/contract.rs` freezes every
  exported function signature against `pg_proc`, so a downstream consumer can
  never again discover a signature drift at runtime the way pg_tviews did.
  pg_tviews resolved its side (fraiseql/pg_tviews#50) by removing the 4-arg
  call entirely, so **no overload is added here**; `jsonb_smart_patch_scalar(jsonb, jsonb)`,
  the one symbol it still calls, is pinned hardest.
- **Benchmark suite runs again** (#13): the harness under `test/bench/` and the
  fixture ordering were repaired — the scripts referenced the pre-rename
  `jsonb_ivm` extension and an undocumented three-step fixture dependency, so the
  suite had been unrunnable since the rename.

### Security
- **Path segment-count cap**: `jsonb_apply_changeset` rejects op paths with more than `MAX_JSONB_DEPTH` (1000) segments, preventing construction of documents deeper than the depth cap (which would otherwise feed serde's unbounded output-serialization recursion). Changeset size is also capped at 10,000 ops per call.
- **Overflow-checked `increment`**: integer increments use checked arithmetic and raise an error on overflow instead of silently wrapping.

## [0.2.0] - 2024-04-17

### Security
- **Array Bounds Protection**: Added array index cap (`MAX_JSONB_ARRAY_SIZE = 100,000`) to prevent OOM attacks via large index padding in `jsonb_delta_set_path` and `jsonb_delta_array_update_where_path`.
- **Input Validation**: Added `match_key` non-empty validation to all 7 array-matching functions (`jsonb_array_update_where`, `jsonb_array_delete_where`, `jsonb_array_insert_where`, `jsonb_array_update_where_batch`, `jsonb_array_update_multi_row`, `jsonb_smart_patch_array`, `jsonb_delta_array_update_where_path`).
- **Path Security**: Added path key-segment length cap (`MAX_KEY_LENGTH = 256` bytes) in `parse_path()` to prevent unbounded memory allocation.
- **Depth Protection**: Added JSONB nesting depth validation (max 1,000 levels) to prevent stack overflow attacks.

### Performance
- **Binary Search Optimization**: `find_insertion_point()` now uses binary search (`partition_point`) for O(log n) complexity down from O(n), significantly improving sorted array insertions.
- **SIMD Integer Matching**: Leverages auto-vectorization for integer ID lookups, optimized for the trinity pattern (`id` UUID / `pk_{entity}` BIGINT / `fk_{entity}` BIGINT / `identifier` text).
- **Helper Consolidation**: Removed duplicate code paths, reducing compilation overhead and improving maintainability.

### Developer Experience
- **Comprehensive Testing**: Added 34 unit tests, property-based fuzzing, and SQL integration tests covering all functions and edge cases.
- **Error Messages**: Improved error messages with specific values (actual depth found, key lengths, etc.) for better debugging.
- **Documentation**: Added detailed API documentation with security limits and usage examples.

### Fixed
- Depth validation error now reports the actual depth found instead of generic `>max`.
- Consolidated duplicate helper functions (`value_type_name`, `find_element_by_match`) across modules.

### Changed
- Simplified GitHub Actions CI workflow (removed macOS, platform detection logic)
- Expanded PostgreSQL test matrix from PG17 only to PG13-17

## [0.1.0] - 2024-12-17

### Added
- Initial release
- `jsonb_delta()` function to compute efficient deltas between JSONB values
- `jsonb_patch()` function to apply deltas to JSONB values
- Support for PostgreSQL versions 13-18
- Comprehensive test suite
- SQL integration tests
- Property-based fuzzing tests
- Load/performance tests
- Security scanning and compliance checks

### Features
- Efficient delta computation with minimal output size
- Support for nested objects and arrays
- Handles all JSONB value types (objects, arrays, strings, numbers, booleans, null)
- Idempotent patch application
- Round-trip guarantee: patch(original, delta(original, modified)) = modified

[0.3.0]: https://github.com/evoludigit/jsonb_delta/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/evoludigit/jsonb_delta/releases/tag/v0.2.0
[0.1.0]: https://github.com/evoludigit/jsonb_delta/releases/tag/v0.1.0
