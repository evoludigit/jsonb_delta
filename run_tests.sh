#!/bin/bash
set -e

# Test runner script for jsonb_ivm extension

echo "========================================="
echo "Starting jsonb_ivm SQL Integration Tests"
echo "========================================="

# Use pgrx-managed PostgreSQL 17
PG_BIN=~/.pgrx/17.7/pgrx-install/bin
export PATH="$PG_BIN:$PATH"

# Connection parameters for pgrx PostgreSQL
PGHOST=localhost
PGPORT=28817
PGDATABASE=postgres
export PGHOST PGPORT

# Create test database
echo "Creating test database..."
$PG_BIN/dropdb --if-exists test_jsonb_ivm 2>/dev/null || true
$PG_BIN/createdb test_jsonb_ivm

# Run SQL test file (includes CREATE EXTENSION)
echo "Running SQL regression tests..."
$PG_BIN/psql -d test_jsonb_ivm \
    -f test/sql/01_merge_shallow.sql \
    > test/results/01_merge_shallow.out 2>&1

# Compare with expected output
echo "Comparing results with expected output..."
if diff -u test/expected/01_merge_shallow.out test/results/01_merge_shallow.out; then
    echo "✓ SQL integration tests PASSED"
    EXIT_CODE=0
else
    echo "✗ SQL integration tests FAILED - see diff above"
    EXIT_CODE=1
fi

# Cleanup
echo "Cleaning up test database..."
$PG_BIN/dropdb test_jsonb_ivm

echo "========================================="
if [ $EXIT_CODE -eq 0 ]; then
    echo "ALL TESTS PASSED ✓"
else
    echo "TESTS FAILED ✗"
fi
echo "========================================="

exit $EXIT_CODE
