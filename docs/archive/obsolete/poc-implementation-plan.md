# JSONB IVM POC: Detailed Implementation Plan

**Goal**: Validate that surgical JSONB updates via Rust can achieve 5-10x performance improvement over native PostgreSQL operations for nested document updates.

**Timeline**: 3-4 days for complete POC with benchmarks
**Decision Point**: End of Day 3 - proceed with full implementation or pivot to alternatives

---

## Executive Summary

### What We're Building

Four high-value JSONB operations for incremental materialized view maintenance:

1. **`jsonb_array_update_where`** (CRITICAL) - Update single array element by predicate
2. **`jsonb_merge_at_path`** (HIGH) - Merge JSONB at nested path without full rebuild
3. **`jsonb_has_path_changed`** (MEDIUM) - Detect if specific path changed between old/new
4. **`jsonb_array_upsert_where`** (MEDIUM) - Atomic insert-or-update in array

### Success Criteria

**Performance targets** (vs native SQL equivalents):
- Small documents (<1KB): Break-even (0.8x-1.2x)
- Medium documents (1-10KB): >2x faster
- Large documents (>10KB): >5x faster
- Large arrays (>50 elements): >3x faster

**Quality gates**:
- ✅ Zero crashes in 10,000 operation stress test
- ✅ Memory usage <1.5x native approach
- ✅ All tests pass on PostgreSQL 13-17
- ✅ Type-safe error handling

### Decision Criteria

**PROCEED with full implementation if**:
- ✅ >2x improvement on realistic CQRS cascade scenario
- ✅ >5x improvement on 100-element array updates
- ✅ No memory leaks or correctness issues

**PIVOT to alternative approach if**:
- ❌ <1.5x improvement overall
- ❌ Any operation slower than native on realistic workloads
- ❌ Memory usage >2x native
- ❌ Implementation complexity exceeds maintenance budget

---

## Phase 1: Foundation & Baseline (Day 1 - 8 hours)

### 1.1: Benchmark Infrastructure Setup (2 hours)

**Objective**: Create repeatable, realistic benchmark framework

#### 1.1.1: Synthetic Dataset Generator

**File**: `test/fixtures/generate_cqrs_data.sql`

```sql
-- Generates realistic nested JSONB documents mimicking CQRS architecture
-- Scale: 100 network configurations × 50 DNS servers each

CREATE OR REPLACE FUNCTION generate_cqrs_test_data()
RETURNS void AS $$
BEGIN
    -- Drop existing test tables
    DROP TABLE IF EXISTS bench_dns_servers CASCADE;
    DROP TABLE IF EXISTS bench_network_configs CASCADE;
    DROP TABLE IF EXISTS bench_allocations CASCADE;
    DROP TABLE IF EXISTS bench_nc_dns_mapping CASCADE;

    -- Base table: DNS servers (500 records)
    CREATE TABLE bench_dns_servers (
        id INTEGER PRIMARY KEY,
        ip TEXT NOT NULL,
        port INTEGER NOT NULL,
        status TEXT NOT NULL
    );

    INSERT INTO bench_dns_servers
    SELECT
        i,
        '8.8.' || (i % 256) || '.' || ((i / 256) % 256),
        53 + (i % 10),
        CASE WHEN i % 5 = 0 THEN 'inactive' ELSE 'active' END
    FROM generate_series(1, 500) i;

    -- Materialized view: v_dns_server (leaf view)
    CREATE MATERIALIZED VIEW v_dns_server AS
    SELECT
        id,
        jsonb_build_object(
            'id', id,
            'ip', ip,
            'port', port,
            'status', status
        ) AS data
    FROM bench_dns_servers;

    CREATE INDEX idx_v_dns_server_id ON v_dns_server(id);

    -- Base table: Network configurations (100 records)
    CREATE TABLE bench_network_configs (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        gateway_ip TEXT NOT NULL,
        subnet_mask TEXT NOT NULL,
        created_at TIMESTAMPTZ NOT NULL DEFAULT now()
    );

    INSERT INTO bench_network_configs
    SELECT
        i,
        'Network Config ' || i,
        '192.168.' || (i % 256) || '.1',
        '255.255.255.0',
        now() - (i || ' days')::interval
    FROM generate_series(1, 100) i;

    -- Mapping table: Network Config ↔ DNS Servers (50 DNS servers per config)
    CREATE TABLE bench_nc_dns_mapping (
        network_configuration_id INTEGER REFERENCES bench_network_configs(id),
        dns_server_id INTEGER REFERENCES bench_dns_servers(id),
        priority INTEGER NOT NULL,
        PRIMARY KEY (network_configuration_id, dns_server_id)
    );

    INSERT INTO bench_nc_dns_mapping
    SELECT
        (i / 50) + 1 AS network_configuration_id,
        i AS dns_server_id,
        (i % 50) + 1 AS priority
    FROM generate_series(1, 5000) i;

    -- Materialized view: tv_network_configuration (intermediate composition)
    CREATE MATERIALIZED VIEW tv_network_configuration AS
    SELECT
        nc.id,
        jsonb_build_object(
            'id', nc.id,
            'name', nc.name,
            'gateway_ip', nc.gateway_ip,
            'subnet_mask', nc.subnet_mask,
            'dns_servers', COALESCE((
                SELECT jsonb_agg(v.data ORDER BY m.priority)
                FROM bench_nc_dns_mapping m
                JOIN v_dns_server v ON v.id = m.dns_server_id
                WHERE m.network_configuration_id = nc.id
            ), '[]'::jsonb),
            'created_at', nc.created_at
        ) AS data
    FROM bench_network_configs nc;

    CREATE INDEX idx_tv_network_configuration_id ON tv_network_configuration(id);

    -- Base table: Allocations (500 records, 5 per network config)
    CREATE TABLE bench_allocations (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        network_configuration_id INTEGER REFERENCES bench_network_configs(id),
        machine_name TEXT NOT NULL,
        storage_size_gb INTEGER NOT NULL
    );

    INSERT INTO bench_allocations
    SELECT
        i,
        'Allocation ' || i,
        ((i - 1) / 5) + 1 AS network_configuration_id,
        'Machine-' || i,
        (i % 10 + 1) * 100
    FROM generate_series(1, 500) i;

    -- Materialized view: tv_allocation (top-level composition)
    CREATE MATERIALIZED VIEW tv_allocation AS
    SELECT
        a.id,
        jsonb_build_object(
            'id', a.id,
            'name', a.name,
            'network_configuration', nc.data,  -- ← Nested 10KB+ document
            'machine', jsonb_build_object(
                'name', a.machine_name,
                'status', 'active'
            ),
            'storage', jsonb_build_object(
                'size_gb', a.storage_size_gb,
                'type', 'SSD'
            )
        ) AS data
    FROM bench_allocations a
    LEFT JOIN tv_network_configuration nc ON nc.id = a.network_configuration_id;

    CREATE INDEX idx_tv_allocation_id ON tv_allocation(id);
    CREATE INDEX idx_tv_allocation_nc_id ON tv_allocation((data->'network_configuration'->>'id'));

    RAISE NOTICE 'Test data generated successfully:';
    RAISE NOTICE '  - 500 DNS servers';
    RAISE NOTICE '  - 100 network configurations (50 DNS servers each)';
    RAISE NOTICE '  - 500 allocations (5 per network config)';
    RAISE NOTICE '  - Total JSONB size: ~50MB';
END;
$$ LANGUAGE plpgsql;

-- Execute generator
SELECT generate_cqrs_test_data();

-- Verify data sizes
SELECT
    'v_dns_server' AS view,
    COUNT(*) AS rows,
    pg_size_pretty(pg_total_relation_size('v_dns_server')) AS size
FROM v_dns_server
UNION ALL
SELECT
    'tv_network_configuration',
    COUNT(*),
    pg_size_pretty(pg_total_relation_size('tv_network_configuration'))
FROM tv_network_configuration
UNION ALL
SELECT
    'tv_allocation',
    COUNT(*),
    pg_size_pretty(pg_total_relation_size('tv_allocation'))
FROM tv_allocation;
```

