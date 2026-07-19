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
    @cargo clippy --all-targets -- -D warnings
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

# Build and install extension (release mode); optionally into a specific pg_config
install pg_config="":
    #!/usr/bin/env bash
    set -euo pipefail
    # Installs into whichever PostgreSQL `pg_config` resolves to. The benchmark
    # recipes talk to the server `psql` connects to — if that is a system server
    # rather than a pgrx-managed one, pass its pg_config explicitly:
    #
    #     just install /opt/postgresql17/bin/pg_config
    #
    # Writing into a system PostgreSQL's share directory usually needs elevation:
    #
    #     sudo -E $(command -v cargo) pgrx install --release --pg-config <path>
    echo "→ Installing extension (release)..."
    if [ -n "{{pg_config}}" ]; then
        cargo pgrx install --release --pg-config "{{pg_config}}"
    else
        cargo pgrx install --release
    fi

# Database used by the benchmark recipes (never the default `postgres` database)
BENCH_DB := "jsonb_delta_bench"

# Load benchmark fixtures into BENCH_DB (idempotent; creates the database if absent)
bench-setup:
    #!/usr/bin/env bash
    set -euo pipefail
    createdb {{BENCH_DB}} 2>/dev/null || true
    psql -v ON_ERROR_STOP=1 -q -d {{BENCH_DB}} -c 'CREATE EXTENSION IF NOT EXISTS jsonb_delta;'
    psql -v ON_ERROR_STOP=1 -q -d {{BENCH_DB}} -f test/fixtures/setup_benchmark_env.sql

# Run the headline array-update benchmark
bench: bench-setup
    @echo "→ Running benchmark: array update where..."
    @psql -v ON_ERROR_STOP=1 -d {{BENCH_DB}} -f test/benchmark_array_update_where.sql

# Run the full benchmark suite
bench-all: bench-setup
    #!/usr/bin/env bash
    set -euo pipefail
    for file in test/benchmark_*.sql; do
        echo "→ $(basename "$file")"
        psql -v ON_ERROR_STOP=1 -d {{BENCH_DB}} -f "$file"
    done
    echo "✅ Benchmark suite complete"

# Assert every benchmark script runs clean (exit status only, no timing)
bench-smoke:
    @./test/benchmark_smoke.sh

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
