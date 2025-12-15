# PGRX SQL Generation Issue: Functions Detected But Not Generated

## Problem Summary

pgrx successfully detects custom Rust functions during compilation (shows "3 SQL entities discovered") but fails to generate the corresponding `CREATE FUNCTION` statements in the extension SQL file. This prevents the functions from being callable from PostgreSQL SQL.

## Current Status

- ✅ **Functions compile successfully** in Rust
- ✅ **pgrx detects functions** ("Discovered 3 SQL entities: 0 schemas, 3 functions")
- ❌ **SQL generation fails** - only 1/3 functions appear in generated `.sql` file
- ❌ **Functions not callable** from PostgreSQL SQL

## Environment

- **pgrx version**: 0.12.8
- **PostgreSQL version**: 17.7 (pgrx-managed)
- **Rust version**: stable-x86_64-unknown-linux-gnu
- **OS**: Linux (Ubuntu/Debian-based)
- **Project**: jsonb_ivm extension

## Affected Functions

### Working Function (Generates SQL)
```rust
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_merge_shallow(
    target: Option<JsonB>,
    source: Option<JsonB>,
) -> Option<JsonB>
```
**SQL Generated**: ✅
```sql
CREATE FUNCTION jsonb_merge_shallow(target jsonb, source jsonb)
RETURNS jsonb IMMUTABLE STRICT PARALLEL SAFE LANGUAGE c
AS 'MODULE_PATHNAME', 'jsonb_merge_shallow_wrapper';
```

### Broken Functions (Detected but no SQL)

#### Function 1: jsonb_array_update_where
```rust
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where(
    target: JsonB,
    array_path: String,
    match_key: String,
    match_value: JsonB,
    updates: JsonB,
) -> JsonB
```
**SQL Generated**: ❌ (not present)

#### Function 2: jsonb_merge_at_path
```rust
#[pg_extern(immutable, parallel_safe)]
fn jsonb_merge_at_path(
    target: Option<JsonB>,
    source: Option<JsonB>,
    path: pgrx::Array<&str>,
) -> Option<JsonB>
```
**SQL Generated**: ❌ (not present)

## Investigation Notes

### What We've Tried

1. **Parameter Type Variations**:
   - `String` vs `&str` vs `Option<String>` vs `Option<&str>`
   - `pgrx::Array<&str>` vs `Vec<String>`
   - `JsonB` vs `Option<JsonB>`

2. **Function Attributes**:
   - `#[pg_extern(immutable, parallel_safe, strict)]`
   - `#[pg_extern(immutable, parallel_safe)]` (without strict)

3. **Build Variations**:
   - Clean builds (`cargo clean`)
   - Different feature flags
   - Manual extension recreation

4. **Code Structure**:
   - Functions in same module as working function
   - Same naming conventions
   - Same error handling patterns

### Key Observations

- **Detection works**: pgrx consistently reports "3 functions" discovered
- **Compilation succeeds**: No Rust compilation errors
- **Extension builds**: Shared library created successfully
- **Only working function**: The one with `Option<JsonB>` parameters generates SQL
- **Pattern**: Functions with string parameters fail SQL generation

### Generated SQL File Analysis

**Expected** (if working):
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

**Actual** (current):
```sql
-- jsonb_ivm extension version 0.1.0

CREATE FUNCTION jsonb_merge_shallow(target jsonb, source jsonb)
RETURNS jsonb IMMUTABLE STRICT PARALLEL SAFE LANGUAGE c
AS 'MODULE_PATHNAME', 'jsonb_merge_shallow_wrapper';
```

## Steps to Reproduce

1. **Clone and setup project**:
   ```bash
   git clone <repo>
   cd jsonb_ivm
   cargo pgrx init --pg17
   ```

2. **Build extension**:
   ```bash
   cargo pgrx install --release --pg-config ~/.pgrx/17.7/pgrx-install/bin/pg_config
   ```

3. **Check detection** (should show 3 functions):
   ```
   Discovered 3 SQL entities: 0 schemas (0 unique), 3 functions
   ```

4. **Check generated SQL** (only 1 function):
   ```bash
   cat ~/.pgrx/17.7/pgrx-install/share/postgresql/extension/jsonb_ivm--0.1.0.sql
   ```

5. **Try to call missing functions** (should fail):
   ```sql
   SELECT jsonb_array_update_where('{"test": []}'::jsonb, 'test', 'id', '1'::jsonb, '{}'::jsonb);
   -- ERROR: function jsonb_array_update_where(jsonb, unknown, unknown, jsonb, jsonb) does not exist
   ```

## Root Cause Hypothesis

The issue appears to be in pgrx's SQL generation logic for functions with non-`JsonB` parameter types. The working function only uses `Option<JsonB>` parameters, while the broken functions include `String`, `&str`, and `pgrx::Array<&str>` parameters.

Possible causes:
1. **Type mapping issue**: pgrx doesn't know how to map `String`/`&str` to PostgreSQL `text` type in SQL generation
2. **Array handling**: `pgrx::Array<T>` types not fully supported in SQL generation
3. **Lifetime issues**: `&str` parameters causing complications
4. **Code generation bug**: pgrx generates the detection logic but fails the SQL emission

## Expected Behavior

All detected functions should generate corresponding `CREATE FUNCTION` statements in the extension SQL file, allowing them to be called from PostgreSQL SQL.

## Impact

- **Blocks POC completion**: Cannot measure performance of custom Rust functions
- **Prevents deployment**: Functions exist but are inaccessible
- **Limits functionality**: Users cannot leverage the implemented algorithms

## Priority

**HIGH** - This is a critical blocker for the JSONB IVM POC and prevents validation of the core performance hypothesis.

## Files to Examine

- `src/lib.rs` - Function definitions
- `~/.pgrx/17.7/pgrx-install/share/postgresql/extension/jsonb_ivm--0.1.0.sql` - Generated SQL
- `Cargo.toml` - Dependencies and features
- pgrx source code (if accessible) - SQL generation logic

## Next Steps

1. **Investigate pgrx source**: Find where SQL generation differs from detection
2. **Test minimal reproduction**: Create minimal extension with string parameters
3. **Check pgrx issues/PRs**: Look for similar reported problems
4. **Alternative approaches**: Consider manual SQL generation or different parameter types

## Related Issues

- pgrx issue tracker search for "SQL generation" or "function detection"
- PostgreSQL extension development documentation
- Similar issues with complex parameter types in pgrx

---

**Assigned to**: @pgrx-expert or @rust-postgres-specialist
**Priority**: HIGH
**Estimated effort**: 2-4 hours investigation + fix
**Deadline**: Needed to complete POC performance validation</content>
<parameter name="filePath">PGRX_INTEGRATION_ISSUE.md