**Verification**:
```bash
psql -d jsonb_ivm_test -f test/fixtures/generate_cqrs_data.sql
# Expected output:
#   - 500 DNS servers
#   - 100 network configs
#   - 500 allocations
```

#### 1.1.2: Baseline Performance Scripts

**File**: `test/benchmark_baseline.sql`

```sql
-- Baseline: Native PostgreSQL approach (what we're trying to beat)
-- Scenario: Update single DNS server IP address, measure cascade time

\timing on
\set ON_ERROR_STOP on

\echo '========================================'
\echo 'BASELINE: Native PostgreSQL Cascade'
\echo '========================================'
\echo ''

-- Scenario: Update DNS server #42, propagate to all dependent views
\echo 'Scenario: UPDATE bench_dns_servers SET ip = ''9.9.9.9'' WHERE id = 42'
\echo ''

-- Step 1: Update leaf view (v_dns_server)
\echo '--- Step 1: Update v_dns_server ---'
BEGIN;

\echo 'Baseline approach: Full JSONB rebuild'
EXPLAIN ANALYZE
UPDATE v_dns_server
SET data = (
    SELECT jsonb_build_object(
        'id', id,
        'ip', ip,
        'port', port,
        'status', status
    )
    FROM bench_dns_servers
    WHERE bench_dns_servers.id = v_dns_server.id
)
WHERE id = 42;

ROLLBACK;

-- Step 2: Propagate to tv_network_configuration
\echo ''
\echo '--- Step 2: Propagate to tv_network_configuration ---'
BEGIN;

-- First, update the leaf
UPDATE v_dns_server
SET data = jsonb_set(data, '{ip}', '"9.9.9.9"')
WHERE id = 42;

-- Then propagate (WORST CASE: full rebuild)
\echo 'Baseline approach: Re-aggregate entire dns_servers array (50 elements)'
EXPLAIN ANALYZE
UPDATE tv_network_configuration
SET data = (
    SELECT jsonb_build_object(
        'id', nc.id,
        'name', nc.name,
        'gateway_ip', nc.gateway_ip,
        'subnet_mask', nc.subnet_mask,
        'dns_servers', (
            SELECT jsonb_agg(v.data ORDER BY m.priority)
            FROM bench_nc_dns_mapping m
            JOIN v_dns_server v ON v.id = m.dns_server_id
            WHERE m.network_configuration_id = nc.id
        ),
        'created_at', nc.created_at
    )
    FROM bench_network_configs nc
    WHERE nc.id = tv_network_configuration.id
)
WHERE id IN (
    SELECT network_configuration_id
    FROM bench_nc_dns_mapping
    WHERE dns_server_id = 42
);

ROLLBACK;

-- Step 3: Propagate to tv_allocation
\echo ''
\echo '--- Step 3: Propagate to tv_allocation ---'
BEGIN;

-- Update leaf and intermediate
UPDATE v_dns_server SET data = jsonb_set(data, '{ip}', '"9.9.9.9"') WHERE id = 42;

UPDATE tv_network_configuration
SET data = jsonb_set(
    data,
    '{dns_servers}',
    (
        SELECT jsonb_agg(v.data ORDER BY m.priority)
        FROM bench_nc_dns_mapping m
        JOIN v_dns_server v ON v.id = m.dns_server_id
        WHERE m.network_configuration_id = tv_network_configuration.id
    )
)
WHERE id IN (
    SELECT network_configuration_id
    FROM bench_nc_dns_mapping
    WHERE dns_server_id = 42
);

-- Propagate to allocation (WORST CASE: full rebuild)
\echo 'Baseline approach: Full rebuild of 50KB allocation document'
EXPLAIN ANALYZE
UPDATE tv_allocation
SET data = (
    SELECT jsonb_build_object(
        'id', a.id,
        'name', a.name,
        'network_configuration', nc.data,
        'machine', jsonb_build_object(
            'name', a.machine_name,
            'status', 'active'
        ),
        'storage', jsonb_build_object(
            'size_gb', a.storage_size_gb,
            'type', 'SSD'
        )
    )
    FROM bench_allocations a
    LEFT JOIN tv_network_configuration nc ON nc.id = a.network_configuration_id
    WHERE a.id = tv_allocation.id
)
WHERE (data->'network_configuration'->>'id')::int IN (
    SELECT network_configuration_id
    FROM bench_nc_dns_mapping
    WHERE dns_server_id = 42
);

ROLLBACK;

-- Step 4: END-TO-END timing (full cascade)
\echo ''
\echo '--- Step 4: END-TO-END cascade timing ---'
\echo 'Full cascade: Leaf → Intermediate → Top'
\echo ''

BEGIN;

-- Measure total time for complete propagation
\timing on
UPDATE v_dns_server SET data = jsonb_set(data, '{ip}', '"9.9.9.9"') WHERE id = 42;

UPDATE tv_network_configuration
SET data = jsonb_set(
    data,
    '{dns_servers}',
    (
        SELECT jsonb_agg(v.data ORDER BY m.priority)
        FROM bench_nc_dns_mapping m
        JOIN v_dns_server v ON v.id = m.dns_server_id
        WHERE m.network_configuration_id = tv_network_configuration.id
    )
)
WHERE id IN (SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42);

UPDATE tv_allocation
SET data = jsonb_set(
    data,
    '{network_configuration}',
    (SELECT data FROM tv_network_configuration WHERE id = (data->'network_configuration'->>'id')::int)
)
WHERE (data->'network_configuration'->>'id')::int IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

COMMIT;

\echo ''
\echo '========================================'
\echo 'Baseline Complete'
\echo '========================================'
\echo ''
\echo 'Key metrics:'
\echo '  - Leaf update time: <measure from Step 1>'
\echo '  - Intermediate propagation: <measure from Step 2>'
\echo '  - Top-level propagation: <measure from Step 3>'
\echo '  - Total end-to-end time: <measure from Step 4>'
\echo ''
\echo 'This is the baseline to beat with custom Rust functions.'
```

**Expected baseline times** (rough estimates):
- Leaf update: 1-5ms
- Intermediate propagation (re-aggregate 50-element array): 50-100ms
- Top-level propagation (full rebuild): 100-200ms
- **Total: 150-300ms for single DNS server update**

---

### 1.2: Operation 1 - `jsonb_array_update_where` (4 hours)

**Objective**: Implement highest-value operation with serde_json approach

#### 1.2.1: Core Implementation

**File**: `src/lib.rs` (add to existing file)

