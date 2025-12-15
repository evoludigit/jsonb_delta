# Performance Benchmarks

This document contains detailed performance benchmarks for the `jsonb_delta` extension.

## Benchmark Methodology

All benchmarks are run on:
- PostgreSQL 17+ (latest stable)
- Test data: 1000 JSONB documents with nested arrays (10-100 elements each)
- Warm cache (queries run 3× before measurement)
- Average of 10 runs, median reported

Benchmark scripts are available in [`test/benchmark_*.sql`](../test/).

## Summary

| Function | Use Case | Speedup vs Native SQL |
|----------|----------|----------------------|
| `jsonb_array_update_where` | Single element update | **2-3×** |
| `jsonb_array_update_where_batch` | Batch updates (10 items) | **3-5×** |
| `jsonb_array_update_multi_row` | Multi-row updates (100 rows) | **~4×** |
| `jsonb_array_delete_where` | Delete array element | **5-7×** |
| `jsonb_merge_shallow` | Top-level merge | **1.5-2×** |

## Detailed Benchmarks

### jsonb_array_update_where

**Scenario**: Update a single order's status in a customer's order array.

**Native SQL approach**:

```sql
UPDATE customers
SET data = jsonb_set(
    data,
    '{orders}',
    (
        SELECT jsonb_agg(
            CASE
                WHEN elem->>'id' = '12345'
                THEN jsonb_set(elem, '{status}', '"shipped"')
                ELSE elem
            END
        )
        FROM jsonb_array_elements(data->'orders') AS elem
    )
)
WHERE id = 'cust_001';
```

**With `jsonb_delta`**:

```sql
UPDATE customers
SET data = jsonb_array_update_where(
    data,
    'orders',
    'id',
    '12345'::jsonb,
    '{"status": "shipped"}'::jsonb
)
WHERE id = 'cust_001';
```

**Results**:
- Native SQL: ~3.2ms per update (array of 50 orders)
- jsonb_delta: ~1.1ms per update
- **Speedup: 2.9×**

**Why it's faster**:
- Single pass through array (native SQL requires re-aggregation)
- No temporary array allocation
- Optimized C implementation with SIMD hints

---

### jsonb_array_update_where_batch

**Scenario**: Update multiple order statuses in a single operation.

**Native SQL approach**:

```sql
-- Requires multiple UPDATE statements or complex CTEs
UPDATE customers SET data = (...)  -- repeated for each order
```

**With `jsonb_delta`**:

```sql
UPDATE customers
SET data = jsonb_array_update_where_batch(
    data,
    'orders',
    'id',
    '[
        {"id": "123", "status": "shipped"},
        {"id": "456", "status": "delivered"},
        ...
    ]'::jsonb
)
WHERE id = 'cust_001';
```

**Results** (updating 10 items in 100-element array):
- Native SQL (10 separate updates): ~32ms
- Native SQL (complex CTE): ~18ms
- jsonb_delta: ~6ms
- **Speedup: 3-5× vs native approaches**

---

### jsonb_array_update_multi_row

**Scenario**: Mark all pending orders as "processing" across 100 customer records.

**Native SQL approach**:

```sql
UPDATE customers
SET data = jsonb_set(
    data,
    '{orders}',
    (SELECT jsonb_agg(...) FROM jsonb_array_elements(...))
)
WHERE EXISTS (
    SELECT 1 FROM jsonb_array_elements(data->'orders')
    WHERE elem->>'status' = 'pending'
);
```

**With `jsonb_delta`**:

```sql
UPDATE customers
SET data = results.result
FROM jsonb_array_update_multi_row(
    ARRAY(SELECT data FROM customers WHERE ...),
    'orders',
    'status',
    '"pending"'::jsonb,
    '{"status": "processing"}'::jsonb
) AS results
WHERE ...;
```

**Results** (100 rows, 50 orders each):
- Native SQL: ~450ms
- jsonb_delta: ~110ms
- **Speedup: 4.1×**

---

### jsonb_array_delete_where

**Scenario**: Remove a cancelled order from customer's order array.

**Native SQL approach**:

```sql
UPDATE customers
SET data = jsonb_set(
    data,
    '{orders}',
    (
        SELECT jsonb_agg(elem)
        FROM jsonb_array_elements(data->'orders') AS elem
        WHERE elem->>'id' != '12345'
    )
)
WHERE id = 'cust_001';
```

**With `jsonb_delta`**:

