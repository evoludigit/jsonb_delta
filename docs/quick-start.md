# Quick Start Guide

Get started with jsonb_ivm in 5 minutes.

## Prerequisites

- PostgreSQL 13-17
- Rust 1.70+ (for building from source)

## Installation

### Option 1: From Source

```bash
# Install Rust if not already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install cargo-pgrx
cargo install --locked cargo-pgrx

# Initialize pgrx (one-time setup)
cargo pgrx init

# Clone and build
git clone https://github.com/fraiseql/jsonb_ivm.git
cd jsonb_ivm
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
    ARRAY['owner']
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
    'tasks',
    'id',
    '5'::jsonb
)
WHERE id = 1;
```

### Delete Array Element

```sql
-- Remove task #5 from project's tasks array
UPDATE project_views
SET data = jsonb_array_delete_where(
    data,
    'tasks',
    'id',
    '5'::jsonb
)
WHERE id = 1;
```

### Insert Sorted Element

```sql
-- Add new task, sorted by priority
UPDATE project_views
SET data = jsonb_array_insert_where(
    data,
    'tasks',
    '{"id": 10, "name": "New Task", "priority": 5}'::jsonb,
    'priority',
    'ASC'
)
WHERE id = 1;
```

### Deep Merge Nested Objects

```sql
-- Merge nested configuration without losing existing fields
UPDATE project_views
SET data = jsonb_deep_merge(
    data,
    '{"config": {"theme": "dark"}}'::jsonb
)
WHERE id = 1;
-- Preserves all existing config fields, just adds/updates "theme"
```

## Real-World Example: CQRS Projection Update

In a CQRS architecture, when a source entity changes, you need to update denormalized projections:

```sql
-- Scenario: User #42 changed their name
-- Need to update all projects where this user appears

-- With jsonb_ivm (surgical update):
UPDATE project_views
SET data = jsonb_smart_patch_nested(
    data,
    '{"name": "Alice Smith"}'::jsonb,
    ARRAY['owner']
)
WHERE data->'owner'->>'id' = '42';

-- Or update user in a members array:
UPDATE project_views
SET data = jsonb_smart_patch_array(
    data,
    '{"name": "Alice Smith"}'::jsonb,
    'members',
    'user_id',
    '42'::jsonb
)
WHERE jsonb_array_contains_id(data, 'members', 'user_id', '42'::jsonb);
```

## Next Steps

- **[API Reference](api-reference.md)** - Complete function reference
- **[Integration Guide](integration-guide.md)** - Real-world CRUD workflows
- **[Architecture](architecture.md)** - How it works under the hood
- **[Troubleshooting](troubleshooting.md)** - Common issues

## Need Help?

- [GitHub Issues](https://github.com/fraiseql/jsonb_ivm/issues)
- [Troubleshooting Guide](troubleshooting.md)
