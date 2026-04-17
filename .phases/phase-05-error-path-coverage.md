# Phase 5: Error-Path Test Coverage

## Objective

The test suite is strong on happy paths but light on error cases, batch
operations, and property tests for non-depth operations. This phase adds
targeted coverage in all three areas, plus a small improvement to the depth
error message.

## Problems

1. **Error-path SQL tests**: No file exercises type mismatches, invalid paths,
   or non-existent array paths at the SQL level. Regressions in error handling
   go undetected.

2. **Batch operation coverage**: `jsonb_array_update_where_batch()` and
   `jsonb_array_update_multi_row()` have only smoke-test assertions in
   `02_array_update_where.sql` — edge cases (no matches, malformed specs) are
   untested.

3. **Property test coverage**: `src/property_tests.rs` has 3 properties, all
   for depth validation and path navigation. Merge and array operations have no
   property tests.

4. **Depth error message**: `src/depth.rs:31` reports `found >max` instead of
   the actual depth. Improve it.

## Success Criteria

- [ ] New file `test/sql/07_error_cases.sql` with ≥ 15 distinct error-path tests
- [ ] Batch and multi-row tests expanded in `test/sql/02_array_update_where.sql`
- [ ] At least 3 new property tests in `src/property_tests.rs`
- [ ] Depth error message reports actual depth found
- [ ] All tests pass (`cargo pgrx test pg18`, `just test-sql`)

## Background for New Engineers

### Error-testing pattern in SQL

PostgreSQL's `DO` block with `EXCEPTION WHEN OTHERS THEN` is the standard way
to assert that a function raises an error:

```sql
DO $$
BEGIN
    PERFORM some_function_that_should_error(...);
    RAISE EXCEPTION 'Expected error but got none';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%expected substring%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_name_here' AS passed;
```

The `SELECT 'test_name_here' AS passed` after the block confirms the test
completed successfully — it will only execute if the `DO` block didn't re-raise.

### What `strict` does on pgrx functions

Functions decorated with `#[pg_extern(..., strict)]` return `NULL` rather than
being called when any argument is `NULL`. Functions without `strict` are called
normally with `NULL` arguments — the `Option<T>` wrapper is used in Rust.

---

## TDD Cycles

### Cycle 1: SQL error-path test file

Create `test/sql/07_error_cases.sql` with all error tests below. Run it against
a live instance before committing to verify all 15 tests produce `passed`.

