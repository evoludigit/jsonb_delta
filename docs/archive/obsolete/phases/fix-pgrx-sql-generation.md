# Phase Plan: Fix pgrx SQL Generation for JSONB Functions

## Objective
Fix pgrx SQL generation issue where 2 out of 3 functions are detected during compilation but fail to generate `CREATE FUNCTION` statements in the extension SQL file. Make all three custom Rust functions (`jsonb_merge_shallow`, `jsonb_array_update_where`, `jsonb_merge_at_path`) accessible from PostgreSQL SQL.

## Summary of Fix

**Root Cause**: Inconsistent type signatures across functions confuse pgrx's SQL generator.

**Solution**: Match all functions to the working pattern used by `jsonb_merge_shallow`:
1. Use `Option<T>` for JSONB parameters ✅
2. Include `strict` attribute ✅
3. Use `&str` for text parameters (not `String`) ✅

**Changes Required**:
- **`jsonb_array_update_where`**: Change `JsonB` → `Option<JsonB>`, `String` → `&str`, add return `Some()`
- **`jsonb_merge_at_path`**: Add `strict` attribute, change path collection to owned strings
- **Tests**: Update function calls to match new signatures (add `Some()` wrappers)

**Impact**: Type signature changes only - no logic changes, no new features.

## Context

### Current Situation
- **pgrx version**: 0.12.8
- **PostgreSQL version**: 17.7 (pgrx-managed)
- **Detection**: All 3 functions are discovered during compilation
- **SQL generation**: Only 1 function (`jsonb_merge_shallow`) appears in generated SQL file
- **Impact**: Functions `jsonb_array_update_where` and `jsonb_merge_at_path` are compiled but not callable from SQL

### Root Cause Analysis

After analyzing the code in `src/lib.rs`, I've identified **inconsistent parameter type declarations** as the root cause:

#### Working Function (SQL Generated)
```rust
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_merge_shallow(
    target: Option<JsonB>,  // ✅ Option<JsonB> with strict
    source: Option<JsonB>,  // ✅ Option<JsonB> with strict
) -> Option<JsonB>
```

**Note**: This function uses `Option<T>` WITH `strict`, which is technically redundant (strict means PostgreSQL returns NULL before calling the function if any arg is NULL), but pgrx accepts this pattern and generates SQL correctly.

#### Broken Functions (No SQL Generated)

**Function 1: jsonb_array_update_where**
```rust
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where(
    target: JsonB,        // ❌ NOT Option<JsonB> (inconsistent with working function)
    array_path: String,   // ❌ Owned String instead of &str
    match_key: String,    // ❌ Owned String instead of &str
    match_value: JsonB,   // ❌ NOT Option<JsonB>
    updates: JsonB,       // ❌ NOT Option<JsonB>
) -> JsonB                // ❌ NOT Option<JsonB>
```

**Function 2: jsonb_merge_at_path**
```rust
#[pg_extern(immutable, parallel_safe)]  // ❌ Missing strict attribute
fn jsonb_merge_at_path(
    target: Option<JsonB>,           // ✅ Option<JsonB> (correct)
    source: Option<JsonB>,           // ✅ Option<JsonB> (correct)
    path: pgrx::Array<&str>,         // ✅ Actually correct (common pattern)
) -> Option<JsonB>
```

### Key Issues Identified

1. **Inconsistency with working pattern**: The working function uses `Option<T>` + `strict`, but broken functions deviate from this pattern
2. **Owned `String` vs `&str`**: Using owned `String` parameters instead of `&str` may confuse pgrx's SQL generator (though both map to `text`, `&str` is more idiomatic)
3. **Missing `strict` attribute**: `jsonb_merge_at_path` lacks the `strict` attribute that the working function has
4. **Test code mismatch**: Some tests call functions with wrong parameter types

### Strategy: Match the Working Pattern

Since `jsonb_merge_shallow` (with `Option<T>` + `strict`) generates SQL correctly, we'll apply the same pattern to the broken functions. While technically redundant, this pattern is proven to work with pgrx 0.12.8.

### Expected Behavior

All three functions should generate SQL like:

