# Phase 2: Version Coherence and Upgrade Path

Resolves: #14

## Objective

Make the shipped extension self-report the version of the code it actually
contains, and give existing 0.1.0 installs a supported upgrade path.

## Success Criteria

- [ ] `SELECT extversion FROM pg_extension WHERE extname='jsonb_delta'` returns `0.2.0`
- [ ] `sql/jsonb_delta--0.2.0.sql` exists and is pgrx-generated
- [ ] `sql/jsonb_delta--0.1.0--0.2.0.sql` exists and applies cleanly
- [ ] `ALTER EXTENSION jsonb_delta UPDATE` from a real 0.1.0 install succeeds
- [ ] CI fails if `Cargo.toml` version ≠ `jsonb_delta.control` `default_version`

## Prerequisite: resolve the pgrx bump

`Cargo.toml` carries an uncommitted `pgrx 0.16.1 → 0.17.0` bump. `cargo check
--no-default-features --features pg17` passes. Decide and commit **before**
regenerating SQL, because `just schema` output is toolchain-dependent and we do
not want an unreviewed pgrx upgrade smuggled into the shipped contract.

Recommendation: commit the bump as its own `chore(deps)` commit, run the full
test suite against it, *then* regenerate. If anything is off, revert to 0.16.1 —
the version fix does not require the bump.

## Design decisions

**What is the contract?** The `.sql` script is what users get; the `.so` is what
runs. Today they disagree in label only — no function signature differs between
the 0.1.0 script and 0.2.0 code (verified: 15 exports, all present). So the
0.1.0→0.2.0 upgrade script is close to a no-op plus any function-body changes,
which for C-language functions means `CREATE OR REPLACE FUNCTION` re-pointing at
the same symbols. Keep it explicit rather than empty so the upgrade is auditable.

**Diff before shipping.** Regenerating with pgrx will produce a 0.2.0 script;
diff it against `jsonb_delta--0.1.0.sql`. Any signature change that appears is a
silent breaking change shipped between releases and must be called out in
CHANGELOG — this is exactly the class of bug #12 is.

## TDD Cycles

### Cycle 1: Version guard
- **RED**: Add `tests/version_coherence.rs` asserting `env!("CARGO_PKG_VERSION")`
  equals the `default_version` parsed from `jsonb_delta.control`. Fails (0.2.0 vs 0.1.0).
- **GREEN**: Set `default_version = '0.2.0'` in `jsonb_delta.control`.
- **REFACTOR**: Factor the control-file parse into a small helper reusable by CI.
- **CLEANUP**: Wire the test into `just ci`.

### Cycle 2: Ship the 0.2.0 script
- **RED**: Test asserts `sql/jsonb_delta--{CARGO_PKG_VERSION}.sql` exists. Fails.
- **GREEN**: `cargo pgrx schema > sql/jsonb_delta--0.2.0.sql`.
- **REFACTOR**: Fix `justfile:schema`, which hardcodes `sql/jsonb_delta--0.1.0.sql`
  — it would overwrite the old script on every run. Derive the version from
  `Cargo.toml`.
- **CLEANUP**: Decide whether `--0.1.0.sql` stays in-tree (yes: needed for
  reinstall of the older label and to apply the upgrade script against).

### Cycle 3: Upgrade path
- **RED**: Integration test: install 0.1.0, `ALTER EXTENSION jsonb_delta UPDATE TO '0.2.0'`,
  assert `extversion` = 0.2.0 and all 15 functions resolve. Fails — no upgrade script.
- **GREEN**: Write `sql/jsonb_delta--0.1.0--0.2.0.sql`.
- **REFACTOR**: Diff the generated 0.2.0 script against 0.1.0; fold any real
  signature deltas into the upgrade script and CHANGELOG.
- **CLEANUP**: Document the release checklist (bump Cargo.toml → bump control →
  regenerate → write upgrade script) in `CONTRIBUTING.md`.

## Dependencies

- Requires: none
- Blocks: nothing (but Phase 3's overload must land in the 0.2.0 script if both
  ship in the same release — sequence Phase 3 before cutting the release)

## Status
[ ] Not Started
