# Phase Plan: Documentation Cleanup & README Improvement

**Project**: jsonb_ivm
**Phase**: Documentation Cleanup & README Improvement
**Created**: 2025-12-12
**Objective**: Consolidate scattered documentation and create an excellent README that clearly communicates jsonb_ivm's purpose, usage, and value proposition

---

## Context

The jsonb_ivm repository has grown organically with documentation spread across multiple locations:
- Root-level docs (README.md, development.md, contributing.md, changelog.md, TESTING.md)
- `docs/` directory with architecture, troubleshooting, integration examples
- `docs/implementation/` with technical implementation details and benchmarks
- `docs/archive/` with historical POC and phase plans
- Various loose documentation files (documentation-structure.md, achievement-summary.md, phase-4-summary.md, etc.)

**Current Problems**:
1. **README.md is overwhelming** (18,272 bytes) - tries to be everything to everyone
2. **Purpose is buried** - takes reading to understand "surgical JSONB updates for CQRS"
3. **Scattered examples** - integration examples in separate doc, API examples in README
4. **Unclear entry points** - "where do I start?" is not obvious
5. **Loose documentation files** cluttering root and docs/
6. **Documentation about documentation** (documentation-structure.md, documentation-cleanup-summary.md)

**Goal**:
- Create a **punchy, clear README** that hooks readers in 30 seconds
- **Consolidate** documentation into logical, easy-to-find locations
- **Archive or remove** meta-documentation and summary files
- Make it **dead simple** to understand what jsonb_ivm does and why you'd use it

---

## Files to Modify

### Root Level
- **README.md** - Complete rewrite for clarity and impact
- **development.md** - Minor updates to reference new doc structure
- **contributing.md** - Keep as-is (already excellent)
- **changelog.md** - Keep as-is (already excellent)
- **TESTING.md** - Keep as-is (unique, important content)

### docs/ Directory
- **docs/architecture.md** - Keep, minor cleanup
- **docs/troubleshooting.md** - Keep, minor improvements
- **docs/pg-tview-integration-examples.md** - Keep, possibly rename to `integration-guide.md`
- **docs/pg-tview-helpers-proposal.md** - Move to `docs/archive/proposals/`

### Files to Archive
Move to `docs/archive/`:
- `documentation-structure.md` - Meta-documentation, no longer needed
- `documentation-cleanup-summary.md` - Historical summary
- `achievement-summary.md` - Historical summary
- `phase-4-summary.md` - Historical phase summary
- `code-review-prompt.md` - Internal tool, not user-facing

### docs/implementation/
- Keep all files as-is (valuable technical content)
- Add README.md explaining what this directory contains

### docs/contributing/
- **documentation-standards.md** - Keep, ensure it's referenced in contributing.md

---

## Files to Create

1. **docs/README.md** - Directory index explaining documentation organization
2. **docs/implementation/README.md** - Explain technical implementation docs
3. **docs/archive/proposals/README.md** - Explain archived proposals
4. **docs/quick-start.md** - Step-by-step getting started guide (extracted from README)
5. **docs/api-reference.md** - Complete API reference (extracted from README)

---

## Implementation Steps

### Step 1: Create Documentation Structure

**Create directory indexes**:

```bash
mkdir -p docs/archive/proposals
```

**Create `docs/README.md`**:
```markdown
# jsonb_ivm Documentation

Welcome to the jsonb_ivm documentation. This directory contains comprehensive guides for using, integrating, and understanding the extension.

## Quick Links

- **[Quick Start Guide](quick-start.md)** - Get up and running in 5 minutes
- **[API Reference](api-reference.md)** - Complete function reference with examples
- **[Integration Guide](integration-guide.md)** - Real-world CRUD workflow examples
- **[Architecture](architecture.md)** - Technical design and implementation details
- **[Troubleshooting](troubleshooting.md)** - Common issues and solutions

## Developer Documentation

- **[Implementation Details](implementation/)** - Technical implementation docs and benchmarks
- **[Contributing Standards](contributing/documentation-standards.md)** - Documentation guidelines

## Archive

- **[Archived Documents](archive/)** - Historical proposals, POCs, and phase plans
```

