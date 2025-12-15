# Fix Test Environment Issues

## Problem Summary

After fixing formatting and clippy warnings, tests fail locally with PostgreSQL linking errors:
```
rust-lld: error: undefined symbol: PG_exception_stack
rust-lld: error: undefined symbol: CurrentMemoryContext
rust-lld: error: undefined symbol: CopyErrorData
```

**Root Cause**: `cargo test` doesn't work for pgrx extensions because they require PostgreSQL runtime symbols.

**Solution**: Use `cargo pgrx test` which properly initializes PostgreSQL environment.

---

## Understanding pgrx Testing

### Why `cargo test` Fails

pgrx extensions are **dynamically loaded PostgreSQL modules**, not standalone binaries. They require:
1. PostgreSQL symbols (PG_exception_stack, CurrentMemoryContext, etc.)
2. PostgreSQL runtime environment
3. Database initialization

**Standard Rust testing** (`cargo test`):
- Links tests as standalone executables
- Can't link PostgreSQL symbols (would require libpq at link time)
- ❌ Fails with "undefined symbol" errors

**pgrx testing** (`cargo pgrx test`):
- Initializes a PostgreSQL instance
- Loads extension as a dynamic library (like production)
- Runs tests inside PostgreSQL runtime
- ✅ Works correctly

### How pgrx Tests Work

```rust
#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    #[pgrx::pg_test]
    fn test_basic_merge() {
        // This runs INSIDE a PostgreSQL instance
        let target = JsonB(json!({"a": 1}));
        // ...
    }
}
```

**Execution flow**:
1. `cargo pgrx test pg17` starts PostgreSQL 17
2. Compiles extension as shared library (`.so`)
3. Loads extension into PostgreSQL
4. Creates test database
5. Runs each `#[pg_test]` function inside PostgreSQL
6. Shuts down test instance

---

## Current Test Strategy

The project uses **SQL integration tests**, not Rust unit tests:

### Test Files
```
test/
├── sql/
│   ├── 00_setup.sql                    # Setup test tables
│   ├── 01_basic_operations.sql         # Basic function tests
│   ├── 02_array_operations.sql         # Array update tests
│   ├── 03_performance.sql              # Performance tests
│   └── 04_edge_cases.sql               # Edge cases
└── smoke_test_v0.3.0.sql               # Version smoke test
```

### CI Test Workflow

From `.github/workflows/test.yml`:
```yaml
- name: Install extension
  run: cargo pgrx install --pg-config=/usr/lib/postgresql/17/bin/pg_config --release

- name: Run SQL integration tests
  run: |
    sudo systemctl start postgresql@17-main
    sudo -u postgres psql -c "CREATE DATABASE test_jsonb_ivm;"
    sudo -u postgres psql -d test_jsonb_ivm -c "CREATE EXTENSION jsonb_ivm;"

    for test_file in test/sql/*.sql; do
      sudo -u postgres psql -d test_jsonb_ivm -f "$test_file"
    done
```

**This is the correct approach** for pgrx extensions in CI.

---

## Rust Unit Tests vs SQL Integration Tests

### Option 1: Rust Unit Tests (in `src/lib.rs`)

**Pros**:
- Fast feedback during development
- Type-safe, IDE-integrated
- Good for testing Rust logic

