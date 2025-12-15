# Architecture

Technical architecture and design decisions for jsonb_ivm.

---

## Table of Contents

- [System Overview](#system-overview)
- [Design Goals](#design-goals)
- [Performance Characteristics](#performance-characteristics)
- [Implementation Details](#implementation-details)
- [Design Decisions](#design-decisions)

---

## System Overview

jsonb_ivm is a PostgreSQL extension that provides **surgical JSONB manipulation** for incremental view maintenance in CQRS architectures.

### Problem Statement

In CQRS systems with denormalized projection tables:
- Entities are embedded in JSONB documents
- Single entity changes require updating hundreds of documents
- Native PostgreSQL requires re-aggregating entire JSONB structures
- Performance bottleneck for high-throughput systems

### Solution

Surgical, in-place JSONB updates:
- Update only changed elements (not entire arrays)
- Preserve unchanged nested fields
- Rust implementation for maximum performance
- Zero-copy operations where possible

---

## Design Goals

1. **Performance**: 2-7× faster than native SQL re-aggregation
2. **Safety**: Memory-safe via Rust ownership system
3. **Correctness**: Preserve data integrity, no silent failures
4. **Ergonomics**: Simple API, clear intent
5. **Compatibility**: Works with existing CQRS patterns

---

## Performance Characteristics

### Time Complexity

| Function | Best Case | Average | Worst Case | Notes |
|----------|-----------|---------|------------|-------|
| `jsonb_array_update_where` | O(n) | O(n) | O(n) | n = array length |
| `jsonb_array_delete_where` | O(n) | O(n) | O(n) | Single-pass scan |
| `jsonb_array_insert_where` | O(n log n) | O(n log n) | O(n log n) | Includes sorting |
| `jsonb_deep_merge` | O(d × k) | O(d × k) | O(d × k) | d = depth, k = keys |
| `jsonb_smart_patch_*` | O(k) | O(k) | O(k) | k = number of keys |

**Optimization:** Loop unrolling for arrays > 32 elements (8-way unroll)

### Space Complexity

All functions: **O(1)** auxiliary space (no temporary copies of entire documents)

### Benchmarked Performance

| Operation | Native SQL | jsonb_ivm | Speedup |
|-----------|-----------|-----------|---------|
| Single array update (50 elements) | 1.91 ms | **0.72 ms** | **2.66×** |
| Array INSERT (100 elements) | 22-35 ms | **5-8 ms** | **4-6×** |
| Array DELETE (100 elements) | 20-30 ms | **4-6 ms** | **5-7×** |
| Deep merge (3 levels) | 8-12 ms | **4-6 ms** | **2×** |
| Cascade (100 updates) | 870 ms | **600 ms** | **1.45×** |

See [benchmark-results.md](implementation/benchmark-results.md) for full details.

---

## Implementation Details

### Technology Stack

- **Language**: Rust (Edition 2021)
- **Framework**: [pgrx](https://github.com/pgcentralfoundation/pgrx) 0.12.8
- **Target**: PostgreSQL 13-17
- **Build**: cargo + cargo-pgrx

### Core Algorithm: Array Update

```rust
// Pseudocode for jsonb_array_update_where
fn update_array_element(array, match_key, match_value, updates):
    result = []

    for element in array:
        if element[match_key] == match_value:
            // Merge updates into element
            result.push(merge_shallow(element, updates))
        else:
            // Preserve unchanged
            result.push(element)

    return result
```

**Optimization:** Loop unrolling for large arrays:

```rust
// 8-way loop unrolling
while i + 8 <= len:
    process_element(i)
    process_element(i+1)
    // ...
    process_element(i+7)
    i += 8

// Handle remainder
while i < len:
    process_element(i)
    i += 1
```

**Benefit:** Enables compiler auto-vectorization, 3× speedup for arrays > 32 elements

### Core Algorithm: Deep Merge

```rust
// Recursive deep merge
fn deep_merge(target, source):
    if not both_objects(target, source):
        return source  // Replace non-objects

    result = clone(target)

    for key, value in source:
        if key in target and both_objects(target[key], value):
            // Recursive merge
            result[key] = deep_merge(target[key], value)
        else:
            // Direct replacement
            result[key] = value

    return result
```

**Key property:** Preserves all target fields not present in source

### Memory Management

- **Rust ownership**: Prevents use-after-free, double-free, buffer overflows
- **pgrx integration**: Automatic memory management via PostgreSQL memory contexts
- **Zero-copy**: Where possible, references are used instead of copies

---

## Design Decisions

### 1. Why Rust over C?

**Decision:** Implement in Rust using pgrx

**Rationale:**
- **Memory safety**: Eliminates entire classes of bugs (segfaults, buffer overflows)
- **Performance**: Comparable to C, sometimes faster due to better optimizations
- **Maintainability**: Type system catches bugs at compile-time
- **Ecosystem**: cargo, rustfmt, clippy provide excellent tooling

**Trade-off:**
- Larger binary size (~2-3× vs C)
- Longer compile times
- **Accepted**: Safety and maintainability worth the cost

---

### 2. Why pgrx Framework?

**Decision:** Use pgrx instead of raw PostgreSQL C API

**Rationale:**
- **Ergonomics**: High-level API for PostgreSQL types (JsonB, arrays, etc.)
- **Safety**: Wraps unsafe PostgreSQL C API in safe Rust abstractions
- **Testing**: Built-in integration test framework
- **SQL generation**: Automatic schema generation

**Trade-off:**
- Couples extension to pgrx versioning
- **Accepted**: Benefits far outweigh lock-in risk

---

### 3. Function Signatures: STRICT vs Non-STRICT

**Decision:** Mark most functions `STRICT` (return NULL on NULL input)

**Rationale:**
- **Predictability**: NULL propagates naturally (SQL standard behavior)
- **Performance**: PostgreSQL skips function call if any arg is NULL
- **Safety**: Prevents unexpected behavior from NULL parameters

**Exception:** `jsonb_array_insert_where` allows NULL for `sort_key`/`sort_order`
- **Rationale:** Valid use case (unsorted insertion) requires NULL

---

### 4. Error Handling: Return Original vs Error

**Decision:** Return original JSONB unchanged when paths/keys don't exist

**Rationale:**
- **Idempotency**: Safe to call multiple times
- **Debugging**: Easy to detect no-ops (compare before/after)
- **Composability**: Can chain operations without error checking

**Alternative considered:** Throw errors
- **Rejected**: Would break transactions, require try/catch everywhere

---

### 5. Array Operations: In-place vs Copy

**Decision:** Create new arrays (functional approach)

**Rationale:**
- **PostgreSQL MVCC**: Requires new version anyway
- **Safety**: Original data preserved until transaction commits
- **Simplicity**: No complex in-place mutation logic

**Trade-off:**
- Higher memory usage for very large arrays (1000+ elements)
- **Mitigated**: Most CQRS arrays are 10-200 elements

---

### 6. Loop Unrolling: Manual vs Auto

**Decision:** Manual 8-way loop unrolling with compiler hints

**Rationale:**
- **Performance**: 3× speedup for large arrays (measured)
- **Portability**: Works with stable Rust (no nightly features)
- **Control**: Explicit unrolling factor (8) chosen empirically

**Alternative considered:** External SIMD library
- **Rejected**: Adds dependency, requires nightly Rust, marginal gains

---

### 7. Function Naming: Verbose vs Terse

**Decision:** Verbose names (`jsonb_array_update_where` not `jb_aupd`)

**Rationale:**
- **Clarity**: Intent obvious from name
- **Discoverability**: Tab-completion friendly (`jsonb_<TAB>`)
- **SQL convention**: PostgreSQL core uses verbose names

**Trade-off:**
- Longer queries
- **Accepted**: Readability > brevity for production code

---

### 8. Smart Patch Dispatch: Static vs Dynamic

**Decision:** Three separate functions (`scalar`, `nested`, `array`) vs one dynamic dispatcher

**Rationale:**
- **Type safety**: Compile-time verification of parameters
- **Performance**: No runtime type checking overhead
- **Clarity**: Function name indicates operation type

**Alternative considered:** Single `jsonb_smart_patch(data, updates, OPTIONS)`
- **Rejected**: Complex options parameter, error-prone, slower

---

## Data Flow

### Example: Company Name Update Propagation

```text
1. UPDATE v_company SET name = 'New Name'
   └─> Trigger fires

2. UPDATE tv_user (50 rows)
   └─> jsonb_smart_patch_nested(data, new_name, ARRAY['company'])
   └─> Rust function:
       ├─ Parse JSONB
       ├─ Navigate to data.company
       ├─ Merge {name: 'New Name'}
       └─ Return updated JSONB

3. UPDATE tv_post (500 rows)
   └─> jsonb_deep_merge(data, {author: {company: company_data}})
   └─> Rust function:
       ├─ Recursive descent to data.author.company
       ├─ Merge all company fields
       └─> Preserves author.name, author.email, post.title, etc.

4. UPDATE tv_feed (1 row with 100-element array)
   └─> jsonb_array_update_where(data, 'posts', 'id', post_id, updates)
   └─> Rust function:
       ├─ Extract posts array
       ├─ Scan for matching id (loop unrolled 8-way)
       ├─ Merge updates into matched element
       └─> Return updated array
```

**Total time:** ~20-25ms (vs ~60-80ms native SQL)

---

## Extension Points

### Adding New Functions

1. Implement Rust function in `src/lib.rs`:

   ```rust
   #[pg_extern(immutable, parallel_safe, strict)]
   fn my_new_function(data: JsonB) -> JsonB {
       // Implementation
   }
   ```

2. Add tests in `src/lib.rs`:

   ```rust
   #[cfg(test)]
   mod tests {
       #[test]
       fn test_my_new_function() { /* ... */ }
   }
   ```

3. Add SQL tests in `test/sql/`:

   ```sql
   -- test/sql/08_my_feature.sql
   SELECT my_new_function('{"test": true}'::jsonb);
   ```

4. Add benchmarks in `test/benchmark_my_feature.sql`

5. Update README.md API reference

6. Regenerate SQL: `cargo pgrx schema > sql/jsonb_ivm--X.Y.Z.sql`

7. Create upgrade path: `sql/jsonb_ivm--old--new.sql`

---

## Future Enhancements

### Potential Optimizations

1. **SIMD intrinsics**: Direct SIMD operations (requires nightly Rust)
2. **Parallel processing**: Multi-threaded array operations for very large arrays
3. **Caching**: Cache parsed JSONB structures across function calls
4. **JIT compilation**: LLVM-based just-in-time compilation for hot paths

### Potential Features

1. **Batch path operations**: Update multiple paths in one call
2. **Conditional updates**: Update only if condition met
3. **Atomic increment/decrement**: For counters in JSONB
4. **JSONB diff/patch**: Generate and apply patches
5. **Change tracking**: Return which fields changed

---

## References

- **pgrx Documentation**: [pgrx repository](https://github.com/pgcentralfoundation/pgrx)
- **PostgreSQL JSONB Internals**: [PostgreSQL JSONB Documentation](https://www.postgresql.org/docs/current/datatype-json.html)
- **CQRS Pattern**: [CQRS Pattern](https://martinfowler.com/bliki/CQRS.html)
- **Rust Book**: [The Rust Programming Language](https://doc.rust-lang.org/book/)

---

## Performance Tuning

See [integration-guide.md](integration-guide.md#performance-tuning) for:
- Index strategies
- Batch operation patterns
- Containment check optimization
- Array size considerations

---

**Questions?** See [troubleshooting.md](troubleshooting.md) or open an issue.