**Create `docs/implementation/README.md`**:
```markdown
# Implementation Documentation

This directory contains technical implementation details, benchmarks, and verification documents.

## Contents

- **[implementation-success.md](implementation-success.md)** - Technical implementation verification
- **[benchmark-results.md](benchmark-results.md)** - Performance analysis and comparisons
- **[uuid-and-tree-benchmark-results.md](uuid-and-tree-benchmark-results.md)** - Specialized benchmarks
- **[new-benchmarks-plan.md](new-benchmarks-plan.md)** - Benchmark planning document
- **[pgrx-integration-issue.md](pgrx-integration-issue.md)** - SQL generation troubleshooting

## Audience

These documents are intended for:
- Contributors working on performance optimizations
- Developers debugging implementation issues
- Users seeking detailed performance characteristics
```

**Create `docs/archive/proposals/README.md`**:
```markdown
# Archived Proposals

Historical design proposals and planning documents.

## Contents

- **pg-tview-helpers-proposal.md** - Original proposal for helper functions
```

**Expected Output**: Three new README.md files explaining documentation organization

---

### Step 2: Archive Meta-Documentation

**Move files to archive**:

```bash
# Archive meta-documentation
git mv documentation-structure.md docs/archive/
git mv documentation-cleanup-summary.md docs/archive/
git mv achievement-summary.md docs/archive/
git mv phase-4-summary.md docs/archive/
git mv code-review-prompt.md docs/archive/

# Archive proposal
git mv docs/pg-tview-helpers-proposal.md docs/archive/proposals/
```

**Expected Output**:
```
renamed: documentation-structure.md -> docs/archive/documentation-structure.md
renamed: documentation-cleanup-summary.md -> docs/archive/documentation-cleanup-summary.md
renamed: achievement-summary.md -> docs/archive/achievement-summary.md
renamed: phase-4-summary.md -> docs/archive/phase-4-summary.md
renamed: code-review-prompt.md -> docs/archive/code-review-prompt.md
renamed: docs/pg-tview-helpers-proposal.md -> docs/archive/proposals/pg-tview-helpers-proposal.md
```

---

### Step 3: Rename Integration Examples

**Rename for clarity**:

```bash
git mv docs/pg-tview-integration-examples.md docs/integration-guide.md
```

**Update internal references**:
- Search for references to `pg-tview-integration-examples.md` in all docs
- Update to `integration-guide.md`

**Expected Output**: File renamed, all references updated

---

### Step 4: Extract Quick Start Guide

**Create `docs/quick-start.md`** extracted from README.md:

```markdown
# Quick Start Guide

Get started with jsonb_ivm in 5 minutes.

## Prerequisites

- PostgreSQL 13-17
- Rust 1.70+ (for building from source)

## Installation

### Option 1: From Source

```bash
# Clone repository
git clone https://github.com/yourusername/jsonb_ivm.git
cd jsonb_ivm

# Install with cargo-pgrx
cargo install cargo-pgrx --version 0.12.8
cargo pgrx init

# Build and install
cargo pgrx install --release
```

### Option 2: From Release (coming soon)

Binary releases will be available on GitHub releases page.

## Enable Extension

```sql
CREATE EXTENSION jsonb_ivm;
```

## Your First Query

### Example: Update User's Email in Denormalized View

**Scenario**: You have a `project_views` table with denormalized user data:

```sql
CREATE TABLE project_views (
    id INTEGER PRIMARY KEY,
    data JSONB
);

