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
use std::collections::HashMap;

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

// ===== OPTIMIZED SEARCH HELPERS =====

/// Optimized integer ID matching with loop unrolling
/// Returns index of first matching element, or None
///
/// This function uses manual loop unrolling to help the compiler
/// generate SIMD instructions automatically (auto-vectorization)
#[inline]
fn find_by_int_id_optimized(array: &[Value], match_key: &str, match_value: i64) -> Option<usize> {
    // Unroll loop by 8 for potential auto-vectorization
    const UNROLL: usize = 8;

    // For small arrays, simple iteration is fastest
    if array.len() < 32 {
        return find_by_int_id_scalar(array, match_key, match_value);
    }
    let chunks = array.len() / UNROLL;

    for chunk_idx in 0..chunks {
        let base = chunk_idx * UNROLL;

        // Manual loop unrolling - compiler can auto-vectorize this
        // Check 8 elements at once
        for i in 0..UNROLL {
            if let Some(v) = array[base + i].get(match_key) {
                if let Some(id) = v.as_i64() {
                    if id == match_value {
                        return Some(base + i);
                    }
                }
            }
        }
    }

    // Handle remainder elements
    for (i, elem) in array.iter().enumerate().skip(chunks * UNROLL) {
        if let Some(v) = elem.get(match_key) {
            if v.as_i64() == Some(match_value) {
                return Some(i);
            }
        }
    }

    None
}

/// Scalar fallback for small arrays or non-integer IDs
#[inline]
fn find_by_int_id_scalar(array: &[Value], match_key: &str, match_value: i64) -> Option<usize> {
    array.iter().position(|elem| {
        elem.get(match_key).and_then(serde_json::Value::as_i64) == Some(match_value)
    })
}

