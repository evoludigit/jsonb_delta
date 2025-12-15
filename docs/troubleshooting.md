# Troubleshooting Guide

Common issues and solutions for jsonb_ivm PostgreSQL extension.

---

## Table of Contents

- [Installation Issues](#installation-issues)
- [Extension Loading Problems](#extension-loading-problems)
- [Function Errors](#function-errors)
- [Performance Issues](#performance-issues)
- [Version Conflicts](#version-conflicts)
- [Debugging Tips](#debugging-tips)

---

## Installation Issues

### Problem: `cargo pgrx install` fails with "pgrx not initialized"

**Symptom:**

```bash
$ cargo pgrx install
Error: pgrx not initialized
```

**Solution:**

```bash
# Initialize pgrx (one-time setup)
cargo pgrx init

# If you have multiple PostgreSQL versions, specify one:
cargo pgrx init --pg17 /usr/bin/pg_config
```

---

### Problem: Missing PostgreSQL development headers

**Symptom:**

```bash
error: failed to run custom build command for `pgrx-pg-sys`
  Could not find `pg_config`
```

**Solution:**

**Debian/Ubuntu:**

```bash
sudo apt-get install postgresql-server-dev-17 build-essential libclang-dev
```

**Arch Linux:**

```bash
sudo pacman -S postgresql-libs base-devel clang
```

**macOS:**

```bash
brew install postgresql@17 llvm
```

Then reinitialize pgrx:

```bash
cargo pgrx init
```

---

### Problem: Permission denied during install

**Symptom:**

```bash
error: failed to copy `/target/release/libjsonb_ivm.so` to `/usr/lib/postgresql/17/lib/`
Permission denied
```

**Solution:**

```bash
# Use sudo for system-wide installation
sudo cargo pgrx install --release

# Or install to user directory
cargo pgrx install --release --pg-config ~/.pgrx/17.7/pgrx-install/bin/pg_config
```

---

### Problem: Wrong PostgreSQL version detected

**Symptom:**

```bash
Building for PostgreSQL 13, but I want PostgreSQL 17
```

**Solution:**

```bash
# Specify version explicitly
cargo pgrx install --release --pg-config /usr/bin/pg_config

# Or set default version in Cargo.toml
default = ["pg17"]  # Change to desired version
```

---

## Extension Loading Problems

### Problem: `CREATE EXTENSION` fails with "file not found"

**Symptom:**

```sql
postgres=# CREATE EXTENSION jsonb_ivm;
ERROR:  could not open extension control file "/usr/share/postgresql/17/extension/jsonb_ivm.control": No such file or directory
```

**Solution:**

```bash
# Check if extension is installed
ls -la $(pg_config --sharedir)/extension/jsonb_ivm*

# If missing, reinstall:
sudo cargo pgrx install --release

# Verify installation:
ls -la $(pg_config --pkglibdir)/jsonb_ivm.so
ls -la $(pg_config --sharedir)/extension/jsonb_ivm*
```

**Expected output:**

```text
```

---

### Problem: Function not found after installing extension

**Symptom:**

```sql
postgres=# CREATE EXTENSION jsonb_ivm;
CREATE EXTENSION

postgres=# SELECT jsonb_smart_patch_scalar('{"a":1}'::jsonb, '{"b":2}'::jsonb);
ERROR:  function jsonb_smart_patch_scalar(jsonb, jsonb) does not exist
```

**Solution:**

### Option 1: Reinstall extension

```sql
DROP EXTENSION jsonb_ivm CASCADE;
CREATE EXTENSION jsonb_ivm;
```

### Option 2: Check version

```sql
-- Check installed version
SELECT * FROM pg_available_extensions WHERE name = 'jsonb_ivm';

-- Should show version 0.3.0
-- If showing older version, upgrade:
ALTER EXTENSION jsonb_ivm UPDATE TO '0.3.0';
```

### Option 3: Verify SQL generation

```bash
# Regenerate SQL files
cargo pgrx schema > sql/jsonb_ivm--0.3.0.sql

# Reinstall
sudo cargo pgrx install --release
```

---

### Problem: Version mismatch after upgrade

**Symptom:**

```sql
ALTER EXTENSION jsonb_ivm UPDATE TO '0.3.0';
ERROR:  extension "jsonb_ivm" has no update path from version "0.2.0" to version "0.3.0"
```

**Solution:**

```bash
# Ensure upgrade path file exists
ls -la $(pg_config --sharedir)/extension/jsonb_ivm--0.2.0--0.3.0.sql

# If missing, regenerate and reinstall:
cargo pgrx schema > sql/jsonb_ivm--0.3.0.sql
# Create upgrade path manually if needed (see sql/jsonb_ivm--0.2.0--0.3.0.sql)
sudo cargo pgrx install --release
```

---

## Function Errors

### Problem: NULL results when expecting data

**Symptom:**

```sql
SELECT jsonb_array_update_where(
    '{"posts": [{"id": 1}]}'::jsonb,
    'posts',
    'id',
    '1'::jsonb,
    '{"title": "New"}'::jsonb
);
-- Result: NULL (unexpected)
```

**Cause:**
Most functions are marked `STRICT` and return NULL if any parameter is NULL.

**Solution:**

```sql
-- Check for NULL parameters
SELECT
    '{"posts": [{"id": 1}]}'::jsonb IS NULL AS target_null,
    'posts' IS NULL AS path_null,
    'id' IS NULL AS key_null,
    '1'::jsonb IS NULL AS value_null,
    '{"title": "New"}'::jsonb IS NULL AS updates_null;

-- Common cause: JSONB cast failures
-- WRONG: '1'::jsonb (becomes integer, not string)
-- RIGHT: '"1"'::jsonb (string) or to_jsonb('1'::text)
```

---

### Problem: Function returns original unchanged

**Symptom:**

```sql
SELECT jsonb_array_update_where(
    '{"posts": [{"id": 1}]}'::jsonb,
    'posts',
    'id',
    '99'::jsonb,  -- doesn't exist
    '{"title": "New"}'::jsonb
);
-- Result: {"posts": [{"id": 1}]} (unchanged)
```

**Cause:**
This is **expected behavior**. Functions return the original JSONB if:
- Path doesn't exist
- Match key not found
- Array is empty

**Solution:**

```sql
-- Verify element exists before update
SELECT jsonb_array_contains_id(
    '{"posts": [{"id": 1}]}'::jsonb,
    'posts',
    'id',
    '99'::jsonb
);
-- Returns: false (element not found)

-- Only update if element exists:
UPDATE tv_feed
SET data = jsonb_array_update_where(data, 'posts', 'id', match_id, updates)
WHERE jsonb_array_contains_id(data, 'posts', 'id', match_id);
```

---

### Problem: Type mismatch errors

**Symptom:**

```sql
SELECT jsonb_array_update_where(
    '{"posts": "not an array"}'::jsonb,
    'posts',
    'id',
    '1'::jsonb,
    '{}'::jsonb
);
-- Result: {"posts": "not an array"} (unchanged, no error)
```

**Cause:**
Functions are defensive and return original JSONB if types don't match.

**Solution:**

```sql
-- Verify structure before calling:
SELECT jsonb_typeof(data->'posts') FROM tv_feed;
-- Should return: 'array'

-- Check array element structure:
SELECT jsonb_array_length(data->'posts') FROM tv_feed;
```

---

### Problem: `jsonb_array_insert_where` doesn't sort

**Symptom:**

```sql
SELECT jsonb_array_insert_where(
    '{"posts": [{"id": 1, "date": "2024-01-01"}]}'::jsonb,
    'posts',
    '{"id": 2, "date": "2024-01-02"}'::jsonb,
    'date',
    'DESC'
);
-- Result: Wrong order
```

**Cause:**
Sort key field type mismatch or wrong format.

**Solution:**

```sql
-- Ensure sort field exists and has correct type
SELECT
    data->'posts'->0->'date' AS first_post_date,
    jsonb_typeof(data->'posts'->0->'date') AS date_type
FROM tv_feed;

-- If date is a string, ensure consistent format:
-- ISO 8601: "2024-01-02T10:30:00Z"
-- Or use numeric timestamps

-- For NULL sort (no sorting):
SELECT jsonb_array_insert_where(data, 'posts', new_post, NULL, NULL);
```

---

## Performance Issues

### Problem: Updates slower than expected

**Symptom:**

```sql
-- Expected: 2-3× faster than native SQL
-- Actual: Same speed or slower
```

**Diagnosis:**

```sql
-- 1. Check array size (small arrays may not benefit)
SELECT
    jsonb_array_length(data->'posts') AS array_size
FROM tv_feed
WHERE pk = 1;
-- jsonb_ivm benefits most for arrays > 32 elements

-- 2. Check query plan
EXPLAIN ANALYZE
UPDATE tv_feed
SET data = jsonb_array_update_where(data, 'posts', 'id', '42'::jsonb, updates)
WHERE pk = 1;

-- 3. Ensure indexes exist
SELECT * FROM pg_indexes WHERE tablename = 'tv_feed';
```

**Solutions:**

**Solutions:**

```sql
-- 1. Add foreign key indexes
CREATE INDEX idx_tv_user_fk_company ON tv_user(fk_company);
CREATE INDEX idx_tv_post_fk_user ON tv_post(fk_user);

-- 2. Add GIN index for JSONB containment checks
CREATE INDEX idx_tv_feed_posts ON tv_feed USING gin((data->'posts'));

-- 3. Filter rows before updating
UPDATE tv_feed
SET data = jsonb_array_update_where(data, 'posts', 'id', '42'::jsonb, updates)
WHERE jsonb_array_contains_id(data, 'posts', 'id', '42'::jsonb);  -- Filter first!

-- 4. Use batch functions for multiple updates
SELECT jsonb_array_update_where_batch(data, 'posts', 'id', updates_array);
-- vs multiple single updates
```

---

### Problem: Memory usage spikes

**Symptom:**

```text
PostgreSQL using excessive RAM during jsonb_ivm operations
```

**Cause:**
Processing very large JSONB documents or arrays.

**Solution:**

```sql
-- 1. Check JSONB document sizes
SELECT
    pg_size_pretty(pg_column_size(data)) AS doc_size,
    jsonb_array_length(data->'posts') AS array_length
FROM tv_feed
ORDER BY pg_column_size(data) DESC
LIMIT 10;

-- 2. Paginate large arrays (if > 1000 elements)
-- Instead of one 10,000-element array:
CREATE TABLE tv_feed_page (
    feed_id INT,
    page_num INT,
    data JSONB,  -- 100 posts per page
    PRIMARY KEY (feed_id, page_num)
);

-- 3. Increase work_mem if needed
SET work_mem = '256MB';  -- For current session
-- Or in postgresql.conf for permanent change
```

---

## Version Conflicts

### Problem: Multiple PostgreSQL versions installed

**Symptom:**

```bash
cargo pgrx install installs to wrong PostgreSQL version
```

**Solution:**

```bash
# List installed PostgreSQL versions
ls /usr/lib/postgresql/

# Install to specific version
cargo pgrx install --release --pg-config /usr/lib/postgresql/17/bin/pg_config

# Or set default in .cargo/config.toml:
[target.'cfg(all())']
rustflags = ["--cfg", "feature=\"pg17\""]
```

---

### Problem: pgrx version mismatch

**Symptom:**

```bash
error: package `pgrx v0.12.8` cannot be built because it requires rustc 1.70 or newer
```

**Solution:**

```bash
# Update Rust toolchain
rustup update stable
rustc --version  # Should be 1.70+

# Update cargo-pgrx
cargo install --locked --force cargo-pgrx

# Clean and rebuild
cargo clean
cargo build --release
```

---

## Debugging Tips

### Enable Debug Logging

```sql
-- Enable PostgreSQL debug logging
SET client_min_messages = DEBUG1;

-- Test function
SELECT jsonb_array_update_where(...);
```

### Check Extension Version

```sql
SELECT * FROM pg_extension WHERE extname = 'jsonb_ivm';
-- Shows installed version and schema

SELECT * FROM pg_available_extensions WHERE name = 'jsonb_ivm';
-- Shows available versions
```

### Inspect Function Definitions

```sql
\df+ jsonb_smart_patch_scalar
-- Shows function signature, volatility, parallel safety

\dx+ jsonb_ivm
-- Shows all objects in extension
```

### Test in Isolation

```sql
-- Create test database
CREATE DATABASE jsonb_ivm_test;
\c jsonb_ivm_test

CREATE EXTENSION jsonb_ivm;

-- Test function in isolation
SELECT jsonb_smart_patch_scalar('{"a": 1}'::jsonb, '{"b": 2}'::jsonb);
```

### Reproduce with Minimal Example

```sql
-- Isolate problem
BEGIN;

-- Create minimal test case
CREATE TEMP TABLE test_data (data JSONB);
INSERT INTO test_data VALUES ('{"posts": [{"id": 1}]}'::jsonb);

-- Test function
UPDATE test_data
SET data = jsonb_array_update_where(data, 'posts', 'id', '1'::jsonb, '{"title": "New"}'::jsonb);

SELECT * FROM test_data;

ROLLBACK;
```

---

## Getting Help

If you're still stuck:

1. **Search existing issues**: [GitHub Issues](https://github.com/fraiseql/jsonb_ivm/issues)
2. **Check documentation**: See README.md, docs/PG_TVIEW_INTEGRATION_EXAMPLES.md
3. **Open a new issue**: Provide:
   - jsonb_ivm version (`SELECT * FROM pg_extension WHERE extname = 'jsonb_ivm'`)
   - PostgreSQL version (`SELECT version()`)
   - Minimal reproduction case (SQL)
   - Expected vs actual behavior

---

## Common Error Messages

### "module not found"

```sql
ERROR:  could not load library "/usr/lib/postgresql/17/lib/jsonb_ivm.so": cannot open shared object file: No such file or directory
```

**Fix:** Reinstall extension: `sudo cargo pgrx install --release`

---

### "extension has no update path"

```sql
ERROR:  extension "jsonb_ivm" has no update path from version "X" to version "Y"
```

**Fix:** Ensure upgrade SQL file exists, or drop and recreate extension

---

### "function does not exist"

```sql
ERROR:  function jsonb_smart_patch_scalar(jsonb, jsonb) does not exist
```

**Fix:** Check extension version, reinstall if needed

---

### "wrong number of arguments"

```sql
ERROR:  function jsonb_array_insert_where(jsonb, text, jsonb) does not exist
HINT:  No function matches the given name and argument types. You might need to add explicit type casts.
```

**Fix:** Check function signature, ensure all required parameters provided

---

**Still having issues?** Open an issue: [GitHub Issues](https://github.com/fraiseql/jsonb_ivm/issues)