```rust
/// Update a single element in a JSONB array by matching a key-value predicate
///
/// # Arguments
/// * `target` - JSONB document containing the array
/// * `array_path` - Path to the array within the document (e.g., ["dns_servers"])
/// * `match_key` - Key to match on (e.g., "id")
/// * `match_value` - Value to match (e.g., 42)
/// * `updates` - JSONB object to merge into matched element
///
/// # Returns
/// Updated JSONB document with modified array element
///
/// # Examples
/// ```sql
/// -- Update DNS server #42 in array of 50 servers
/// SELECT jsonb_array_update_where(
///     '{"dns_servers": [{"id": 42, "ip": "1.1.1.1"}, {"id": 43, "ip": "2.2.2.2"}]}'::jsonb,
///     ARRAY['dns_servers'],
///     'id',
///     '42'::jsonb,
///     '{"ip": "8.8.8.8"}'::jsonb
/// );
/// -- Returns: {"dns_servers": [{"id": 42, "ip": "8.8.8.8"}, {"id": 43, "ip": "2.2.2.2"}]}
/// ```
///
/// # Notes
/// - Updates FIRST matching element only
/// - If no match found, returns document unchanged
/// - Performs shallow merge on matched element
/// - O(n) complexity where n = array length
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where(
    target: Option<JsonB>,
    array_path: Vec<&str>,
    match_key: &str,
    match_value: JsonB,
    updates: JsonB,
) -> Option<JsonB> {
    let target = target?;

    // Deserialize target to serde_json::Value
    let mut target_value: Value = target.0;

    // Navigate to array location
    let array = navigate_to_path_mut(&mut target_value, &array_path)?;

    // Validate it's an array
    let array_items = match array.as_array_mut() {
        Some(arr) => arr,
        None => {
            error!(
                "Path {:?} does not point to an array, found: {}",
                array_path,
                value_type_name(array)
            );
        }
    };

    // Extract match value as serde_json::Value
    let match_val = match_value.0;

    // Validate updates is an object
    let updates_obj = match updates.0.as_object() {
        Some(obj) => obj,
        None => {
            error!(
                "updates argument must be a JSONB object, got: {}",
                value_type_name(&updates.0)
            );
        }
    };

    // Find and update first matching element
    for element in array_items.iter_mut() {
        if let Some(elem_obj) = element.as_object_mut() {
            // Check if this element matches
            if let Some(elem_value) = elem_obj.get(match_key) {
                if elem_value == &match_val {
                    // Match found! Merge updates
                    for (key, value) in updates_obj.iter() {
                        elem_obj.insert(key.clone(), value.clone());
                    }
                    // Stop after first match
                    break;
                }
            }
        }
    }

    Some(JsonB(target_value))
}

/// Navigate to a path within a serde_json::Value, returning mutable reference
fn navigate_to_path_mut<'a>(
    value: &'a mut Value,
    path: &[&str],
) -> Option<&'a mut Value> {
    let mut current = value;

    for key in path {
        current = match current.as_object_mut() {
            Some(obj) => obj.get_mut(*key)?,
            None => return None,
        };
    }

    Some(current)
}
```

#### 1.2.2: Unit Tests

**File**: `src/lib.rs` (add to `mod tests` section)

```rust
#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    // ... existing tests ...

    #[pgrx::pg_test]
    fn test_array_update_where_basic() {
        let target = JsonB(json!({
            "dns_servers": [
                {"id": 42, "ip": "1.1.1.1", "port": 53},
                {"id": 43, "ip": "2.2.2.2", "port": 53}
            ]
        }));

        let result = crate::jsonb_array_update_where(
            Some(target),
            vec!["dns_servers"],
            "id",
            JsonB(json!(42)),
            JsonB(json!({"ip": "8.8.8.8"})),
        ).expect("update should succeed");

        let expected = json!({
            "dns_servers": [
                {"id": 42, "ip": "8.8.8.8", "port": 53},
                {"id": 43, "ip": "2.2.2.2", "port": 53}
            ]
        });

        assert_eq!(result.0, expected);
    }

    #[pgrx::pg_test]
    fn test_array_update_where_no_match() {
        let target = JsonB(json!({
            "dns_servers": [
                {"id": 42, "ip": "1.1.1.1"},
                {"id": 43, "ip": "2.2.2.2"}
            ]
        }));

        let result = crate::jsonb_array_update_where(
            Some(target.clone()),
            vec!["dns_servers"],
            "id",
            JsonB(json!(999)),  // No element with id=999
            JsonB(json!({"ip": "8.8.8.8"})),
        ).expect("update should succeed");

        // Should return unchanged
        assert_eq!(result.0, target.0);
    }

    #[pgrx::pg_test]
    fn test_array_update_where_large_array() {
        // Create array with 100 elements
        let mut servers = Vec::new();
        for i in 1..=100 {
            servers.push(json!({
                "id": i,
                "ip": format!("192.168.1.{}", i),
                "port": 53
            }));
        }

        let target = JsonB(json!({"dns_servers": servers}));

        // Update element #99 (near end of array)
        let result = crate::jsonb_array_update_where(
            Some(target),
            vec!["dns_servers"],
            "id",
            JsonB(json!(99)),
            JsonB(json!({"ip": "8.8.8.8", "status": "updated"})),
        ).expect("update should succeed");

        // Verify element #99 was updated
        let updated_server = &result.0["dns_servers"][98];  // 0-indexed
        assert_eq!(updated_server["ip"], "8.8.8.8");
        assert_eq!(updated_server["status"], "updated");
        assert_eq!(updated_server["port"], 53);  // Unchanged field preserved
    }

    #[pgrx::pg_test]
    fn test_array_update_where_nested_path() {
        let target = JsonB(json!({
            "network": {
                "config": {
                    "dns_servers": [
                        {"id": 1, "ip": "1.1.1.1"},
                        {"id": 2, "ip": "2.2.2.2"}
                    ]
                }
            }
        }));

        let result = crate::jsonb_array_update_where(
            Some(target),
            vec!["network", "config", "dns_servers"],
            "id",
            JsonB(json!(2)),
            JsonB(json!({"ip": "8.8.8.8"})),
        ).expect("update should succeed");

        assert_eq!(
            result.0["network"]["config"]["dns_servers"][1]["ip"],
            "8.8.8.8"
        );
    }

    #[pgrx::pg_test]
    #[should_panic(expected = "does not point to an array")]
    fn test_array_update_where_invalid_path() {
        let target = JsonB(json!({"dns_servers": {"id": 42}}));  // Object, not array

        let _ = crate::jsonb_array_update_where(
            Some(target),
            vec!["dns_servers"],
            "id",
            JsonB(json!(42)),
            JsonB(json!({"ip": "8.8.8.8"})),
        );
    }

    #[pgrx::pg_test]
    #[should_panic(expected = "updates argument must be a JSONB object")]
    fn test_array_update_where_invalid_updates() {
        let target = JsonB(json!({"dns_servers": [{"id": 42}]}));

        let _ = crate::jsonb_array_update_where(
            Some(target),
            vec!["dns_servers"],
            "id",
            JsonB(json!(42)),
            JsonB(json!("not an object")),  // Invalid: scalar instead of object
        );
    }
}
```

#### 1.2.3: Integration Test SQL

**File**: `test/sql/02_array_update_where.sql`

```sql
-- Test Suite: jsonb_array_update_where()
-- Expected: All tests pass

CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

-- Test 1: Basic array update
SELECT jsonb_array_update_where(
    '{"dns_servers": [{"id": 1, "ip": "1.1.1.1"}, {"id": 2, "ip": "2.2.2.2"}]}'::jsonb,
    ARRAY['dns_servers'],
    'id',
    '1'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
) = '{"dns_servers": [{"id": 1, "ip": "8.8.8.8"}, {"id": 2, "ip": "2.2.2.2"}]}'::jsonb
AS test_basic_update;

-- Test 2: No match (returns unchanged)
SELECT jsonb_array_update_where(
    '{"dns_servers": [{"id": 1, "ip": "1.1.1.1"}]}'::jsonb,
    ARRAY['dns_servers'],
    'id',
    '999'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
) = '{"dns_servers": [{"id": 1, "ip": "1.1.1.1"}]}'::jsonb
AS test_no_match;