-- Sample data
INSERT INTO project_views VALUES (
    1,
    '{
        "id": 1,
        "name": "Project Alpha",
        "owner": {
            "id": 42,
            "name": "Alice",
            "email": "alice@old.com"
        }
    }'::jsonb
);
```

**Old way** (re-aggregate entire object):
```sql
UPDATE project_views
SET data = jsonb_build_object(
    'id', data->'id',
    'name', data->'name',
    'owner', jsonb_build_object(
        'id', data->'owner'->'id',
        'name', data->'owner'->'name',
        'email', 'alice@new.com'
    )
)
WHERE id = 1;
```

**New way** (surgical update with jsonb_ivm):
```sql
UPDATE project_views
SET data = jsonb_smart_patch_nested(
    data,
    '{"email": "alice@new.com"}'::jsonb,
    '{owner}'
)
WHERE id = 1;
```

**Result**: 2-3× faster, cleaner code, less memory allocation.

## Common Patterns

### Update Array Element by ID

```sql
-- Update status of task #5 in project's tasks array
UPDATE project_views
SET data = jsonb_smart_patch_array(
    data,
    '{"status": "completed"}'::jsonb,
    '{tasks}',
    'id',
    '5'
)
WHERE id = 1;
```

### Delete Array Element

```sql
-- Remove task #5 from project's tasks array
UPDATE project_views
SET data = jsonb_array_delete_where(
    data,
    '{tasks}',
    'id',
    '5'
)
WHERE id = 1;
```

### Insert Sorted Element

```sql
-- Add new task, sorted by priority
UPDATE project_views
SET data = jsonb_array_insert_where(
    data,
    '{tasks}',
    '{"id": 10, "name": "New Task", "priority": 5}'::jsonb,
    'priority',
    'ASC'
)
WHERE id = 1;
```

## Next Steps

- **[API Reference](api-reference.md)** - Complete function reference
- **[Integration Guide](integration-guide.md)** - Real-world CRUD workflows
- **[Architecture](architecture.md)** - How it works under the hood
- **[Troubleshooting](troubleshooting.md)** - Common issues

## Need Help?

- [GitHub Issues](https://github.com/yourusername/jsonb_ivm/issues)
- [Troubleshooting Guide](troubleshooting.md)
```

**Expected Output**: Standalone quick start guide extracted from README

---

### Step 5: Extract API Reference

**Create `docs/api-reference.md`** with complete function reference:

```markdown
# API Reference

Complete reference for all jsonb_ivm functions.

## Function Index

### Smart Patch Functions (v0.3.0+)
- [jsonb_smart_patch_scalar](#jsonb_smart_patch_scalar) - Intelligent shallow merge
- [jsonb_smart_patch_nested](#jsonb_smart_patch_nested) - Merge at nested path
- [jsonb_smart_patch_array](#jsonb_smart_patch_array) - Update array element by key

### Array CRUD Functions (v0.3.0+)
- [jsonb_array_delete_where](#jsonb_array_delete_where) - Delete array element
- [jsonb_array_insert_where](#jsonb_array_insert_where) - Insert sorted element
- [jsonb_array_contains_id](#jsonb_array_contains_id) - Check if array contains element

### Deep Merge (v0.3.0+)
- [jsonb_deep_merge](#jsonb_deep_merge) - Recursive deep merge

### Utility Functions (v0.3.0+)
- [jsonb_extract_id](#jsonb_extract_id) - Safely extract ID field

### Legacy Functions (v0.1.0-v0.2.0)
- [jsonb_array_update_where](#jsonb_array_update_where) - Single element update
- [jsonb_merge_at_path](#jsonb_merge_at_path) - Merge at path
- [jsonb_merge_shallow](#jsonb_merge_shallow) - Shallow merge

---

## jsonb_smart_patch_scalar

Intelligently merge source JSONB into target at the top level.

### Signature

```sql
jsonb_smart_patch_scalar(target jsonb, source jsonb) RETURNS jsonb
```

### Behavior

- **Scalars/arrays in source**: Replace target fields completely
- **Objects in source**: Recursively merge with target objects
- **NULL handling**: Returns NULL if either input is NULL (STRICT)

### Performance

- **2-3× faster** than `jsonb_build_object()` for top-level updates
- Minimal memory allocation

### Examples

**Update scalar field**:
```sql
SELECT jsonb_smart_patch_scalar(
    '{"id": 1, "name": "Alice", "age": 30}'::jsonb,
    '{"age": 31}'::jsonb
);
-- Result: {"id": 1, "name": "Alice", "age": 31}
```

**Merge nested object**:
```sql
SELECT jsonb_smart_patch_scalar(
    '{"user": {"id": 1, "name": "Alice"}}'::jsonb,
    '{"user": {"email": "alice@example.com"}}'::jsonb
);
-- Result: {"user": {"id": 1, "name": "Alice", "email": "alice@example.com"}}
```

**Replace array** (does not merge arrays):
```sql
SELECT jsonb_smart_patch_scalar(
    '{"tags": ["old"]}'::jsonb,
    '{"tags": ["new"]}'::jsonb
);
-- Result: {"tags": ["new"]}
```

---

## jsonb_smart_patch_nested

Merge source JSONB into target at a specific nested path.

### Signature

```sql
jsonb_smart_patch_nested(target jsonb, source jsonb, path text[]) RETURNS jsonb
```

### Behavior

- Navigates to `path` in target
- Applies smart patch logic at that location
- Returns original if path doesn't exist (graceful)

### Performance

- **2-5× faster** than re-building nested objects with SQL

### Examples

**Update nested user email**:
```sql
SELECT jsonb_smart_patch_nested(
    '{"project": {"owner": {"id": 1, "name": "Alice", "email": "old@example.com"}}}'::jsonb,
    '{"email": "new@example.com"}'::jsonb,
    '{project, owner}'
);
-- Result: {"project": {"owner": {"id": 1, "name": "Alice", "email": "new@example.com"}}}
```

**Path doesn't exist** (graceful):
```sql
SELECT jsonb_smart_patch_nested(
    '{"project": {}}'::jsonb,
    '{"email": "new@example.com"}'::jsonb,
    '{nonexistent, path}'
);
-- Result: {"project": {}} (unchanged)
```

---

## jsonb_smart_patch_array

Update a specific element in a JSONB array by matching a key-value pair.

### Signature

```sql
jsonb_smart_patch_array(
    target jsonb,
    source jsonb,
    array_path text[],
    match_key text,
    match_value text
) RETURNS jsonb
```

### Behavior

- Finds array at `array_path`
- Searches for element where `element[match_key] = match_value`
- Applies smart patch to that element only
- Returns original if no match found (graceful)

### Performance

- **3-7× faster** than SQL re-aggregation for array updates
- Optimized integer ID matching with SIMD-friendly loops

### Examples

**Update task status**:
```sql
SELECT jsonb_smart_patch_array(
    '{"tasks": [
        {"id": 1, "name": "Task 1", "status": "pending"},
        {"id": 2, "name": "Task 2", "status": "pending"}
    ]}'::jsonb,
    '{"status": "completed"}'::jsonb,
    '{tasks}',
    'id',
    '2'
);
-- Result: task #2 status changed to "completed"
```

**Update nested array**:
```sql
SELECT jsonb_smart_patch_array(
    '{"project": {"members": [
        {"user_id": 10, "role": "viewer"},
        {"user_id": 20, "role": "editor"}
    ]}}'::jsonb,
    '{"role": "admin"}'::jsonb,
    '{project, members}',
    'user_id',
    '20'
);
-- Result: user_id 20 role changed to "admin"
```

---

## jsonb_array_delete_where

Delete a specific element from a JSONB array by matching a key-value pair.

### Signature

```sql
jsonb_array_delete_where(
    target jsonb,
    array_path text[],
    match_key text,
    match_value text
) RETURNS jsonb
```

### Behavior

- Finds array at `array_path`
- Removes element where `element[match_key] = match_value`
- Returns original if no match found (graceful)

### Performance

- **5-7× faster** than SQL re-aggregation for deletes
- Single-pass array reconstruction

### Examples

**Delete task by ID**:
```sql
SELECT jsonb_array_delete_where(
    '{"tasks": [
        {"id": 1, "name": "Task 1"},
        {"id": 2, "name": "Task 2"},
        {"id": 3, "name": "Task 3"}
    ]}'::jsonb,
    '{tasks}',
    'id',
    '2'
);
-- Result: task #2 removed, tasks #1 and #3 remain
```

**No match** (graceful):
```sql
SELECT jsonb_array_delete_where(
    '{"tasks": [{"id": 1}]}'::jsonb,
    '{tasks}',
    'id',
    '999'
);
-- Result: {"tasks": [{"id": 1}]} (unchanged)
```

---

## jsonb_array_insert_where

Insert an element into a JSONB array at the correct position based on sorting.

### Signature

```sql
jsonb_array_insert_where(
    target jsonb,
    array_path text[],
    new_element jsonb,
    sort_key text,
    sort_order text  -- 'ASC' or 'DESC'
) RETURNS jsonb
```

### Behavior

- Finds array at `array_path`
- Inserts `new_element` at position maintaining sort order by `sort_key`
- Supports both numeric and text sorting
- Returns original if path invalid (graceful)

### Performance

- **4-6× faster** than SQL re-aggregation for inserts
- Optimized insertion point search

### Examples

**Insert task sorted by priority (ascending)**:
```sql
SELECT jsonb_array_insert_where(
    '{"tasks": [
        {"id": 1, "priority": 1},
        {"id": 3, "priority": 3}
    ]}'::jsonb,
    '{tasks}',
    '{"id": 2, "priority": 2}'::jsonb,
    'priority',
    'ASC'
);
-- Result: task #2 inserted between #1 and #3
```

**Insert sorted by name (descending)**:
```sql
SELECT jsonb_array_insert_where(
    '{"users": [
        {"name": "Charlie"},
        {"name": "Alice"}
    ]}'::jsonb,
    '{users}',
    '{"name": "Bob"}'::jsonb,
    'name',
    'DESC'
);
-- Result: Bob inserted between Charlie and Alice
```

---

## jsonb_array_contains_id

Check if a JSONB array contains an element with a specific key-value pair.

### Signature

```sql
jsonb_array_contains_id(
    data jsonb,
    array_path text[],
    id_key text,
    id_value text
) RETURNS boolean
```

### Behavior

- Finds array at `array_path`
- Returns `true` if any element has `element[id_key] = id_value`
- Returns `false` if no match or path invalid

### Performance

- Optimized linear search with early exit

### Examples

**Check if task exists**:
```sql
SELECT jsonb_array_contains_id(
    '{"tasks": [{"id": 1}, {"id": 2}]}'::jsonb,
    '{tasks}',
    'id',
    '2'
);
-- Result: true
```

**Element doesn't exist**:
```sql
SELECT jsonb_array_contains_id(
    '{"tasks": [{"id": 1}]}'::jsonb,
    '{tasks}',
    'id',
    '999'
);
-- Result: false
```

---

## jsonb_deep_merge

Recursively merge two JSONB objects, deeply merging nested objects.

### Signature

```sql
jsonb_deep_merge(target jsonb, source jsonb) RETURNS jsonb
```

### Behavior

- Recursively merges all nested objects
- Arrays/scalars in source replace target values
- NULL handling: Returns NULL if either input is NULL (STRICT)

### Performance

- **2× faster** than native PostgreSQL `||` operator for deep structures
- Optimized recursive algorithm

### Examples

**Deep merge nested objects**:
```sql
SELECT jsonb_deep_merge(
    '{"a": {"b": {"c": 1, "d": 2}}, "x": 10}'::jsonb,
    '{"a": {"b": {"c": 99}}, "y": 20}'::jsonb
);
-- Result: {"a": {"b": {"c": 99, "d": 2}}, "x": 10, "y": 20}
```

**Arrays are replaced, not merged**:
```sql
SELECT jsonb_deep_merge(
    '{"tags": ["old1", "old2"]}'::jsonb,
    '{"tags": ["new"]}'::jsonb
);
-- Result: {"tags": ["new"]}
```

---

## jsonb_extract_id

Safely extract an ID field from JSONB data with type validation.

### Signature

```sql
jsonb_extract_id(data jsonb, key text DEFAULT 'id') RETURNS text
```

### Behavior

- Extracts `data[key]` as text
- Validates it's a number or string
- Returns NULL if invalid type or missing

### Examples

**Extract integer ID**:
```sql
SELECT jsonb_extract_id('{"id": 42}'::jsonb);
-- Result: '42'
```

**Extract UUID**:
```sql
SELECT jsonb_extract_id('{"uuid": "123e4567-e89b"}'::jsonb, 'uuid');
-- Result: '123e4567-e89b'
```

**Invalid type** (graceful):
```sql
SELECT jsonb_extract_id('{"id": {"nested": "object"}}'::jsonb);
-- Result: NULL
```

---

## Error Handling

All functions follow these principles:

### NULL Handling
- All functions are **STRICT**: return NULL if any input is NULL
- Prevents unexpected behavior in UPDATE statements

### Graceful Failures
- Invalid paths → return original unchanged
- Missing keys → return original unchanged
- Type mismatches → return original unchanged
- No element matches → return original unchanged

### No Exceptions
- Functions never throw errors on invalid input
- Defensive coding ensures stability in production

---

## Performance Characteristics

| Function | Native SQL Time | jsonb_ivm Time | Speedup |
|----------|----------------|----------------|---------|
| `jsonb_smart_patch_scalar` | 1.2 ms | 0.4 ms | **3.0×** |
| `jsonb_smart_patch_array` | 1.91 ms | 0.72 ms | **2.66×** |
| `jsonb_array_delete_where` | 20-30 ms | 4-6 ms | **5-7×** |
| `jsonb_array_insert_where` | 22-35 ms | 5-8 ms | **4-6×** |
| `jsonb_deep_merge` | 8-12 ms | 4-6 ms | **2×** |

**Benchmarks**: See [benchmark-results.md](implementation/benchmark-results.md) for detailed analysis.

---

## Version History

- **v0.3.0** - Smart patch functions, complete CRUD, deep merge (2025-01)
- **v0.2.0** - Batch update functions (2024-12)
- **v0.1.0** - Core array update and merge functions (2024-11)

See [CHANGELOG.md](../changelog.md) for detailed version history.
```

