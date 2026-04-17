# Phase 1: Array Size Bounds (OOM Prevention)

## Objective

Prevent out-of-memory attacks by capping array growth whenever `set_path()` or
`jsonb_delta_array_update_where_path()` pads an array up to a requested index.

## Problem

`src/path.rs` — `set_path()` has two array-padding loops (lines 211 and 233)
that use `while arr.len() <= *idx { arr.push(Value::Null); }` with no upper
bound. A call like `jsonb_delta_set_path(doc, 'arr[999999999]', val)` allocates
~8 GB, crashing the PostgreSQL backend process.

The same unbounded growth appears in `src/lib.rs` around lines 286-289 inside
`jsonb_delta_array_update_where_path()`.

## Success Criteria

- [ ] `pub const MAX_JSONB_ARRAY_SIZE: usize = 100_000` added to `src/depth.rs`
- [ ] `pub fn validate_array_index(idx: usize, max: usize) -> Result<(), String>`
      added to `src/depth.rs`; error message includes both `idx` and `max`
- [ ] Both padding loops in `set_path()` (`src/path.rs` lines 211, 233) call
      `validate_array_index` before touching the array and propagate `Err`
- [ ] The padding loop in `jsonb_delta_array_update_where_path()` (`src/lib.rs`
      ~line 287) does the same
- [ ] Unit tests: index 99,999 → `Ok`, index 100,000 → `Err`
- [ ] `cargo pgrx test pg18` and `just test-sql` both pass

## Background for New Engineers

`set_path` navigates (and creates) intermediate nodes along a path. When it
hits an `Index(idx)` segment it must ensure the array is at least `idx+1` long,
padding with JSON `null`. Without a cap, any caller can request an arbitrarily
large index. The fix is to reject the call before touching memory.

`validate_depth` in `src/depth.rs` follows the same pattern — define a constant,
write a small validation helper, call it from the hot path, propagate the error.
Follow that exact style.

## TDD Cycles

### Cycle 1: Define constant and validation helper in `src/depth.rs`

**RED**

Add these two tests at the bottom of the `#[cfg(test)] mod tests` block in
`src/depth.rs`:

```rust
#[test]
fn test_array_index_within_limit() {
    assert!(validate_array_index(99_999, MAX_JSONB_ARRAY_SIZE).is_ok());
}

#[test]
fn test_array_index_exceeds_limit() {
    let err = validate_array_index(100_000, MAX_JSONB_ARRAY_SIZE).unwrap_err();
    assert!(err.contains("100000"), "error should contain the bad index");
    assert!(err.contains("100000"), "error should contain the limit");
}
```

Run `cargo test -p jsonb_delta depth::tests` — both should fail with "not found".

**GREEN**

Add to `src/depth.rs`, after the existing `MAX_JSONB_DEPTH` constant:

```rust
/// Maximum number of elements allowed in a single JSONB array.
/// Prevents OOM attacks via large index padding (e.g., arr[999999999]).
pub const MAX_JSONB_ARRAY_SIZE: usize = 100_000;

/// Return `Err` if `idx` would require padding an array beyond `max` elements.
///
/// # Errors
/// Returns an error string if `idx >= max`.
pub fn validate_array_index(idx: usize, max: usize) -> Result<(), String> {
    if idx >= max {
        Err(format!(
            "Array index {idx} exceeds maximum allowed size {max}"
        ))
    } else {
        Ok(())
    }
}
```

Run tests again — both should pass.

**REFACTOR**

The error message already includes both values. Nothing more to do.

**CLEANUP**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

Fix any warnings. Commit:
```
feat(depth): add MAX_JSONB_ARRAY_SIZE constant and validate_array_index helper
```

---

### Cycle 2: Guard `set_path()` in `src/path.rs`

**RED**

Add to the `#[cfg(test)] mod tests` block in `src/path.rs`:

```rust
#[test]
fn test_set_path_rejects_huge_index() {
    let mut doc = serde_json::json!({});
    let path = parse_path("arr[200000]").unwrap();
    let result = set_path(&mut doc, &path, serde_json::json!(1));
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("200000"), "error should mention the index");
}

#[test]
fn test_set_path_accepts_index_at_limit() {
    // Index 99,999 is the last legal index (array would be 100,000 elements)
    let mut doc = serde_json::json!({});
    let path = parse_path("arr[99999]").unwrap();
    // This allocates 100k nulls — just verify it doesn't panic
    assert!(set_path(&mut doc, &path, serde_json::json!(1)).is_ok());
}
```

Run `cargo test -p jsonb_delta path::tests` — the first test fails because
`set_path` currently returns `Ok` for huge indices.

**GREEN**

In `src/path.rs`, add this import at the top:

```rust
use crate::depth::{validate_array_index, MAX_JSONB_ARRAY_SIZE};
```

Then in `set_path()`, find the `PathSegment::Index(idx)` arm inside the
`parent_path` loop (line ~205):