-- Test 3: Nested path
SELECT jsonb_array_update_where(
    '{"network": {"dns_servers": [{"id": 1, "ip": "1.1.1.1"}]}}'::jsonb,
    ARRAY['network', 'dns_servers'],
    'id',
    '1'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
)->'network'->'dns_servers'->0->>'ip' = '8.8.8.8'
AS test_nested_path;

-- Test 4: Large array (100 elements)
WITH large_array AS (
    SELECT jsonb_build_object(
        'dns_servers',
        jsonb_agg(jsonb_build_object('id', i, 'ip', '192.168.1.' || i))
    ) AS data
    FROM generate_series(1, 100) i
)
SELECT (
    jsonb_array_update_where(
        data,
        ARRAY['dns_servers'],
        'id',
        '99'::jsonb,
        '{"ip": "8.8.8.8", "status": "updated"}'::jsonb
    )->'dns_servers'->98->>'ip'
) = '8.8.8.8'
AS test_large_array
FROM large_array;

-- Test 5: Preserve existing fields
SELECT jsonb_array_update_where(
    '{"dns_servers": [{"id": 1, "ip": "1.1.1.1", "port": 53, "status": "active"}]}'::jsonb,
    ARRAY['dns_servers'],
    'id',
    '1'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
)->'dns_servers'->0 = '{"id": 1, "ip": "8.8.8.8", "port": 53, "status": "active"}'::jsonb
AS test_preserve_fields;

-- Test 6: NULL handling
SELECT jsonb_array_update_where(
    NULL::jsonb,
    ARRAY['dns_servers'],
    'id',
    '1'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
) IS NULL
AS test_null_handling;

\echo 'All tests should return TRUE'
```

---

### 1.3: Build and Test (1 hour)

```bash
# Build extension
cd /home/lionel/code/jsonb_ivm
cargo pgrx install --release --pg-config=/usr/bin/pg_config

# Run Rust unit tests
cargo pgrx test

# Run SQL integration tests
psql -d jsonb_ivm_test -c "DROP EXTENSION IF EXISTS jsonb_ivm CASCADE; CREATE EXTENSION jsonb_ivm;"
psql -d jsonb_ivm_test -f test/sql/02_array_update_where.sql

# Generate test data
psql -d jsonb_ivm_test -f test/fixtures/generate_cqrs_data.sql

# Run baseline benchmark
psql -d jsonb_ivm_test -f test/benchmark_baseline.sql > results/baseline_day1.txt
```

**Acceptance criteria for Day 1**:
- ✅ All unit tests pass
- ✅ All integration tests pass
- ✅ Baseline measurements collected
- ✅ Test dataset generated (500 DNS servers, 100 configs, 500 allocations)

---

## Phase 2: POC Benchmarking (Day 2 - 8 hours)

### 2.1: Operation Benchmark - `jsonb_array_update_where` (3 hours)

**File**: `test/benchmark_array_update_where.sql`

```sql
-- Benchmark: jsonb_array_update_where vs native SQL equivalent
-- Goal: Demonstrate >3x improvement on 50-element arrays

\timing on
\set ON_ERROR_STOP on

CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

\echo '========================================'
\echo 'BENCHMARK: jsonb_array_update_where'
\echo '========================================'
\echo ''

-- Ensure test data exists
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_tables WHERE tablename = 'tv_network_configuration') THEN
        RAISE EXCEPTION 'Test data not found. Run generate_cqrs_data.sql first.';
    END IF;
END $$;

-- ============================================================================
-- Benchmark 1: Single element update in 50-element array
-- ============================================================================

\echo '=== Benchmark 1: Update 1 element in 50-element array (1000 iterations) ==='
\echo ''

-- NATIVE APPROACH (baseline)
\echo '--- Native SQL (re-aggregate array with CASE) ---'
BEGIN;
EXPLAIN ANALYZE
WITH updated AS (
    SELECT
        id,
        (
            SELECT jsonb_agg(
                CASE
                    WHEN elem->>'id' = '42'
                    THEN elem || '{"ip": "8.8.8.8"}'::jsonb
                    ELSE elem
                END
                ORDER BY ordinality
            )
            FROM jsonb_array_elements(data->'dns_servers') WITH ORDINALITY AS elem
        ) AS updated_array
    FROM tv_network_configuration
    WHERE id = 1
)
UPDATE tv_network_configuration
SET data = jsonb_set(data, '{dns_servers}', updated_array)
FROM updated
WHERE tv_network_configuration.id = updated.id;
ROLLBACK;

\echo ''
\echo '--- Custom Rust function ---'
BEGIN;
EXPLAIN ANALYZE
UPDATE tv_network_configuration
SET data = jsonb_array_update_where(
    data,
    ARRAY['dns_servers'],
    'id',
    '42'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
)
WHERE id = 1;
ROLLBACK;

-- ============================================================================
-- Benchmark 2: Update propagation in CQRS cascade
-- ============================================================================

\echo ''
\echo '=== Benchmark 2: CQRS Cascade - Update DNS Server #42 ==='
\echo 'Propagate through: v_dns_server → tv_network_configuration → tv_allocation'
\echo ''

-- NATIVE APPROACH
\echo '--- Native SQL Cascade ---'
BEGIN;

UPDATE v_dns_server
SET data = jsonb_set(data, '{ip}', '"9.9.9.9"')
WHERE id = 42;

-- Propagate to tv_network_configuration (re-aggregate full array)
WITH affected_configs AS (
    SELECT DISTINCT network_configuration_id AS id
    FROM bench_nc_dns_mapping
    WHERE dns_server_id = 42
),
updated_configs AS (
    SELECT
        nc.id,
        (
            SELECT jsonb_agg(v.data ORDER BY m.priority)
            FROM bench_nc_dns_mapping m
            JOIN v_dns_server v ON v.id = m.dns_server_id
            WHERE m.network_configuration_id = nc.id
        ) AS updated_dns_servers
    FROM affected_configs ac
    JOIN tv_network_configuration nc ON nc.id = ac.id
)
UPDATE tv_network_configuration
SET data = jsonb_set(data, '{dns_servers}', updated_dns_servers)
FROM updated_configs uc
WHERE tv_network_configuration.id = uc.id;

-- Propagate to tv_allocation (replace network_configuration object)
UPDATE tv_allocation
SET data = jsonb_set(
    data,
    '{network_configuration}',
    (SELECT nc.data FROM tv_network_configuration nc WHERE nc.id = (tv_allocation.data->'network_configuration'->>'id')::int)
)
WHERE (data->'network_configuration'->>'id')::int IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

ROLLBACK;

\echo ''
\echo '--- Custom Rust Cascade ---'
BEGIN;

UPDATE v_dns_server
SET data = jsonb_set(data, '{ip}', '"9.9.9.9"')
WHERE id = 42;

-- Propagate to tv_network_configuration (surgical array update)
UPDATE tv_network_configuration
SET data = jsonb_array_update_where(
    data,
    ARRAY['dns_servers'],
    'id',
    '42'::jsonb,
    (SELECT data FROM v_dns_server WHERE id = 42)
)
WHERE id IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

-- Propagate to tv_allocation (replace network_configuration object)
UPDATE tv_allocation
SET data = jsonb_set(
    data,
    '{network_configuration}',
    (SELECT nc.data FROM tv_network_configuration nc WHERE nc.id = (tv_allocation.data->'network_configuration'->>'id')::int)
)
WHERE (data->'network_configuration'->>'id')::int IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

