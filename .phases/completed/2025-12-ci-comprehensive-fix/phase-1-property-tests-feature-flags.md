# Phase 1: Fix Property Tests Feature Flag Issue

## Objective

Fix property-based tests CI failure caused by missing PostgreSQL version feature flag in cargo test commands. The tests compile pgrx-pg-sys without specifying which PostgreSQL version to use, causing "$PGRX_HOME does not exist" error.

## Context

**Current State:**
- Property tests job successfully installs PostgreSQL 17
- Property tests job successfully installs cargo-pgrx and initializes pgrx
- Script `run_property_tests.sh` runs `cargo test --release` without feature flags
- Compilation fails because pgrx-pg-sys doesn't know which PostgreSQL version to use

**Root Cause:**
The project uses `pgrx` with multiple PostgreSQL version support. The default features are disabled in `Cargo.toml`, requiring explicit feature selection (pg13, pg14, pg15, pg16, pg17, pg18).

When `cargo test --release` runs without `--no-default-features --features pg17`, it tries to build with default features (which are disabled), causing pgrx-pg-sys to fail finding the PostgreSQL configuration.

**Error Pattern:**
```
error: failed to run custom build command for `pgrx-pg-sys v0.16.1`
Error: $PGRX_HOME does not exist
Process completed with exit code 101.
```

**Why This Happens:**
1. `cargo test --release` compiles the project
2. pgrx-pg-sys build script looks for PostgreSQL configuration
3. Without feature flag, it doesn't know which pg version in `~/.pgrx/` to use
4. Build fails even though pgrx was initialized correctly

## Files to Modify

1. `scripts/run_property_tests.sh` - Add feature flags to all cargo test commands

## Implementation Steps

### Step 1: Add Feature Flags to All Cargo Test Commands

Update each `cargo test` command in `run_property_tests.sh` to include feature flags:

**Find this pattern:**
```bash
QUICKCHECK_TESTS=$ITERATIONS cargo test --release property_tests::prop_merge_associative -- --nocapture
```

**Replace with:**
```bash
QUICKCHECK_TESTS=$ITERATIONS cargo test --release --no-default-features --features pg17 property_tests::prop_merge_associative -- --nocapture
```

**Apply to all 8 test commands:**

```bash
#!/bin/bash

set -e

echo "🧪 Running property-based tests for jsonb_ivm..."
echo "==============================================="

# Default iterations if not specified
ITERATIONS=${1:-10000}

echo "🎯 Running QuickCheck tests with $ITERATIONS iterations per property"
echo ""

# Feature flags for pgrx (must match CI PostgreSQL version)
CARGO_FEATURES="--no-default-features --features pg17"

# Run property tests with specified iterations
echo "🔬 Testing merge operation properties..."
QUICKCHECK_TESTS=$ITERATIONS cargo test --release $CARGO_FEATURES property_tests::prop_merge_associative -- --nocapture
QUICKCHECK_TESTS=$ITERATIONS cargo test --release $CARGO_FEATURES property_tests::prop_merge_identity -- --nocapture
QUICKCHECK_TESTS=$ITERATIONS cargo test --release $CARGO_FEATURES property_tests::prop_merge_idempotence -- --nocapture
QUICKCHECK_TESTS=$ITERATIONS cargo test --release $CARGO_FEATURES property_tests::prop_merge_commutative_shallow -- --nocapture

echo ""
echo "🔬 Testing depth validation properties..."
QUICKCHECK_TESTS=$ITERATIONS cargo test --release $CARGO_FEATURES property_tests::prop_depth_validation_rejects_deep_jsonb -- --nocapture
QUICKCHECK_TESTS=$ITERATIONS cargo test --release $CARGO_FEATURES property_tests::prop_depth_validation_accepts_shallow_jsonb -- --nocapture

echo ""
echo "🔬 Testing array operation properties..."
QUICKCHECK_TESTS=$ITERATIONS cargo test --release $CARGO_FEATURES property_tests::prop_array_update_preserves_length -- --nocapture

echo ""
echo "🔬 Testing path navigation properties..."
QUICKCHECK_TESTS=$ITERATIONS cargo test --release $CARGO_FEATURES property_tests::prop_path_navigation_consistent -- --nocapture

echo ""
echo "✅ All property tests completed successfully!"
echo "=============================================="
echo "📊 Test Results:"
echo "   - Iterations per property: $ITERATIONS"
echo "   - Properties tested: 8"
echo "   - Total test cases: $((ITERATIONS * 8))"
echo ""
echo "🎯 Mathematical correctness verified through property-based testing!"
```

**Why this works:**
- `--no-default-features`: Disables default features (matches Cargo.toml config)
- `--features pg17`: Explicitly selects PostgreSQL 17 support
- Matches the feature flag used in the integration test jobs
- Uses the same PostgreSQL version we initialized pgrx with

### Step 2: Make Script PostgreSQL Version Agnostic (Optional Enhancement)

For better maintainability, make the script detect or accept PostgreSQL version:

```bash
#!/bin/bash

set -e

echo "🧪 Running property-based tests for jsonb_ivm..."
echo "==============================================="

# Default iterations if not specified
ITERATIONS=${1:-10000}

# PostgreSQL version (default to 17, can be overridden)
PG_VERSION=${PG_VERSION:-17}

echo "🎯 Running QuickCheck tests with $ITERATIONS iterations per property"
echo "📦 Using PostgreSQL $PG_VERSION features"
echo ""

# Feature flags for pgrx
CARGO_FEATURES="--no-default-features --features pg${PG_VERSION}"

# Rest of script unchanged...
```

**Benefits:**
- Can test with different PostgreSQL versions locally
- Matches CI environment expectations
- Self-documenting (shows which PG version is used)

### Step 3: Verify Locally

Test the script with the new feature flags:

```bash
# Clean build to ensure fresh compilation
cargo clean

# Initialize pgrx if not already done
cargo pgrx init --pg17=$(which pg_config)

# Run property tests with new script
./scripts/run_property_tests.sh 1000

# Should compile and run successfully
```

## Verification Commands

**Local verification:**
```bash
# Test with small iteration count first
./scripts/run_property_tests.sh 100

# Expected output:
# 🧪 Running property-based tests for jsonb_ivm...
# 🎯 Running QuickCheck tests with 100 iterations per property
# 🔬 Testing merge operation properties...
# [Compilation messages...]
# running 1 test
# test property_tests::prop_merge_associative ... ok
# [More tests...]
# ✅ All property tests completed successfully!

# Test with full iteration count
./scripts/run_property_tests.sh 10000
```

**CI verification:**
```bash
# After committing, check the workflow
gh run list --limit 1

# Watch the property-tests job
gh run watch <run-id>

# Should see:
# ✓ Install PostgreSQL 17
# ✓ Install cargo-pgrx
# ✓ Initialize pgrx
# ✓ Run property tests
#   🧪 Running property-based tests...
#   Compiling jsonb_ivm v0.1.0
#   [All 8 tests pass]
#   ✅ All property tests completed successfully!
```

**Verify feature flags are applied:**
```bash
# Check compilation uses correct features
QUICKCHECK_TESTS=10 cargo test --release --no-default-features --features pg17 property_tests::prop_merge_associative -v

# Should see in output:
# Compiling pgrx-pg-sys v0.16.1 (with features: pg17)
# Compiling jsonb_ivm v0.1.0 (with features: pg17)
```

## Acceptance Criteria

- [ ] All `cargo test` commands in script include `--no-default-features --features pg17`
- [ ] Property tests compile successfully without "$PGRX_HOME does not exist" error
- [ ] All 8 property tests run with 10000 iterations each
- [ ] Script completes successfully in CI (property-tests job passes)
- [ ] Local testing works with the modified script
- [ ] Feature flags are consistent with CI environment (PostgreSQL 17)
- [ ] Job completes in reasonable time (< 4 minutes in CI)

## DO NOT

- Do NOT change the test code itself - only the build flags
- Do NOT reduce the number of QuickCheck iterations (keep 10000 for thorough testing)
- Do NOT hardcode feature flags in Cargo.toml - keep them as CLI arguments
- Do NOT remove any of the 8 property tests - they provide valuable correctness guarantees
- Do NOT use default features - the project explicitly disables them for multi-version support

## Notes

**Why pgrx requires explicit feature selection:**

The project `Cargo.toml` has:
```toml
[features]
default = []
pg13 = ["pgrx/pg13"]
pg14 = ["pgrx/pg14"]
pg15 = ["pgrx/pg15"]
pg16 = ["pgrx/pg16"]
pg17 = ["pgrx/pg17"]
pg18 = ["pgrx/pg18"]
```

With `default = []`, cargo doesn't know which PostgreSQL version to compile for. The `--features pg17` flag tells pgrx to use PostgreSQL 17 bindings from `~/.pgrx/17.2/`.

**Why this wasn't caught in development:**

- Local development likely uses `cargo pgrx test` which automatically handles features
- Or developers have `PGRX_PG_CONFIG_PATH` environment variable set
- The script worked before refactoring because it wasn't being used in CI

**Difference from integration tests:**

Integration tests in workflow use:
```bash
cargo build --release --locked --no-default-features --features pg17
```

Property tests should use the same pattern:
```bash
cargo test --release --no-default-features --features pg17 [test-name]
```

**Performance impact:**

Adding feature flags doesn't slow down compilation - it's required anyway. The script will still take ~2-3 minutes in CI:
- First test: ~1.5 min (compilation)
- Subsequent tests: ~15-20 sec each (already compiled)
- Total: ~2.5-3 minutes for all 8 tests

**Alternative considered but rejected:**

Setting default features to pg17 in Cargo.toml:
- ❌ Would break multi-version support
- ❌ Would require changes when testing different PostgreSQL versions
- ❌ Doesn't follow pgrx best practices
- ✅ CLI flags are more explicit and flexible