```sql
-- jsonb_ivm extension version 0.1.0

CREATE FUNCTION jsonb_merge_shallow(target jsonb, source jsonb)
RETURNS jsonb IMMUTABLE STRICT PARALLEL SAFE LANGUAGE c
AS 'MODULE_PATHNAME', 'jsonb_merge_shallow_wrapper';

CREATE FUNCTION jsonb_array_update_where(target jsonb, array_path text, match_key text, match_value jsonb, updates jsonb)
RETURNS jsonb IMMUTABLE STRICT PARALLEL SAFE LANGUAGE c
AS 'MODULE_PATHNAME', 'jsonb_array_update_where_wrapper';

CREATE FUNCTION jsonb_merge_at_path(target jsonb, source jsonb, path text[])
RETURNS jsonb IMMUTABLE PARALLEL SAFE LANGUAGE c
AS 'MODULE_PATHNAME', 'jsonb_merge_at_path_wrapper';
```

## Files to Modify

- `src/lib.rs` - Fix function signatures and parameter types

## Implementation Steps

### Step 1: Standardize `jsonb_array_update_where` to Match Working Pattern

**Change function signature** to match `jsonb_merge_shallow` pattern: `Option<T>` wrapping with `strict` attribute, and use `&str` instead of owned `String`:

```rust
// BEFORE (current - broken)
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where(
    target: JsonB,
    array_path: String,
    match_key: String,
    match_value: JsonB,
    updates: JsonB,
) -> JsonB {
    let mut target_value: Value = target.0;

    // Navigate to array location (single level for now)
    let array = match target_value.get_mut(&array_path) {
        Some(arr) => arr,
        None => {
            error!(
                "Path '{}' does not exist in document",
                array_path
            );
        }
    };

    // ... rest of implementation ...

    JsonB(target_value)
}

// AFTER (fixed - matches working pattern)
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where(
    target: Option<JsonB>,      // ✅ Now matches jsonb_merge_shallow
    array_path: &str,            // ✅ More idiomatic than String
    match_key: &str,             // ✅ More idiomatic than String
    match_value: Option<JsonB>,  // ✅ Now matches jsonb_merge_shallow
    updates: Option<JsonB>,      // ✅ Now matches jsonb_merge_shallow
) -> Option<JsonB> {             // ✅ Now matches jsonb_merge_shallow
    // Unwrap Options (strict means PostgreSQL ensures non-NULL, but we unwrap for safety)
    let target = target?;
    let match_value = match_value?;
    let updates = updates?;

    let mut target_value: Value = target.0;

    // Navigate to array location (single level for now)
    // Note: array_path is now &str, so no & needed
    let array = match target_value.get_mut(array_path) {
        Some(arr) => arr,
        None => {
            error!(
                "Path '{}' does not exist in document",
                array_path
            );
        }
    };

    // ... rest of implementation unchanged (no .to_string() calls needed) ...

    Some(JsonB(target_value))  // ✅ Wrap in Some()
}
```

**Key changes**:
- `target: JsonB` → `target: Option<JsonB>` (matches working function)
- `array_path: String` → `array_path: &str` (more idiomatic, avoid allocation)
- `match_key: String` → `match_key: &str` (more idiomatic, avoid allocation)
- `match_value: JsonB` → `match_value: Option<JsonB>` (matches working function)
- `updates: JsonB` → `updates: Option<JsonB>` (matches working function)
- Return type: `JsonB` → `Option<JsonB>` (matches working function)
- Add Option unwrapping: `let target = target?;` etc. (matches working function pattern)
- Update return statement: `JsonB(...)` → `Some(JsonB(...))` (matches working function)
- Remove `&` in `get_mut(&array_path)` → `get_mut(array_path)` since now using `&str`

### Step 2: Add `strict` Attribute to `jsonb_merge_at_path`

**Add `strict` attribute** to match the working function pattern. Keep `Array<&str>` as-is (this is a common pgrx pattern):

