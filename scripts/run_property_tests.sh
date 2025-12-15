#!/bin/bash

set -e

echo "🧪 Running property-based tests for jsonb_delta..."
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
