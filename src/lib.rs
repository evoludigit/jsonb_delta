// jsonb_ivm - Incremental JSONB View Maintenance Extension
//
// High-performance PostgreSQL extension for intelligent partial updates
// of JSONB materialized views in CQRS architectures.
//
// Copyright (c) 2025, Lionel Hamayon
// Licensed under the PostgreSQL License

use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::Value;

// Tell pgrx which PostgreSQL versions we support
pgrx::pg_module_magic!();

// pgrx test setup module
#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {
        // Perform one-time initialization when the pg_test framework starts
    }

    #[must_use]
    pub fn postgresql_conf_options() -> Vec<&'static str> {
        // Specify additional postgresql.conf settings if needed
        vec![]
    }
}

// Module declarations (Phase 0: Modularization)
mod array_ops;
mod depth;
mod merge;
mod search;

// Re-exports for public API (maintains backward compatibility)
pub use array_ops::*;
pub use depth::validate_depth;
pub use depth::MAX_JSONB_DEPTH;
pub use merge::*;

/// Extract ID value from JSONB document
///
/// Simplifies ID extraction for `pg_tview` implementations by providing a safe,
/// type-flexible ID extraction function.
///
/// # Arguments
///
/// * `data` - JSONB document containing the ID
/// * `key` - Key to extract (default: 'id')
///
/// # Returns
///
/// ID value as text, or NULL if not found or invalid type
///
/// # Supported ID Types
///
/// - **String**: UUID, custom IDs (returned as-is)
/// - **Number**: Integer IDs (converted to string)
/// - **Other types**: Returns NULL (boolean, null, array, object)
///
/// # Examples
///
/// ```sql
/// -- UUID extraction
/// SELECT jsonb_extract_id('{"id": "550e8400-e29b-41d4-a716-446655440000", "name": "Alice"}'::jsonb);
/// -- Returns: '550e8400-e29b-41d4-a716-446655440000'
///
/// -- Integer extraction
/// SELECT jsonb_extract_id('{"id": 42, "title": "Post"}'::jsonb);
/// -- Returns: '42'
///
/// -- Custom key
/// SELECT jsonb_extract_id('{"post_id": 123, "title": "..."}'::jsonb, 'post_id');
/// -- Returns: '123'
///
/// -- Not found
/// SELECT jsonb_extract_id('{"name": "Alice"}'::jsonb);
/// -- Returns: NULL
///
/// -- Invalid type (boolean)
/// SELECT jsonb_extract_id('{"id": true}'::jsonb);
/// -- Returns: NULL
///
/// -- pg_tview usage: Extract ID for propagation
/// SELECT jsonb_extract_id(data) AS user_id
/// FROM tv_user
/// WHERE jsonb_extract_id(data, 'company_id') = '123';
/// ```
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe)]
fn jsonb_extract_id(data: JsonB, key: default!(&str, "'id'")) -> Option<String> {
    let obj = data.0.as_object()?;
    let id_value = obj.get(key)?;

    match id_value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Check if JSONB array contains element with specific ID
///
/// Fast containment check for `pg_tview` implementations, with optimized
/// search for integer IDs using loop unrolling.
///
/// # Arguments
///
/// * `data` - JSONB document containing the array
/// * `array_path` - Path to array field (e.g., 'posts')
/// * `id_key` - Key to match on (e.g., 'id')
/// * `id_value` - Value to search for
///
/// # Returns
///
/// true if array contains element with matching ID, false otherwise
///
/// # Performance
///
/// - **Integer IDs**: Uses `find_by_int_id_optimized()` with loop unrolling (~100ns/element)
/// - **Non-integer IDs**: Generic search (~200ns/element)
///
/// # Examples
///
/// ```sql
/// -- Integer ID (uses optimized search)
/// SELECT jsonb_array_contains_id(
///     '{"posts": [{"id": 1}, {"id": 2}, {"id": 3}]}'::jsonb,
///     'posts',
///     'id',
///     '2'::jsonb
/// );
/// -- Returns: true
///
/// -- UUID (generic search)
/// SELECT jsonb_array_contains_id(
///     '{"posts": [{"id": "550e8400-..."}, {"id": "660f9500-..."}]}'::jsonb,
///     'posts',
///     'id',
///     '"550e8400-..."'::jsonb
/// );
/// -- Returns: true
///
/// -- Not found
/// SELECT jsonb_array_contains_id(
///     '{"posts": [{"id": 1}, {"id": 2}]}'::jsonb,
///     'posts',
///     'id',
///     '999'::jsonb
/// );
/// -- Returns: false
///
/// -- Array doesn't exist
/// SELECT jsonb_array_contains_id(
///     '{"other": []}'::jsonb,
///     'posts',
///     'id',
///     '1'::jsonb
/// );
/// -- Returns: false
///
/// -- pg_tview usage: Check if feed contains post
/// SELECT pk_feed FROM tv_feed
/// WHERE jsonb_array_contains_id(data, 'posts', 'id', '123'::jsonb);
/// ```
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_contains_id(data: JsonB, array_path: &str, id_key: &str, id_value: JsonB) -> bool {
    let Some(obj) = data.0.as_object() else {
        return false;
    };

    let Some(array) = obj.get(array_path).and_then(|v| v.as_array()) else {
        return false;
    };

    // Use optimized search helper
    find_element_by_match(array, id_key, &id_value.0).is_some()
}

/// Find element in array by key-value match with integer optimization
#[inline]
fn find_element_by_match(array: &[Value], match_key: &str, match_value: &Value) -> Option<usize> {
    match_value.as_i64().map_or_else(
        || {
            array
                .iter()
                .position(|elem| elem.get(match_key).is_some_and(|v| v == match_value))
        },
        |int_id| crate::search::find_by_int_id_optimized(array, match_key, int_id),
    )
}

/// Helper function to get human-readable type name for error messages
#[allow(dead_code)]
const fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