**Expected Output**: Complete API reference extracted from README

---

### Step 6: Rewrite README.md

**New README.md structure** (punchy, clear, hooks in 30 seconds):

```markdown
# jsonb_ivm

**Surgical JSONB updates for PostgreSQL CQRS architectures**

[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-13%2B-blue)](https://www.postgresql.org/)
[![License](https://img.shields.io/badge/license-PostgreSQL-blue)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.3.1-green)](CHANGELOG.md)

High-performance PostgreSQL extension for incremental JSONB view maintenance in CQRS/event sourcing systems. **2-7× faster** than native SQL re-aggregation.

---

## Why jsonb_ivm?

In CQRS architectures, you denormalize data into JSONB projection tables for read performance. When source data changes, you need to update these projections **surgically** without re-aggregating entire objects.

**The Problem**:
```sql
-- Re-aggregate entire object just to change one field 😢
UPDATE project_views
SET data = jsonb_build_object(
    'id', data->'id',
    'name', data->'name',
    'owner', jsonb_build_object(
        'id', data->'owner'->'id',
        'name', data->'owner'->'name',
        'email', 'alice@new.com'  -- only this changed!
    )
)
WHERE id = 1;
```

**The Solution**:
```sql
-- Surgical update with jsonb_ivm 🎯
UPDATE project_views
SET data = jsonb_smart_patch_nested(
    data,
    '{"email": "alice@new.com"}'::jsonb,
    '{owner}'
)
WHERE id = 1;
```

**Result**: **2-3× faster**, cleaner code, less memory allocation.

---

## Features

- ✅ **Complete CRUD** for JSONB arrays (create, read, update, delete)
- ⚡ **2-7× faster** than native SQL re-aggregation
- 🎯 **Surgical updates** - modify only what changed
- 🛡️ **Null-safe** - graceful handling of missing paths/keys
- 🔧 **Production-ready** - extensively tested on PostgreSQL 13-17
- 📦 **Zero dependencies** - pure Rust with pgrx

---

## Quick Start

### Installation

```bash
# Build from source
git clone https://github.com/yourusername/jsonb_ivm.git
cd jsonb_ivm
cargo install cargo-pgrx --version 0.12.8
cargo pgrx install --release
```

### Enable Extension

```sql
CREATE EXTENSION jsonb_ivm;
```

### Your First Query

```sql
-- Update array element by ID
UPDATE project_views
SET data = jsonb_smart_patch_array(
    data,
    '{"status": "completed"}'::jsonb,
    '{tasks}',    -- array path
    'id',         -- match key
    '5'           -- match value
)
WHERE id = 1;
```

**See**: [Quick Start Guide](docs/quick-start.md) for full walkthrough

---

## Performance

| Operation | Native SQL | jsonb_ivm | Speedup |
|-----------|-----------|-----------|---------|
| Array element update | 1.91 ms | 0.72 ms | **2.66×** |
| Array DELETE | 20-30 ms | 4-6 ms | **5-7×** |
| Array INSERT (sorted) | 22-35 ms | 5-8 ms | **4-6×** |
| Deep merge | 8-12 ms | 4-6 ms | **2×** |

**See**: [Benchmark Results](docs/implementation/benchmark-results.md) for detailed analysis

---

## API Overview

### Smart Patch Functions
- `jsonb_smart_patch_scalar(target, source)` - Intelligent shallow merge
- `jsonb_smart_patch_nested(target, source, path)` - Merge at nested path
- `jsonb_smart_patch_array(target, source, array_path, match_key, match_value)` - Update array element

### Array CRUD
- `jsonb_array_insert_where(target, array_path, element, sort_key, order)` - Sorted insertion
- `jsonb_array_delete_where(target, array_path, match_key, match_value)` - Delete element
- `jsonb_array_contains_id(data, array_path, key, value)` - Check existence

### Deep Merge
- `jsonb_deep_merge(target, source)` - Recursive deep merge

**See**: [API Reference](docs/api-reference.md) for complete function documentation

---

## Documentation

- **[Quick Start Guide](docs/quick-start.md)** - Get up and running in 5 minutes
- **[API Reference](docs/api-reference.md)** - Complete function reference
- **[Integration Guide](docs/integration-guide.md)** - Real-world CRUD workflows
- **[Architecture](docs/architecture.md)** - Technical design and implementation
- **[Troubleshooting](docs/troubleshooting.md)** - Common issues and solutions

---

## Use Cases

### CQRS Projection Maintenance
Update denormalized views when source entities change without re-aggregating.

### Event Sourcing
Incrementally update materialized views from event streams.

### pg_tview Integration
Optimize materialized view maintenance with surgical JSONB updates.

**See**: [Integration Guide](docs/integration-guide.md) for detailed examples

---

## Requirements

- PostgreSQL 13-17
- Rust 1.70+ (for building from source)
- pgrx 0.12.8

---

## Development

```bash
# Run tests
just test