```rust
// BEFORE (current - broken, missing strict)
#[pg_extern(immutable, parallel_safe)]  // ❌ Missing strict
fn jsonb_merge_at_path(
    target: Option<JsonB>,
    source: Option<JsonB>,
    path: pgrx::Array<&str>,
) -> Option<JsonB> {
    let target = target?;
    let source = source?;

    let mut target_value: Value = target.0;
    // ...
    let path_vec: Vec<&str> = path.iter().flatten().collect();
    // ...
}

// AFTER (fixed - added strict to match working pattern)
#[pg_extern(immutable, parallel_safe, strict)]  // ✅ Added strict
fn jsonb_merge_at_path(
    target: Option<JsonB>,       // ✅ Keeps Option (matches jsonb_merge_shallow)
    source: Option<JsonB>,       // ✅ Keeps Option (matches jsonb_merge_shallow)
    path: pgrx::Array<&str>,     // ✅ Keep Array<&str> (common pgrx pattern)
) -> Option<JsonB> {
    let target = target?;
    let source = source?;

    let mut target_value: Value = target.0;

    let source_obj = match source.0.as_object() {
        Some(obj) => obj,
        None => {
            error!(
                "source argument must be a JSONB object, got: {}",
                value_type_name(&source.0)
            );
        }
    };

    // Collect path into owned Vec<String> to avoid lifetime issues
    let path_vec: Vec<String> = path
        .iter()
        .flatten()
        .map(|s| s.to_owned())  // Convert &str to String
        .collect();

    // If path is empty, merge at root
    if path_vec.is_empty() {
        let target_obj = match target_value.as_object_mut() {
            Some(obj) => obj,
            None => {
                error!(
                    "target argument must be a JSONB object when path is empty, got: {}",
                    value_type_name(&target_value)
                );
            }
        };

        for (key, value) in source_obj.iter() {
            target_obj.insert(key.clone(), value.clone());
        }

        return Some(JsonB(target_value));
    }

    // Navigate to parent of target path
    let mut current = &mut target_value;
    for (i, key) in path_vec.iter().enumerate() {
        let is_last = i == path_vec.len() - 1;

        if is_last {
            // At target location - merge here
            let parent_obj = match current.as_object_mut() {
                Some(obj) => obj,
                None => {
                    error!(
                        "Path navigation failed: expected object at {:?}, got: {}",
                        &path_vec[..i],
                        value_type_name(current)
                    );
                }
            };

            let target_at_path = parent_obj
                .entry(key.clone())
                .or_insert_with(|| Value::Object(Default::default()));

            let merge_target = match target_at_path.as_object_mut() {
                Some(obj) => obj,
                None => {
                    error!(
                        "Cannot merge into non-object at path {:?}, found: {}",
                        path_vec,
                        value_type_name(target_at_path)
                    );
                }
            };

            for (key, value) in source_obj.iter() {
                merge_target.insert(key.clone(), value.clone());
            }
        } else {
            // Navigate deeper
            let current_type = value_type_name(current);
            let obj = match current.as_object_mut() {
                Some(obj) => obj,
                None => {
                    error!(
                        "Path navigation failed at {:?}, expected object, got: {}",
                        &path_vec[..=i],
                        current_type
                    );
                }
            };

            current = obj
                .entry(key.clone())
                .or_insert_with(|| Value::Object(Default::default()));
        }
    }

    Some(JsonB(target_value))
}
```

**Key changes**:
- Add `strict` attribute to `#[pg_extern]` (matches working function)
- Keep `path: pgrx::Array<&str>` unchanged (this is a valid pgrx pattern)
- Change path collection to owned: `.map(|s| s.to_owned()).collect()` (avoids lifetime issues in function body)
- No other changes needed to implementation logic

### Step 3: Fix Test Code to Match Updated Signatures

**Update tests** in `src/lib.rs` to use correct parameter types that match the new signatures.

#### For `jsonb_array_update_where` tests:

```rust
// BEFORE (current - calling with wrong types)
let result = crate::jsonb_array_update_where(
    target,                      // ❌ Should be Some(target)
    "dns_servers".to_string(),   // ❌ Should be &str
    "id".to_string(),            // ❌ Should be &str
    JsonB(json!(42)),            // ❌ Should be Some(JsonB(...))
    JsonB(json!({"ip": "8.8.8.8"})),  // ❌ Should be Some(JsonB(...))
).expect("update should succeed");

// AFTER (fixed - matches new signature with Option<T>)
let result = crate::jsonb_array_update_where(
    Some(target),                    // ✅ Option<JsonB>
    "dns_servers",                   // ✅ &str literal (no .to_string())
    "id",                            // ✅ &str literal (no .to_string())
    Some(JsonB(json!(42))),          // ✅ Option<JsonB>
    Some(JsonB(json!({"ip": "8.8.8.8"}))),  // ✅ Option<JsonB>
).expect("update should succeed");
```