```sql
CREATE EXTENSION IF NOT EXISTS jsonb_delta;

-- 1. jsonb_merge_shallow with array argument
DO $$
BEGIN
    PERFORM jsonb_merge_shallow('[1,2]'::jsonb, '{"b":2}'::jsonb);
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_01_merge_shallow_rejects_array' AS passed;

-- 2. jsonb_merge_shallow with scalar argument
DO $$
BEGIN
    PERFORM jsonb_merge_shallow('42'::jsonb, '{"b":2}'::jsonb);
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_02_merge_shallow_rejects_scalar' AS passed;

-- 3. jsonb_array_update_where with non-existent path
DO $$
BEGIN
    PERFORM jsonb_array_update_where(
        '{"items": [{"id":1}]}'::jsonb,
        'missing_path',
        'id', '1'::jsonb,
        '{"x":1}'::jsonb
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%does not exist%' AND sqlerrm NOT LIKE '%missing_path%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_03_array_update_nonexistent_path' AS passed;

-- 4. jsonb_array_update_where with path pointing to object (not array)
DO $$
BEGIN
    PERFORM jsonb_array_update_where(
        '{"user": {"id":1}}'::jsonb,
        'user',
        'id', '1'::jsonb,
        '{"x":1}'::jsonb
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%array%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_04_array_update_path_not_array' AS passed;

-- 5. jsonb_array_update_where with non-object updates
DO $$
BEGIN
    PERFORM jsonb_array_update_where(
        '{"items":[{"id":1}]}'::jsonb,
        'items',
        'id', '1'::jsonb,
        '[1,2,3]'::jsonb      -- updates must be an object
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_05_array_update_non_object_updates' AS passed;

-- 6. jsonb_merge_at_path with non-object source
DO $$
BEGIN
    PERFORM jsonb_merge_at_path(
        '{"a": {"b":1}}'::jsonb,
        'a',
        '[1,2,3]'::jsonb      -- patch must be an object
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_06_merge_at_path_non_object_patch' AS passed;

-- 7. jsonb_merge_at_path with non-object at path
DO $$
BEGIN
    PERFORM jsonb_merge_at_path(
        '{"a": [1,2,3]}'::jsonb,   -- 'a' is an array, not an object
        'a',
        '{"x":1}'::jsonb
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' AND sqlerrm NOT LIKE '%array%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_07_merge_at_path_target_not_object' AS passed;

-- 8. jsonb_deep_merge with non-object replaces (not errors)
-- deep_merge source-replaces non-objects — this should NOT error
SELECT
    jsonb_deep_merge('{"a": 1}'::jsonb, '{"a": {"b": 2}}'::jsonb)
    = '{"a": {"b": 2}}'::jsonb
    AS test_08_deep_merge_replaces_scalar;

-- 9. jsonb_array_insert_where on non-object target
DO $$
BEGIN
    PERFORM jsonb_array_insert_where('[1,2,3]'::jsonb, 'items', '{"id":1}'::jsonb, NULL, NULL);
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_09_array_insert_non_object_target' AS passed;

-- 10. jsonb_array_insert_where on path pointing to non-array
DO $$
BEGIN
    PERFORM jsonb_array_insert_where(
        '{"items": "not-an-array"}'::jsonb,
        'items',
        '{"id":1}'::jsonb,
        NULL, NULL
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%array%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_10_array_insert_path_not_array' AS passed;

-- 11. jsonb_delta_set_path with empty path string
DO $$
BEGIN
    PERFORM jsonb_delta_set_path('{}'::jsonb, '', '1'::jsonb);
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%empty%' AND sqlerrm NOT LIKE '%path%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_11_set_path_empty_path' AS passed;

-- 12. jsonb_delta_set_path with invalid path syntax (consecutive dots)
DO $$
BEGIN
    PERFORM jsonb_delta_set_path('{}'::jsonb, 'a..b', '1'::jsonb);
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%path%' AND sqlerrm NOT LIKE '%dot%' AND sqlerrm NOT LIKE '%Invalid%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_12_set_path_invalid_syntax' AS passed;

-- 13. jsonb_delta_array_update_where_path with non-existent array
DO $$
BEGIN
    PERFORM jsonb_delta_array_update_where_path(
        '{"users": [{"id":1}]}'::jsonb,
        'missing',
        'id', '1'::jsonb,
        'name',
        '"Alice"'::jsonb
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%missing%' AND sqlerrm NOT LIKE '%exist%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_13_array_update_where_path_missing_array' AS passed;

-- 14. jsonb_extract_id with non-object returns NULL
SELECT
    jsonb_extract_id('[1,2,3]'::jsonb, 'id') IS NULL
    AS test_14_extract_id_non_object_returns_null;

-- 15. jsonb_array_contains_id with non-object element
-- The function should handle gracefully (return false or similar)
SELECT
    jsonb_array_contains_id(
        '{"items": [1, 2, 3]}'::jsonb,    -- array of scalars, not objects
        'items',
        'id',
        '1'::jsonb
    ) = false
    AS test_15_contains_id_non_object_elements;
```

> **Note**: Run each test individually against a live instance first to confirm
> the actual behavior — some functions may handle edge cases differently from
> what the comments predict. Adjust the expected values/error messages to match
> reality.

**CLEANUP**

Commit:
```
test(errors): add 07_error_cases.sql with 15 error-path SQL tests
```

---

### Cycle 2: Expand batch operation tests

**RED**