# Run benchmarks
just benchmark

# Format code
cargo fmt

# Lint
cargo clippy --all-targets --all-features -- -D warnings
```

**See**: [development.md](development.md) for detailed development guide

---

## Contributing

Contributions welcome! Please see:
- **[Contributing Guide](contributing.md)** - Development workflow, code standards
- **[Testing Guide](TESTING.md)** - How to run tests (why `cargo test` doesn't work)

---

## License

This project is licensed under the **PostgreSQL License** - see [LICENSE](LICENSE) for details.

---

## Changelog

See [CHANGELOG.md](changelog.md) for version history.

**Latest**: v0.3.1 (2025-12-09) - Performance improvements and code quality

---

## Author

**Lionel Hamayon**

- GitHub: [@yourusername](https://github.com/yourusername)

---

## Acknowledgments

- Built with [pgrx](https://github.com/pgcentralfoundation/pgrx)
- Inspired by real-world CQRS challenges in production systems
```

**Expected Output**: Clear, punchy README that hooks readers in 30 seconds

---

### Step 7: Update Cross-References

**Files to check and update**:

1. **development.md**:
   - Update references to README sections → link to new docs/
   - Add link to docs/README.md for full documentation index

2. **contributing.md**:
   - Update references to documentation → link to docs/contributing/documentation-standards.md
   - Add note about documentation organization

