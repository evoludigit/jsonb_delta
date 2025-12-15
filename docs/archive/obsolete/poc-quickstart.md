# JSONB IVM POC - Quick Start Guide

**Goal**: Validate 5-10x performance improvement for surgical JSONB updates in 3 days

---

## TL;DR - Execute POC

```bash
# 1. Read the detailed plan
cat POC_IMPLEMENTATION_PLAN.md

# 2. Implement Day 1 operations (see plan for code)
#    - Add jsonb_array_update_where to src/lib.rs
#    - Create test/fixtures/generate_cqrs_data.sql
#    - Create test/benchmark_baseline.sql

# 3. Run automated benchmarks
./scripts/run_poc_benchmarks.sh

# 4. Review results
cat results/e2e_cascade_day2.txt

# 5. Make decision based on data
```

---

## What This POC Tests

### 4 Custom Operations

1. **`jsonb_array_update_where`** ⭐ CRITICAL
   - Update 1 element in 50-element array without rebuilding entire array
   - Target: >3x faster than native SQL

2. **`jsonb_merge_at_path`** ⭐ HIGH VALUE
   - Merge JSONB at nested path without full document rebuild
   - Target: >2x faster than native SQL

3. **`jsonb_has_path_changed`** (Optional)
   - Detect if specific path changed between old/new JSONB
   - Target: >2x faster than native comparison

4. **`jsonb_array_upsert_where`** (Optional)
   - Atomic insert-or-update in array
   - Target: >2x faster than separate operations

### Realistic Scenario

**CQRS cascade**: DNS server update propagates through 3 levels
- Leaf: `v_dns_server` (500 records)
- Intermediate: `tv_network_configuration` (100 records, 50 DNS servers each)
- Top: `tv_allocation` (500 records, 50KB documents)

**Current problem**: Updating 1 DNS server triggers full rebuild of 100 network configs + 500 allocations = 2+ seconds

**Target**: <200ms with surgical updates (10x improvement)

---

## Implementation Checklist

### Day 1: Foundation (8 hours)

#### ✅ Task 1.1: Test Data Generator (1 hour)
**File**: `test/fixtures/generate_cqrs_data.sql`

Copy complete SQL from `POC_IMPLEMENTATION_PLAN.md` Section 1.1.1

**Verify**:
```bash
psql -d jsonb_ivm_test -f test/fixtures/generate_cqrs_data.sql
# Expected: 500 DNS servers, 100 configs, 500 allocations
```

#### ✅ Task 1.2: Baseline Benchmark (1 hour)
**File**: `test/benchmark_baseline.sql`

Copy complete SQL from `POC_IMPLEMENTATION_PLAN.md` Section 1.1.2

**Verify**:
```bash
psql -d jsonb_ivm_test -f test/benchmark_baseline.sql > results/baseline.txt
grep "Time:" results/baseline.txt
# Expected: 150-300ms total cascade time
```

#### ✅ Task 1.3: Implement `jsonb_array_update_where` (4 hours)
**File**: `src/lib.rs`

Copy complete Rust code from `POC_IMPLEMENTATION_PLAN.md` Section 1.2.1

**Key function signature**:
```rust
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where(
    target: Option<JsonB>,
    array_path: Vec<&str>,
    match_key: &str,
    match_value: JsonB,
    updates: JsonB,
) -> Option<JsonB>
```

**Build & test**:
```bash
cargo pgrx test
# Expected: All unit tests pass

cargo pgrx install --release
psql -d jsonb_ivm_test -c "DROP EXTENSION IF EXISTS jsonb_ivm CASCADE; CREATE EXTENSION jsonb_ivm;"
psql -d jsonb_ivm_test -f test/sql/02_array_update_where.sql
# Expected: All tests return TRUE
```

#### ✅ Task 1.4: Integration Tests (1 hour)
**File**: `test/sql/02_array_update_where.sql`

Copy complete SQL from `POC_IMPLEMENTATION_PLAN.md` Section 1.2.3

**Verify**:
```bash
psql -d jsonb_ivm_test -f test/sql/02_array_update_where.sql
# Expected: "All tests should return TRUE"
```

---

### Day 2: Benchmarking (8 hours)

#### ✅ Task 2.1: Operation Benchmark (3 hours)
**File**: `test/benchmark_array_update_where.sql`

Copy complete SQL from `POC_IMPLEMENTATION_PLAN.md` Section 2.1

**Run**:
```bash
psql -d jsonb_ivm_test -f test/benchmark_array_update_where.sql > results/array_update.txt

# Analyze results
grep "Time:" results/array_update.txt
```

**Success criteria**:
- Benchmark 1 (single update): >2x faster
- Benchmark 2 (cascade): >3x faster
- Benchmark 3 (stress): >5x faster

#### ✅ Task 2.2: Implement `jsonb_merge_at_path` (3 hours)
**File**: `src/lib.rs`

Copy complete Rust code from `POC_IMPLEMENTATION_PLAN.md` Section 2.2