ROLLBACK;

-- ============================================================================
-- Benchmark 3: Stress test - Update 100 different DNS servers
-- ============================================================================

\echo ''
\echo '=== Benchmark 3: Stress Test - Update 100 DNS servers sequentially ==='
\echo ''

\echo '--- Native SQL (100 cascades) ---'
\timing on
DO $$
DECLARE
    dns_id INTEGER;
BEGIN
    FOR dns_id IN 1..100 LOOP
        -- Update leaf
        UPDATE v_dns_server
        SET data = jsonb_set(data, '{ip}', to_jsonb('10.0.0.' || dns_id))
        WHERE id = dns_id;

        -- Propagate to configs (native re-aggregate)
        WITH affected_configs AS (
            SELECT DISTINCT network_configuration_id AS id
            FROM bench_nc_dns_mapping
            WHERE dns_server_id = dns_id
        ),
        updated_configs AS (
            SELECT
                nc.id,
                (
                    SELECT jsonb_agg(v.data ORDER BY m.priority)
                    FROM bench_nc_dns_mapping m
                    JOIN v_dns_server v ON v.id = m.dns_server_id
                    WHERE m.network_configuration_id = nc.id
                ) AS updated_dns_servers
            FROM affected_configs ac
            JOIN tv_network_configuration nc ON nc.id = ac.id
        )
        UPDATE tv_network_configuration
        SET data = jsonb_set(data, '{dns_servers}', updated_dns_servers)
        FROM updated_configs uc
        WHERE tv_network_configuration.id = uc.id;
    END LOOP;
END $$;
\timing off

\echo ''
\echo '--- Custom Rust (100 cascades) ---'
\timing on
DO $$
DECLARE
    dns_id INTEGER;
BEGIN
    FOR dns_id IN 1..100 LOOP
        -- Update leaf
        UPDATE v_dns_server
        SET data = jsonb_set(data, '{ip}', to_jsonb('10.0.0.' || dns_id))
        WHERE id = dns_id;

        -- Propagate to configs (surgical array update)
        UPDATE tv_network_configuration
        SET data = jsonb_array_update_where(
            data,
            ARRAY['dns_servers'],
            'id',
            to_jsonb(dns_id),
            (SELECT data FROM v_dns_server WHERE id = dns_id)
        )
        WHERE id IN (
            SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = dns_id
        );
    END LOOP;
END $$;
\timing off

\echo ''
\echo '========================================'
\echo 'Benchmark Complete'
\echo '========================================'
\echo ''
\echo 'Expected Results:'
\echo '  - Benchmark 1 (single update): Rust 2-3x faster'
\echo '  - Benchmark 2 (cascade): Rust 3-5x faster'
\echo '  - Benchmark 3 (stress): Rust 5-10x faster'
\echo ''
\echo 'If Rust is <1.5x faster, reconsider approach.'
```

**Run benchmark**:
```bash
psql -d jsonb_ivm_test -f test/benchmark_array_update_where.sql > results/poc_array_update_day2.txt

# Extract timings and compare
grep "Time:" results/poc_array_update_day2.txt
```

---

### 2.2: Operation 2 - `jsonb_merge_at_path` (3 hours)

**Objective**: Implement nested path merge for allocation-level updates

**File**: `src/lib.rs` (add function)

```rust
/// Merge JSONB object at a specific nested path
///
/// # Arguments
/// * `target` - Base JSONB document
/// * `source` - JSONB object to merge
/// * `path` - Path where to merge (empty array = root level)
///
/// # Returns
/// Updated JSONB with source merged at path
///
/// # Examples
/// ```sql
/// -- Update network_configuration in allocation document
/// SELECT jsonb_merge_at_path(
///     '{"id": 1, "network_configuration": {"id": 17, "name": "old"}}'::jsonb,
///     '{"name": "updated"}'::jsonb,
///     ARRAY['network_configuration']
/// );
/// -- Returns: {"id": 1, "network_configuration": {"id": 17, "name": "updated"}}
/// ```
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_merge_at_path(
    target: Option<JsonB>,
    source: JsonB,
    path: Vec<&str>,
) -> Option<JsonB> {
    let target = target?;
    let mut target_value: Value = target.0;

    // Validate source is an object
    let source_obj = match source.0.as_object() {
        Some(obj) => obj,
        None => {
            error!(
                "source argument must be a JSONB object, got: {}",
                value_type_name(&source.0)
            );
        }
    };

    // If path is empty, merge at root
    if path.is_empty() {
        let target_obj = match target_value.as_object_mut() {
            Some(obj) => obj,
            None => {
                error!(
                    "target argument must be a JSONB object when path is empty, got: {}",
                    value_type_name(&target_value)
                );
            }
        };

        // Shallow merge at root
        for (key, value) in source_obj.iter() {
            target_obj.insert(key.clone(), value.clone());
        }

        return Some(JsonB(target_value));
    }

    // Navigate to parent of target path
    let mut current = &mut target_value;
    for (i, key) in path.iter().enumerate() {
        let is_last = i == path.len() - 1;

        if is_last {
            // At target location - merge here
            let parent_obj = match current.as_object_mut() {
                Some(obj) => obj,
                None => {
                    error!(
                        "Path navigation failed: expected object at {:?}, got: {}",
                        &path[..i],
                        value_type_name(current)
                    );
                }
            };

            // Get existing value at key (or create empty object)
            let target_at_path = parent_obj
                .entry(*key)
                .or_insert_with(|| Value::Object(Default::default()));

            // Merge source into target at path
            let merge_target = match target_at_path.as_object_mut() {
                Some(obj) => obj,
                None => {
                    error!(
                        "Cannot merge into non-object at path {:?}, found: {}",
                        path,
                        value_type_name(target_at_path)
                    );
                }
            };

            for (key, value) in source_obj.iter() {
                merge_target.insert(key.clone(), value.clone());
            }
        } else {
            // Navigate deeper
            let obj = match current.as_object_mut() {
                Some(obj) => obj,
                None => {
                    error!(
                        "Path navigation failed at {:?}, expected object, got: {}",
                        &path[..=i],
                        value_type_name(current)
                    );
                }
            };

            current = obj
                .entry(*key)
                .or_insert_with(|| Value::Object(Default::default()));
        }
    }

    Some(JsonB(target_value))
}
```

**Tests**: (Add to `mod tests`)

```rust
#[pgrx::pg_test]
fn test_merge_at_path_root() {
    let target = JsonB(json!({"a": 1, "b": 2}));
    let source = JsonB(json!({"b": 99, "c": 3}));

    let result = crate::jsonb_merge_at_path(
        Some(target),
        source,
        vec![],  // Empty path = root merge
    ).expect("merge should succeed");

    assert_eq!(result.0, json!({"a": 1, "b": 99, "c": 3}));
}

#[pgrx::pg_test]
fn test_merge_at_path_nested() {
    let target = JsonB(json!({
        "id": 1,
        "network_configuration": {
            "id": 17,
            "name": "old",
            "gateway_ip": "192.168.1.1"
        }
    }));
    let source = JsonB(json!({"name": "updated", "dns_count": 50}));

    let result = crate::jsonb_merge_at_path(
        Some(target),
        source,
        vec!["network_configuration"],
    ).expect("merge should succeed");

    let expected = json!({
        "id": 1,
        "network_configuration": {
            "id": 17,
            "name": "updated",
            "gateway_ip": "192.168.1.1",
            "dns_count": 50
        }
    });

    assert_eq!(result.0, expected);
}

