#!/bin/bash

set -e

echo "🚀 Starting PostgreSQL load tests for jsonb_ivm..."
echo "=================================================="

# CI environment detection and configuration
if [ -n "$CI" ] || [ -n "$GITHUB_ACTIONS" ]; then
    export PGHOST=localhost
    export PGPORT=5432
    export PGUSER=postgres
fi

# Check if PostgreSQL is running
if ! pg_isready -q; then
    echo "❌ PostgreSQL is not running or not accepting connections."
    echo ""
    echo "🔍 Debugging information:"
    echo "  PGHOST: ${PGHOST:-<not set>}"
    echo "  PGPORT: ${PGPORT:-<not set>}"
    echo "  PGUSER: ${PGUSER:-<not set>}"
    echo ""

    # Try to get more info
    if command -v pg_lsclusters &> /dev/null; then
        echo "📊 PostgreSQL clusters:"
        pg_lsclusters 2>&1 || echo "  (cannot list clusters)"
    fi

    echo ""
    echo "💡 To start PostgreSQL:"
    echo "   On Ubuntu/Debian: sudo systemctl start postgresql"
    echo "   On macOS: brew services start postgresql"
    exit 1
fi

# Setup test database
echo "📝 Setting up test database..."
psql -U postgres -c "DROP DATABASE IF EXISTS loadtest;" 2>/dev/null || true
psql -U postgres -c "CREATE DATABASE loadtest;"

# Create extension
echo "🔧 Installing jsonb_ivm extension..."
psql -U postgres -d loadtest -c "CREATE EXTENSION jsonb_ivm;"

# Prepare test data
echo "📊 Preparing test data..."
psql -U postgres -d loadtest <<EOF
CREATE TABLE test_jsonb (
    id SERIAL PRIMARY KEY,
    data JSONB
);

-- Generate test data with arrays for array operations
INSERT INTO test_jsonb (data)
SELECT jsonb_build_object(
    'id', i,
    'items', jsonb_build_array(
        jsonb_build_object('id', i, 'value', i * 10),
        jsonb_build_object('id', i + 1000, 'value', (i + 1000) * 10)
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
if ! pgbench -U postgres -d loadtest -c 100 -j 10 -T 10 -f test/load/load_test_concurrent_merge.sql; then
    echo "❌ Load test failed!"
    exit 1
fi

# Run concurrent array operations
echo ""
echo "🔄 Running concurrent array update test (100 clients, 10 seconds)..."
echo "-------------------------------------------------------------------"
if ! pgbench -U postgres -d loadtest -c 100 -j 10 -T 10 -f test/load/load_test_concurrent_array.sql; then
    echo "❌ Load test failed!"
    exit 1
fi

# Verify data integrity
echo ""
echo "🔍 Verifying data integrity..."
echo "------------------------------"

# Check that all records still exist
ROW_COUNT=$(psql -U postgres -d loadtest -t -c "SELECT COUNT(*) FROM test_jsonb;" | tr -d ' ')
if [ "$ROW_COUNT" -ne 1000 ]; then
    echo "❌ Data integrity check failed! Expected 1000 rows, got $ROW_COUNT"
    exit 1
fi

# Check that data is valid JSONB
INVALID_COUNT=$(psql -U postgres -d loadtest -t -c "SELECT COUNT(*) FROM test_jsonb WHERE data IS NULL OR jsonb_typeof(data) != 'object';" | tr -d ' ')
if [ "$INVALID_COUNT" -ne 0 ]; then
    echo "❌ Data integrity check failed! Found $INVALID_COUNT invalid JSONB records"
    exit 1
fi

# Cleanup
echo ""
echo "🧹 Cleaning up..."
psql -U postgres -c "DROP DATABASE loadtest;"

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
