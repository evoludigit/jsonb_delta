#!/bin/bash
# POC Benchmark Runner
# Executes complete 3-day POC benchmark suite for JSONB IVM extension

set -e  # Exit on error

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RESULTS_DIR="$PROJECT_ROOT/results"
TEST_DIR="$PROJECT_ROOT/test"

# Database configuration
DB_NAME="${DB_NAME:-jsonb_ivm_test}"
PG_VERSION="${PG_VERSION:-17}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() {
    echo -e "${BLUE}[$(date +'%H:%M:%S')]${NC} $1"
}

success() {
    echo -e "${GREEN}✓${NC} $1"
}

error() {
    echo -e "${RED}✗${NC} $1"
    exit 1
}

warn() {
    echo -e "${YELLOW}⚠${NC} $1"
}

# Create necessary directories
mkdir -p "$RESULTS_DIR"
mkdir -p "$TEST_DIR/fixtures"
mkdir -p "$TEST_DIR/sql"

log "========================================="
log "JSONB IVM POC Benchmark Suite"
log "========================================="
echo ""

# Check prerequisites
log "Checking prerequisites..."

if ! command -v psql &> /dev/null; then
    error "psql not found. Please install PostgreSQL client."
fi

if ! command -v cargo &> /dev/null; then
    error "cargo not found. Please install Rust toolchain."
fi

if ! command -v pg_config &> /dev/null; then
    warn "pg_config not found. Using default PostgreSQL installation."
fi

success "Prerequisites OK"
echo ""

# Phase 0: Build and install extension
log "Phase 0: Building extension..."

cd "$PROJECT_ROOT"

log "Running cargo pgrx install..."
if cargo pgrx install --release --pg-config="$(which pg_config)" 2>&1 | tee "$RESULTS_DIR/build.log"; then
    success "Extension built and installed"
else
    error "Extension build failed. Check $RESULTS_DIR/build.log"
fi

echo ""

# Phase 1: Setup test database
log "Phase 1: Setting up test database..."

# Drop and recreate database
log "Creating fresh test database: $DB_NAME"
psql -d postgres -c "DROP DATABASE IF EXISTS $DB_NAME;" 2>/dev/null || true
psql -d postgres -c "CREATE DATABASE $DB_NAME;" || error "Failed to create database"

# Load extension
log "Loading jsonb_ivm extension..."
psql -d "$DB_NAME" -c "CREATE EXTENSION IF NOT EXISTS jsonb_ivm;" || error "Failed to load extension"

success "Test database ready"
echo ""

# Phase 1.1: Generate test data
log "Phase 1.1: Generating CQRS test data..."

if [ -f "$TEST_DIR/fixtures/generate_cqrs_data.sql" ]; then
    log "Running test data generator..."
    if psql -d "$DB_NAME" -f "$TEST_DIR/fixtures/generate_cqrs_data.sql" > "$RESULTS_DIR/data_generation.log" 2>&1; then
        success "Test data generated (500 DNS servers, 100 configs, 500 allocations)"
    else
        error "Data generation failed. Check $RESULTS_DIR/data_generation.log"
    fi
else
    warn "Test data generator not found at $TEST_DIR/fixtures/generate_cqrs_data.sql"
    warn "Please create this file according to POC_IMPLEMENTATION_PLAN.md"
    exit 1
fi

echo ""

# Phase 1.2: Baseline benchmark
log "Phase 1.2: Running baseline benchmarks..."

if [ -f "$TEST_DIR/benchmark_baseline.sql" ]; then
    log "Measuring native PostgreSQL performance..."
    if psql -d "$DB_NAME" -f "$TEST_DIR/benchmark_baseline.sql" > "$RESULTS_DIR/baseline_day1.txt" 2>&1; then
        success "Baseline benchmarks complete"

        # Extract key timing
        echo ""
        log "Baseline Results:"
        grep -E "Time:|total time:" "$RESULTS_DIR/baseline_day1.txt" | head -5
    else
        error "Baseline benchmark failed"
    fi
else
    warn "Baseline benchmark script not found"
    warn "Expected: $TEST_DIR/benchmark_baseline.sql"
fi

echo ""

# Phase 2: POC Operation Benchmarks
log "Phase 2: Running POC operation benchmarks..."

# Check if POC operations are implemented
log "Checking for jsonb_array_update_where function..."
if psql -d "$DB_NAME" -c "SELECT jsonb_array_update_where('{\"a\":[1]}'::jsonb, ARRAY['a'], 'x', '1'::jsonb, '{\"y\":2}'::jsonb);" &> /dev/null; then
    success "jsonb_array_update_where found"
else
    warn "jsonb_array_update_where not implemented yet"
    warn "Skipping operation-specific benchmarks"
    exit 0
fi

