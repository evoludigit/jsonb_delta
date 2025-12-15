// jsonb_delta - Array Operations Module
//
// High-performance JSONB array manipulation functions for delta operations.
// Supports update, delete, insert operations with optimized search and sorting.
//
// Part of Phase 0: Code Modularization

use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::Value;
use std::collections::HashMap;

// Import from other modules
use crate::search::find_by_int_id_optimized;

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
pub fn jsonb_array_update_where(
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

    // Security: Validate depth limits to prevent DoS attacks
    crate::validate_depth(&updates.0, crate::MAX_JSONB_DEPTH).unwrap_or_else(|e| error!("{}", e));

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
pub fn jsonb_array_update_where_batch(
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
pub fn jsonb_array_update_multi_row(
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
#[must_use]
pub fn jsonb_array_delete_where(
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
pub fn jsonb_array_insert_where(
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
#[must_use]
pub fn find_insertion_point(
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
#[must_use]
pub fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
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

// Helper function
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
