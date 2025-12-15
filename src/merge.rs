// jsonb_delta - Merge Operations Module
//
// High-performance JSONB merge operations for delta and patch operations.
// Supports shallow, deep, and path-based merging with optimized recursion.
//
// Part of Phase 0: Code Modularization

use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::Value;

// Import from other modules
use crate::search::find_by_int_id_optimized;

/// Merge top-level keys from source JSONB into target JSONB
///
/// # Arguments
/// * `target` - Base JSONB object to merge into
/// * `source` - JSONB object whose keys will be merged
///
/// # Returns
/// New JSONB object with merged keys (source overwrites target on conflicts)
///
/// # Errors
/// * Returns `NULL` if either argument is `NULL`
/// * Errors if either argument is not a JSONB object (arrays/scalars rejected)
///
/// # Examples
/// ```sql
/// SELECT jsonb_merge_shallow('{"a":1,"b":2}'::jsonb, '{"b":99,"c":3}'::jsonb);
/// -- Returns: {"a":1,"b":99,"c":3}
/// ```
///
/// # Notes
/// - Performs shallow merge only (nested objects are replaced, not merged)
/// - For deeply nested updates, use `jsonb_merge_at_path` (planned for v0.2.0)
#[pg_extern(immutable, parallel_safe, strict)]
pub fn jsonb_merge_shallow(target: Option<JsonB>, source: Option<JsonB>) -> Option<JsonB> {
    // Handle NULL inputs - marked with `strict` so PostgreSQL handles this,
    // but we keep explicit handling for clarity
    let target = target?;
    let source = source?;

    // Extract inner serde_json::Value from pgrx JsonB wrapper
    let target_value = target.0;
    let source_value = source.0;

    // Security: Validate depth limits to prevent DoS attacks
    crate::validate_depth(&source_value, crate::MAX_JSONB_DEPTH)
        .unwrap_or_else(|e| error!("{}", e));

    // Validate that both are JSON objects (not arrays or scalars)
    let Some(target_obj) = target_value.as_object() else {
        error!(
            "target argument must be a JSONB object, got: {}",
            value_type_name(&target_value)
        );
    };

    let Some(source_obj) = source_value.as_object() else {
        error!(
            "source argument must be a JSONB object, got: {}",
            value_type_name(&source_value)
        );
    };

    // Perform shallow merge: clone target, then merge source keys
    let mut merged = target_obj.clone();

    for (key, value) in source_obj {
        merged.insert(key.clone(), value.clone());
    }

    // Wrap result in pgrx JsonB and return
    Some(JsonB(Value::Object(merged)))
}

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
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
pub fn jsonb_merge_at_path(target: JsonB, source: JsonB, path: pgrx::Array<&str>) -> JsonB {
    // No Option unwrapping needed - strict guarantees non-NULL
    let mut target_value: Value = target.0;

    // Validate source is an object
    let Some(source_obj) = source.0.as_object() else {
        error!(
            "source argument must be a JSONB object, got: {}",
            value_type_name(&source.0)
        );
    };

    // Collect path into owned Vec<String> to avoid lifetime issues
    let path_vec: Vec<String> = path.iter().flatten().map(ToString::to_string).collect();

    // If path is empty, merge at root
    if path_vec.is_empty() {
        let Some(target_obj) = target_value.as_object_mut() else {
            error!(
                "target argument must be a JSONB object when path is empty, got: {}",
                value_type_name(&target_value)
            );
        };

        // Shallow merge at root
        for (key, value) in source_obj {
            target_obj.insert(key.clone(), value.clone());
        }

        return JsonB(target_value);
    }

    // Navigate to parent of target path
    let mut current = &mut target_value;
    for (i, key) in path_vec.iter().enumerate() {
        let is_last = i == path_vec.len() - 1;

        if is_last {
            // At target location - merge here
            let Some(parent_obj) = current.as_object_mut() else {
                error!(
                    "Path navigation failed: expected object at {:?}, got: {}",
                    &path_vec[..i],
                    value_type_name(current)
                );
            };

            // Get existing value at key (or create empty object)
            let target_at_path = parent_obj
                .entry(key.clone())
                .or_insert_with(|| Value::Object(serde_json::Map::default()));

            // Merge source into target at path
            let Some(merge_target) = target_at_path.as_object_mut() else {
                error!(
                    "Cannot merge into non-object at path {:?}, found: {}",
                    path_vec,
                    value_type_name(target_at_path)
                );
            };

            for (key, value) in source_obj {
                merge_target.insert(key.clone(), value.clone());
            }
        } else {
            // Navigate deeper
            let current_type = value_type_name(current);
            let Some(obj) = current.as_object_mut() else {
                error!(
                    "Path navigation failed at {:?}, expected object, got: {}",
                    &path_vec[..=i],
                    current_type
                );
            };

            current = obj
                .entry(key.clone())
                .or_insert_with(|| Value::Object(serde_json::Map::default()));
        }
    }

    JsonB(target_value)
}

