# Architecture & Design Decisions

## Overview

`jsonb_delta` is a PostgreSQL extension written in Rust using the [pgrx](https://github.com/pgcentralfoundation/pgrx) framework. It provides high-performance incremental update operations for JSONB documents, optimized for CQRS (Command Query Responsibility Segregation) architectures.

## Module Structure (Phase 0: Modularization)

The codebase is organized into focused modules for maintainability and parallel development:

### `src/lib.rs` (200 lines)

Core entry point with pgrx integration:

- Module declarations and re-exports
- pgrx FFI magic and test setup
- Utility functions: `jsonb_extract_id`, `jsonb_array_contains_id`

### `src/search.rs` (200 lines)

Optimized search algorithms:

- `find_by_int_id_optimized()`: SIMD-friendly integer ID matching with loop unrolling
- `find_by_int_id_scalar()`: Fallback for small arrays
- Performance: ~100ns/element for integer lookups

### `src/merge.rs` (400 lines)

JSONB merge operations:

- `jsonb_merge_shallow()`: Top-level key merging
- `jsonb_merge_at_path()`: Nested path merging
- `jsonb_smart_patch_*()`: CQRS-optimized merge variants
- `jsonb_deep_merge()`: Recursive object merging

### `src/array_ops.rs` (300 lines)

Array manipulation functions:

- `jsonb_array_update_where()`: Single element updates
- `jsonb_array_update_where_batch()`: Multiple updates in one pass
- `jsonb_array_update_multi_row()`: Cross-document batch updates
- `jsonb_array_delete_where()`: Surgical element removal
- `jsonb_array_insert_where()`: Ordered insertion with sorting

### `src/depth.rs` (100 lines)

Security validation:

- `validate_depth()`: Prevents DoS via deeply nested JSONB
- `MAX_JSONB_DEPTH = 1000`: Configurable nesting limit
- `get_max_depth()`: Analysis utility for testing

### Module Dependencies

```text
lib.rs (re-exports)
├── search.rs (no deps)
├── merge.rs (uses search)
├── array_ops.rs (uses search)
└── depth.rs (no deps)
```

### Benefits of Modularization

- **Parallel development**: Teams can work on different modules simultaneously
- **Focused testing**: Each module has clear responsibilities
- **Reduced compilation times**: Changes to one module don't rebuild everything
- **Clear APIs**: Module boundaries define stable interfaces

## Design Principles

### 1. Zero-Copy Where Possible

When updating nested structures, we avoid copying unchanged portions:

```rust
// Only the modified path is cloned
let mut target_clone = target.clone();  // Shallow clone
target_clone["nested"]["field"] = new_value;  // Deep path update
```

### 2. Single-Pass Array Operations

Native SQL requires re-aggregating entire arrays for updates:

```sql
-- Native: Scans array twice (filter + rebuild)
SELECT jsonb_agg(
    CASE WHEN elem->>'id' = '123' THEN updated ELSE elem END
) FROM jsonb_array_elements(data);
```

`jsonb_delta` uses single-pass updates:

```rust
// Rust: Single scan with in-place mutation
for (i, elem) in array.iter_mut().enumerate() {
    if matches_predicate(elem) {
        *elem = apply_update(elem);
        break;  // Early termination
    }
}
```

### 3. SIMD-Friendly Code Structure

For integer ID matching, we use loop unrolling to enable auto-vectorization:

```rust
const UNROLL: usize = 8;
for chunk in array.chunks(UNROLL) {
    // Compiler can vectorize this into SIMD instructions
    for i in 0..UNROLL {
        if chunk[i]["id"].as_i64() == target_id {
            return Some(i);
        }
    }
}
```

On modern CPUs, this generates SSE/AVX instructions that check 4-8 IDs simultaneously.

## Key Components

### Core Merge Logic (`jsonb_merge_shallow`, `jsonb_deep_merge`)

**Decision**: Implement both shallow and deep merge variants.

**Rationale**:
- Shallow merge: 90% of CQRS use cases (top-level field updates)
- Deep merge: Complex nested documents (rare but essential)

**Trade-off**: Code duplication vs. runtime branching
- ✅ Chose: Separate functions (better performance)
- ❌ Avoided: Single function with `deep: bool` flag (extra branch per call)

### Array Update Strategy

**Challenge**: PostgreSQL JSONB arrays are immutable.

**Options considered**:
1. Full array re-aggregation (native SQL approach)
2. Binary tree-based updates (persistent data structures)
3. Clone-and-mutate (current approach)

**Decision**: Clone-and-mutate with early termination

**Rationale**:
- Simpler implementation (no complex data structures)
- Faster for small-medium arrays (< 1000 elements)
- PostgreSQL already uses copy-on-write for JSONB

**When this breaks down**: Arrays with 10,000+ elements
- Future: Consider rope-based or tree-based structures

### Batch Operations

**Design**: Separate batch functions vs. array parameters

**Decision**: Both strategies provided
- `jsonb_array_update_where_batch`: Single document, multiple updates
- `jsonb_array_update_multi_row`: Multiple documents, single update pattern

**Rationale**: Different SQL patterns have different optimal strategies

```sql
-- Pattern 1: Update many items in one document
UPDATE orders SET data = jsonb_array_update_where_batch(...);

-- Pattern 2: Update one item across many documents
UPDATE orders SET data = result
FROM jsonb_array_update_multi_row(...);
```

## Performance Optimizations

### 1. Integer Fast Path

For integer IDs (the most common case in CQRS), we use a specialized fast path:

```rust
match match_value {
    Value::Number(n) if n.is_i64() => {
        // Fast path: Direct integer comparison (no string allocation)
        find_by_int_id_optimized(array, match_key, n.as_i64().unwrap())
    }
    _ => {
        // Slow path: Generic value comparison
        find_by_value_generic(array, match_key, match_value)
    }
}
```

**Impact**: 2-3× speedup for integer ID lookups

### 2. Small Array Optimization

For arrays < 32 elements, we skip hash table creation:

```rust
if array.len() < 32 {
    // Linear scan is faster due to cache locality
    return array.iter().position(predicate);
}
// For large arrays, build hash map
let mut hash_map = HashMap::with_capacity(array.len());
```

**Rationale**: Hash map overhead exceeds linear scan cost for small N.

### 3. Parallel Safety

All functions are marked `PARALLEL SAFE`:

```rust
#[pg_extern(immutable, strict, parallel_safe)]
fn jsonb_array_update_where(...) -> JsonB {
    // No global state, no locks
}
```

This allows PostgreSQL to use parallel query plans for large updates.

## Error Handling Philosophy

**Principle**: Fail fast with clear error messages

```rust
if !target.is_object() {
    error!("target argument must be a JSONB object, got {:?}", target.type());
}
```

**Rationale**:
- View maintenance code should be deterministic
- Silent failures lead to data corruption
- PostgreSQL transactions handle rollback

**Trade-off**:
- ❌ More defensive: Return NULL on invalid input
- ✅ Fail fast: ERROR immediately (current approach)

## Testing Strategy

### 1. Unit Tests (Rust)

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_merge_shallow() {
        let target = json!({"a": 1});
        let source = json!({"b": 2});
        assert_eq!(merge_shallow(target, source), json!({"a": 1, "b": 2}));
    }
}
```

### 2. Integration Tests (SQL)

```sql
-- test/sql/test_array_update.sql
BEGIN;
SELECT jsonb_array_update_where(...) AS result \gset
SELECT :result = expected_value AS test_passed;
ROLLBACK;
```

### 3. Property-Based Tests (Planned)

Use QuickCheck to verify invariants:

```rust
#[quickcheck]
fn merge_is_associative(a: JsonB, b: JsonB, c: JsonB) -> bool {
    merge(merge(a, b), c) == merge(a, merge(b, c))
}
```

## Security Considerations

### 1. No SQL Injection

All functions use pgrx's type-safe API:

```rust
#[pg_extern]
fn jsonb_array_update_where(
    target: JsonB,  // Type-checked by PostgreSQL
    array_path: &str,  // Cannot contain SQL
    ...
) -> JsonB
```

### 2. Memory Safety

Rust's ownership system prevents:
- Buffer overflows
- Use-after-free
- Data races

### 3. DoS Protection

**Concern**: Malicious inputs with deeply nested structures

**Mitigation**: PostgreSQL's `max_stack_depth` prevents stack overflow

**Future**: Add explicit nesting depth limits

## Extension Metadata

### Version Management

We follow semantic versioning:

```toml
# Cargo.toml
version = "0.1.0"
```

### Upgrade Path

```sql
-- Future: Support for extension updates
ALTER EXTENSION jsonb_delta UPDATE TO '0.2.0';
```

Currently, upgrades require:
1. DROP EXTENSION
2. Re-create extension
3. Recreate dependent views

**Future**: Migration scripts in `sql/jsonb_delta--0.1.0--0.2.0.sql`

## Build System

### pgrx Integration

```toml
[dependencies]
pgrx = "0.16.1"
serde_json = "1.0"
```

### Feature Flags

```toml
[features]
pg13 = ["pgrx/pg13"]
pg14 = ["pgrx/pg14"]
...
pg18 = ["pgrx/pg18"]
```

**Build command**:

```bash
cargo pgrx install --pg-config=/usr/bin/pg_config --features pg18
```

## Future Architecture Improvements

### 1. Custom Memory Allocator

**Proposal**: Use `jemalloc` for better fragmentation characteristics

**Expected benefit**: 10-15% memory reduction for large documents

### 2. JIT Compilation

**Proposal**: Leverage PostgreSQL 17's JIT for hot functions

**Expected benefit**: 20-30% speedup for large batch operations

### 3. Async I/O Support

**Proposal**: Async functions for remote JSONB fetching

**Use case**: Federated view maintenance across databases

## Design Alternatives Considered

### Alternative 1: Pure PL/pgSQL Implementation

**Rejected because**:
- 10-50× slower than Rust/C
- No SIMD optimization possible
- Harder to maintain complex logic

### Alternative 2: Foreign Data Wrapper (FDW)

**Rejected because**:
- Overkill for in-database operations
- FDW overhead for local data
- Better suited for external data sources

### Alternative 3: Stored Procedures with JSON Libraries

**Rejected because**:
- Python/V8: Large runtime overhead
- Limited PostgreSQL integration
- Security concerns (sandbox escapes)

## References

- [pgrx Documentation](https://github.com/pgcentralfoundation/pgrx)
- [PostgreSQL JSONB Internals](https://www.postgresql.org/docs/current/datatype-json.html)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [CQRS Pattern](https://martinfowler.com/bliki/CQRS.html)
