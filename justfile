# justfile - Task runner for jsonb_delta
# Install just: cargo install just
# Usage: just test, just check, just install

# Default PostgreSQL version for local testing
PG_VERSION := "18"

# Default: show available commands
default:
    @just --list

# Run all tests (Rust + SQL)
test: test-rust test-sql

# Run Rust unit tests via pgrx
test-rust:
    @echo "→ Running Rust unit tests..."
    cargo pgrx test pg{{PG_VERSION}}

# Run SQL integration tests
test-sql:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "→ Installing extension..."
    cargo pgrx install --release --pg-config ~/.pgrx/{{PG_VERSION}}.*/pgrx-install/bin/pg_config
    echo "→ Setting up test database..."
    dropdb test_jsonb_delta 2>/dev/null || true
    createdb test_jsonb_delta
    psql -d test_jsonb_delta -c "CREATE EXTENSION jsonb_delta;" >/dev/null
    echo "→ Running SQL tests..."
    for file in test/sql/*.sql; do
        echo "  → $(basename $file)..."
        psql -d test_jsonb_delta -f "$file" >/dev/null || exit 1
    done
    echo "→ Running smoke test..."
    psql -d test_jsonb_delta -f test/smoke_test_v0.1.0.sql >/dev/null || exit 1
    echo "✅ All SQL tests passed"

# Quick development checks (no tests)
check:
    @echo "→ Checking formatting..."
    @cargo fmt --check
    @echo "→ Running clippy..."
    @cargo clippy --all-targets --all-features -- -D warnings
    @echo "✅ All checks passed"

# Auto-fix formatting and clippy issues
fix:
    @echo "→ Fixing formatting..."
    @cargo fmt
    @echo "→ Fixing clippy warnings..."
    @cargo clippy --fix --allow-dirty --allow-staged
    @echo "✅ Fixes applied"

# Build extension (debug mode)
build:
    @echo "→ Building extension (debug)..."
    @cargo build

# Build and install extension (release mode)
install:
    @echo "→ Installing extension (release)..."
    @cargo pgrx install --release

# Run benchmarks
bench:
    @echo "→ Running benchmarks..."
    @cargo pgrx install --release
    @psql -d postgres -f test/benchmark_array_update_where.sql

# Clean build artifacts
clean:
    @echo "→ Cleaning build artifacts..."
    @cargo clean
    @echo "✅ Clean complete"

# Generate SQL schema
schema:
    @echo "→ Generating SQL schema..."
    @cargo pgrx schema > sql/jsonb_delta--0.1.0.sql
    @echo "✅ Schema generated"

# Full CI-like check (what GitHub Actions runs)
ci: check build test
    @echo "✅ All CI checks passed"

# Development loop (fast feedback)
dev: fix build
    @echo "✅ Development loop complete"

# Initialize pgrx for first-time setup
init:
    @echo "→ Initializing pgrx..."
    @cargo install cargo-pgrx --locked --version 0.16.1
    @cargo pgrx init
    @echo "✅ pgrx initialized"