if [ -f "$TEST_DIR/benchmark_array_update_where.sql" ]; then
    log "Benchmarking jsonb_array_update_where..."
    if psql -d "$DB_NAME" -f "$TEST_DIR/benchmark_array_update_where.sql" > "$RESULTS_DIR/poc_array_update_day2.txt" 2>&1; then
        success "Array update benchmarks complete"

        # Extract speedup results
        echo ""
        log "Array Update Results:"
        grep -E "Time:|Expected Results:" "$RESULTS_DIR/poc_array_update_day2.txt" | head -10
    else
        error "Array update benchmark failed"
    fi
else
    warn "Array update benchmark not found at $TEST_DIR/benchmark_array_update_where.sql"
fi

echo ""

# Phase 2.2: End-to-end cascade benchmark
if [ -f "$TEST_DIR/benchmark_e2e_cascade.sql" ]; then
    log "Running end-to-end cascade benchmark..."
    if psql -d "$DB_NAME" -f "$TEST_DIR/benchmark_e2e_cascade.sql" > "$RESULTS_DIR/e2e_cascade_day2.txt" 2>&1; then
        success "E2E cascade benchmarks complete"

        # Calculate speedup
        echo ""
        log "Calculating overall speedup..."
        python3 << 'EOF'
import re
import sys

try:
    with open('results/e2e_cascade_day2.txt') as f:
        content = f.read()

    times = re.findall(r'Time: ([\d.]+) ms', content)

    if len(times) >= 6:
        native_total = sum(float(t) for t in times[:3])
        rust_total = sum(float(t) for t in times[3:6])
        speedup = native_total / rust_total if rust_total > 0 else 0

        print(f"\n=== PERFORMANCE SUMMARY ===")
        print(f"Native SQL total: {native_total:.2f}ms")
        print(f"Custom Rust total: {rust_total:.2f}ms")
        print(f"Speedup: {speedup:.1f}x")

        if speedup >= 2.0:
            print(f"\n✓ SUCCESS: Target 2x speedup achieved!")
            sys.exit(0)
        else:
            print(f"\n⚠ WARNING: Target 2x speedup NOT achieved")
            print(f"Consider pivoting to alternative approach")
            sys.exit(1)
    else:
        print("⚠ Could not parse benchmark times")
        sys.exit(2)
except FileNotFoundError:
    print("⚠ Benchmark results file not found")
    sys.exit(2)
except Exception as e:
    print(f"⚠ Error analyzing results: {e}")
    sys.exit(2)
EOF

        SPEEDUP_STATUS=$?
    else
        error "E2E cascade benchmark failed"
    fi
else
    warn "E2E cascade benchmark not found at $TEST_DIR/benchmark_e2e_cascade.sql"
fi

echo ""

# Phase 3: Memory profiling
log "Phase 3: Memory profiling..."

if [ -f "$TEST_DIR/profile_memory.sql" ]; then
    log "Running memory profiling..."
    if psql -d "$DB_NAME" -f "$TEST_DIR/profile_memory.sql" > "$RESULTS_DIR/memory_profile_day3.txt" 2>&1; then
        success "Memory profiling complete"

        echo ""
        log "Memory Profile Summary:"
        grep -E "memory delta:|Memory|KB|MB" "$RESULTS_DIR/memory_profile_day3.txt" | tail -10
    else
        warn "Memory profiling had issues (may be OK)"
    fi
else
    warn "Memory profile script not found at $TEST_DIR/profile_memory.sql"
fi

echo ""

# Summary
log "========================================="
log "POC Benchmark Suite Complete"
log "========================================="
echo ""

log "Results saved to: $RESULTS_DIR/"
log ""
log "Generated files:"
ls -lh "$RESULTS_DIR"/*.txt "$RESULTS_DIR"/*.log 2>/dev/null || true

echo ""
log "Next steps:"
echo "1. Review results in $RESULTS_DIR/"
echo "2. Run: cat $RESULTS_DIR/baseline_day1.txt"
echo "3. Run: cat $RESULTS_DIR/e2e_cascade_day2.txt"
echo "4. Make decision: PROCEED / PIVOT / CONDITIONAL"
echo ""

if [ ${SPEEDUP_STATUS:-1} -eq 0 ]; then
    success "POC shows promising results (>2x speedup achieved)"
    echo ""
    echo "Recommendation: PROCEED with full implementation"
elif [ ${SPEEDUP_STATUS:-1} -eq 1 ]; then
    warn "POC shows mixed results (<2x speedup)"
    echo ""
    echo "Recommendation: Review results carefully, consider CONDITIONAL approach"
else
    warn "POC results inconclusive"
    echo ""
    echo "Recommendation: Review benchmark output manually"
fi

echo ""
log "For detailed decision framework, see: POC_IMPLEMENTATION_PLAN.md"