/// Smart JSONB patch for scalar (root-level) updates
///
/// Simplifies `pg_tview` implementations by providing a dedicated function for
/// root-level shallow merges. This is the most common update pattern.
///
/// # Arguments
///
/// * `target` - Current JSONB document
/// * `source` - JSONB object with fields to merge
///
/// # Returns
///
/// Updated JSONB with source fields merged at root level
///
/// # Panics
///
/// Panics if `jsonb_merge_shallow` returns `None` (should not happen with valid inputs).
///
/// # Examples
///
/// ```sql
/// -- Simple scalar update
/// SELECT jsonb_smart_patch_scalar(
///     '{"id": 1, "name": "old", "count": 10}'::jsonb,
///     '{"name": "new", "active": true}'::jsonb
/// );
/// -- Result: {"id": 1, "name": "new", "count": 10, "active": true}
///
/// -- pg_tview pattern usage
/// UPDATE tv_company
/// SET data = jsonb_smart_patch_scalar(data, NEW.data)
/// WHERE pk_company = NEW.pk_company;
/// ```
#[pg_extern(immutable, parallel_safe, strict)]
#[must_use]
pub fn jsonb_smart_patch_scalar(target: JsonB, source: JsonB) -> JsonB {
    jsonb_merge_shallow(Some(target), Some(source))
        .expect("jsonb_merge_shallow should not return NULL with valid inputs")
}

/// Smart JSONB patch for nested object updates
///
/// Simplifies `pg_tview` implementations for nested reference updates.
/// Merges source into a nested object at the specified path.
///
/// # Arguments
///
/// * `target` - Current JSONB document
/// * `source` - JSONB object to merge
/// * `path` - Path to nested object (e.g., ARRAY['user', 'company'])
///
/// # Returns
///
/// Updated JSONB with source merged at nested path
///
/// # Examples
///
/// ```sql
/// -- Nested object update
/// SELECT jsonb_smart_patch_nested(
///     '{"id": 1, "user": {"name": "Alice", "company": {"name": "ACME", "city": "NYC"}}}'::jsonb,
///     '{"name": "ACME Corp"}'::jsonb,
///     ARRAY['user', 'company']
/// );
/// -- Result: {"id": 1, "user": {"name": "Alice", "company": {"name": "ACME Corp", "city": "NYC"}}}
///
/// -- pg_tview pattern usage (user references company)
/// UPDATE tv_user
/// SET data = jsonb_smart_patch_nested(data, NEW.data, ARRAY['company'])
/// WHERE pk_user IN (SELECT pk_user FROM user_has_company WHERE fk_company = NEW.pk_company);
/// ```
#[pg_extern(immutable, parallel_safe, strict)]
#[must_use]
pub fn jsonb_smart_patch_nested(target: JsonB, source: JsonB, path: pgrx::Array<&str>) -> JsonB {
    jsonb_merge_at_path(target, source, path)
}