#[pgrx::pg_test]
fn test_merge_at_path_deep() {
    let target = JsonB(json!({
        "level1": {
            "level2": {
                "level3": {
                    "existing": "value"
                }
            }
        }
    }));
    let source = JsonB(json!({"new": "data"}));

    let result = crate::jsonb_merge_at_path(
        Some(target),
        source,
        vec!["level1", "level2", "level3"],
    ).expect("merge should succeed");

    assert_eq!(
        result.0["level1"]["level2"]["level3"],
        json!({"existing": "value", "new": "data"})
    );
}
```

**Integration test**: `test/sql/03_merge_at_path.sql`

```sql
CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

-- Test 1: Root level merge
SELECT jsonb_merge_at_path(
    '{"a": 1, "b": 2}'::jsonb,
    '{"b": 99, "c": 3}'::jsonb,
    ARRAY[]::text[]
) = '{"a": 1, "b": 99, "c": 3}'::jsonb AS test_root_merge;

-- Test 2: Nested merge
SELECT jsonb_merge_at_path(
    '{"id": 1, "network_configuration": {"id": 17, "name": "old"}}'::jsonb,
    '{"name": "updated"}'::jsonb,
    ARRAY['network_configuration']
) = '{"id": 1, "network_configuration": {"id": 17, "name": "updated"}}'::jsonb AS test_nested_merge;

-- Test 3: Use in allocation update
WITH updated_nc AS (
    SELECT jsonb_build_object('name', 'Updated Network', 'dns_count', 50) AS new_data
)
SELECT jsonb_merge_at_path(
    '{"id": 100, "name": "Allocation 1", "network_configuration": {"id": 17, "name": "old"}}'::jsonb,
    new_data,
    ARRAY['network_configuration']
)->'network_configuration'->>'name' = 'Updated Network' AS test_allocation_update
FROM updated_nc;

\echo 'All tests should return TRUE'
```

---

### 2.3: End-to-End Cascade Benchmark (2 hours)

**File**: `test/benchmark_e2e_cascade.sql`

```sql
-- End-to-end benchmark: Complete CQRS cascade with custom operations
-- Compare: Native SQL vs Custom Rust cascade

\timing on

\echo '========================================'
\echo 'END-TO-END CASCADE BENCHMARK'
\echo '========================================'
\echo ''
\echo 'Scenario: Update DNS server #42, propagate through all levels'
\echo '  - Affected: 1 DNS server → 2 network configs → 10 allocations'
\echo ''

-- ============================================================================
-- NATIVE SQL CASCADE (Baseline)
-- ============================================================================

\echo '=== NATIVE SQL CASCADE ==='
\echo ''

BEGIN;

-- Step 1: Update leaf (v_dns_server)
\echo '--- Step 1: Update v_dns_server ---'
UPDATE v_dns_server
SET data = jsonb_set(data, '{ip}', '"9.9.9.9"')
WHERE id = 42;

-- Step 2: Propagate to tv_network_configuration (re-aggregate entire array)
\echo '--- Step 2: Propagate to tv_network_configuration (FULL RE-AGGREGATE) ---'
WITH affected_configs AS (
    SELECT DISTINCT network_configuration_id AS id
    FROM bench_nc_dns_mapping
    WHERE dns_server_id = 42
),
updated_configs AS (
    SELECT
        nc.id,
        (
            SELECT jsonb_agg(v.data ORDER BY m.priority)
            FROM bench_nc_dns_mapping m
            JOIN v_dns_server v ON v.id = m.dns_server_id
            WHERE m.network_configuration_id = nc.id
        ) AS updated_dns_servers
    FROM affected_configs ac
    JOIN tv_network_configuration nc ON nc.id = ac.id
)
UPDATE tv_network_configuration
SET data = jsonb_set(data, '{dns_servers}', updated_dns_servers)
FROM updated_configs uc
WHERE tv_network_configuration.id = uc.id;

-- Step 3: Propagate to tv_allocation (replace entire network_configuration)
\echo '--- Step 3: Propagate to tv_allocation (REPLACE OBJECT) ---'
UPDATE tv_allocation
SET data = jsonb_set(
    data,
    '{network_configuration}',
    (SELECT nc.data FROM tv_network_configuration nc WHERE nc.id = (tv_allocation.data->'network_configuration'->>'id')::int)
)
WHERE (data->'network_configuration'->>'id')::int IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

ROLLBACK;

\echo ''

-- ============================================================================
-- CUSTOM RUST CASCADE (Optimized)
-- ============================================================================

\echo '=== CUSTOM RUST CASCADE ==='
\echo ''

BEGIN;

-- Step 1: Update leaf (v_dns_server)
\echo '--- Step 1: Update v_dns_server ---'
UPDATE v_dns_server
SET data = jsonb_set(data, '{ip}', '"9.9.9.9"')
WHERE id = 42;

-- Step 2: Propagate to tv_network_configuration (SURGICAL array update)
\echo '--- Step 2: Propagate to tv_network_configuration (SURGICAL UPDATE) ---'
UPDATE tv_network_configuration
SET data = jsonb_array_update_where(
    data,
    ARRAY['dns_servers'],
    'id',
    '42'::jsonb,
    (SELECT data FROM v_dns_server WHERE id = 42)
)
WHERE id IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

-- Step 3: Propagate to tv_allocation (SURGICAL nested merge)
\echo '--- Step 3: Propagate to tv_allocation (SURGICAL MERGE) ---'
UPDATE tv_allocation a
SET data = jsonb_merge_at_path(
    data,
    nc.data,
    ARRAY['network_configuration']
)
FROM tv_network_configuration nc
WHERE nc.id = (a.data->'network_configuration'->>'id')::int
  AND nc.id IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
  );

ROLLBACK;

\echo ''
\echo '========================================'
\echo 'Expected Results'
\echo '========================================'
\echo ''
\echo 'Native SQL:'
\echo '  - Step 2: 50-100ms (re-aggregate 50-element array × 2 configs)'
\echo '  - Step 3: 100-200ms (replace 50KB object × 10 allocations)'
\echo '  - Total: 150-300ms'
\echo ''
\echo 'Custom Rust:'
\echo '  - Step 2: 10-20ms (surgical update 1 element × 2 configs)'
\echo '  - Step 3: 20-40ms (surgical merge × 10 allocations)'
\echo '  - Total: 30-60ms'
\echo ''
\echo 'Target: 5x improvement (300ms → 60ms)'
\echo ''
```

**Run and analyze**:
```bash
psql -d jsonb_ivm_test -f test/benchmark_e2e_cascade.sql > results/e2e_cascade_day2.txt

# Extract and compare timings
echo "=== NATIVE SQL TIMING ===" >> results/analysis.txt
grep "Time:" results/e2e_cascade_day2.txt | head -3 >> results/analysis.txt

echo "=== RUST TIMING ===" >> results/analysis.txt
grep "Time:" results/e2e_cascade_day2.txt | tail -3 >> results/analysis.txt

# Calculate speedup
python3 << 'EOF'
import re

with open('results/e2e_cascade_day2.txt') as f:
    content = f.read()

times = re.findall(r'Time: ([\d.]+) ms', content)
native_total = sum(float(t) for t in times[:3])
rust_total = sum(float(t) for t in times[3:6])

speedup = native_total / rust_total if rust_total > 0 else 0