3. **docs/troubleshooting.md**:
   - Update any references to README → link to docs/api-reference.md or docs/quick-start.md

4. **docs/architecture.md**:
   - Update any README references → link to specific docs

**Search and replace**:
```bash
# Find all markdown files with references to moved content
grep -r "pg-tview-integration-examples" docs/ *.md

# Find references to README sections that are now in separate docs
grep -r "README.md#" docs/ *.md
```

**Expected Output**: All cross-references updated to new locations

---

### Step 8: Update GitHub Links

**Update README.md with actual GitHub repository URLs**:

Current placeholders:
- `https://github.com/yourusername/jsonb_ivm`
- `[@yourusername](https://github.com/yourusername)`

**Replace with actual repository URL** (need to get from user or infer from git remote)

```bash
# Check git remote
git remote -v
```

**Expected Output**: All GitHub links point to actual repository

---

## Verification Commands

After each step, verify the changes:

**Step 1-3: Check file structure**:
```bash
tree docs/ -L 2
ls -la docs/archive/
ls -la docs/archive/proposals/
```

**Expected**:
```
docs/
├── README.md
├── quick-start.md
├── api-reference.md
├── integration-guide.md
├── architecture.md
├── troubleshooting.md
├── contributing/
│   └── documentation-standards.md
├── implementation/
│   ├── README.md
│   ├── benchmark-results.md
│   ├── implementation-success.md
│   └── ...
└── archive/
    ├── README.md (if created)
    ├── documentation-structure.md
    ├── achievement-summary.md
    └── proposals/
        ├── README.md
        └── pg-tview-helpers-proposal.md
```

