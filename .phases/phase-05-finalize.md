# Phase 5: Finalize

## Objective

Close out the four issues and return the repository to a shippable,
archaeology-free state.

## Steps

### 1. Quality Control Review
- [ ] Benchmark harness API is coherent; adding a scenario is one table row
- [ ] Error messages name the script/command that actually resolves them
- [ ] No new public API surface beyond what #12's resolution required
- [ ] Release checklist in CONTRIBUTING.md prevents recurrence of #14

### 1b. Cloud Resource Teardown
- [ ] `hcloud server list` shows no benchmark hosts still running
- [ ] No orphaned volumes, floating IPs, or firewalls from the Phase 4 measurement
- [ ] Any SSH key added for the benchmark host removed if it was created for it
- [ ] Decide whether `scripts/provision_bench_vps.sh` ships (it makes the published
      numbers reproducible, which argues for keeping it) or is development
      archaeology to remove — if it ships, it must contain no account identifiers,
      tokens, or context names

### 2. Security Audit
- [ ] Nested-path recursion (if Phase 3 Cycle 3 landed) respects SECURITY.md depth bounds
- [ ] `validate_match_key` still covers any new entry point into the patch path
- [ ] Benchmark fixture scripts create no world-readable or persistent artifacts
- [ ] pgrx 0.17.0 bump reviewed for advisories (`cargo audit`, `cargo deny`)
- [ ] No Hetzner token, SSH private key, or `hcloud` context leaked into the
      provisioning script, the results artifact, or CI configuration

### 3. Archaeology Removal
- [ ] Remove `.phases/` from the main branch
- [ ] No `// Phase N:` / `TODO: Phase` markers introduced
- [ ] `grep -rn "jsonb_ivm"` returns hits only under `docs/archive/`
- [ ] No commented-out benchmark variants left behind

### 4. Documentation Polish
- [ ] README.md and docs/PERFORMANCE.md numbers all trace to a committed artifact
- [ ] CHANGELOG.md documents: version-label fix, upgrade script, benchmark repair,
      corrected performance claims, and any #12 API change
- [ ] Reproduction steps verified by running them on a clean machine

### 5. Issue Closure
- [ ] #13 closed — with the note that the table-rename fix would have broken
      `benchmark_baseline.sql`, and that the `jsonb_ivm` breakage was the larger cause
- [ ] #14 closed — with upgrade instructions for existing 0.1.0 installs
- [ ] #12 closed or handed to fraiseql/pg_tviews#50 with the contract test as the
      durable guard
- [ ] #15 closed — with measurements and whatever correction they warranted

### 6. Final Verification
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` clean
- [ ] `cargo test` and `cargo pgrx test` pass
- [ ] `just bench` runs clean from a fresh database
- [ ] `ALTER EXTENSION jsonb_delta UPDATE` verified from a real 0.1.0 install
- [ ] Release cut with Cargo.toml, control file, and SQL script all agreeing

## Status
[ ] Not Started