print(f"\n=== PERFORMANCE SUMMARY ===")
print(f"Native SQL total: {native_total:.2f}ms")
print(f"Custom Rust total: {rust_total:.2f}ms")
print(f"Speedup: {speedup:.1f}x")
print(f"\nDecision: {'✅ PROCEED' if speedup >= 2.0 else '❌ RECONSIDER'}")
EOF
```

**Acceptance criteria for Day 2**:
- ✅ `jsonb_array_update_where` >2x faster than native on 50-element arrays
- ✅ `jsonb_merge_at_path` >2x faster than native on 50KB documents
- ✅ End-to-end cascade >2x faster overall
- ✅ No crashes or memory leaks in stress tests

---

## Phase 3: Memory Profiling & Decision (Day 3 - 6 hours)

### 3.1: Memory Usage Analysis (3 hours)

**File**: `test/profile_memory.sql`

```sql
-- Memory profiling: Compare memory usage of native vs custom operations

CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

\echo '========================================'
\echo 'MEMORY PROFILING'
\echo '========================================'
\echo ''

-- Reset stats
SELECT pg_stat_statements_reset();

-- ============================================================================
-- Test 1: Memory usage for array update (1000 iterations)
-- ============================================================================

\echo '=== Test 1: Array Update Memory Usage ==='
\echo ''

-- Native approach
\echo '--- Native SQL ---'
SELECT
    pg_backend_memory_contexts()
WHERE name = 'TopMemoryContext' \gset native_before_

DO $$
BEGIN
    FOR i IN 1..1000 LOOP
        PERFORM (
            SELECT jsonb_set(
                data,
                '{dns_servers}',
                (
                    SELECT jsonb_agg(
                        CASE WHEN elem->>'id' = '42'
                        THEN elem || '{"ip": "8.8.8.8"}'::jsonb
                        ELSE elem END
                    )
                    FROM jsonb_array_elements(data->'dns_servers') elem
                )
            )
            FROM tv_network_configuration
            WHERE id = 1
        );
    END LOOP;
END $$;

SELECT
    pg_backend_memory_contexts()
WHERE name = 'TopMemoryContext' \gset native_after_

\echo ''
\echo '--- Custom Rust ---'
SELECT
    pg_backend_memory_contexts()
WHERE name = 'TopMemoryContext' \gset rust_before_

DO $$
BEGIN
    FOR i IN 1..1000 LOOP
        PERFORM jsonb_array_update_where(
            data,
            ARRAY['dns_servers'],
            'id',
            '42'::jsonb,
            '{"ip": "8.8.8.8"}'::jsonb
        )
        FROM tv_network_configuration
        WHERE id = 1;
    END LOOP;
END $$;

SELECT
    pg_backend_memory_contexts()
WHERE name = 'TopMemoryContext' \gset rust_after_

\echo ''
\echo 'Native SQL memory delta: ' :native_after_total_bytes - :native_before_total_bytes ' bytes'
\echo 'Custom Rust memory delta: ' :rust_after_total_bytes - :rust_before_total_bytes ' bytes'
\echo ''

-- ============================================================================
-- Test 2: Large document handling (100KB documents)
-- ============================================================================

\echo '=== Test 2: Large Document Memory Usage ==='
\echo ''

-- Create 100KB test document
WITH large_doc AS (
    SELECT jsonb_build_object(
        'id', 1,
        'large_array', (
            SELECT jsonb_agg(
                jsonb_build_object(
                    'id', i,
                    'data', repeat('x', 100),
                    'metadata', jsonb_build_object('index', i, 'timestamp', now())
                )
            )
            FROM generate_series(1, 1000) i
        )
    ) AS doc
)
SELECT pg_size_pretty(pg_column_size(doc)) AS document_size
FROM large_doc;

-- Test memory growth with large documents
\echo '--- Native SQL (100 iterations on 100KB docs) ---'
DO $$
DECLARE
    large_doc jsonb;
BEGIN
    SELECT jsonb_build_object(
        'id', 1,
        'large_array', (
            SELECT jsonb_agg(jsonb_build_object('id', i, 'data', repeat('x', 100)))
            FROM generate_series(1, 1000) i
        )
    ) INTO large_doc;

    FOR i IN 1..100 LOOP
        large_doc := large_doc || jsonb_build_object('updated', now());
    END LOOP;
END $$;

\echo '--- Custom Rust (100 iterations on 100KB docs) ---'
DO $$
DECLARE
    large_doc jsonb;
BEGIN
    SELECT jsonb_build_object(
        'id', 1,
        'large_array', (
            SELECT jsonb_agg(jsonb_build_object('id', i, 'data', repeat('x', 100)))
            FROM generate_series(1, 1000) i
        )
    ) INTO large_doc;

    FOR i IN 1..100 LOOP
        large_doc := jsonb_merge_at_path(large_doc, jsonb_build_object('updated', now()), ARRAY[]::text[]);
    END LOOP;
END $$;

-- Check for memory leaks
SELECT
    query,
    calls,
    total_exec_time,
    mean_exec_time,
    shared_blks_hit,
    shared_blks_read,
    temp_blks_written
FROM pg_stat_statements
WHERE query LIKE '%jsonb%'
ORDER BY total_exec_time DESC
LIMIT 10;

\echo ''
\echo '=== Memory Profiling Complete ==='
\echo ''
\echo 'Review memory deltas. Custom Rust should be <1.5x native.'
```

**Run profiling**:
```bash
psql -d jsonb_ivm_test -f test/profile_memory.sql > results/memory_profile_day3.txt

# Check for memory leaks using valgrind (if available)
valgrind --leak-check=full --show-leak-kinds=all \
  psql -d jsonb_ivm_test -c "
    SELECT jsonb_array_update_where(
      data, ARRAY['dns_servers'], 'id', '42'::jsonb, '{\"ip\": \"8.8.8.8\"}'::jsonb
    ) FROM tv_network_configuration LIMIT 1000;
  " 2> results/valgrind_output.txt
```

---

### 3.2: Operations 3 & 4 - Quick Implementations (2 hours)

**Operation 3**: `jsonb_has_path_changed` (LOWER PRIORITY)

```rust
/// Check if a specific path has changed between two JSONB documents
///
/// Returns true if values at path differ, false otherwise
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_has_path_changed(
    old_jsonb: Option<JsonB>,
    new_jsonb: Option<JsonB>,
    path: Vec<&str>,
) -> bool {
    let old_value = old_jsonb.map(|j| j.0);
    let new_value = new_jsonb.map(|j| j.0);

    match (old_value, new_value) {
        (Some(old), Some(new)) => {
            let old_at_path = navigate_to_path(&old, &path);
            let new_at_path = navigate_to_path(&new, &path);
            old_at_path != new_at_path
        }
        (None, None) => false,  // Both NULL = no change
        _ => true,  // One NULL, one not = changed
    }
}

fn navigate_to_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}
```

**Operation 4**: `jsonb_array_upsert_where`

```rust
/// Insert or update element in JSONB array based on key match
///
/// If element with matching key exists, updates it. Otherwise, appends element.
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_upsert_where(
    target: Option<JsonB>,
    array_path: Vec<&str>,
    match_key: &str,
    match_value: JsonB,
    element: JsonB,
) -> Option<JsonB> {
    let target = target?;
    let mut target_value: Value = target.0;

    // Navigate to array
    let array = navigate_to_path_mut(&mut target_value, &array_path)?;
    let array_items = match array.as_array_mut() {
        Some(arr) => arr,
        None => {
            error!("Path {:?} does not point to an array", array_path);
        }
    };

    let match_val = match_value.0;
    let element_obj = match element.0.as_object() {
        Some(obj) => obj.clone(),
        None => {
            error!("element argument must be a JSONB object");
        }
    };

    // Try to find and update existing element
    let mut found = false;
    for existing in array_items.iter_mut() {
        if let Some(existing_obj) = existing.as_object() {
            if existing_obj.get(match_key) == Some(&match_val) {
                // Match found - update it
                *existing = Value::Object(element_obj.clone());
                found = true;
                break;
            }
        }
    }

    // If not found, append
    if !found {
        array_items.push(Value::Object(element_obj));
    }

    Some(JsonB(target_value))
}
```

---

### 3.3: Decision Report Generation (1 hour)

**File**: `results/POC_DECISION_REPORT.md`

```markdown
# JSONB IVM POC: Decision Report