**Build & test**:
```bash
cargo pgrx test
cargo pgrx install --release
psql -d jsonb_ivm_test -c "DROP EXTENSION IF EXISTS jsonb_ivm CASCADE; CREATE EXTENSION jsonb_ivm;"
psql -d jsonb_ivm_test -f test/sql/03_merge_at_path.sql
```

#### ✅ Task 2.3: End-to-End Cascade (2 hours)
**File**: `test/benchmark_e2e_cascade.sql`

Copy complete SQL from `POC_IMPLEMENTATION_PLAN.md` Section 2.3

**Run**:
```bash
psql -d jsonb_ivm_test -f test/benchmark_e2e_cascade.sql > results/e2e.txt

# Calculate speedup
python3 << 'EOF'
import re
with open('results/e2e.txt') as f:
    times = re.findall(r'Time: ([\d.]+) ms', f.read())
native = sum(float(t) for t in times[:3])
rust = sum(float(t) for t in times[3:6])
print(f"Native: {native:.1f}ms, Rust: {rust:.1f}ms, Speedup: {native/rust:.1f}x")
EOF
```

**Decision point**: If speedup <2x, STOP and reconsider approach

---

### Day 3: Memory & Decision (6 hours)

#### ✅ Task 3.1: Memory Profile (2 hours)
**File**: `test/profile_memory.sql`

Copy complete SQL from `POC_IMPLEMENTATION_PLAN.md` Section 3.1

**Run**:
```bash
psql -d jsonb_ivm_test -f test/profile_memory.sql > results/memory.txt
```

**Success criteria**: Memory usage <1.5x native

#### ✅ Task 3.2: Optional Operations (2 hours)
**File**: `src/lib.rs`

If Day 2 results are promising, implement Operations 3 & 4 from `POC_IMPLEMENTATION_PLAN.md` Section 3.2

#### ✅ Task 3.3: Decision Report (2 hours)

**Run complete suite**:
```bash
./scripts/run_poc_benchmarks.sh
```

**Review results**:
```bash
cat results/baseline_day1.txt
cat results/poc_array_update_day2.txt
cat results/e2e_cascade_day2.txt
cat results/memory_profile_day3.txt
```

**Make decision**:
- ✅ **PROCEED**: >2x improvement, memory OK, no issues
- ⚠️ **CONDITIONAL**: Some ops >5x, others <1.5x → Implement selectively
- ❌ **PIVOT**: <1.5x improvement → Optimize triggers instead

---

## Decision Framework

### PROCEED Criteria (All must be met)

- [x] End-to-end cascade >2x faster
- [x] Array update >3x faster on 50-element arrays
- [x] Memory usage <1.5x native
- [x] Zero crashes in stress tests (10K ops)
- [x] PostgreSQL 13-17 compatible

**Next steps if proceeding**:
1. Implement remaining operations
2. Add comprehensive benchmarks to CI
3. Write production documentation
4. Plan v0.2.0 release

**Estimated effort**: 2-3 weeks

---

### PIVOT Criteria (Any one triggers pivot)

- [x] Overall speedup <1.5x
- [x] Any operation slower than native on realistic workloads
- [x] Memory usage >2x native
- [x] Crashes or correctness issues found

**Alternative approach**:
1. Optimize trigger logic (use `jsonb_set` instead of `jsonb_build_object`)
2. Add dirty flags for lazy evaluation
3. Evaluate `pg_ivm` extension
4. Consider denormalization strategy changes

**Estimated effort**: 1 week

---

### CONDITIONAL Criteria

- [x] Some operations show >5x improvement
- [x] Others show <1.5x improvement
- [x] No critical correctness issues

**Selective implementation**:
- Implement only high-value operations (>3x speedup)
- Skip low-value operations (<1.5x speedup)
- Document when to use custom vs native

**Estimated effort**: 1-2 weeks

---

## Key Performance Expectations

### Realistic Baseline (Native SQL)

| Operation | Time | Notes |
|-----------|------|-------|
| Update single DNS server | 1-5ms | Leaf view |
| Propagate to network config | 50-100ms | Re-aggregate 50-element array |
| Propagate to allocation | 100-200ms | Replace 50KB object |
| **Total cascade** | **150-300ms** | **Baseline to beat** |

### Target Performance (Custom Rust)

| Operation | Time | Speedup | Notes |
|-----------|------|---------|-------|
| Update single DNS server | 1-5ms | 1x | Same as native |
| Propagate to network config | 10-20ms | 5x | Surgical array update |
| Propagate to allocation | 20-40ms | 5x | Surgical nested merge |
| **Total cascade** | **30-60ms** | **5x** | **Target** |

---

## Common Pitfalls

### ❌ Pitfall 1: Benchmark Warm vs Cold Cache

**Problem**: Running benchmark twice on same data shows faster results due to cache

**Solution**: Reset cache between runs
```sql
-- Add to benchmark scripts
SELECT pg_stat_reset();
DISCARD PLANS;
```

### ❌ Pitfall 2: Small Test Data

**Problem**: Benefits only visible with large arrays/documents