```rust
PathSegment::Index(idx) => {
    // BEFORE touching the array, check the size
    validate_array_index(*idx, MAX_JSONB_ARRAY_SIZE)
        .map_err(|e| e)?;          // propagate as Err(String)
    if !current.is_array() {
        *current = Value::Array(Vec::new());
    }
    let arr = current.as_array_mut().unwrap();
    while arr.len() <= *idx {
        arr.push(Value::Null);
    }
    current = &mut arr[*idx];
}
```

Apply the same guard in the `final_segment` match arm for `PathSegment::Index`
(line ~228):

```rust
PathSegment::Index(idx) => {
    validate_array_index(*idx, MAX_JSONB_ARRAY_SIZE)
        .map_err(|e| e)?;
    if !current.is_array() {
        *current = Value::Array(Vec::new());
    }
    let arr = current.as_array_mut().unwrap();
    while arr.len() <= *idx {
        arr.push(Value::Null);
    }
    arr[*idx] = value;
}
```

**REFACTOR**

The two array-expansion blocks are nearly identical. Extract a helper:

```rust
/// Extend `arr` to hold at least `idx + 1` elements, padding with `Value::Null`.
///
/// Returns `Err` if `idx` exceeds [`MAX_JSONB_ARRAY_SIZE`].
fn ensure_array_capacity(arr: &mut Vec<Value>, idx: usize) -> Result<(), String> {
    validate_array_index(idx, MAX_JSONB_ARRAY_SIZE)?;
    while arr.len() <= idx {
        arr.push(Value::Null);
    }
    Ok(())
}
```

Replace both `validate_array_index` + `while` blocks with a single call to
`ensure_array_capacity(arr, *idx)?`.

**CLEANUP**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p jsonb_delta path::tests
```

Commit:
```
feat(path): guard set_path() against oversized array index
```

---

### Cycle 3: Guard `jsonb_delta_array_update_where_path()` in `src/lib.rs`

**RED**

Locate the `while arr.len() <= *idx` loop inside
`jsonb_delta_array_update_where_path()` (~line 287). Write a `#[pg_test]` near
this function:

```rust
#[pg_test]
fn test_array_update_where_path_rejects_huge_index() {
    // Should panic/error, not OOM
    let result = std::panic::catch_unwind(|| {
        crate::jsonb_delta_array_update_where_path(
            JsonB(serde_json::json!({"items": [{"id": 1}]})),
            "items",
            "id", JsonB(serde_json::json!(1)),
            "items[200000]",
            JsonB(serde_json::json!("x")),
        )
    });
    assert!(result.is_err(), "expected an error for huge index");
}
```

`#[pg_test]` functions run inside a real PostgreSQL instance via pgrx. Run:
```bash
cargo pgrx test pg18 -- jsonb_delta_array_update_where_path_rejects_huge_index
```
Expect failure.

**GREEN**

In `src/lib.rs`, add the same import:
```rust
use crate::depth::{validate_array_index, MAX_JSONB_ARRAY_SIZE};
```

Before the `while arr.len() <= *idx` loop, add:
```rust
validate_array_index(*idx, MAX_JSONB_ARRAY_SIZE)
    .unwrap_or_else(|e| error!("{}", e));
```

pgrx's `error!()` macro triggers a PostgreSQL `ereport(ERROR, ...)` — the
`#[pg_test]`'s `catch_unwind` will catch it.

**REFACTOR**

Consider whether this entire block should call `set_path()` from `src/path.rs`
instead of duplicating path-navigation logic. If so, do it now — the guard
would come for free. If the duplication is small enough to leave, leave it and
add a `// TODO(dedup): consider calling path::set_path` comment for the
finalize phase.

**CLEANUP**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo pgrx test pg18
```

Commit:
```
feat(lib): guard array update where path against oversized index
```

---

### Cycle 4: SQL integration test

**RED**

Add to `test/sql/security_depth_limits.sql`:

```sql
-- Test: Array index bound enforcement
SELECT jsonb_delta_set_path(
    '{"a": []}'::jsonb,
    'a[200000]',
    '1'::jsonb
);
-- Expected: ERROR containing "Array index"
```

Run `just test-sql` and confirm the test fails (currently `set_path` allows it;
after Phase 1 Cycles 1-3 it will error as expected).

**GREEN**

Already implemented in Cycles 1-3. Run `just test-sql` — should pass now.

**CLEANUP**

Commit:
```
test(security): add SQL test for array index bound
```

---

## Files Modified

| File | Change |
|------|--------|
| `src/depth.rs` | Add `MAX_JSONB_ARRAY_SIZE`, `validate_array_index`, `ensure_array_capacity` |
| `src/path.rs` | Guard both `Index` arms in `set_path()` |
| `src/lib.rs` | Guard the `Index` padding loop in `jsonb_delta_array_update_where_path()` |
| `test/sql/security_depth_limits.sql` | New SQL test case |

## Dependencies

- Requires: None (first phase)
- Blocks: Phase 3 (input validation builds on the validation patterns here)

## Status
[ ] Not Started
