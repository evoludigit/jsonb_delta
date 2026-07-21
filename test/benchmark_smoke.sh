#!/usr/bin/env bash
# Smoke test: every benchmark script must run to completion against a clean database.
#
# This asserts exit status only, never timing — it exists so the benchmark suite
# cannot silently rot again (it was unrunnable from the extension rename until the
# repair, which is why no benchmark artifact in the repo postdates it).
#
# Usage: test/benchmark_smoke.sh [dbname]

set -uo pipefail

DB="${1:-jsonb_delta_bench_smoke}"
PSQL_BASE=(psql -v ON_ERROR_STOP=1 -q)
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${TMPDIR:-/tmp}/jsonb_delta_smoke"

cd "$REPO_ROOT" || exit 1
mkdir -p "$LOG_DIR"

echo "→ Recreating database '$DB'"
dropdb --if-exists "$DB" 2>/dev/null
if ! createdb "$DB"; then
    echo "✗ FATAL: could not create database '$DB'" >&2
    exit 1
fi

echo "→ Verifying extension is installed"
if ! "${PSQL_BASE[@]}" -d "$DB" -c 'CREATE EXTENSION IF NOT EXISTS jsonb_delta;' >"$LOG_DIR/extension.log" 2>&1; then
    echo "✗ FATAL: CREATE EXTENSION jsonb_delta failed" >&2
    sed 's/^/    /' "$LOG_DIR/extension.log" >&2
    echo "  Run 'just install' first (needs write access to the PostgreSQL share dir)." >&2
    dropdb --if-exists "$DB" 2>/dev/null
    exit 1
fi

echo "→ Loading benchmark fixtures"
if ! "${PSQL_BASE[@]}" -d "$DB" -f test/fixtures/setup_benchmark_env.sql >"$LOG_DIR/setup.log" 2>&1; then
    echo "✗ FATAL: fixture setup failed" >&2
    tail -20 "$LOG_DIR/setup.log" | sed 's/^/    /' >&2
    dropdb --if-exists "$DB" 2>/dev/null
    exit 1
fi

echo "→ Calibrating the benchmark harness"
if ! "${PSQL_BASE[@]}" -d "$DB" -f test/bench/harness_test.sql >"$LOG_DIR/harness_test.log" 2>&1; then
    echo "✗ FATAL: benchmark harness self-test failed" >&2
    grep -E '^(psql:|ERROR|NOTICE)' "$LOG_DIR/harness_test.log" | tail -10 | sed 's/^/    /' >&2
    echo "  The harness is the instrument every published number comes from." >&2
    echo "  Numbers taken with a failing instrument are not publishable." >&2
    dropdb --if-exists "$DB" 2>/dev/null
    exit 1
fi

failed=0
passed=0
declare -a FAILURES=()

for script in test/benchmark_*.sql; do
    name="$(basename "$script")"
    log="$LOG_DIR/${name}.log"
    printf '  %-42s ' "$name"
    if "${PSQL_BASE[@]}" -d "$DB" -f "$script" >"$log" 2>&1; then
        echo "✓"
        passed=$((passed + 1))
    else
        echo "✗"
        failed=$((failed + 1))
        FAILURES+=("$name")
    fi
done

echo
echo "  passed: $passed   failed: $failed"

if [ "$failed" -gt 0 ]; then
    echo
    echo "Failures:"
    for name in "${FAILURES[@]}"; do
        echo "  --- $name ---"
        grep -E '^(psql:|ERROR|FATAL)' "$LOG_DIR/${name}.log" | head -5 | sed 's/^/      /'
    done
    echo
    echo "  Full logs in $LOG_DIR"
    dropdb --if-exists "$DB" 2>/dev/null
    exit 1
fi

echo "✅ All benchmark scripts run clean"
dropdb --if-exists "$DB" 2>/dev/null
exit 0
