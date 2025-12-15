# New Benchmark Plans - UUID & Tree Composition

**Date**: 2025-12-08
**Status**: Running

## Overview

Created two new benchmark suites to explore performance characteristics beyond integer array element updates:

1. **UUID Array Element Matching** - Compare performance with UUID strings vs integers
2. **Deep JSONB Tree Composition** - Non-array updates (nested object composition)

---

## Benchmark 1: UUID vs Integer ID Performance

### Motivation

Current benchmarks use **integer IDs in array elements** (e.g., `{"id": 42, "ip": "..."}`). Need to validate performance with **UUID strings** which are common in distributed systems.

**Research question**: Does the 8-way loop unrolling optimization work for UUID string matching, or is it specific to integers?

### Test Design

**Test data structure**:
```json
{
  "dns_servers": [
    {"id": "99458cbd-5ea9-4543-87a6-e5258d684768", "ip": "8.8.8.8", ...},
    {"id": "61375f5c-e6c1-4a9e-88c8-e364c155f9cb", "ip": "1.1.1.1", ...}
  ]
}
```

**Benchmarks**:
1. Single UUID element update (50-element array)
2. UUID-based cascade (100 rows)
3. String vs Integer comparison overhead at different array sizes (10, 50, 100 elements)

**Files**:
- `test/fixtures/generate_uuid_test_data.sql` - Creates UUID-based test data
- `test/benchmark_uuid_performance.sql` - Runs UUID vs Integer benchmarks

### Expected Findings

**Hypothesis**: UUID string matching will be **slower** than integer matching because:
1. String comparison is more expensive than integer comparison
2. Loop unrolling optimization targets `i64` type (integers), not strings
3. UUID strings use generic `find_by_jsonb_value()` path (no optimization)

**Predicted performance**:
- Integer IDs: 0.332 ms (baseline, with loop unrolling)
- UUID IDs: 0.5-0.8 ms (1.5-2.4× slower, no loop unrolling)

**Impact**: If UUID performance is significantly worse, may need to add string-specific optimizations or document the performance trade-off.

---

## Benchmark 2: Deep JSONB Tree Composition

### Motivation

Current benchmarks focus on **array updates** (`jsonb_array_update_where`). Need to validate `jsonb_merge_at_path` performance for **non-array composition patterns** common in CQRS.

**Research question**: Does `jsonb_merge_at_path` provide speedup for nested object updates (child data payload composition)?

### Test Design

**Test data structure** (4-level deep JSONB tree):
```json
{
  "id": 1,
  "username": "user1",
  "email": "user1@example.com",
  "address": {                          ← Level 2
    "street": "100 Main St",
    "city": "Los Angeles",
    "state": "CA"
  },
  "billing": {                          ← Level 2
    "card_type": "Visa",
    "subscription": {                   ← Level 3
      "tier": "premium",
      "monthly_cost": 49.99
    }
  },
  "preferences": {                      ← Level 2
    "ui": {                             ← Level 3
      "theme": "dark",
      "language": "en"
    },
    "notifications": {                  ← Level 3
      "enabled": true,
      "email_frequency": "daily"
    }
  }
}
```

**Benchmarks**:
1. Single-level nested update (`address.city`)
2. Deep nested update (`billing.subscription.tier`)
3. Multi-field update at same level (`preferences.ui.*`)
4. CQRS composition - propagate child table changes to 100 profiles
5. Multi-level cascade (Address → Profile → Aggregated Report)
6. Batch update - 100 profiles

**Comparison**:
- **Native SQL**: `jsonb_set()` with full object replacement
- **jsonb_merge_at_path**: Partial updates (merge only changed fields)

**Files**:
- `test/fixtures/generate_tree_composition_data.sql` - Creates deep JSONB trees (1000 user profiles)
- `test/benchmark_tree_composition.sql` - Runs tree composition benchmarks

### Expected Findings

**Hypothesis**: `jsonb_merge_at_path` will provide **modest speedup** (1.5-2×) for:
1. **Multi-field updates** at same nesting level (single merge vs multiple jsonb_set calls)
2. **Partial updates** (avoids full object serialization)
3. **CQRS cascades** with incremental changes

**Predicted performance**:
- Single field update: ~1-1.2× (similar, no major advantage)
- Multi-field update: 1.5-2× faster (single pass)
- Deep path update: ~1× (path traversal dominates)
- Partial vs full replacement: 1.5-2× faster (less serialization)

**Key insight**: `jsonb_merge_at_path` value is in **reducing serialization overhead** and **single-pass multi-field updates**, not raw speed.

---

## Success Criteria

### UUID Benchmark

✅ **Success**: Document UUID string matching performance
✅ **Success**: Quantify integer vs UUID overhead
⚠️ **Action needed if UUID >2× slower**: Consider adding string-optimized path or document trade-off

### Tree Composition Benchmark

✅ **Success**: Validate `jsonb_merge_at_path` provides measurable benefit (>1.2×) for:
  - Multi-field updates
  - Partial composition
✅ **Success**: Identify use cases where `jsonb_merge_at_path` shines vs native `jsonb_set`
⚠️ **Action needed if no benefit**: Document when to use each function

---

## Analysis Plan

Once benchmarks complete:

1. **Extract key metrics**:
   - Execution times for each scenario
   - Speedup ratios (extension vs native)
   - Performance breakdown by array/tree size

2. **Create comparison tables**:
   - UUID vs Integer performance matrix
   - jsonb_merge_at_path vs jsonb_set comparison

3. **Update documentation**:
   - Add UUID performance notes to README
   - Document tree composition patterns in ACHIEVEMENT_SUMMARY.md
   - Update API reference with performance characteristics

4. **Identify optimization opportunities**:
   - If UUID performance poor → consider string-specific optimization
   - If tree composition shows no benefit → clarify use cases

---

## Benchmark Execution

**Running**:
```bash
# UUID benchmark
cargo pgrx connect pg17 < test/benchmark_uuid_performance.sql

# Tree composition benchmark
cargo pgrx connect pg17 < test/benchmark_tree_composition.sql
```

**Output files**:
- `/tmp/uuid_benchmark_results.txt`
- `/tmp/tree_composition_benchmark_results.txt`

---

**Next steps**: Wait for benchmarks to complete → Analyze results → Document findings