**Step 4-6: Check content**:
```bash
# Check quick-start.md has content
wc -l docs/quick-start.md

# Check api-reference.md has all functions
grep -c "^## jsonb_" docs/api-reference.md  # Should be ~8

# Check README.md is concise
wc -c README.md  # Should be < 5000 bytes (vs current 18,272)
```

**Step 7: Verify links**:
```bash
# Check for broken internal links (manual review)
grep -r "\[.*\](.*.md" docs/ README.md contributing.md development.md

# Check for old file references
grep -r "pg-tview-integration-examples" docs/ *.md
grep -r "documentation-structure.md" docs/ *.md
```

**Step 8: Verify GitHub links**:
```bash
grep -n "yourusername" README.md  # Should be 0 results if replaced
```

---

## Acceptance Criteria

- [ ] README.md is **< 5,000 bytes** (down from 18,272)
- [ ] README.md hooks readers in **first 30 seconds** (clear value prop)
- [ ] docs/ directory has clear **README.md index**
- [ ] All meta-documentation **archived** (not deleted)
- [ ] Quick start guide is **standalone** and easy to follow
- [ ] API reference has **all 13 functions** documented
- [ ] Integration guide is **renamed** from pg-tview-* for clarity
- [ ] All **cross-references updated** to new locations
- [ ] GitHub links point to **actual repository** (not placeholders)
- [ ] No **broken links** in any documentation
- [ ] `tree docs/` shows **logical organization**

---

## DO NOT

- ❌ Delete any documentation (archive instead)
- ❌ Change API function names or signatures
- ❌ Modify TESTING.md, contributing.md, changelog.md (already excellent)
- ❌ Remove technical details from architecture.md or implementation/
- ❌ Create new proposal documents (archive existing, don't create more)
- ❌ Add speculative "roadmap" or "future work" sections
- ❌ Include placeholder "TODO" sections in new docs

---

## Success Metrics

**Before**:
- README: 18,272 bytes, 500+ lines
- Documentation scattered across root, docs/, and loose files
- Unclear entry point for new users
- Purpose buried in technical details

**After**:
- README: < 5,000 bytes, ~150 lines
- Clear docs/ structure with index
- Quick start in < 5 minutes
- Value proposition in first 30 seconds
- All content preserved (archived, not deleted)

---

## Notes

- This is a **single-phase plan** - no TDD, no RED/GREEN/REFACTOR
- Focus is on **organization and clarity**, not new content
- All existing content is **preserved** (moved, not deleted)
- README.md is **completely rewritten** for impact
- Documentation becomes **discoverable** and **logically organized**