Append to `test/sql/02_array_update_where.sql`:

```sql
-- Batch update: 3 updates in a single call
SELECT
    jsonb_array_update_where_batch(
        '{"items": [{"id":1,"v":0}, {"id":2,"v":0}, {"id":3,"v":0}]}'::jsonb,
        'items',
        'id',
        '[{"match": 1, "updates": {"v": 10}},
          {"match": 2, "updates": {"v": 20}},
          {"match": 3, "updates": {"v": 30}}]'::jsonb
    ) = '{"items": [{"id":1,"v":10}, {"id":2,"v":20}, {"id":3,"v":30}]}'::jsonb
    AS test_batch_update_three_elements;

-- Batch update: no matches returns document unchanged
SELECT
    jsonb_array_update_where_batch(
        '{"items": [{"id":1,"v":0}]}'::jsonb,
        'items',
        'id',
        '[{"match": 99, "updates": {"v": 99}}]'::jsonb
    ) = '{"items": [{"id":1,"v":0}]}'::jsonb
    AS test_batch_update_no_matches_unchanged;

-- Multi-row update: 3 documents
SELECT
    jsonb_array_update_multi_row(
        ARRAY[
            '{"items":[{"id":1,"v":0}]}'::jsonb,
            '{"items":[{"id":2,"v":0}]}'::jsonb,
            '{"items":[{"id":3,"v":0}]}'::jsonb
        ],
        'items',
        'id',
        ARRAY['1'::jsonb, '2'::jsonb, '3'::jsonb],
        ARRAY['{"v":10}'::jsonb, '{"v":20}'::jsonb, '{"v":30}'::jsonb]
    ) = ARRAY[
        '{"items":[{"id":1,"v":10}]}'::jsonb,
        '{"items":[{"id":2,"v":20}]}'::jsonb,
        '{"items":[{"id":3,"v":30}]}'::jsonb
    ]
    AS test_multi_row_update_three_docs;

-- Multi-row update: empty document array
SELECT
    jsonb_array_update_multi_row(
        ARRAY[]::jsonb[],
        'items',
        'id',
        ARRAY[]::jsonb[],
        ARRAY[]::jsonb[]
    ) = ARRAY[]::jsonb[]
    AS test_multi_row_update_empty;
```

> **Note**: Check the actual signatures of `jsonb_array_update_where_batch` and
> `jsonb_array_update_multi_row` in `src/array_ops.rs` before writing tests —
> the parameter types/order here are approximate. Adjust accordingly.

**CLEANUP**

Commit:
```
test(batch): expand batch and multi-row operation SQL tests
```

---

### Cycle 3: New property tests

**RED**

Add to `src/property_tests.rs` inside the `mod property_tests` block:

```rust
// ── Merge property tests ────────────────────────────────────────────────────

/// After shallow merging A and B, all keys in A that are NOT in B are preserved.
#[quickcheck]
fn prop_shallow_merge_preserves_target_keys(base: ArbJsonB, patch: ArbJsonB) -> TestResult {
    use crate::merge::jsonb_merge_shallow_impl; // or however it's exposed

    // Only run if both are objects
    let (Some(base_obj), Some(patch_obj)) = (base.0.as_object(), patch.0.as_object()) else {
        return TestResult::discard();
    };

    let result = // call the internal merge function on base.0 and patch.0
        // This depends on how the merge is exposed. You may need to call it
        // directly or via the public pgrx-free helper if one exists.
        // If not exposed, add a #[cfg(test)] helper or refactor to expose it.
        todo!();

    let result_obj = result.as_object().unwrap();

    // Every key in base that is NOT in patch must be in result with the same value
    for (key, val) in base_obj {
        if !patch_obj.contains_key(key) {
            if result_obj.get(key) != Some(val) {
                return TestResult::failed();
            }
        }
    }

    TestResult::passed()
}

/// Deleting a present element reduces the array length by exactly 1.
#[quickcheck]
fn prop_array_delete_reduces_length(elements: Vec<u8>) -> TestResult {
    if elements.is_empty() {
        return TestResult::discard();
    }

    // Build a JSONB array of objects with distinct ids
    let arr: Vec<Value> = elements
        .iter()
        .enumerate()
        .map(|(i, _)| serde_json::json!({"id": i}))
        .collect();

    let target = Value::Object(serde_json::Map::from_iter([(
        "items".to_string(),
        Value::Array(arr.clone()),
    )]));

    // Delete the first element (id=0)
    let result = // call delete function internally

    let result_arr = result
        .get("items")
        .and_then(|v| v.as_array())
        .unwrap();

    TestResult::from_bool(result_arr.len() == arr.len() - 1)
}

/// deep_merge(a, a) == a for any object a.
#[quickcheck]
fn prop_deep_merge_self_is_identity(val: ArbJsonB) -> TestResult {
    // Only run for objects
    if !val.0.is_object() {
        return TestResult::discard();
    }

    let result = // call deep_merge(val, val)

    TestResult::from_bool(result == val.0)
}
```