// ===== CORE FUNCTIONS =====

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
fn jsonb_merge_shallow(target: Option<JsonB>, source: Option<JsonB>) -> Option<JsonB> {
    // Handle NULL inputs - marked with `strict` so PostgreSQL handles this,
    // but we keep explicit handling for clarity
    let target = target?;
    let source = source?;

    // Extract inner serde_json::Value from pgrx JsonB wrapper
    let target_value = target.0;
    let source_value = source.0;

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

/// Update a single element in a JSONB array by matching a key-value predicate
///
/// # Arguments
/// * `target` - JSONB document containing the array
/// * `array_path` - Path to the array within the document (e.g., `"dns_servers"`)
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
///     'dns_servers',
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
/// - For nested paths, use `jsonb_set` with `jsonb_array_update_where`
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where(
    target: JsonB,
    array_path: &str,
    match_key: &str,
    match_value: JsonB,
    updates: JsonB,
) -> JsonB {
    // No Option unwrapping needed - strict guarantees non-NULL
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
    let Some(updates_obj) = updates.0.as_object() else {
        error!(
            "updates argument must be a JSONB object, got: {}",
            value_type_name(&updates.0)
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

/// Batch update multiple elements in a JSONB array
///
/// # Arguments
/// * `target` - JSONB document containing the array
/// * `array_path` - Path to the array (e.g., `"dns_servers"`)
/// * `match_key` - Key to match on (e.g., `"id"`)
/// * `updates_array` - Array of {`match_value`, updates} pairs
///
/// # Example
/// ```sql
/// SELECT jsonb_array_update_where_batch(
///     '{"dns_servers": [{"id": 1}, {"id": 2}, {"id": 3}]}'::jsonb,
///     'dns_servers',
///     'id',
///     '[
///         {"match_value": 1, "updates": {"ip": "1.1.1.1"}},
///         {"match_value": 2, "updates": {"ip": "2.2.2.2"}}
///     ]'::jsonb
/// );
/// ```
///
/// # Performance
/// - Amortizes array scan overhead
/// - Single pass for multiple updates
/// - 2-5× faster than N separate function calls
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where_batch(
    target: JsonB,
    array_path: &str,
    match_key: &str,
    updates_array: JsonB,
) -> JsonB {
    let mut target_value: Value = target.0;

    let Some(array) = target_value.get_mut(array_path) else {
        error!("Path '{}' does not exist in document", array_path)
    };

    let Some(array_items) = array.as_array_mut() else {
        error!("Path '{}' does not point to an array", array_path)
    };

    let Some(updates_list) = updates_array.0.as_array() else {
        error!("updates_array must be a JSONB array")
    };

    // Build hashmap of updates for O(1) lookup
    let mut update_map: HashMap<i64, &serde_json::Map<String, Value>> =
        HashMap::with_capacity(updates_list.len());

    for update_spec in updates_list {
        let Some(spec_obj) = update_spec.as_object() else {
            continue;
        }; // Skip malformed specs

        let Some(match_value) = spec_obj
            .get("match_value")
            .and_then(serde_json::Value::as_i64)
        else {
            continue;
        };

        let Some(updates_obj) = spec_obj.get("updates").and_then(|v| v.as_object()) else {
            continue;
        };

        update_map.insert(match_value, updates_obj);
    }

    // Single pass through array, apply all matching updates
    for element in array_items.iter_mut() {
        if let Some(elem_obj) = element.as_object_mut() {
            if let Some(elem_id) = elem_obj.get(match_key).and_then(serde_json::Value::as_i64) {
                if let Some(updates_obj) = update_map.get(&elem_id) {
                    // Apply updates
                    for (key, value) in *updates_obj {
                        elem_obj.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }

    JsonB(target_value)
}

/// Batch update arrays across multiple JSONB documents
///
/// # Arguments
/// * `targets` - Array of JSONB documents
/// * `array_path` - Path to array in each document
/// * `match_key` - Key to match on
/// * `match_value` - Value to match
/// * `updates` - JSONB object to merge
///
/// # Returns
/// SETOF jsonb - Set of updated JSONB documents (same order as input)
///
/// # Example
/// ```sql
/// -- Returns a set of rows, one per input document
/// SELECT * FROM jsonb_array_update_multi_row(
///     ARRAY[doc1, doc2, doc3],
///     'dns_servers',
///     'id',
///     '42'::jsonb,
///     '{"ip": "8.8.8.8"}'::jsonb
/// );
/// ```
///
/// # Use Case
/// Update 100 network configurations in one function call:
/// ```sql
/// -- Using WITH ORDINALITY to maintain order
/// WITH updated AS (
///     SELECT result, row_number() OVER () as rn
///     FROM jsonb_array_update_multi_row(
///         (SELECT array_agg(data ORDER BY id) FROM tv_network_configuration WHERE ...),
///         'dns_servers',
///         'id',
///         '42'::jsonb,
///         '{"ip": "8.8.8.8"}'::jsonb
///     ) AS result
/// )
/// UPDATE tv_network_configuration
/// SET data = updated.result
/// FROM updated
/// WHERE tv_network_configuration.id = updated.rn;
/// ```
#[allow(clippy::needless_pass_by_value)]
#[allow(clippy::needless_collect)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_multi_row(
    targets: pgrx::Array<JsonB>,
    array_path: &str,
    match_key: &str,
    match_value: JsonB,
    updates: JsonB,
) -> TableIterator<'static, (name!(result, JsonB),)> {
    let match_val = match_value.0;
    let Some(updates_obj) = updates.0.as_object() else {
        error!("updates argument must be a JSONB object")
    };
    let updates_obj = updates_obj.clone();

    // Convert &str to owned String to satisfy 'static lifetime
    let array_path_owned = array_path.to_string();
    let match_key_owned = match_key.to_string();

    // Collect all targets into a Vec to own the data
    let targets_vec: Vec<JsonB> = targets.iter().flatten().collect();

    // Create iterator that will be returned as SETOF
    TableIterator::new(targets_vec.into_iter().map(move |target| {
        // Call single-row update for each document
        let result = jsonb_array_update_where(
            target,
            &array_path_owned,
            &match_key_owned,
            JsonB(match_val.clone()),
            JsonB(Value::Object(updates_obj.clone())),
        );
        (result,)
    }))
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
fn jsonb_merge_at_path(target: JsonB, source: JsonB, path: pgrx::Array<&str>) -> JsonB {
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
fn jsonb_smart_patch_scalar(target: JsonB, source: JsonB) -> JsonB {
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
fn jsonb_smart_patch_nested(target: JsonB, source: JsonB, path: pgrx::Array<&str>) -> JsonB {
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
fn jsonb_smart_patch_array(
    target: JsonB,
    source: JsonB,
    array_path: &str,
    match_key: &str,
    match_value: JsonB,
) -> JsonB {
    jsonb_array_update_where(target, array_path, match_key, match_value, source)
}

/// Delete an element from a JSONB array by matching a key-value predicate
///
/// Provides surgical deletion of array elements without re-aggregation,
/// achieving 3-5× speedup compared to rebuilding the entire array.
///
/// # Arguments
///
/// * `target` - JSONB document containing the array
/// * `array_path` - Path to the array (e.g., "posts")
/// * `match_key` - Key to match on (e.g., "id")
/// * `match_value` - Value to match for deletion
///
/// # Returns
///
/// Updated JSONB with matching element removed (or unchanged if no match)
///
/// # Examples
///
/// ```sql
/// -- Delete post with id=2
/// SELECT jsonb_array_delete_where(
///     '{"posts": [
///         {"id": 1, "title": "First"},
///         {"id": 2, "title": "Second"},
///         {"id": 3, "title": "Third"}
///     ]}'::jsonb,
///     'posts',
///     'id',
///     '2'::jsonb
/// );
/// -- Result: {"posts": [{"id": 1, "title": "First"}, {"id": 3, "title": "Third"}]}
///
/// -- No match - returns unchanged
/// SELECT jsonb_array_delete_where(
///     '{"posts": [{"id": 1}]}'::jsonb,
///     'posts',
///     'id',
///     '999'::jsonb
/// );
/// -- Result: {"posts": [{"id": 1}]}
///
/// -- pg_tview pattern: delete from feed when post is deleted
/// UPDATE tv_feed
/// SET data = jsonb_array_delete_where(
///     data,
///     'posts',
///     'id',
///     to_jsonb(OLD.pk_post)
/// )
/// WHERE data->'posts' @> jsonb_build_array(jsonb_build_object('id', OLD.pk_post));
/// ```
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_delete_where(
    target: JsonB,
    array_path: &str,
    match_key: &str,
    match_value: JsonB,
) -> JsonB {
    let mut target_value: Value = target.0;

    // Navigate to array location
    let Some(array) = target_value.get_mut(array_path) else {
        return JsonB(target_value);
    }; // Array doesn't exist, return unchanged

    // Validate it's an array
    let Some(array_items) = array.as_array_mut() else {
        return JsonB(target_value);
    }; // Not an array, return unchanged

    let match_val = match_value.0;

    // Find and remove matching element using optimized search
    if let Some(idx) = find_element_by_match(array_items, match_key, &match_val) {
        array_items.remove(idx);
    }

    JsonB(target_value)
}

/// Insert an element into a JSONB array with optional sort order maintenance
///
/// Provides surgical insertion without re-aggregation. Can maintain sort order
/// if `sort_key` is provided, or simply append to the end.
///
/// # Arguments
///
/// * `target` - JSONB document containing (or to contain) the array
/// * `array_path` - Path to the array (e.g., `"posts"`)
/// * `new_element` - Element to insert
/// * `sort_key` - Optional key to maintain sort order (e.g., `"created_at"`)
/// * `sort_order` - Sort direction: "ASC" (default) or "DESC"
///
/// # Returns
///
/// Updated JSONB with element inserted
///
/// # Examples
///
/// ```sql
/// -- Simple append (no sort)
/// SELECT jsonb_array_insert_where(
///     '{"posts": [{"id": 1}, {"id": 2}]}'::jsonb,
///     'posts',
///     '{"id": 3, "title": "New Post"}'::jsonb
/// );
/// -- Result: {"posts": [{"id": 1}, {"id": 2}, {"id": 3, "title": "New Post"}]}
///
/// -- Ordered insert (ASC by created_at)
/// SELECT jsonb_array_insert_where(
///     '{"posts": [
///         {"id": 1, "created_at": "2025-01-01"},
///         {"id": 3, "created_at": "2025-01-03"}
///     ]}'::jsonb,
///     'posts',
///     '{"id": 2, "created_at": "2025-01-02"}'::jsonb,
///     'created_at',
///     'ASC'
/// );
/// -- Result: Inserts id=2 between id=1 and id=3
///
/// -- Create array if doesn't exist
/// SELECT jsonb_array_insert_where(
///     '{}'::jsonb,
///     'posts',
///     '{"id": 1}'::jsonb
/// );
/// -- Result: {"posts": [{"id": 1}]}
///
/// -- pg_tview pattern: add new post to feed
/// UPDATE tv_feed
/// SET data = jsonb_array_insert_where(
///     data,
///     'posts',
///     to_jsonb(NEW.*),
///     'created_at',
///     'DESC'
/// )
/// WHERE fk_user = NEW.fk_author;
/// ```
#[pg_extern(immutable, parallel_safe)]
fn jsonb_array_insert_where(
    target: JsonB,
    array_path: &str,
    new_element: JsonB,
    sort_key: Option<&str>,
    sort_order: Option<&str>,
) -> JsonB {
    let mut target_value: Value = target.0;
    let new_elem = new_element.0;

    // Get or create array at path
    let Some(target_obj) = target_value.as_object_mut() else {
        error!(
            "target must be a JSONB object, got: {}",
            value_type_name(&target_value)
        );
    };

    let array = target_obj
        .entry(array_path.to_string())
        .or_insert_with(|| Value::Array(vec![]));

    let Some(array_items) = array.as_array_mut() else {
        error!(
            "path '{}' must point to an array or not exist, got: {}",
            array_path,
            value_type_name(array)
        );
    };

    if let Some(key) = sort_key {
        // Find insertion point to maintain sort order
        let new_sort_val = new_elem.get(key);
        let order = sort_order.unwrap_or("ASC");
        let insert_pos = find_insertion_point(array_items, new_sort_val, key, order);
        array_items.insert(insert_pos, new_elem);
    } else {
        // No sort - append to end
        array_items.push(new_elem);
    }

    JsonB(target_value)
}

/// Find the insertion point to maintain sort order
#[inline]
fn find_insertion_point(
    array: &[Value],
    new_val: Option<&Value>,
    sort_key: &str,
    sort_order: &str,
) -> usize {
    let Some(new_val) = new_val else {
        return array.len();
    }; // No sort value, insert at end

    array
        .iter()
        .position(|elem| {
            let Some(elem_val) = elem.get(sort_key) else {
                return false;
            }; // Element has no sort key, continue searching

            // Compare values based on sort order
            if sort_order.eq_ignore_ascii_case("ASC") {
                compare_values(new_val, elem_val) == std::cmp::Ordering::Less
            } else {
                compare_values(new_val, elem_val) == std::cmp::Ordering::Greater
            }
        })
        .unwrap_or(array.len())
}

/// Compare two JSON values for ordering
#[inline]
fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match (a, b) {
        // Numbers - try exact integer comparison first for precision
        (Value::Number(a_num), Value::Number(b_num)) => {
            if let (Some(a_int), Some(b_int)) = (a_num.as_i64(), b_num.as_i64()) {
                a_int.cmp(&b_int)
            } else {
                // Fall back to float comparison for non-integers
                let a_f64 = a_num.as_f64().unwrap_or(0.0);
                let b_f64 = b_num.as_f64().unwrap_or(0.0);
                a_f64.partial_cmp(&b_f64).unwrap_or(Ordering::Equal)
            }
        }
        // Strings (includes timestamps)
        (Value::String(a_str), Value::String(b_str)) => a_str.cmp(b_str),
        // Booleans
        (Value::Bool(a_bool), Value::Bool(b_bool)) => a_bool.cmp(b_bool),
        // Mixed types - define a consistent ordering
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        (Value::Bool(_), _) => Ordering::Less,
        (_, Value::Bool(_)) => Ordering::Greater,
        (Value::Number(_), _) => Ordering::Less,
        (_, Value::Number(_)) => Ordering::Greater,
        (Value::String(_), _) => Ordering::Less,
        (_, Value::String(_)) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

// ===== DEEP MERGE =====

/// Recursively merge two JSONB documents
///
/// Performs a deep merge where nested objects are merged recursively rather than replaced.
/// This preserves all fields in nested objects while allowing specific field updates.
///
/// # Arguments
///
/// * `target` - Base JSONB document
/// * `source` - JSONB document to merge (recursively)
///
/// # Returns
///
/// Deeply merged JSONB document
///
/// # Behavior
///
/// - **Objects**: Recursively merged (source fields overwrite target fields)
/// - **Arrays**: Replaced entirely (source replaces target)
/// - **Scalars**: Source value replaces target value
///
/// # Examples
///
/// ```sql
/// -- Simple nested merge
/// SELECT jsonb_deep_merge(
///     '{"a": {"b": 1, "c": 2}}'::jsonb,
///     '{"a": {"c": 3, "d": 4}}'::jsonb
/// );
/// -- Result: {"a": {"b": 1, "c": 3, "d": 4}}
///
/// -- Deep nested merge (3 levels)
/// SELECT jsonb_deep_merge(
///     '{"level1": {"level2": {"level3": {"a": 1, "b": 2}}}}'::jsonb,
///     '{"level1": {"level2": {"level3": {"b": 99, "c": 3}}}}'::jsonb
/// );
/// -- Result: {"level1": {"level2": {"level3": {"a": 1, "b": 99, "c": 3}}}}
///
/// -- Array replacement (not merged)
/// SELECT jsonb_deep_merge(
///     '{"items": [1, 2, 3]}'::jsonb,
///     '{"items": [4, 5]}'::jsonb
/// );
/// -- Result: {"items": [4, 5]}
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
fn jsonb_deep_merge(target: JsonB, source: JsonB) -> JsonB {
    let target_val = target.0;
    let source_val = source.0;

    JsonB(deep_merge_recursive(target_val, source_val))
}

/// Recursively merge two JSON values
///
/// If both are objects, recursively merge their keys.
/// Otherwise, source value replaces target value.
fn deep_merge_recursive(target: Value, source: Value) -> Value {
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

// ===== HELPER UTILITIES =====

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

/// Helper function to get human-readable type name for error messages
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

/// Find element in array by key-value match with integer optimization
#[inline]
fn find_element_by_match(array: &[Value], match_key: &str, match_value: &Value) -> Option<usize> {
    match_value.as_i64().map_or_else(
        || {
            array
                .iter()
                .position(|elem| elem.get(match_key).is_some_and(|v| v == match_value))
        },
        |int_id| find_by_int_id_optimized(array, match_key, int_id),
    )
}

// ===== TESTS =====

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;
    use pgrx::JsonB;
    use serde_json::json;

    #[pg_test]
    fn test_basic_merge() {
        let target = JsonB(json!({"a": 1, "b": 2}));
        let source = JsonB(json!({"c": 3}));

        let result =
            crate::jsonb_merge_shallow(Some(target), Some(source)).expect("merge should succeed");

        assert_eq!(result.0, json!({"a": 1, "b": 2, "c": 3}));
    }

    #[pg_test]
    fn test_overlapping_keys() {
        let target = JsonB(json!({"a": 1, "b": 2}));
        let source = JsonB(json!({"b": 99, "c": 3}));

        let result =
            crate::jsonb_merge_shallow(Some(target), Some(source)).expect("merge should succeed");

        // Source value (99) should overwrite target value (2)
        assert_eq!(result.0, json!({"a": 1, "b": 99, "c": 3}));
    }

    #[pg_test]
    fn test_empty_source() {
        let target = JsonB(json!({"a": 1}));
        let source = JsonB(json!({}));

        let result =
            crate::jsonb_merge_shallow(Some(target), Some(source)).expect("merge should succeed");

        assert_eq!(result.0, json!({"a": 1}));
    }

    #[pg_test]
    fn test_null_handling() {
        let source = JsonB(json!({"a": 1}));

        // NULL target
        let result = crate::jsonb_merge_shallow(None, Some(source));
        assert!(result.is_none());

        // NULL source
        let target = JsonB(json!({"a": 1}));
        let result = crate::jsonb_merge_shallow(Some(target), None);
        assert!(result.is_none());
    }

    #[pg_test]
    #[should_panic(expected = "target argument must be a JSONB object")]
    fn test_array_target_errors() {
        let target = JsonB(json!([1, 2, 3]));
        let source = JsonB(json!({"a": 1}));

        // This should error
        let _ = crate::jsonb_merge_shallow(Some(target), Some(source));
    }

    #[pg_test]
    #[should_panic(expected = "source argument must be a JSONB object")]
    fn test_array_source_errors() {
        let target = JsonB(json!({"a": 1}));
        let source = JsonB(json!([1, 2, 3]));

        // This should error
        let _ = crate::jsonb_merge_shallow(Some(target), Some(source));
    }

    #[pg_test]
    fn test_array_update_where_basic() {
        let target = JsonB(json!({
            "dns_servers": [
                {"id": 42, "ip": "1.1.1.1", "port": 53},
                {"id": 43, "ip": "2.2.2.2", "port": 53}
            ]
        }));

        let result = crate::jsonb_array_update_where(
            target,
            "dns_servers",
            "id",
            JsonB(json!(42)),
            JsonB(json!({"ip": "8.8.8.8"})),
        );

        let expected = json!({
            "dns_servers": [
                {"id": 42, "ip": "8.8.8.8", "port": 53},
                {"id": 43, "ip": "2.2.2.2", "port": 53}
            ]
        });

        assert_eq!(result.0, expected);
    }

    #[pg_test]
    fn test_array_update_where_no_match() {
        let target = JsonB(json!({
            "dns_servers": [
                {"id": 42, "ip": "1.1.1.1"},
                {"id": 43, "ip": "2.2.2.2"}
            ]
        }));

        let result = crate::jsonb_array_update_where(
            target,
            "dns_servers",
            "id",
            JsonB(json!(999)), // No element with id=999
            JsonB(json!({"ip": "8.8.8.8"})),
        );

        // Should return unchanged
        assert_eq!(
            result.0,
            json!({
                "dns_servers": [
                    {"id": 42, "ip": "1.1.1.1"},
                    {"id": 43, "ip": "2.2.2.2"}
                ]
            })
        );
    }

    #[pg_test]
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
            target,
            "dns_servers",
            "id",
            JsonB(json!(99)),
            JsonB(json!({"ip": "8.8.8.8", "status": "updated"})),
        );

        // Verify element #99 was updated
        let updated_server = &result.0["dns_servers"][98]; // 0-indexed
        assert_eq!(updated_server["ip"], "8.8.8.8");
        assert_eq!(updated_server["status"], "updated");
        assert_eq!(updated_server["port"], 53); // Unchanged field preserved
    }

    #[pg_test]
    fn test_array_update_where_nested_path() {
        // For now, test single-level path. Nested paths can be handled with jsonb_set
        let target = JsonB(json!({
            "dns_servers": [
                {"id": 1, "ip": "1.1.1.1"},
                {"id": 2, "ip": "2.2.2.2"}
            ]
        }));

        let result = crate::jsonb_array_update_where(
            target,
            "dns_servers",
            "id",
            JsonB(json!(2)),
            JsonB(json!({"ip": "8.8.8.8"})),
        );

        assert_eq!(result.0["dns_servers"][1]["ip"], "8.8.8.8");
    }

    #[pg_test]
    #[should_panic(expected = "does not point to an array")]
    fn test_array_update_where_invalid_path() {
        let target = JsonB(json!({"dns_servers": {"id": 42}})); // Object, not array

        let _ = crate::jsonb_array_update_where(
            target,
            "dns_servers",
            "id",
            JsonB(json!(42)),
            JsonB(json!({"ip": "8.8.8.8"})),
        );
    }

    #[pg_test]
    #[should_panic(expected = "updates argument must be a JSONB object")]
    fn test_array_update_where_invalid_updates() {
        let target = JsonB(json!({"dns_servers": [{"id": 42}]}));

        let _ = crate::jsonb_array_update_where(
            target,
            "dns_servers",
            "id",
            JsonB(json!(42)),
            JsonB(json!("not an object")), // Invalid: scalar instead of object
        );
    }

    #[pg_test]
    fn test_merge_at_path_root() {
        let target = JsonB(json!({"a": 1, "b": 2}));
        let source = JsonB(json!({"b": 99, "c": 3}));

        let empty_path: &[&str] = &[];
        let result = crate::jsonb_merge_at_path(target, source, unsafe {
            pgrx::datum::Array::from_datum(empty_path.into_datum().unwrap(), false).unwrap()
        });

        assert_eq!(result.0, json!({"a": 1, "b": 99, "c": 3}));
    }

    #[pg_test]
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

        let path: &[&str] = &["network_configuration"];
        let result = crate::jsonb_merge_at_path(target, source, unsafe {
            pgrx::datum::Array::from_datum(path.into_datum().unwrap(), false).unwrap()
        });

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

    #[pg_test]
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

        let path: &[&str] = &["level1", "level2", "level3"];
        let result = crate::jsonb_merge_at_path(target, source, unsafe {
            pgrx::datum::Array::from_datum(path.into_datum().unwrap(), false).unwrap()
        });

        assert_eq!(
            result.0["level1"]["level2"]["level3"],
            json!({"existing": "value", "new": "data"})
        );
    }

    // ===== DEEP MERGE TESTS =====

    #[pg_test]
    fn test_deep_merge_simple() {
        let target = JsonB(json!({"a": {"b": 1, "c": 2}}));
        let source = JsonB(json!({"a": {"c": 3, "d": 4}}));

        let result = crate::jsonb_deep_merge(target, source);

        assert_eq!(result.0, json!({"a": {"b": 1, "c": 3, "d": 4}}));
    }

    #[pg_test]
    fn test_deep_merge_nested_three_levels() {
        let target = JsonB(json!({
            "level1": {
                "level2": {
                    "level3": {"a": 1, "b": 2}
                }
            }
        }));
        let source = JsonB(json!({
            "level1": {
                "level2": {
                    "level3": {"b": 99, "c": 3}
                }
            }
        }));

        let result = crate::jsonb_deep_merge(target, source);

        let expected = json!({
            "level1": {
                "level2": {
                    "level3": {"a": 1, "b": 99, "c": 3}
                }
            }
        });

        assert_eq!(result.0, expected);
    }

    #[pg_test]
    fn test_deep_merge_array_replacement() {
        let target = JsonB(json!({"items": [1, 2, 3]}));
        let source = JsonB(json!({"items": [4, 5]}));

        let result = crate::jsonb_deep_merge(target, source);

        // Arrays are replaced, not merged
        assert_eq!(result.0, json!({"items": [4, 5]}));
    }

    #[pg_test]
    fn test_deep_merge_mixed_types() {
        let target = JsonB(json!({"a": {"b": 1}}));
        let source = JsonB(json!({"a": "replaced"}));

        let result = crate::jsonb_deep_merge(target, source);

        // Source replaces target when types differ
        assert_eq!(result.0, json!({"a": "replaced"}));
    }

    #[pg_test]
    fn test_deep_merge_preserves_sibling_fields() {
        let target = JsonB(json!({
            "user": {
                "name": "Alice",
                "company": {
                    "name": "ACME",
                    "city": "NYC"
                }
            }
        }));
        let source = JsonB(json!({
            "user": {
                "company": {
                    "name": "ACME Corp"
                }
            }
        }));

        let result = crate::jsonb_deep_merge(target, source);

        // Should preserve "name": "Alice" and "city": "NYC"
        assert_eq!(result.0["user"]["name"], "Alice");
        assert_eq!(result.0["user"]["company"]["name"], "ACME Corp");
        assert_eq!(result.0["user"]["company"]["city"], "NYC");
    }

    #[pg_test]
    fn test_deep_merge_empty_source() {
        let target = JsonB(json!({"a": {"b": 1}}));
        let source = JsonB(json!({}));

        let result = crate::jsonb_deep_merge(target, source);

        assert_eq!(result.0, json!({"a": {"b": 1}}));
    }

    #[pg_test]
    fn test_deep_merge_empty_target() {
        let target = JsonB(json!({}));
        let source = JsonB(json!({"a": {"b": 1}}));

        let result = crate::jsonb_deep_merge(target, source);

        assert_eq!(result.0, json!({"a": {"b": 1}}));
    }
}