**Date**: [Generated Date]
**POC Duration**: 3 days
**Decision**: [PROCEED / PIVOT / CONDITIONAL]

---

## Performance Results

### Benchmark 1: Array Update (`jsonb_array_update_where`)

| Test Case | Native SQL | Custom Rust | Speedup |
|-----------|------------|-------------|---------|
| Single element (50-elem array) | XXms | XXms | X.Xx |
| CQRS cascade (2 configs) | XXms | XXms | X.Xx |
| Stress test (100 updates) | XXXms | XXXms | X.Xx |

**Analysis**: [Met/Did not meet 2x target]

### Benchmark 2: Nested Merge (`jsonb_merge_at_path`)

| Test Case | Native SQL | Custom Rust | Speedup |
|-----------|------------|-------------|---------|
| 50KB document update | XXms | XXms | X.Xx |
| 100KB document update | XXms | XXms | X.Xx |

**Analysis**: [Met/Did not meet 2x target]

### Benchmark 3: End-to-End Cascade

| Cascade Step | Native SQL | Custom Rust | Improvement |
|--------------|------------|-------------|-------------|
| Leaf update | Xms | Xms | -% |
| Intermediate propagation | XXms | XXms | X.Xx |
| Top-level propagation | XXXms | XXms | X.Xx |
| **TOTAL** | **XXXms** | **XXms** | **X.Xx** |

**Analysis**: [Met/Did not meet 5x target]

---

## Memory Profile

| Metric | Native SQL | Custom Rust | Ratio |
|--------|------------|-------------|-------|
| Peak memory (1000 ops) | XXX KB | XXX KB | X.Xx |
| Memory per operation | XXX bytes | XXX bytes | X.Xx |
| Memory leaks detected | No | [Yes/No] | - |

**Analysis**: [Within/Exceeds 1.5x target]

---

## Quality Metrics

| Metric | Status | Notes |
|--------|--------|-------|
| Unit tests passing | [✅/❌] | XX/XX tests |
| Integration tests passing | [✅/❌] | XX/XX tests |
| PostgreSQL 13-17 compatible | [✅/❌] | Tested on: XX,XX,XX |
| Zero crashes (10K ops) | [✅/❌] | - |
| Memory safety verified | [✅/❌] | Valgrind: [clean/issues] |

---

## Decision Matrix

### Option 1: PROCEED with Full Implementation

**Conditions met**:
- [✅/❌] End-to-end speedup >2x
- [✅/❌] Array update speedup >3x
- [✅/❌] Memory usage <1.5x native
- [✅/❌] Zero correctness issues

**If proceeding**:
- Implement Operations 3 & 4
- Add comprehensive benchmarks
- Write production documentation
- Plan release v0.2.0

**Estimated effort**: 2-3 weeks

---

### Option 2: PIVOT to Trigger Optimization

**Recommended if**:
- Speedup <1.5x overall
- Native SQL approach can be optimized further
- Maintenance burden too high

**Alternative approach**:
1. Rewrite triggers to use `jsonb_set` instead of `jsonb_build_object`
2. Add dirty flags for lazy evaluation
3. Consider pg_ivm extension

**Estimated effort**: 1 week

---

### Option 3: CONDITIONAL - Implement High-Value Operations Only

**Recommended if**:
- Some operations show 5x+ improvement
- Others show <1.5x improvement

**Selective implementation**:
- ✅ Implement: [List operations with >3x speedup]
- ❌ Skip: [List operations with <1.5x speedup]

**Estimated effort**: 1-2 weeks

---

## Recommendation

[DETAILED RECOMMENDATION BASED ON RESULTS]

### Rationale

[EXPLAIN DECISION WITH DATA]

### Next Steps

1. [Action 1]
2. [Action 2]
3. [Action 3]

### Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| [Risk 1] | [L/M/H] | [L/M/H] | [Strategy] |

---

## Appendix: Raw Data

[ATTACH BENCHMARK OUTPUT FILES]
```

**Generate report**:
```bash
cd /home/lionel/code/jsonb_ivm

# Run complete benchmark suite
./scripts/run_poc_benchmarks.sh > results/complete_benchmark.txt 2>&1

# Generate decision report
python3 scripts/generate_decision_report.py \
  --baseline results/baseline_day1.txt \
  --poc results/poc_array_update_day2.txt \
  --e2e results/e2e_cascade_day2.txt \
  --memory results/memory_profile_day3.txt \
  --output results/POC_DECISION_REPORT.md
```

---

## Phase 4: Documentation & Handoff (Day 4 - Optional)

**If decision is PROCEED**:

### 4.1: API Documentation

- Add function signatures to README
- Write usage examples for each operation
- Document performance characteristics
- Add migration guide from native SQL

### 4.2: Benchmark Suite

- Package benchmarks as reusable scripts
- Add to CI/CD pipeline
- Create performance regression tests

### 4.3: Release Planning

- Version bump to v0.2.0-alpha1
- Update CHANGELOG
- Create GitHub release with artifacts
- Announce on PostgreSQL mailing lists

---

## Summary: Key Files to Create

### Day 1
- `test/fixtures/generate_cqrs_data.sql` - Test data generator
- `test/benchmark_baseline.sql` - Native SQL baseline
- `src/lib.rs` - Add `jsonb_array_update_where`
- `test/sql/02_array_update_where.sql` - Integration tests

### Day 2
- `test/benchmark_array_update_where.sql` - Operation benchmark
- `src/lib.rs` - Add `jsonb_merge_at_path`
- `test/sql/03_merge_at_path.sql` - Integration tests
- `test/benchmark_e2e_cascade.sql` - End-to-end benchmark

### Day 3
- `test/profile_memory.sql` - Memory profiling
- `src/lib.rs` - Add Operations 3 & 4
- `results/POC_DECISION_REPORT.md` - Decision document

### Supporting Scripts
- `scripts/run_poc_benchmarks.sh` - Automated benchmark runner
- `scripts/generate_decision_report.py` - Report generator
- `scripts/analyze_performance.py` - Performance analysis tool

---

## Success Metrics Summary

| Metric | Target | Measurement |
|--------|--------|-------------|
| Array update speedup | >3x | Benchmark 1 |
| Nested merge speedup | >2x | Benchmark 2 |
| E2E cascade speedup | >2x | Benchmark 3 |
| Memory overhead | <1.5x | Memory profile |
| Correctness | 100% | Tests + stress |
| PostgreSQL compatibility | 13-17 | CI tests |

**Decision Threshold**:
- **PROCEED**: All metrics met
- **CONDITIONAL**: 2+ metrics met with >5x in at least one
- **PIVOT**: <2 metrics met or any critical issue

---

This implementation plan provides a clear, data-driven path to validating whether custom Rust JSONB operations can deliver meaningful performance improvements for CQRS materialized view maintenance. The POC is designed to fail fast if the approach isn't viable, while collecting the evidence needed to make an informed decision.
