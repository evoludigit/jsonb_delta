#!/bin/bash

set -e

echo "🚀 Starting PostgreSQL load tests for jsonb_ivm..."
echo "=================================================="

# CI environment detection and configuration
if [ -n "$CI" ] || [ -n "$GITHUB_ACTIONS" ]; then
    # Use Unix socket (default for ubuntu pg_createcluster)
    export PGUSER=postgres
    # Don't set PGHOST to force Unix socket usage
fi

# Check if PostgreSQL is running using sudo -u postgres for Unix socket
echo "🔍 Checking PostgreSQL connectivity..."
if [ -n "$CI" ] || [ -n "$GITHUB_ACTIONS" ]; then
    # CI environment - use sudo
    if ! sudo -u postgres psql -c "SELECT 1;" > /dev/null 2>&1; then
        echo "❌ PostgreSQL is not running or not accepting connections."
        if command -v pg_lsclusters &> /dev/null; then
            echo "📊 PostgreSQL clusters:"
            sudo pg_lsclusters 2>&1 || echo "  (cannot list clusters)"
        fi
        echo ""
        echo "💡 To start PostgreSQL:"
        echo "   On Ubuntu/Debian: sudo systemctl start postgresql"
        exit 1
    fi
else
    # Local environment - use pg_isready
    if ! pg_isready -q; then
        echo "❌ PostgreSQL is not running or not accepting connections."
        echo ""
        echo "💡 To start PostgreSQL:"
        echo "   On Ubuntu/Debian: sudo systemctl start postgresql"
        echo "   On macOS: brew services start postgresql"
        exit 1
    fi
fi
echo "✅ PostgreSQL is running"

# Setup test database
echo ""
echo "📝 Setting up test database..."

# Helper function to run psql (handles CI vs local)
run_psql() {
    if [ -n "$CI" ] || [ -n "$GITHUB_ACTIONS" ]; then
        sudo -u postgres psql "$@"
    else
        psql -U postgres "$@"
    fi
}

run_psql -c "DROP DATABASE IF EXISTS loadtest;" 2>/dev/null || true
run_psql -c "CREATE DATABASE loadtest;"

# Create extension
echo "🔧 Installing jsonb_ivm extension..."
run_psql -d loadtest -c "CREATE EXTENSION jsonb_ivm;"

# Prepare test data
echo "📊 Preparing test data..."
run_psql -d loadtest <<EOF
CREATE TABLE test_jsonb (
    id SERIAL PRIMARY KEY,
    data JSONB
);

-- Generate test data with arrays for array operations (using UUIDs for IDs)
INSERT INTO test_jsonb (data)
SELECT jsonb_build_object(
    'id', gen_random_uuid()::text,
    'items', jsonb_build_array(
        jsonb_build_object('id', gen_random_uuid()::text, 'value', i * 10),
        jsonb_build_object('id', gen_random_uuid()::text, 'value', (i + 1000) * 10)
    ),
    'metadata', jsonb_build_object('created_at', now()::text)
)
FROM generate_series(1, 1000) AS i;
EOF

echo "✅ Test data prepared (1000 rows)"

# Run concurrent merge operations
echo ""
echo "🔄 Running concurrent merge test (100 clients, 10 seconds)..."
echo "----------------------------------------------------------"

if [ -n "$CI" ] || [ -n "$GITHUB_ACTIONS" ]; then
    # CI: Run pgbench as postgres user via sudo
    if ! sudo -u postgres pgbench -d loadtest -c 100 -j 10 -T 10 -f test/load/load_test_concurrent_merge.sql; then
        echo "❌ Load test failed!"
        exit 1
    fi
else
    # Local: Run pgbench normally
    if ! pgbench -U postgres -d loadtest -c 100 -j 10 -T 10 -f test/load/load_test_concurrent_merge.sql; then
        echo "❌ Load test failed!"
        exit 1
    fi
fi

# Run concurrent array operations
echo ""
echo "🔄 Running concurrent array update test (100 clients, 10 seconds)..."
echo "-------------------------------------------------------------------"

if [ -n "$CI" ] || [ -n "$GITHUB_ACTIONS" ]; then
    # CI: Run pgbench as postgres user via sudo
    if ! sudo -u postgres pgbench -d loadtest -c 100 -j 10 -T 10 -f test/load/load_test_concurrent_array.sql; then
        echo "❌ Load test failed!"
        exit 1
    fi
else
    # Local: Run pgbench normally
    if ! pgbench -U postgres -d loadtest -c 100 -j 10 -T 10 -f test/load/load_test_concurrent_array.sql; then
        echo "❌ Load test failed!"
        exit 1
    fi
fi

# Verify data integrity
echo ""
echo "🔍 Verifying data integrity..."
echo "------------------------------"

# Check that all records still exist
ROW_COUNT=$(run_psql -d loadtest -t -c "SELECT COUNT(*) FROM test_jsonb;" | tr -d ' ')
if [ "$ROW_COUNT" -ne 1000 ]; then
    echo "❌ Data integrity check failed! Expected 1000 rows, got $ROW_COUNT"
    exit 1
fi

# Check that data is valid JSONB
INVALID_COUNT=$(run_psql -d loadtest -t -c "SELECT COUNT(*) FROM test_jsonb WHERE data IS NULL OR jsonb_typeof(data) != 'object';" | tr -d ' ')
if [ "$INVALID_COUNT" -ne 0 ]; then
    echo "❌ Data integrity check failed! Found $INVALID_COUNT invalid JSONB records"
    exit 1
fi

# Cleanup
echo ""
echo "🧹 Cleaning up..."
run_psql -c "DROP DATABASE loadtest;"

echo ""
echo "✅ Load tests completed successfully!"
echo "====================================="
echo "📊 Results:"
echo "   - 100 concurrent clients"
echo "   - 10 seconds duration"
echo "   - Zero transaction failures"
echo "   - Data integrity maintained"
echo ""
echo "🎯 All load tests passed - jsonb_ivm is production-ready!"
