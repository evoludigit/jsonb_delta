# Phase 1: Fix Property-Based Tests CI Failure

## Objective

Fix the property-based tests CI job failure caused by missing pgrx initialization. The tests are failing with `$PGRX_HOME does not exist` error because the workflow doesn't set up the pgrx environment before running the tests.

## Context

**Current State:**
- Property-based tests job in `.github/workflows/test.yml` only installs Rust toolchain
- Missing PostgreSQL installation and pgrx initialization
- Tests compile pgrx-pg-sys which requires `$PGRX_HOME` to be set

**Root Cause:**
The property tests script (`scripts/run_property_tests.sh`) runs `cargo test` which compiles the entire project including pgrx dependencies. Without pgrx initialization, the build fails during dependency compilation.

**Error Log:**
```
error: failed to run custom build command for `pgrx-pg-sys v0.16.1`
Error: $PGRX_HOME does not exist
Process completed with exit code 101.
```

## Files to Modify

1. `.github/workflows/test.yml` - Add pgrx setup steps to property-tests job

## Implementation Steps

### Step 1: Add PostgreSQL Installation

Add PostgreSQL installation step to property-tests job (after "Cache Rust dependencies"):

```yaml
- name: Install PostgreSQL 17
  run: |
    sudo apt-get install -y wget gnupg
    sudo sh -c 'echo "deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list'
    wget --quiet -O - https://www.postgresql.org/media/keys/ACCC4CF8.asc | sudo apt-key add -
    sudo apt-get update
    sudo apt-get install -y postgresql-17 postgresql-server-dev-17
```

**Why:** Property tests need PostgreSQL headers to compile pgrx-pg-sys.

### Step 2: Install cargo-pgrx

Add cargo-pgrx installation (after PostgreSQL installation):

```yaml
- name: Install cargo-pgrx
  run: cargo install --locked cargo-pgrx --version 0.16.1
```

**Why:** cargo-pgrx is required to initialize the pgrx development environment.

### Step 3: Initialize pgrx

Add pgrx initialization step (after cargo-pgrx installation):

```yaml
- name: Initialize pgrx
  run: cargo pgrx init --pg17=/usr/lib/postgresql/17/bin/pg_config
```

**Why:** This creates `$PGRX_HOME` (~/.pgrx) with PostgreSQL bindings and test databases.

### Step 4: Verify the Fix

The property tests should now compile successfully. The workflow will:
1. Install PostgreSQL 17 with development headers
2. Install cargo-pgrx tool
3. Initialize pgrx (creates ~/.pgrx with pg17 config)
4. Run property tests with QuickCheck

## Verification Commands

**Local verification:**
```bash
# Simulate CI environment
rm -rf ~/.pgrx
cargo pgrx init --pg17=$(which pg_config)
./scripts/run_property_tests.sh 1000
```

**Expected output:**
```
🧪 Running property-based tests for jsonb_ivm...
🎯 Running QuickCheck tests with 1000 iterations per property
🔬 Testing merge operation properties...
   Compiling jsonb_ivm v0.1.0 (/home/runner/work/jsonb_ivm/jsonb_ivm)
    Finished test [unoptimized + debuginfo] target(s) in XX.XXs
     Running unittests src/lib.rs (target/debug/deps/jsonb_ivm-XXXXX)

running 5 tests
test tests::property::prop_merge_associative ... ok
test tests::property::prop_merge_idempotent ... ok
...
✅ All property tests passed!
```

**CI verification:**
```bash
# After committing changes, check the workflow
gh run watch <run-id>

# Should see:
# ✓ Install PostgreSQL 17
# ✓ Install cargo-pgrx
# ✓ Initialize pgrx
# ✓ Run property tests
```

## Acceptance Criteria

- [ ] Property-based tests job installs PostgreSQL 17 with dev headers
- [ ] cargo-pgrx is installed in the workflow
- [ ] pgrx is initialized before running tests
- [ ] Property tests compile successfully without `$PGRX_HOME` errors
- [ ] All QuickCheck property tests pass in CI
- [ ] Job completes in reasonable time (< 5 minutes)

## DO NOT

- Do NOT remove the property tests - they provide valuable correctness guarantees
- Do NOT skip pgrx initialization - it's required for compilation
- Do NOT change the number of QuickCheck iterations (10000) - we want thorough testing
- Do NOT use a different PostgreSQL version - stay consistent with pg17

## Notes

**Why we need full pgrx setup for property tests:**
- Property tests are Rust unit tests (`#[quickcheck]` macros in `src/lib.rs`)
- They compile the entire crate including pgrx proc macros
- pgrx proc macros require PostgreSQL bindings at compile time
- These bindings are generated during `cargo pgrx init`

**Alternative approach considered but rejected:**
- Running property tests without pgrx setup: Not possible due to proc macro requirements
- Mocking pgrx dependencies: Would invalidate the tests (not testing real code)
- Moving property tests to separate crate: Too much refactoring, breaks project structure

**Performance consideration:**
- Installing cargo-pgrx takes ~2-3 minutes (downloads and compiles)
- This is acceptable for a comprehensive test suite
- cargo cache will help on subsequent runs
