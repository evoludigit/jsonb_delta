# Phase 3: Input Validation Hardening

## Objective

Close two remaining input-validation gaps:

1. Empty `match_key` passed to any array-matching function — silently scans the
   entire array without ever matching, wasting CPU.
2. Unbounded key-segment length in `parse_path()` — a 1 MB key segment forces a
   1 MB `String` allocation per call.

## Problem Details

### Empty `match_key`

These functions accept `match_key: &str` and compare it against JSONB object
keys. If `match_key` is `""`, the comparison `elem.get("")` never finds anything
because PostgreSQL JSONB objects never have an empty key in normal use. The
function silently iterates the entire array and returns the document unchanged.
An explicit error is far more useful.

Affected functions (search for `match_key` in `src/`):

| Function | File |
|---|---|
| `jsonb_array_update_where` | `src/array_ops.rs` ~line 49 |
| `jsonb_array_delete_where` | `src/array_ops.rs` |
| `jsonb_array_update_where_batch` | `src/array_ops.rs` |
| `jsonb_array_update_multi_row` | `src/array_ops.rs` |
| `jsonb_smart_patch_array` | `src/merge.rs` |
| `jsonb_array_contains_id` | `src/lib.rs` |
| `jsonb_delta_array_update_where_path` | `src/lib.rs` |

### Unbounded key length

`parse_path()` in `src/path.rs` accumulates characters into `current_key`
(line 95: `current_key.push(ch)`). A path like `"a" * 1_000_000 + ".b"` forces
a 1 MB allocation just for the key string. A cap of 256 characters covers every
realistic use case.

## Success Criteria

- [ ] `validate_match_key(key: &str)` helper exists in `src/array_ops.rs`
      (or a shared location) and is called at the start of all 7 functions above
- [ ] `parse_path()` rejects key segments longer than 256 characters
- [ ] `const MAX_KEY_LENGTH: usize = 256` defined at module level in `src/path.rs`
- [ ] Unit tests for both validations
- [ ] SQL integration tests in `test/sql/security_depth_limits.sql`
- [ ] All existing tests still pass (`cargo pgrx test pg18`, `just test-sql`)

## TDD Cycles

### Cycle 1: `validate_match_key` helper

**RED**

In `src/array_ops.rs`, add a unit test at the bottom of the file (outside the
pgrx module, inside `#[cfg(test)] mod tests`):

```rust
#[test]
fn test_validate_match_key_empty() {
    let err = validate_match_key("").unwrap_err();
    assert!(err.contains("empty"), "error should mention 'empty'");
}

#[test]
fn test_validate_match_key_valid() {
    assert!(validate_match_key("id").is_ok());
    assert!(validate_match_key("user_id").is_ok());
}
```

Run `cargo test -p jsonb_delta array_ops::tests` — fails with "not found".

**GREEN**

Add the helper near the top of `src/array_ops.rs` (after the imports, before
the `#[pg_extern]` functions):

```rust
/// Validate that `match_key` is non-empty.
///
/// # Errors
/// Returns `Err` if `key` is an empty string.
fn validate_match_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        Err("match_key must not be empty".into())
    } else {
        Ok(())
    }
}
```

Then, at the very start of `jsonb_array_update_where()` (right after the
`let mut target_value: Value = target.0;` line):

```rust
validate_match_key(match_key).unwrap_or_else(|e| error!("{}", e));
```

Run tests again — unit tests pass.

**REFACTOR**

Now add the same guard to the remaining 6 functions. Use `grep -n match_key
src/array_ops.rs src/merge.rs src/lib.rs` to locate each one. The pattern is
always the same one-liner immediately after the function's first local variable
binding.

If `validate_match_key` needs to be called from `src/merge.rs` or `src/lib.rs`,
either:
- Make it `pub(crate)` in `src/array_ops.rs` and import it, or
- Move it to a new `src/validation.rs` module and `pub(crate)` expose it.

Either approach is fine; pick whichever feels cleaner given the existing module
structure.

**CLEANUP**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run
```

Commit:
```
feat(validation): reject empty match_key in all array functions
```

---

### Cycle 2: Cap key-segment length in `parse_path()`

**RED**

Add to the `#[cfg(test)] mod tests` block in `src/path.rs`:

```rust
#[test]
fn test_parse_path_rejects_key_too_long() {
    let long_key = "a".repeat(257);
    let path = format!("{long_key}.b");
    let err = parse_path(&path).unwrap_err();
    assert!(
        err.contains("257") || err.contains("256"),
        "error should mention the length or limit: {err}"
    );
}

#[test]
fn test_parse_path_accepts_key_at_limit() {
    let key_at_limit = "a".repeat(256);
    assert!(parse_path(&key_at_limit).is_ok());
}
```

Run `cargo test -p jsonb_delta path::tests` — both fail.

**GREEN**

Add the constant near the top of `src/path.rs` (after the imports):

```rust
/// Maximum allowed length (in bytes) for a single key segment in a path.
const MAX_KEY_LENGTH: usize = 256;
```

In `parse_path()`, find the `_ =>` arm that calls `current_key.push(ch)` (line
~94). After the push, add:

```rust
if current_key.len() > MAX_KEY_LENGTH {
    return Err(format!(
        "Invalid path: key segment exceeds {MAX_KEY_LENGTH} characters \
         (got {} so far)",
        current_key.len()
    ));
}
```

> **Why check after push?** We detect the violation at the byte it crosses the
> threshold — the error message reports the actual (truncated) length found,
> which is more informative than checking before.

**REFACTOR**

The check is a one-liner inside a hot character-processing loop. No further
refactoring needed.

**CLEANUP**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run
```

Commit:
```
feat(path): cap key segment length at 256 characters in parse_path
```

---

### Cycle 3: SQL integration tests

**RED**

Add to `test/sql/security_depth_limits.sql`:

```sql
-- Test: Empty match_key rejected
DO $$
BEGIN
    PERFORM jsonb_array_update_where(
        '{"items": [{"id": 1}]}'::jsonb,
        'items',
        '',                  -- empty match_key
        '1'::jsonb,
        '{"updated": true}'::jsonb
    );
    RAISE EXCEPTION 'Expected error but got none';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%empty%' THEN
        RAISE EXCEPTION 'Unexpected error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_empty_match_key_rejected' AS passed;

-- Test: Long key segment rejected
DO $$
DECLARE
    long_path text := repeat('a', 257) || '.b';
BEGIN
    PERFORM jsonb_delta_set_path('{}'::jsonb, long_path, '1'::jsonb);
    RAISE EXCEPTION 'Expected error but got none';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%exceeds%' AND sqlerrm NOT LIKE '%256%' THEN
        RAISE EXCEPTION 'Unexpected error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_long_key_segment_rejected' AS passed;
```

Run `just test-sql` — tests fail because validation doesn't exist yet. After
Cycles 1-2 are done, rerun — both rows should show `passed`.

**CLEANUP**

Commit:
```
test(security): add SQL tests for empty match_key and long key segment
```

---

## Files Modified

| File | Change |
|------|--------|
| `src/array_ops.rs` | Add `validate_match_key`, guard 4 functions here |
| `src/merge.rs` | Guard `jsonb_smart_patch_array` |
| `src/lib.rs` | Guard `jsonb_array_contains_id`, `jsonb_delta_array_update_where_path` |
| `src/path.rs` | Add `MAX_KEY_LENGTH`, guard in `parse_path()` |
| `test/sql/security_depth_limits.sql` | 2 new SQL test cases |

## Dependencies

- Requires: Phase 1 complete (establishes the validation pattern to follow)
- Blocks: None

## Status
[ ] Not Started
