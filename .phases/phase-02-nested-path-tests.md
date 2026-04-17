# Phase 2: Activate Nested Path Integration Tests

## Objective

`test/sql/nested_paths.sql` was written as a TDD scaffold before the path
functions existed. The functions now work, but the file still says "All tests
should fail initially" and tests 6-8 expect errors for operations that actually
succeed. Fix the file so that `just test-sql` reports all nested-path tests as
passing.

## Problem Details

1. **Line 2**: `-- Expected: All tests fail initially (functions don't exist yet)`
   — the functions now exist.

2. **Tests 1-5** (lines 7-48): These run valid operations against working
   functions. They probably produce correct output, but the file has no assertions
   — only comments describing expected results. `just test-sql` passes them
   vacuously, hiding any regressions.

3. **Tests 6-8** (lines 51-70): These expect errors (`-- Expected: ERROR: ...`)
   for operations that `set_path()` currently handles without error:
   - `set_path('{"a":{"b":1}}', 'a.c', 2)` creates `{"a":{"b":1,"c":2}}` — no error.
   - `set_path('{"a":[1,2,3]}', 'a[10]', 4)` pads the array — no error (until
     Phase 1 caps it, and index 10 is well within the new cap).
   - `set_path('{"a":{"b":1}}', 'a[0]', 2)` replaces the object with an array.

4. **Line 72**: `\echo 'All tests should fail initially...'` — misleading noise
   that will break the expected-output diff if one exists.

## Success Criteria

- [ ] Tests 1-5 use `SELECT ... = '...'::jsonb AS passed` assertions (or
      equivalent) so a wrong result produces a `false` row rather than silent success
- [ ] Tests 6-8 rewritten to assert the actual create-on-navigate behavior with
      concrete expected JSON
- [ ] Line 2 comment and line 72 `\echo` removed
- [ ] `just test-sql` output for `nested_paths.sql` is deterministic and clean

## Background for New Engineers

Look at `test/sql/01_merge_shallow.sql` for the canonical test style used in
this project. Most tests use a pattern like:

```sql
SELECT
    jsonb_merge_shallow('{"a":1}'::jsonb, '{"b":2}'::jsonb)
    = '{"a":1,"b":2}'::jsonb AS test_shallow_merge_basic;
```

The column alias becomes the test name; the value is `true` (pass) or `false`
(fail). Some tests use `DO $$ BEGIN ... EXCEPTION ... END $$` blocks to
assert that a call raises an error.

`just test-sql` diffs the actual query output against files in `test/expected/`.
Check whether `test/expected/nested_paths.expected` exists — if it does, you
must update it to match your new output; if it doesn't, create it after running
the updated SQL once against a live instance.

## TDD Cycles

### Cycle 1: Understand the actual behavior of each test case

**RED (observe)**

Run the SQL file against a live instance:

```bash
psql -f test/sql/nested_paths.sql
```

Note the actual output for each SELECT. Write it down — you will use these
exact results as expected values in the next cycle. In particular:
- Test 7 (`a[10]` on a 3-element array): pads with 7 nulls — note the exact
  JSON output.
- Test 8 (`a[0]` on an object): the object key `"a"` is replaced by the array
  — note the exact JSON output.

**GREEN**

No code change in this cycle — just gathering ground truth.

---

### Cycle 2: Rewrite tests 1-5 with proper assertions

Replace each bare `SELECT func(...)` with an assertion:

```sql
-- Test 1: Dot notation — update nested field via array predicate
SELECT
    jsonb_delta_array_update_where_path(
        '{"users": [{"id": 1, "profile": {"name": "Alice"}}]}'::jsonb,
        'users',
        'id', '1'::jsonb,
        'profile.name',
        '"Bob"'::jsonb
    ) = '{"users": [{"id": 1, "profile": {"name": "Bob"}}]}'::jsonb
    AS test_01_dot_notation_nested_update;

-- Test 2: Array index — update deeply nested array element
SELECT
    jsonb_delta_set_path(
        '{"orders": [{"items": [{"price": 10}]}]}'::jsonb,
        'orders[0].items[0].price',
        '20'::jsonb
    ) = '{"orders": [{"items": [{"price": 20}]}]}'::jsonb
    AS test_02_nested_array_index;

-- Test 3: Mixed path — complex nested navigation
SELECT
    jsonb_delta_array_update_where_path(
        '{"companies": [{"id": 1, "departments": [{"name": "engineering", "employees": [{"name": "Alice", "salary": 50000}]}]}]}'::jsonb,
        'companies',
        'id', '1'::jsonb,
        'departments[0].employees[0].salary',
        '60000'::jsonb
    ) = '{"companies": [{"id": 1, "departments": [{"name": "engineering", "employees": [{"name": "Alice", "salary": 60000}]}]}]}'::jsonb
    AS test_03_mixed_path_complex;

-- Test 4: Simple dot notation (no arrays)
SELECT
    jsonb_delta_set_path(
        '{"user": {"profile": {"settings": {"theme": "light"}}}}'::jsonb,
        'user.profile.settings.theme',
        '"dark"'::jsonb
    ) = '{"user": {"profile": {"settings": {"theme": "dark"}}}}'::jsonb
    AS test_04_deep_dot_notation;

-- Test 5: Array indexing into existing array
SELECT
    jsonb_delta_set_path(
        '{"items": [{"name": "item1"}, {"name": "item2"}]}'::jsonb,
        'items[1].name',
        '"updated_item2"'::jsonb
    ) = '{"items": [{"name": "item1"}, {"name": "updated_item2"}]}'::jsonb
    AS test_05_array_index_update;
```