**Update these test functions** (search for `crate::jsonb_array_update_where` calls):
- `test_array_update_where_basic`
- `test_array_update_where_no_match`
- `test_array_update_where_large_array`
- `test_array_update_where_nested_path`
- `test_array_update_where_invalid_path`
- `test_array_update_where_invalid_updates`

**Search pattern**: `jsonb_array_update_where\(`

#### For `jsonb_merge_at_path` tests:

```rust
// BEFORE (current - may already be correct)
let result = crate::jsonb_merge_at_path(
    Some(target),   // ✅ Already Option<JsonB>
    Some(source),   // ✅ Already Option<JsonB>
    pgrx::Array::from(vec!["network_configuration"]),  // ✅ &str inferred
).expect("merge should succeed");

// AFTER (should remain the same - no changes needed)
let result = crate::jsonb_merge_at_path(
    Some(target),   // ✅ Option<JsonB>
    Some(source),   // ✅ Option<JsonB>
    pgrx::Array::from(vec!["network_configuration"]),  // ✅ &str works fine
).expect("merge should succeed");
```

**Check these test functions** (they may already be correct):
- `test_merge_at_path_root`
- `test_merge_at_path_nested`
- `test_merge_at_path_deep`

**Search pattern**: `jsonb_merge_at_path\(`

**Note**: The `jsonb_merge_at_path` tests likely don't need changes since the function already used `Option<T>`. Only verify they compile correctly.

### Step 4: Clean Build and Reinstall Extension

**Execute clean build sequence**:

```bash
# Clean all build artifacts
cargo clean

# Remove existing pgrx installation
rm -rf ~/.pgrx/17.7/pgrx-install/share/postgresql/extension/jsonb_ivm*
rm -rf ~/.pgrx/17.7/pgrx-install/lib/postgresql/jsonb_ivm*

# Rebuild and install extension
cargo pgrx install --release --pg-config ~/.pgrx/17.7/pgrx-install/bin/pg_config
```

**Expected output should show**:
```
Discovered 3 SQL entities: 0 schemas (0 unique), 3 functions
```

### Step 5: Verify SQL Generation

**Check generated SQL file** contains all three functions:

```bash
cat ~/.pgrx/17.7/pgrx-install/share/postgresql/extension/jsonb_ivm--0.1.0.sql
```

**Expected output**:
```sql
-- jsonb_ivm extension version 0.1.0

CREATE FUNCTION jsonb_merge_shallow(target jsonb, source jsonb)
RETURNS jsonb IMMUTABLE STRICT PARALLEL SAFE LANGUAGE c
AS 'MODULE_PATHNAME', 'jsonb_merge_shallow_wrapper';

CREATE FUNCTION jsonb_array_update_where(target jsonb, array_path text, match_key text, match_value jsonb, updates jsonb)
RETURNS jsonb IMMUTABLE STRICT PARALLEL SAFE LANGUAGE c
AS 'MODULE_PATHNAME', 'jsonb_array_update_where_wrapper';

CREATE FUNCTION jsonb_merge_at_path(target jsonb, source jsonb, path text[])
RETURNS jsonb IMMUTABLE PARALLEL SAFE LANGUAGE c
AS 'MODULE_PATHNAME', 'jsonb_merge_at_path_wrapper';
```

**Success criteria**:
- ✅ All 3 `CREATE FUNCTION` statements present
- ✅ Parameter types correctly mapped (`text`, `jsonb`, `text[]`)
- ✅ Function attributes correct (`IMMUTABLE`, `STRICT`, `PARALLEL SAFE`)

### Step 6: Run Rust Tests

**Execute pgrx test suite** to verify all functionality works:

```bash
cargo pgrx test --release
```

**Expected output**:
- All tests should pass
- No compilation errors
- No NULL handling issues
- Array parameter tests work correctly

**If tests fail**:
- Check error messages for NULL handling issues
- Verify `Option<T>` unwrapping is correct
- Check string parameter usage (no `.to_string()` needed for `&str`)
- Verify array parameter collection

### Step 7: Test Functions from PostgreSQL

**Start PostgreSQL** and load extension:

```bash
cargo pgrx start pg17
psql -h localhost -p 28817 -d postgres
```

**Create extension and test each function**:

```sql
-- Load extension
CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

-- Test 1: jsonb_merge_shallow (should already work)
SELECT jsonb_merge_shallow(
    '{"a": 1, "b": 2}'::jsonb,
    '{"b": 99, "c": 3}'::jsonb
);
-- Expected: {"a": 1, "b": 99, "c": 3}

-- Test 2: jsonb_array_update_where (CURRENTLY BROKEN - should now work)
SELECT jsonb_array_update_where(
    '{"dns_servers": [{"id": 42, "ip": "1.1.1.1"}, {"id": 43, "ip": "2.2.2.2"}]}'::jsonb,
    'dns_servers',
    'id',
    '42'::jsonb,  -- Note: JSON number 42, not string "42"
    '{"ip": "8.8.8.8"}'::jsonb
);
-- Expected: {"dns_servers": [{"id": 42, "ip": "8.8.8.8"}, {"id": 43, "ip": "2.2.2.2"}]}
-- Note: If the "id" field in JSON is a string ("id": "42"), use '"42"'::jsonb instead

-- Test 3: jsonb_merge_at_path (CURRENTLY BROKEN - should now work)
SELECT jsonb_merge_at_path(
    '{"id": 1, "network_configuration": {"id": 17, "name": "old"}}'::jsonb,
    '{"name": "updated"}'::jsonb,
    ARRAY['network_configuration']
);
-- Expected: {"id": 1, "network_configuration": {"id": 17, "name": "updated"}}

-- Test 4: jsonb_merge_at_path with empty array (root merge)
SELECT jsonb_merge_at_path(
    '{"a": 1, "b": 2}'::jsonb,
    '{"b": 99, "c": 3}'::jsonb,
    ARRAY[]::text[]
);
-- Expected: {"a": 1, "b": 99, "c": 3}

-- Test 5: Error handling - NULL inputs
SELECT jsonb_array_update_where(
    NULL,
    'path',
    'key',
    '1'::jsonb,
    '{}'::jsonb
);
-- Expected: NULL (due to strict attribute)
```

**Success criteria**:
- ✅ All 5 queries execute without errors
- ✅ No "function does not exist" errors
- ✅ Results match expected output
- ✅ NULL handling works correctly

## Verification Commands

### Build Verification
```bash
# Should succeed with no errors
cargo pgrx install --release --pg-config ~/.pgrx/17.7/pgrx-install/bin/pg_config

# Should show "Discovered 3 SQL entities"
# Check build output for this line
```

### SQL Generation Verification
```bash
# Should show 3 CREATE FUNCTION statements
cat ~/.pgrx/17.7/pgrx-install/share/postgresql/extension/jsonb_ivm--0.1.0.sql | grep "CREATE FUNCTION" | wc -l
# Expected output: 3

# Should list all three function names
cat ~/.pgrx/17.7/pgrx-install/share/postgresql/extension/jsonb_ivm--0.1.0.sql | grep "CREATE FUNCTION"
```

### Test Verification
```bash
# All tests should pass
cargo pgrx test --release
# Expected: "test result: ok"
```

### Runtime Verification
```sql
-- Connect to test database
psql -h localhost -p 28817 -d postgres

-- List all functions in extension
\dx+ jsonb_ivm

-- Should show 3 functions:
-- jsonb_merge_shallow(jsonb, jsonb)
-- jsonb_array_update_where(jsonb, text, text, jsonb, jsonb)
-- jsonb_merge_at_path(jsonb, jsonb, text[])
```

## Acceptance Criteria

- [ ] Build succeeds with no errors or warnings
- [ ] pgrx reports "Discovered 3 SQL entities: 0 schemas (0 unique), 3 functions"
- [ ] Generated SQL file contains exactly 3 `CREATE FUNCTION` statements
- [ ] All Rust tests pass (`cargo pgrx test --release`)
- [ ] All 3 functions are callable from PostgreSQL SQL
- [ ] Functions produce correct results for valid inputs
- [ ] Functions handle NULL inputs correctly (return NULL for strict functions)
- [ ] Functions produce proper error messages for invalid inputs
- [ ] No regression in existing `jsonb_merge_shallow` functionality

## DO NOT