> **Key implementation note**: The property tests above have `todo!()` placeholders
> because the merge and delete functions are pgrx-wrapped — they require a live
> PostgreSQL connection. You have two options:
>
> **Option A** (preferred): Extract the pure-Rust core of each function into a
> `*_impl` helper (e.g., `pub(crate) fn merge_shallow_impl(a: &Value, b: &Value)
> -> Value`) and call the helper from both the `#[pg_extern]` wrapper and the
> property test.
>
> **Option B**: Expand only `prop_path_navigation_consistent` and `prop_depth_*`
> tests (which already work because `path::navigate_path` and `validate_depth`
> are pure functions), and accept that merge/delete properties must be tested at
> the SQL level.
>
> Discuss with the team which option fits the current architecture. If merge
> functions already have pure helpers (search for `_impl` in `src/`), use them.

**CLEANUP**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run
```

Commit:
```
test(property): add merge and array property tests
```

---

### Cycle 4: Improve depth error message in `src/depth.rs`

**RED**

Update `test_validate_depth_too_deep` in `src/depth.rs` to assert the error
contains the actual depth:

```rust
#[test]
fn test_validate_depth_too_deep() {
    let mut deep = json!({"level": 1});
    for _ in 0..MAX_JSONB_DEPTH {
        deep = json!({"nested": deep});
    }

    let result = validate_depth(&deep, MAX_JSONB_DEPTH);
    assert!(result.is_err());
    let err_msg = result.unwrap_err();
    assert!(err_msg.contains("JSONB nesting too deep"));
    assert!(err_msg.contains("max 1000"));
    // NEW: assert the actual depth appears in the message
    assert!(
        err_msg.contains("1001") || err_msg.contains("found depth"),
        "error should report the actual depth, got: {err_msg}"
    );
}
```

Run — fails because the current message says `found >1000`, not the actual depth.

**GREEN**

Change `src/depth.rs` line 31:

```rust
// Before
return Err(format!("JSONB nesting too deep (max {max}, found >{max})"));

// After
return Err(format!(
    "JSONB nesting too deep (max {max}, found depth {current})"
));
```

Run tests — passes.

**REFACTOR**

None needed.

**CLEANUP**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run
```

Commit:
```
fix(depth): report actual depth in nesting error message
```

---

## Files Modified

| File | Change |
|------|--------|
| `test/sql/07_error_cases.sql` | New file — 15 error-path tests |
| `test/sql/02_array_update_where.sql` | Expanded batch + multi-row tests |
| `src/property_tests.rs` | 3 new property tests (or scaffolds for them) |
| `src/depth.rs` | Improved error message + updated test assertion |

## Dependencies

- Requires: Phase 1 (array bounds), Phase 2 (nested path tests), Phase 3
  (input validation — some error tests depend on these guards)
- Blocks: Phase 6 (finalization)

## Status
[ ] Not Started