**CLEANUP**

Run `psql -f test/sql/nested_paths.sql`. All five rows should show `t`. If any
shows `f`, the function behavior differs from what you expect — go back to
Cycle 1 and correct the expected value.

---

### Cycle 3: Rewrite tests 6-8 to assert create-on-navigate semantics

The comment in the original file says these "should error" but the actual
behavior is intentional: `set_path` *creates* missing intermediate nodes rather
than erroring.

```sql
-- Test 6: Create-on-navigate — adding a new key to an existing object
-- set_path creates {"a": {"b": 1, "c": 2}}, it does NOT error
SELECT
    jsonb_delta_set_path(
        '{"a": {"b": 1}}'::jsonb,
        'a.c',
        '2'::jsonb
    ) = '{"a": {"b": 1, "c": 2}}'::jsonb
    AS test_06_create_new_key_in_object;

-- Test 7: Pad-on-extend — array shorter than requested index gets null-padded
-- index 10 on a 3-element array → [1, 2, 3, null, null, null, null, null, null, null, 4]
SELECT
    jsonb_delta_set_path(
        '{"a": [1, 2, 3]}'::jsonb,
        'a[10]',
        '4'::jsonb
    ) = '{"a": [1, 2, 3, null, null, null, null, null, null, null, 4]}'::jsonb
    AS test_07_array_pad_with_nulls;

-- Test 8: Type coercion — object at path is replaced by array when an index is used
SELECT
    jsonb_delta_set_path(
        '{"a": {"b": 1}}'::jsonb,
        'a[0]',
        '2'::jsonb
    ) = '{"a": [2]}'::jsonb
    AS test_08_object_replaced_by_array;
```

> **Note**: Verify the exact JSON output from Cycle 1 before finalising tests 7
> and 8 — the padding and replacement behaviour is implementation-defined.

**CLEANUP**

Remove the original line 2 comment and line 72 `\echo`. Run the full file. All
8 rows should show `t`.

---

### Cycle 4: Add edge-case tests

```sql
-- Test 9: Create entire chain from empty object
SELECT
    jsonb_delta_set_path(
        '{}'::jsonb,
        'a.b.c.d.e',
        '"deep"'::jsonb
    ) = '{"a": {"b": {"c": {"d": {"e": "deep"}}}}}'::jsonb
    AS test_09_create_deep_chain;

-- Test 10: Index 0 on empty array works
SELECT
    jsonb_delta_set_path(
        '{"a": []}'::jsonb,
        'a[0]',
        '"first"'::jsonb
    ) = '{"a": ["first"]}'::jsonb
    AS test_10_set_index_zero_empty_array;

-- Test 11: Empty path string is rejected
DO $$
BEGIN
    PERFORM jsonb_delta_set_path('{}'::jsonb, '', '1'::jsonb);
    RAISE EXCEPTION 'Expected error for empty path but got none';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%empty%' THEN
        RAISE EXCEPTION 'Unexpected error message: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_11_empty_path_rejected' AS passed;
```

**CLEANUP**

Run `just test-sql`. If the project uses `test/expected/nested_paths.expected`,
regenerate it:

```bash
psql -f test/sql/nested_paths.sql > test/expected/nested_paths.expected
```

Commit:
```
test(nested-paths): rewrite test file to assert actual behavior
```

---

## Files Modified

| File | Change |
|------|--------|
| `test/sql/nested_paths.sql` | Complete rewrite of assertions and removal of stale comments |
| `test/expected/nested_paths.expected` | Regenerate (create if missing) |

## Dependencies

- Requires: Phase 1 complete (Phase 1 changes what `a[10]` does for indices
  ≥ 100,000, but index 10 is unaffected — still safe to start Phase 2 first
  if desired, but finish both before Phase 5)
- Blocks: Phase 5 (error-path tests assume nested-path behavior is documented)

## Status
[ ] Not Started
