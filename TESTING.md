# Testing Guide

Complete guide to testing the jsonb_delta PostgreSQL extension.

## Table of Contents

- [Quick Start](#quick-start)
- [Why `cargo test` Doesn't Work](#why-cargo-test-doesnt-work)
- [Test Types](#test-types)
- [Running Tests](#running-tests)
- [Writing Tests](#writing-tests)
- [CI/CD Testing](#cicd-testing)
- [Troubleshooting](#troubleshooting)

---

## Quick Start

```bash
# Install dependencies (one-time)
cargo install just
cargo install cargo-pgrx
cargo pgrx init

# Run all tests
just test

# Or manually
cargo pgrx test pg17          # Rust unit tests
just test-sql                 # SQL integration tests
```

---

## Why `cargo test` Doesn't Work

### The Problem

If you run `cargo test`, you'll see errors like:

```text
error: linking with `cc` failed: exit status: 1
rust-lld: error: undefined symbol: PG_exception_stack
rust-lld: error: undefined symbol: CurrentMemoryContext
rust-lld: error: undefined symbol: CopyErrorData
```

### Root Cause

pgrx extensions are **PostgreSQL plugins**, not standalone Rust programs. They:

1. **Dynamically link** to PostgreSQL at runtime (like `.so` files)
2. **Require PostgreSQL symbols** (PG_exception_stack, CurrentMemoryContext, etc.)
3. **Must run inside PostgreSQL** (loaded via `CREATE EXTENSION`)

Standard `cargo test`:
- ❌ Links tests as standalone executables
- ❌ Can't find PostgreSQL symbols (not linked at build time)
- ❌ No PostgreSQL runtime environment

### The Solution

Use `cargo pgrx test` which:
- ✅ Initializes a PostgreSQL test instance
- ✅ Compiles extension as a shared library (`.so`)
- ✅ Loads extension into PostgreSQL
- ✅ Runs tests inside PostgreSQL runtime

**This is the correct and only way to test pgrx extensions.**

---

## Test Types

### 1. Rust Unit Tests

**Location**: `src/lib.rs` - `mod tests`

**Purpose**: Test Rust logic, algorithms, edge cases

**Example**:

```rust
#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    #[pgrx::pg_test]
    fn test_basic_merge() {
        let target = JsonB(json!({"a": 1, "b": 2}));
        let source = JsonB(json!({"c": 3}));

        let result = crate::jsonb_merge_shallow(Some(target), Some(source))
            .expect("merge should succeed");

        assert_eq!(result.0, json!({"a": 1, "b": 2, "c": 3}));
    }
}
```

**Run**:

```bash
cargo pgrx test pg17          # PostgreSQL 17
cargo pgrx test pg16          # PostgreSQL 16
cargo pgrx test               # All configured versions
```

**Count**: 30+ tests covering:
- Basic operations
- Edge cases
- NULL handling
- Type validation
- Error conditions

### 2. SQL Integration Tests

**Location**: `test/sql/*.sql`

**Purpose**: Test production-like usage, SQL interface, user workflows

**Files**:

```text
test/sql/
├── 00_setup.sql                    # Setup test tables
├── 01_basic_operations.sql         # Basic function tests
├── 02_array_operations.sql         # Array update tests
├── 03_performance.sql              # Performance tests
└── 04_edge_cases.sql               # Edge cases
```

**Example** (`test/sql/01_basic_operations.sql`):

```sql
-- Test jsonb_merge_shallow
SELECT jsonb_merge_shallow(
    '{"a": 1, "b": 2}'::jsonb,
    '{"c": 3}'::jsonb
) = '{"a": 1, "b": 2, "c": 3}'::jsonb;

-- Test jsonb_array_update_where
SELECT jsonb_array_update_where(
    '{"items": [{"id": 1, "name": "old"}, {"id": 2, "name": "test"}]}'::jsonb,
    'items',
    'id',
    '1'::jsonb,
    '{"name": "new"}'::jsonb
) @> '{"items": [{"id": 1, "name": "new"}]}'::jsonb;
```

**Run**:

```bash
just test-sql                 # Using justfile

# Or manually
cargo pgrx install --release
dropdb test_jsonb_delta 2>/dev/null || true
createdb test_jsonb_delta
psql -d test_jsonb_delta -c "CREATE EXTENSION jsonb_delta;"
psql -d test_jsonb_delta -f test/sql/01_basic_operations.sql
```

**Count**: 5 test files covering all 13 public functions

### 3. Performance Benchmarks

**Location**: `test/benchmark_*.sql`

**Purpose**: Validate performance claims (2-3× speedup)

**Example**:

```sql
-- Benchmark: jsonb_array_update_where vs native SQL
EXPLAIN ANALYZE
SELECT jsonb_array_update_where(data, 'dns_servers', 'id', '42'::jsonb, '{"ip": "8.8.8.8"}'::jsonb)
FROM test_table;

EXPLAIN ANALYZE
SELECT jsonb_set(data, '{dns_servers}', (
    SELECT jsonb_agg(CASE WHEN elem->>'id' = '42' THEN elem || '{"ip": "8.8.8.8"}' ELSE elem END)
    FROM jsonb_array_elements(data->'dns_servers') elem
))
FROM test_table;
```

**Run**:

```bash
just bench
```

### 4. Smoke Tests

**Location**: `test/smoke_test_v0.1.0.sql`

**Purpose**: Quick validation that extension loads and basic functions work

**Run**:

```bash
psql -d test_jsonb_delta -f test/smoke_test_v0.1.0.sql
```

---

## Running Tests

### Using `just` (Recommended)

```bash
# Install just (one-time)
cargo install just

# Run all tests
just test

# Individual test types
just test-rust                # Rust unit tests only
just test-sql                 # SQL integration tests only
just bench                    # Performance benchmarks

# Development workflow
just check                    # Fast: formatting + clippy (no tests)
just fix                      # Auto-fix formatting/clippy issues
just dev                      # Fix + build (fast feedback loop)
just ci                       # Full CI-like check (check + build + test)
```

### Manual Commands

**Rust Unit Tests**:

```bash
# All tests on PostgreSQL 17
cargo pgrx test pg17

# Specific test
cargo pgrx test pg17 test_basic_merge

# All configured PostgreSQL versions
cargo pgrx test

# Release mode (slower build, faster execution)
cargo pgrx test pg17 --release
```

**SQL Integration Tests**:

```bash
# Install extension
cargo pgrx install --release

# Setup test database
dropdb test_jsonb_delta 2>/dev/null || true
createdb test_jsonb_delta
psql -d test_jsonb_delta -c "CREATE EXTENSION jsonb_delta;"

# Run specific test file
psql -d test_jsonb_delta -f test/sql/01_basic_operations.sql

# Run all tests
for file in test/sql/*.sql; do
    echo "→ $(basename $file)..."
    psql -d test_jsonb_delta -f "$file" || exit 1
done
```

**Benchmarks**:

```bash
cargo pgrx install --release
psql -d postgres -f test/benchmark_array_update_where.sql
```

### CI-like Testing Locally

Run the same checks as GitHub Actions:

```bash
# Quick checks
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release

# Full test matrix (requires all PostgreSQL versions)
# ⚠️ IMPORTANT: CI only tests PostgreSQL 17 for speed
#    Run this locally before major releases to ensure compatibility
for version in 13 14 15 16 17 18; do
    echo "→ Testing PostgreSQL $version..."
    cargo pgrx test pg$version --release
done
```

---

## Writing Tests

### Writing Rust Unit Tests

**Add to `src/lib.rs`**:

```rust
#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;
    use pgrx::JsonB;
    use serde_json::json;

    #[pgrx::pg_test]
    fn test_your_function() {
        // Arrange
        let input = JsonB(json!({"key": "value"}));

        // Act
        let result = crate::your_function(input);

        // Assert
        assert_eq!(result.0, json!({"expected": "output"}));
    }

    // Test edge cases
    #[pgrx::pg_test]
    #[should_panic(expected = "expected error message")]
    fn test_error_condition() {
        let invalid_input = JsonB(json!([1, 2, 3]));  // Array instead of object
        let _ = crate::your_function(invalid_input);
    }
}
```

**Best Practices**:
- Use descriptive test names: `test_basic_merge`, `test_empty_source`, `test_null_handling`
- Test happy path + edge cases + error conditions
- Use `#[should_panic(expected = "...")]` for error tests
- Test with different input types (objects, arrays, scalars, null)
- Test boundary conditions (empty, large, nested)

### Writing SQL Integration Tests

**Add to `test/sql/*.sql`**:

```sql
-- Test: Basic functionality
SELECT jsonb_your_function(
    '{"input": "data"}'::jsonb
) = '{"expected": "output"}'::jsonb;

-- Test: Edge case - NULL input
SELECT jsonb_your_function(NULL) IS NULL;

-- Test: Error condition (should raise error)
\set ON_ERROR_STOP on
DO $$
BEGIN
    PERFORM jsonb_your_function('invalid input'::jsonb);
    RAISE EXCEPTION 'Should have raised error';
EXCEPTION
    WHEN OTHERS THEN
        -- Expected
END
$$;
```

**Best Practices**:
- Use `\set ON_ERROR_STOP on` to fail fast
- Test with production-like data
- Include comments explaining what each test validates
- Use DO blocks for error condition tests
- Test with real table data, not just literals

---

## CI/CD Testing

### GitHub Actions Workflow

**File**: `.github/workflows/test.yml`

**Matrix**:
- PostgreSQL versions: **17 only** (for CI speed; local testing covers all versions 13-18)
- Operating systems: Ubuntu

**Steps**:
1. Install Rust + PostgreSQL
2. Install cargo-pgrx
3. Initialize pgrx
4. Build extension
5. Install extension
6. Run SQL integration tests
7. Validate schema generation
8. Generate coverage report (PostgreSQL 17 only)

**Runs on**:
- Every push to main
- Every pull request
- Manual workflow dispatch

### Local CI Simulation

```bash
# Full CI check
just ci

# Or manually
cargo fmt --check
cargo clippy -- -D warnings
cargo build --release
cargo pgrx test pg17 --release
just test-sql
```

---

## Troubleshooting

### Error: "undefined symbol: PG_exception_stack"

**Cause**: Using `cargo test` instead of `cargo pgrx test`

**Solution**: Always use `cargo pgrx test` for pgrx extensions

```bash
# ❌ Wrong
cargo test

# ✅ Correct
cargo pgrx test pg17
```

### Error: "could not connect to server"

**Cause**: pgrx hasn't been initialized or PostgreSQL test instance isn't running

**Solution**: Initialize pgrx

```bash
cargo install cargo-pgrx
cargo pgrx init
```

### Error: "extension jsonb_delta does not exist"

**Cause**: Extension not installed before running SQL tests

**Solution**: Install extension first

```bash
cargo pgrx install --release
psql -d test_jsonb_delta -c "CREATE EXTENSION jsonb_delta;"
```

### Error: "database test_jsonb_delta does not exist"

**Cause**: Test database not created

**Solution**: Create database

```bash
createdb test_jsonb_delta
```

### Tests Pass Locally But Fail in CI

**Possible causes**:
1. **Different PostgreSQL version**: Test on all supported versions (13-17)
2. **Platform differences**: Test on both Ubuntu and macOS if possible
3. **Schema drift**: Regenerate schema with `cargo pgrx schema`
4. **Cached state**: Clean build with `cargo clean && just test`

### Slow Test Execution

**Rust tests slow (1-2 min startup)**:
- Normal - pgrx initializes PostgreSQL instance
- Use `just check` for fast feedback (no tests)
- Use `--release` for faster execution after build

**SQL tests slow**:
- Install extension once, run multiple test files
- Use `just test-sql` which batches all tests

### Test Flakiness

If tests occasionally fail:
1. Check for race conditions in tests
2. Verify tests clean up after themselves
3. Use transactions in SQL tests to isolate state
4. Check for temp table naming conflicts

---

## Performance Testing

### Benchmarking Commands

```bash
# Install extension in release mode
cargo pgrx install --release

# Run benchmarks
psql -d postgres -f test/benchmark_array_update_where.sql

# Extract timing from EXPLAIN ANALYZE
psql -d postgres -c "
EXPLAIN (ANALYZE, TIMING)
SELECT jsonb_array_update_where(data, 'array', 'id', '42'::jsonb, '{\"new\": \"value\"}'::jsonb)
FROM generate_series(1, 1000) s(id),
LATERAL (SELECT ('{'\"array\":[{\"id\":' || id || '}]}')::jsonb) d(data);
"
```

### Performance Validation

The extension should demonstrate:
- **2-3× speedup** vs native SQL for array updates
- **Linear scaling** with array size (O(n))
- **Constant memory** (no array copying)

See `docs/implementation/benchmark-results.md` for detailed results.

---

## Additional Resources

- **Development Guide**: `development.md` - Build and installation
- **Contributing Guide**: `contributing.md` - Code standards and workflow
- **CI/CD Plan**: `ci-cd-improvement-plan.md` - Full CI/CD architecture
- **Troubleshooting**: `docs/troubleshooting.md` - Common issues

---

## Summary

### For Developers

```bash
# Daily workflow
just check       # Fast feedback
just dev         # Fix + build
just test        # Full testing
```

### For Contributors

1. Write Rust tests for logic
2. Write SQL tests for user interface
3. Run `just ci` before committing
4. Update docs if adding functions

### For CI/CD

- Uses SQL integration tests (production-like)
- Tests PostgreSQL 13-17 × (Ubuntu + macOS)
- Validates schema generation
- Generates coverage reports

**Key takeaway**: Always use `cargo pgrx test`, never `cargo test` ✅