- ❌ **DO NOT** change function logic or algorithms - only fix parameter types
- ❌ **DO NOT** add new features or functionality
- ❌ **DO NOT** modify error handling logic (except for NULL checks)
- ❌ **DO NOT** change function names or PostgreSQL-visible signatures
- ❌ **DO NOT** upgrade pgrx version (0.12.8 must be maintained)
- ❌ **DO NOT** modify `Cargo.toml` dependencies or features
- ❌ **DO NOT** change function attributes (`immutable`, `parallel_safe`, `strict`)
- ❌ **DO NOT** add new test cases - only fix existing ones
- ❌ **DO NOT** modify comments or documentation strings

## Rollback Plan

If changes don't fix the issue, try these alternatives in order:

1. **Revert changes**: `git checkout src/lib.rs`

2. **Alternative 1**: Remove `strict` from all functions, keep `Option<T>`:
   ```rust
   #[pg_extern(immutable, parallel_safe)]  // No strict
   fn jsonb_array_update_where(
       target: Option<JsonB>,
       array_path: Option<&str>,  // Now can be NULL
       // ...
   ) -> Option<JsonB>
   ```

3. **Alternative 2**: Use unwrapped types WITH `strict` (no Options):
   ```rust
   #[pg_extern(immutable, parallel_safe, strict)]
   fn jsonb_array_update_where(
       target: JsonB,         // No Option
       array_path: &str,      // No Option
       // ...
   ) -> JsonB                 // No Option
   ```

4. **Alternative 3**: Try `Array<String>` instead of `Array<&str>`:
   ```rust
   fn jsonb_merge_at_path(
       target: Option<JsonB>,
       source: Option<JsonB>,
       path: pgrx::Array<String>,  // Owned String
   ) -> Option<JsonB>
   ```

5. **Last resort**: File issue with pgrx project at https://github.com/pgcentralfoundation/pgrx/issues and consider manual SQL file generation as temporary workaround

## Success Metrics

**Before fix**:
- 3 functions compiled, 1 SQL statement generated (33% success rate)
- 2 functions inaccessible from SQL

**After fix**:
- 3 functions compiled, 3 SQL statements generated (100% success rate)
- All functions accessible and working from SQL
- All tests passing
- No performance regression

## References

- **pgrx documentation**: https://github.com/pgcentralfoundation/pgrx
- **pgrx SQL generation**: https://github.com/pgcentralfoundation/pgrx/blob/develop/pgrx-sql-entity-graph/
- **PostgreSQL C function API**: https://www.postgresql.org/docs/current/xfunc-c.html
- **Issue document**: `/home/lionel/code/jsonb_ivm/PGRX_INTEGRATION_ISSUE.md`

## Notes

- This is a **type signature fix**, not a logic change
- **Strategy**: Match the working function's pattern (`Option<T>` + `strict` attribute)
- The core issue is that pgrx's SQL generator requires consistency in type patterns across functions
- While `Option<T>` + `strict` is technically redundant (strict makes NULL checks at PostgreSQL level), pgrx 0.12.8 accepts and generates SQL for this pattern
- Using `&str` instead of `String` is more idiomatic and avoids unnecessary allocations
- Keeping `Array<&str>` is fine (common pgrx pattern), but collect to owned `Vec<String>` internally to avoid lifetime issues

## Technical Background: `strict` vs `Option<T>`

**Understanding the redundancy**:
- PostgreSQL `STRICT` functions return NULL if ANY argument is NULL, WITHOUT calling the function
- Therefore, with `strict`, Rust function parameters never receive NULL values
- Using `Option<T>` with `strict` is redundant but safe (the Option will always be Some)
- pgrx 0.12.8 accepts both patterns, but requires consistency for SQL generation

**Why we use the redundant pattern**:
- `jsonb_merge_shallow` uses `Option<T>` + `strict` and generates SQL correctly
- Rather than risk breaking the working function, we match its pattern
- This is a pragmatic "follow the working example" approach

## Time Estimate

- **Implementation**: 20 minutes (code changes + test fixes)
- **Verification**: 15 minutes (build + test + SQL verification)
- **Total**: 35 minutes

## Priority

**CRITICAL** - Blocks POC completion and performance validation. Cannot proceed with benchmarking until functions are accessible from SQL.