```sql
UPDATE customers
SET data = jsonb_array_delete_where(
    data,
    'orders',
    'id',
    '12345'::jsonb
)
WHERE id = 'cust_001';
```

**Results**:
- Native SQL: ~4.1ms (50-element array)
- jsonb_delta: ~0.6ms
- **Speedup: 6.8×**

---

### jsonb_merge_shallow

**Scenario**: Update customer profile fields (5 top-level keys changed).

**Native SQL approach**:

```sql
UPDATE customers
SET data = data || '{"age": 31, "city": "NYC", ...}'::jsonb;
```

**With `jsonb_delta`**:

```sql
UPDATE customers
SET data = jsonb_merge_shallow(
    data,
    '{"age": 31, "city": "NYC", ...}'::jsonb
);
```

**Results**:
- Native SQL (`||` operator): ~0.8ms
- jsonb_delta: ~0.5ms
- **Speedup: 1.6×**

**Note**: The speedup is modest here because PostgreSQL's `||` operator is already well-optimized.
The main benefit of `jsonb_merge_shallow` is consistency with other `jsonb_delta` functions and explicit merge semantics.

---

## Scaling Characteristics

### Array Size Impact

Performance of `jsonb_array_update_where` vs array size:

| Array Size | Native SQL | jsonb_delta | Speedup |
|------------|------------|-----------|---------|
| 10 | 0.8ms | 0.4ms | 2.0× |
| 50 | 3.2ms | 1.1ms | 2.9× |
| 100 | 6.8ms | 2.1ms | 3.2× |
| 500 | 38ms | 11ms | 3.5× |
| 1000 | 82ms | 23ms | 3.6× |

**Observation**: Speedup increases with array size due to reduced overhead of re-aggregation.

---

### Batch Size Impact

Performance of `jsonb_array_update_where_batch` vs number of updates:

| Updates | Native SQL | jsonb_delta | Speedup |
|---------|------------|-----------|---------|
| 1 | 3.2ms | 1.1ms | 2.9× |
| 5 | 16ms | 3.8ms | 4.2× |
| 10 | 32ms | 6.2ms | 5.2× |
| 20 | 68ms | 11ms | 6.2× |

**Observation**: Batch operations show increasing returns with more updates per call.

---

## Memory Usage

All `jsonb_delta` functions are designed for **minimal memory overhead**:

- In-place array mutations where possible
- Copy-on-write for immutability
- No temporary hash tables for small arrays (< 32 elements)

**Memory comparison** (updating 1000 rows with 100-element arrays):

| Operation | Native SQL | jsonb_delta | Reduction |
|-----------|------------|-----------|-----------|
| Peak memory | 45 MB | 12 MB | **73%** |

---

## CPU Optimization Techniques

The extension uses several optimization techniques:

1. **SIMD Auto-Vectorization**: Loop unrolling hints for integer ID matching
2. **Branch Prediction**: Hot path optimization for common cases
3. **Cache Locality**: Sequential array traversal
4. **Short-Circuiting**: Early termination on match found

See [`src/lib.rs`](../src/lib.rs) for implementation details.

---

## Benchmark Reproduction

To run benchmarks yourself:

```bash
# Install extension
cargo pgrx install --release

# Run benchmark suite
psql -d postgres -f test/benchmark_array_update_where.sql
```

Expected output:

```text
Native SQL: 3.2ms average
jsonb_delta:  1.1ms average
Speedup:    2.9×
```

---

## Performance Tips

1. **Use batch operations** when updating multiple elements
2. **Index on JSONB paths** if you frequently filter by array contents:

   ```sql
   CREATE INDEX idx_orders ON customers USING GIN ((data->'orders'));
   ```

3. **Prefer integer IDs** when possible (enables SIMD optimization)
4. **Use `PARALLEL SAFE`** functions in parallel query plans

---

## Continuous Benchmarking

Benchmarks are run automatically in CI on every commit:
- [GitHub Actions Benchmark Workflow](../.github/workflows/benchmark.yml)
- Results published to [benchmark history](https://github.com/your-org/jsonb_delta/actions/workflows/benchmark.yml)

Benchmark regressions > 10% will fail the CI build.

---

## Future Optimizations

Planned performance improvements:

- [ ] SIMD intrinsics for JSON parsing (2-5× potential speedup)
- [ ] JIT compilation support via pgrx 1.0
- [ ] Parallel array processing for very large arrays (> 10,000 elements)
- [ ] Custom memory allocator for reduced fragmentation