**Solution**: Use realistic data sizes
- Arrays: 50+ elements (not 5)
- Documents: 10-50KB (not 1KB)
- Iterations: 100+ (not 10)

### ❌ Pitfall 3: Ignoring Memory

**Problem**: Functions work but leak memory over time

**Solution**: Run stress test with memory monitoring
```sql
-- Run 10K operations, check memory growth
SELECT pg_backend_memory_contexts();
```

### ❌ Pitfall 4: Premature Optimization

**Problem**: Implementing all 4 operations before validating first one

**Solution**: Implement Operation 1 (`jsonb_array_update_where`) FIRST, benchmark, then decide

---

## Automated Quick-Run

If you've implemented all the code from the plan:

```bash
# Full automated run
./scripts/run_poc_benchmarks.sh

# This will:
# 1. Build extension
# 2. Create test database
# 3. Generate test data
# 4. Run baseline benchmarks
# 5. Run POC benchmarks
# 6. Calculate speedup
# 7. Display decision recommendation
```

**Expected output**:
```
=== PERFORMANCE SUMMARY ===
Native SQL total: 250.00ms
Custom Rust total: 50.00ms
Speedup: 5.0x

✓ SUCCESS: Target 2x speedup achieved!

Recommendation: PROCEED with full implementation
```

---

## Troubleshooting

### Build Issues

```bash
# If cargo pgrx fails:
cargo clean
cargo pgrx init --pg17 /usr/bin/pg_config
cargo pgrx install --release

# If extension won't load:
psql -d postgres -c "DROP EXTENSION IF EXISTS jsonb_ivm CASCADE;"
psql -d postgres -c "CREATE EXTENSION jsonb_ivm;"
```

### Performance Not Improving

**Check**:
1. Are you using realistic data sizes? (50+ element arrays)
2. Is cache affecting results? (Run with cold cache)
3. Is baseline using optimized SQL? (Use `jsonb_set` not `jsonb_build_object`)
4. Is PostgreSQL version correct? (13-17)

**Debug**:
```sql
EXPLAIN ANALYZE
UPDATE tv_network_configuration
SET data = jsonb_array_update_where(...);
-- Look for sequential scans, index usage
```

### Memory Issues

```bash
# Run with valgrind (if available)
valgrind --leak-check=full \
  psql -d jsonb_ivm_test -c "SELECT jsonb_array_update_where(...);"

# Check PostgreSQL logs
tail -f /var/log/postgresql/postgresql-17-main.log
```

---

## Questions Before Starting?

Review the **PostgreSQL/Rust specialist analysis** in the original prompt (`/tmp/jsonb-ivm-rust-postgresql-specialist-prompt.md`):

**Key insights**:
1. Real bottleneck might be trigger design, not JSONB ops
2. Native `jsonb_set` is already highly optimized
3. Benefits only visible for large documents (>10KB) and arrays (>50 elements)
4. Function call overhead matters for small operations

**Critical validation**: Day 1 baseline should show WHERE the time is spent:
- If 80% is in data fetching → Rust won't help
- If 80% is in JSONB manipulation → Rust might help 5x
- If 80% is in lock contention → Different architecture needed

---

## Success Metrics Checklist

Print this and check off as you go:

**Day 1** (Foundation):
- [ ] Test data generated (500 DNS, 100 configs, 500 allocations)
- [ ] Baseline benchmark runs successfully
- [ ] Baseline shows 150-300ms cascade time
- [ ] `jsonb_array_update_where` implemented
- [ ] All unit tests pass (6+ tests)
- [ ] All integration tests pass (6+ tests)

**Day 2** (Benchmarking):
- [ ] Array update >2x faster (single operation)
- [ ] Array update >3x faster (cascade)
- [ ] Array update >5x faster (stress test)
- [ ] `jsonb_merge_at_path` implemented and tested
- [ ] End-to-end cascade >2x faster overall

**Day 3** (Decision):
- [ ] Memory usage <1.5x native
- [ ] No memory leaks detected
- [ ] No crashes in 10K operation stress test
- [ ] Decision report generated
- [ ] Clear recommendation: PROCEED / PIVOT / CONDITIONAL

---

## Final Checklist Before Decision

- [ ] All benchmarks show consistent results (run 3 times)
- [ ] Tested on cold cache (not just warm)
- [ ] Tested realistic data sizes (not toy examples)
- [ ] Memory profiling shows no leaks
- [ ] Stress tests show linear scaling (not degradation)
- [ ] Compared against OPTIMIZED native SQL (not worst-case)
- [ ] Documented decision rationale with data
- [ ] Considered maintenance burden of Rust extension

---

## Resources

- **Full Implementation Plan**: `POC_IMPLEMENTATION_PLAN.md`
- **Benchmark Runner**: `./scripts/run_poc_benchmarks.sh`
- **PostgreSQL JSONB Docs**: https://www.postgresql.org/docs/current/datatype-json.html
- **pgrx Documentation**: https://github.com/pgcentralfoundation/pgrx
- **Existing Code**: `src/lib.rs` (jsonb_merge_shallow example)

---

**Good luck! Let the data decide. 🚀**