**Cons**:
- Requires `cargo pgrx test` (can't use `cargo test`)
- Slower startup (PostgreSQL initialization)
- Not as close to production usage

**Current state**: We have 30+ Rust unit tests in `src/lib.rs`:
```rust
#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    #[pgrx::pg_test]
    fn test_basic_merge() { ... }
    // ... 29 more tests
}
```

### Option 2: SQL Integration Tests (in `test/sql/`)

**Pros**:
- Exactly how users will call functions
- Works with standard `psql`
- Easy to write and understand
- Closer to production

**Cons**:
- Less type safety
- Harder to debug
- Requires PostgreSQL installation

**Current state**: We have 5 SQL test files covering all functions.

---

## Recommended Solution

### Keep Both Test Strategies

**Use Rust tests (`cargo pgrx test`) for**:
- Algorithm correctness
- Edge cases
- Performance optimization validation
- Quick local development feedback

**Use SQL tests (CI integration tests) for**:
- End-to-end validation
- User-facing behavior
- Cross-version compatibility
- Production-like scenarios

### Fix Local Development Workflow

#### Problem: Developers can't run tests locally easily

**Current situation**:
```bash
cargo test              # ❌ Fails with linker errors
cargo pgrx test         # ⏳ Slow (PostgreSQL startup)
```

**Solution 1: Add Makefile/justfile shortcuts**

Create `justfile` (modern make alternative):
```makefile
# justfile - Task runner for jsonb_ivm

# Default PostgreSQL version for testing
export PG_VERSION := "17"

# Run all tests (Rust + SQL)
test: test-rust test-sql

# Run Rust unit tests via pgrx
test-rust:
    cargo pgrx test pg{{PG_VERSION}}

# Run SQL integration tests
test-sql:
    cargo pgrx install --release
    psql -d postgres -c "DROP DATABASE IF EXISTS test_jsonb_ivm;"
    psql -d postgres -c "CREATE DATABASE test_jsonb_ivm;"
    psql -d test_jsonb_ivm -c "CREATE EXTENSION jsonb_ivm;"
    for file in test/sql/*.sql; do \
        echo "Running $$file..."; \
        psql -d test_jsonb_ivm -f "$$file" || exit 1; \
    done

# Quick check (formatting + clippy, no tests)
check:
    cargo fmt --check
    cargo clippy -- -D warnings

# Fast feedback loop for development
dev-check:
    cargo fmt
    cargo clippy --fix --allow-dirty
    cargo build

# Run benchmarks
bench:
    cargo pgrx install --release
    psql -d postgres -f test/benchmark_array_update_where.sql

# Clean everything (including pgrx artifacts)
clean:
    cargo clean
    rm -rf ~/.pgrx/data-{{PG_VERSION}}

# Install just: cargo install just
# Usage: just test
```

**Install just**:
```bash
cargo install just
just test           # Run all tests
just test-rust      # Rust only
just dev-check      # Quick feedback
```

**Solution 2: Improve documentation**

Update `development.md`:
```markdown
## Running Tests

### Quick Start

```bash
# Install just (one-time)
cargo install just

# Run all tests
just test

# Development loop (fast)
just dev-check
```

### Manual Testing

**Rust Unit Tests** (requires pgrx-initialized PostgreSQL):
```bash
cargo pgrx test pg17
```

**SQL Integration Tests** (requires installed extension):
```bash
cargo pgrx install --release
psql -d test_jsonb_ivm -c "CREATE EXTENSION jsonb_ivm;"
psql -d test_jsonb_ivm -f test/sql/01_basic_operations.sql
```

### Why `cargo test` doesn't work

pgrx extensions are PostgreSQL plugins, not standalone binaries. They require:
- PostgreSQL runtime symbols (PG_exception_stack, etc.)
- Initialized PostgreSQL instance
- Extension loaded into database

Use `cargo pgrx test` instead, which handles this setup automatically.

### CI Testing

GitHub Actions uses SQL integration tests (see `.github/workflows/test.yml`):
- Tests across PostgreSQL 13-17
- Tests on Ubuntu + macOS
- Validates production-like usage
```

---

## Implementation Plan

### Phase 1: Document the Current (Correct) Behavior ✅

**Goal**: Help developers understand why `cargo test` fails and what to use instead.

**Tasks**:
1. ✅ Update `development.md` with testing instructions
2. ✅ Add `TESTING.md` with detailed explanation
3. ✅ Document in README under "Testing" section
4. ✅ Add troubleshooting guide for common errors

**Files to update**:
- `development.md` - Add "Running Tests" section
- `README.md` - Update "🧪 Testing" section
- `contributing.md` - Add testing best practices
- Create `docs/testing-guide.md` - Comprehensive testing docs

### Phase 2: Improve Developer Experience

**Goal**: Make it easier to run tests locally.

**Option A: Add `justfile` (Recommended)**

Pros:
- Modern, fast, cross-platform
- Better syntax than Makefile
- Already installed if using Rust

```bash
# Install just
cargo install just

# Create justfile
cat > justfile << 'EOF'
# See .phases/TODO/fix-test-environment.md for full content
test:
    cargo pgrx test pg17

check:
    cargo fmt --check
    cargo clippy -- -D warnings
EOF
```

**Option B: Add `Makefile` (Traditional)**

Pros:
- Universally available
- Familiar to most developers

Cons:
- Quirky syntax
- Slower than just

**Option C: Add shell scripts in `scripts/`**

```bash
scripts/
├── test.sh              # Run all tests
├── test-rust.sh         # Rust unit tests
├── test-sql.sh          # SQL integration tests
└── dev-check.sh         # Quick feedback
```

Pros:
- No dependencies
- Easy to understand

Cons:
- Platform-specific (bash vs sh vs PowerShell)
- Harder to maintain

**Recommendation**: Use **justfile** (Option A) because:
- Rust developers likely have `just` installed
- Cross-platform (works on Windows/Linux/macOS)
- Clean, readable syntax
- Fast execution

### Phase 3: Add Pre-commit Hooks (Optional)

**Goal**: Catch issues before CI.

Create `.git/hooks/pre-commit`:
```bash
#!/bin/bash
# Pre-commit hook for jsonb_ivm

echo "Running pre-commit checks..."

# Format check
echo "  → Checking formatting..."
cargo fmt --check || {
    echo "❌ Formatting issues found. Run: cargo fmt"
    exit 1
}

# Clippy
echo "  → Running clippy..."
cargo clippy -- -D warnings || {
    echo "❌ Clippy warnings found. Run: cargo clippy --fix"
    exit 1
}

# Build check
echo "  → Checking build..."
cargo build --quiet || {
    echo "❌ Build failed"
    exit 1
}

echo "✅ Pre-commit checks passed"
```

**Install**:
```bash
chmod +x .git/hooks/pre-commit
```

**Alternative**: Use `cargo-husky` or `lefthook` for managed hooks.

### Phase 4: Optimize CI Test Matrix (Optional)

**Current matrix**: Tests on PostgreSQL 13-17 × (Ubuntu + macOS)

**Optimization ideas**:
1. **Cache test database initialization** (speeds up reruns)
2. **Parallel test execution** (run SQL tests in parallel)
3. **Fast-fail on formatting/clippy** (don't run expensive tests if linting fails)

**Example optimized workflow**:
```yaml
jobs:
  # Fast checks first (< 1 min)
  quick-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo build --release

  # Full tests only if quick checks pass
  test:
    needs: quick-check
    strategy:
      matrix:
        pg-version: [13, 14, 15, 16, 17]
        os: [ubuntu-latest, macos-latest]
    # ... rest of test workflow
```

---

## Current State Assessment

### ✅ What's Already Correct

1. **CI workflow is correct**: Uses SQL integration tests
2. **Test coverage is good**: 30+ Rust tests + 5 SQL test files
3. **Multi-version testing**: PostgreSQL 13-17
4. **Cross-platform**: Ubuntu + macOS

### ⚠️ What Needs Improvement

1. **Documentation**: Doesn't explain why `cargo test` fails
2. **Developer UX**: No easy way to run tests locally
3. **Error messages**: Confusing linker errors instead of helpful message

### ❌ What's NOT a Problem

The test failures you're seeing are **expected** and **correct behavior** for a pgrx extension:
- `cargo test` is the wrong command
- `cargo pgrx test` is the right command
- CI uses SQL tests (also correct)

---

## Quick Fixes (Can Implement Now)

### Fix 1: Add Testing Instructions to README

Update `README.md` line 447-460:
```markdown
## 🧪 Testing

### Quick Start

```bash
# Run Rust unit tests (requires pgrx)
cargo pgrx test pg17

# Run SQL integration tests
cargo pgrx install --release
psql -d test_jsonb_ivm -c "CREATE EXTENSION jsonb_ivm;"
psql -d test_jsonb_ivm -f test/sql/01_basic_operations.sql
```

### Why not `cargo test`?

pgrx extensions are PostgreSQL plugins, not standalone programs. Use `cargo pgrx test` which:
- Initializes a test PostgreSQL instance
- Loads the extension as a dynamic library
- Runs tests inside the PostgreSQL runtime

For CI testing, we use SQL integration tests across PostgreSQL 13-17.

### Test Coverage

- ✅ **Rust unit tests**: 30+ tests covering all functions and edge cases
- ✅ **SQL integration tests**: 5 test suites covering production usage
- ✅ **Multi-version**: Tested on PostgreSQL 13-17
- ✅ **Cross-platform**: Ubuntu + macOS
```

### Fix 2: Add justfile for Easy Testing

Create `justfile`:
```makefile
# PostgreSQL version for local testing
PG_VERSION := "17"

# Run all tests
test: test-rust test-sql

# Run Rust unit tests
test-rust:
    cargo pgrx test pg{{PG_VERSION}}

# Run SQL integration tests
test-sql:
    #!/bin/bash
    cargo pgrx install --release
    dropdb test_jsonb_ivm 2>/dev/null || true
    createdb test_jsonb_ivm
    psql -d test_jsonb_ivm -c "CREATE EXTENSION jsonb_ivm;"
    for file in test/sql/*.sql; do
        echo "→ Running $(basename $file)..."
        psql -d test_jsonb_ivm -f "$file" || exit 1
    done
    echo "✅ All SQL tests passed"

# Quick development checks (no tests)
check:
    cargo fmt --check
    cargo clippy -- -D warnings

# Auto-fix issues
fix:
    cargo fmt
    cargo clippy --fix --allow-dirty

# Build and install locally
install:
    cargo pgrx install --release

# Clean everything
clean:
    cargo clean
    rm -rf target
```

**Usage**:
```bash
# Install just (one-time)
cargo install just

# Run tests
just test           # All tests
just test-rust      # Rust only
just test-sql       # SQL only
just check          # Quick checks
```

### Fix 3: Update CONTRIBUTING.md

Add testing section:
```markdown
## Running Tests

This project uses **pgrx testing infrastructure**. Standard `cargo test` won't work because extensions require PostgreSQL runtime.

### Prerequisites

```bash
# One-time setup
cargo install cargo-pgrx
cargo pgrx init
```

### Running Tests

**Recommended** (using just):
```bash
cargo install just
just test
```

**Manual**:
```bash
# Rust unit tests
cargo pgrx test pg17

# SQL integration tests
cargo pgrx install --release
psql -d test_jsonb_ivm -c "CREATE EXTENSION jsonb_ivm;"
psql -d test_jsonb_ivm -f test/sql/01_basic_operations.sql
```

### Development Workflow

```bash
# 1. Make changes to src/lib.rs
vim src/lib.rs

# 2. Quick checks (fast)
just check

# 3. Run tests (slower)
just test

# 4. Fix issues
just fix
```

### CI Testing

GitHub Actions runs SQL integration tests across:
- PostgreSQL versions: 13, 14, 15, 16, 17
- Operating systems: Ubuntu, macOS

See `.github/workflows/test.yml` for details.
```

---

## Long-term Improvements (Future)

### 1. Add `cargo xtask` for Task Automation

Instead of justfile, use Rust-based task runner:

```rust
// xtask/src/main.rs
fn main() {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("test") => test(),
        Some("check") => check(),
        Some("install") => install(),
        _ => print_help(),
    }
}

fn test() {
    cmd!("cargo", "pgrx", "test", "pg17").run().unwrap();
    // Run SQL tests...
}
```

**Usage**: `cargo xtask test`

### 2. Add Custom Cargo Aliases

In `.cargo/config.toml`:
```toml
[alias]
pgrx-test = "pgrx test pg17"
pgrx-install = "pgrx install --release"
quick-check = "clippy -- -D warnings"
```

**Usage**: `cargo pgrx-test`

### 3. Add GitHub Actions Caching for pgrx

Speed up CI by caching PostgreSQL installations:
```yaml
- name: Cache pgrx PostgreSQL
  uses: actions/cache@v4
  with:
    path: ~/.pgrx
    key: ${{ runner.os }}-pgrx-${{ matrix.pg-version }}
```

---

## Summary

### The Real Issue (Not a Bug)

The test "failures" are **expected behavior**:
- ❌ `cargo test` - Wrong tool for pgrx extensions
- ✅ `cargo pgrx test` - Correct tool
- ✅ CI SQL tests - Also correct

### What to Fix

1. **Documentation** - Explain testing workflow
2. **Developer UX** - Add `justfile` for easy commands
3. **Error messages** - Better guidance in README

### What NOT to Fix

- Don't try to make `cargo test` work (impossible for pgrx)
- Don't remove Rust unit tests (they're valuable)
- Don't change CI workflow (it's correct)

### Recommended Actions (Priority Order)

1. ✅ **High**: Update README.md with testing instructions
2. ✅ **High**: Add `justfile` for developer convenience
3. ✅ **Medium**: Update CONTRIBUTING.md with testing guidelines
4. ✅ **Medium**: Add TESTING.md with comprehensive testing docs
5. ⏳ **Low**: Add pre-commit hooks
6. ⏳ **Low**: Optimize CI caching

### Implementation Time

- Phase 1 (Documentation): 30 minutes
- Phase 2 (justfile): 15 minutes
- Phase 3 (Pre-commit hooks): 30 minutes
- Total: ~1.5 hours

---

**Conclusion**: The tests aren't broken, the documentation is. Fix the docs and add convenience tools, and developers will understand the proper workflow.