/// Smart JSONB patch for array element updates
///
/// Simplifies `pg_tview` implementations for array updates within JSONB documents.
/// Updates a single array element by matching on an ID field.
///
/// # Arguments
///
/// * `target` - Current JSONB document containing the array
/// * `source` - JSONB object to merge into matched element
/// * `array_path` - Path to the array field (e.g., `"posts"`)
/// * `match_key` - Key to match on (e.g., `"id"`)
/// * `match_value` - Value to match (e.g., `'42'::jsonb`)
///
/// # Returns
///
/// Updated JSONB with array element modified
///
/// # Examples
///
/// ```sql
/// -- Array element update
/// SELECT jsonb_smart_patch_array(
///     '{"posts": [{"id": 1, "title": "Old"}, {"id": 2, "title": "Post 2"}]}'::jsonb,
///     '{"title": "New", "updated": true}'::jsonb,
///     'posts',
///     'id',
///     '1'::jsonb
/// );
/// -- Result: {"posts": [{"id": 1, "title": "New", "updated": true}, {"id": 2, "title": "Post 2"}]}
///
/// -- pg_tview pattern usage (feed contains posts array)
/// UPDATE tv_feed
/// SET data = jsonb_smart_patch_array(
///     data,
///     NEW.data,
///     'posts',
///     'id',
///     to_jsonb(NEW.pk_post)
/// )
/// WHERE data->'posts' @> jsonb_build_array(jsonb_build_object('id', NEW.pk_post));
/// ```
#[pg_extern(immutable, parallel_safe, strict)]
#[allow(clippy::needless_pass_by_value)] // pgrx FFI signature required
pub fn jsonb_smart_patch_array(
    target: JsonB,
    source: JsonB,
    array_path: &str,
    match_key: &str,
    match_value: JsonB,
) -> JsonB {
    // This will be moved to array_ops module, but for now we need to implement it here
    // since it depends on jsonb_array_update_where which we'll move later
    let mut target_value: Value = target.0;

    // Navigate to array location (single level for now)
    let Some(array) = target_value.get_mut(array_path) else {
        error!("Path '{}' does not exist in document", array_path);
    };

    // Validate it's an array
    let Some(array_items) = array.as_array_mut() else {
        error!(
            "Path '{}' does not point to an array, found: {}",
            array_path,
            value_type_name(array)
        );
    };

    // Extract match value as serde_json::Value
    let match_val = match_value.0;

    // Validate updates is an object
    let Some(updates_obj) = source.0.as_object() else {
        error!(
            "updates argument must be a JSONB object, got: {}",
            value_type_name(&source.0)
        );
    };

    // Find matching element using optimized search
    let match_idx = find_element_by_match(array_items, match_key, &match_val);

    // Apply update if match found
    if let Some(idx) = match_idx {
        if let Some(elem_obj) = array_items[idx].as_object_mut() {
            for (key, value) in updates_obj {
                elem_obj.insert(key.clone(), value.clone());
            }
        }
    }

    JsonB(target_value)
}

/// Deep merge two JSONB documents recursively
///
/// Recursively merges source into target, merging nested objects instead of
/// replacing them. Arrays and scalars are replaced (source wins).
///
/// # Arguments
/// * `target` - Base JSONB document
/// * `source` - JSONB document to merge in
///
/// # Returns
/// New JSONB with deep merge applied
///
/// # Examples
/// ```sql
/// -- Deep merge nested objects
/// SELECT jsonb_deep_merge(
///     '{"user": {"name": "Alice", "prefs": {"theme": "light"}}}'::jsonb,
///     '{"user": {"prefs": {"lang": "en"}}}'::jsonb
/// );
/// -- Result: {"user": {"name": "Alice", "prefs": {"theme": "light", "lang": "en"}}}
///
/// -- pg_tview usage: Update nested company info
/// UPDATE tv_user
/// SET data = jsonb_deep_merge(
///     data,
///     jsonb_build_object('company', jsonb_build_object('name', 'ACME Corp'))
/// )
/// WHERE data->>'company_id' = '123';
/// ```
#[pg_extern(immutable, parallel_safe, strict)]
#[must_use]
pub fn jsonb_deep_merge(target: JsonB, source: JsonB) -> JsonB {
    let target_val = target.0;
    let source_val = source.0;

    // Security: Validate depth limits to prevent DoS attacks
    crate::validate_depth(&source_val, crate::MAX_JSONB_DEPTH).unwrap_or_else(|e| error!("{}", e));

    JsonB(deep_merge_recursive(target_val, source_val))
}

/// Recursively merge two JSON values
///
/// If both are objects, recursively merge their keys.
/// Otherwise, source value replaces target value.
#[must_use]
pub fn deep_merge_recursive(target: Value, source: Value) -> Value {
    match (target, source) {
        (Value::Object(mut target_obj), Value::Object(source_obj)) => {
            use serde_json::map::Entry;
            for (key, source_value) in source_obj {
                match target_obj.entry(key) {
                    Entry::Occupied(mut e) => {
                        let target_value = e.get_mut();
                        if target_value.is_object() && source_value.is_object() {
                            // Recursively merge, taking ownership to avoid clone
                            *target_value =
                                deep_merge_recursive(std::mem::take(target_value), source_value);
                        } else {
                            // Replace with source
                            *target_value = source_value;
                        }
                    }
                    Entry::Vacant(e) => {
                        e.insert(source_value);
                    }
                }
            }
            Value::Object(target_obj)
        }
        // If not both objects, source wins (replaces target)
        (_, source) => source,
    }
}

// Helper function - will be moved to a common utils module later
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

// Helper function - will be moved to search module later
fn find_element_by_match(array: &[Value], match_key: &str, match_value: &Value) -> Option<usize> {
    // Try optimized search for integer IDs first
    if let Some(int_val) = match_value.as_i64() {
        if let Some(idx) = find_by_int_id_optimized(array, match_key, int_val) {
            return Some(idx);
        }
    }

    // Fallback to generic search
    array
        .iter()
        .position(|elem| elem.get(match_key) == Some(match_value))
}
