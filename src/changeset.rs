// jsonb_delta - Coalesced Changeset Module (PROTOTYPE)
//
// A single primitive that applies an ordered list of surgical edits to one JSONB
// document in a SINGLE parse -> mutate -> reserialize pass.
//
// Rationale (see jsonb_delta_analysis.md):
//   The pgrx `JsonB` boundary forces a full jsonb<->text<->serde round-trip on every
//   call. For a single edit that makes jsonb_delta merely at parity with native jsonb_set.
//   But cascade / pg_tviews coalesces ALL changes to a given tv_* row into one commit-time
//   refresh. If those N edits are CHAINED (jsonb_smart_patch_nested(jsonb_smart_patch_array(...)))
//   the whole document is reserialized N times. `jsonb_apply_changeset` pays that cost
//   ONCE for the entire changeset. Measured: at 20 edits this is ~18x faster than chaining.
//
// This module is a drop-in: add `mod changeset;` and `pub use changeset::*;` to lib.rs.

// Used only by the serde reference function retained below as the
// differential-test oracle, so present only in test / pg_test builds.
#[cfg(any(test, feature = "pg_test"))]
use pgrx::prelude::*;
#[cfg(any(test, feature = "pg_test"))]
use pgrx::JsonB;
use serde_json::{Map, Value};

use crate::array_ops::{find_insertion_point, validate_match_key};
use crate::depth::{validate_array_index, MAX_JSONB_ARRAY_SIZE};
use crate::find_element_by_match;
use crate::path::{parse_path, set_path, PathSegment};
use crate::{validate_depth, value_type_name, MAX_JSONB_DEPTH};

/// Maximum number of operations accepted in a single changeset (`DoS` guard: bounds the
/// total work of one call, since the ops array is otherwise attacker-controlled).
pub const MAX_CHANGESET_OPS: usize = 10_000;

/// Maximum number of path segments in a single op path.
///
/// Fixes finding S1: `parse_path` itself imposes no segment-count limit, so a long dotted
/// path can build a document deeper than `MAX_JSONB_DEPTH` and feed serde's unbounded
/// output-serialization recursion. We cap it here at the depth limit.
const MAX_PATH_SEGMENTS: usize = MAX_JSONB_DEPTH;

/// Apply an ordered list of surgical edits to `doc` in place.
///
/// `ops` is a slice of operation objects. Each op is a JSON object with an `"op"`
/// discriminator:
///
/// | op                 | required fields                          | effect                                             |
/// |--------------------|------------------------------------------|----------------------------------------------------|
/// | `set`              | `path`, `value`                          | set value at path (creates intermediates)          |
/// | `remove`           | `path` (non-empty)                       | remove the key/index at path (no-op if absent)     |
/// | `merge`            | `value` (object); `path` optional        | shallow-merge object at path (empty path = root)   |
/// | `deep_merge`       | `value` (object); `path` optional        | recursive merge object at path                     |
/// | `increment`        | `path`, `by` (number)                    | add `by` to the number at path (0 if absent)       |
/// | `array_update`     | `path`, `match_key`, `match_value`, `value` (object) | shallow-merge into FIRST matching element |
/// | `array_update_all` | `path`, `match_key`, `match_value`, `value` (object) | shallow-merge into ALL matching elements  |
/// | `array_replace`    | `path`, `match_key`, `match_value`, `value` | replace first matching element wholesale        |
/// | `array_upsert`     | `path`, `match_key`, `match_value`, `value` (object); `sort_key`,`sort_order` optional | update first match, else insert `value` |
/// | `array_delete`     | `path`, `match_key`, `match_value`       | remove first matching element                      |
/// | `array_insert`     | `path`, `value`; `sort_key`,`sort_order` optional | insert element (ordered if `sort_key` given) |
///
/// `path` may be a dot-notation string (`"user.profile.name"`, `"items[0].id"`) OR an
/// array of segments (`["user","profile","name"]`, `["items", 0, "id"]`). The array form
/// is unambiguous and preferred for generated SQL.
///
/// Semantics chosen for cascade robustness (idempotent refresh, do not abort a whole
/// transaction over a document that legitimately lacks a path):
/// - `array_update` / `array_delete` on a missing or non-array path are **no-ops**.
/// - `set` / `merge` / `array_insert` **create** missing intermediate containers.
/// - A **malformed op** (unknown type, missing required field, wrong value type) is a
///   hard error — that is a generator bug and should fail loudly.
///
/// # Errors
/// Returns `Err` on a malformed op, an invalid path, an over-limit path/index, or a
/// value exceeding the depth cap.
pub fn apply_changeset(doc: &mut Value, ops: &[Value]) -> Result<(), String> {
    for (i, op) in ops.iter().enumerate() {
        let Some(op_obj) = op.as_object() else {
            return Err(format!(
                "changeset op #{i} must be an object, got: {}",
                value_type_name(op)
            ));
        };
        let Some(kind) = op_obj.get("op").and_then(Value::as_str) else {
            return Err(format!(
                "changeset op #{i} is missing a string \"op\" field"
            ));
        };

        match kind {
            "set" => apply_set(doc, op_obj, i)?,
            "remove" => apply_remove(doc, op_obj, i)?,
            "merge" => apply_merge(doc, op_obj, i, MergeKind::Shallow)?,
            "deep_merge" => apply_merge(doc, op_obj, i, MergeKind::Deep)?,
            "increment" => apply_increment(doc, op_obj, i)?,
            "array_update" => apply_array_update(doc, op_obj, i, MatchScope::First)?,
            "array_update_all" => apply_array_update(doc, op_obj, i, MatchScope::All)?,
            "array_replace" => apply_array_replace(doc, op_obj, i)?,
            "array_upsert" => apply_array_upsert(doc, op_obj, i)?,
            "array_delete" => apply_array_delete(doc, op_obj, i)?,
            "array_insert" => apply_array_insert(doc, op_obj, i)?,
            other => return Err(format!("changeset op #{i} has unknown op type: {other}")),
        }
    }
    Ok(())
}

// ---- op implementations -----------------------------------------------------------------

fn apply_set(doc: &mut Value, op: &Map<String, Value>, i: usize) -> Result<(), String> {
    let segments = resolve_path(op, i)?;
    let value = require_value(op, i)?;
    validate_depth(value, MAX_JSONB_DEPTH)?;
    if segments.is_empty() {
        *doc = value.clone();
        Ok(())
    } else {
        set_path(doc, &segments, value.clone())
    }
}

/// Whether a merge overwrites nested objects (shallow) or recurses into them (deep).
#[derive(Clone, Copy)]
enum MergeKind {
    Shallow,
    Deep,
}

/// Whether an array op affects the first match or every match.
#[derive(Clone, Copy, PartialEq)]
enum MatchScope {
    First,
    All,
}

fn apply_merge(
    doc: &mut Value,
    op: &Map<String, Value>,
    i: usize,
    kind: MergeKind,
) -> Result<(), String> {
    let segments = resolve_path(op, i)?;
    let value = require_value(op, i)?;
    validate_depth(value, MAX_JSONB_DEPTH)?;
    if !value.is_object() {
        return Err(format!(
            "changeset op #{i} (merge): value must be an object, got: {}",
            value_type_name(value)
        ));
    }
    // Navigate to (creating) the target object at path, then merge.
    let target = navigate_create_mut(doc, &segments)?;
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    match kind {
        MergeKind::Shallow => {
            let target_obj = target.as_object_mut().unwrap();
            for (k, v) in value.as_object().unwrap() {
                target_obj.insert(k.clone(), v.clone());
            }
        }
        MergeKind::Deep => {
            // Reuse the audited recursive merger; take() avoids cloning the target subtree.
            *target = crate::deep_merge_recursive(std::mem::take(target), value.clone());
        }
    }
    Ok(())
}

fn apply_remove(doc: &mut Value, op: &Map<String, Value>, i: usize) -> Result<(), String> {
    let segments = resolve_path(op, i)?;
    let Some((last, parent_segs)) = segments.split_last() else {
        return Err(format!(
            "changeset op #{i} (remove) requires a non-empty path"
        ));
    };
    // No-op if the parent container is absent (cascade-robust).
    let Some(parent) = navigate_existing_mut(doc, parent_segs) else {
        return Ok(());
    };
    match last {
        PathSegment::Key(key) => {
            if let Some(obj) = parent.as_object_mut() {
                obj.remove(key);
            }
        }
        PathSegment::Index(idx) => {
            if let Some(arr) = parent.as_array_mut() {
                if *idx < arr.len() {
                    arr.remove(*idx);
                }
            }
        }
    }
    Ok(())
}

fn apply_increment(doc: &mut Value, op: &Map<String, Value>, i: usize) -> Result<(), String> {
    let segments = resolve_path(op, i)?;
    let Some(by) = op.get("by") else {
        return Err(format!(
            "changeset op #{i} (increment) requires a \"by\" field"
        ));
    };
    if !by.is_number() {
        return Err(format!(
            "changeset op #{i} (increment): \"by\" must be a number, got: {}",
            value_type_name(by)
        ));
    }
    let target = navigate_create_mut(doc, &segments)?;

    let new_value = if target.is_null() {
        // Absent counter starts at 0, so 0 + by == by.
        by.clone()
    } else if let (Some(t), Some(b)) = (target.as_i64(), by.as_i64()) {
        // Integer path: refuse to silently wrap (audit-conscious).
        let sum = t
            .checked_add(b)
            .ok_or_else(|| format!("changeset op #{i} (increment): integer overflow"))?;
        Value::Number(sum.into())
    } else if let Some(t) = target.as_f64() {
        let b = by.as_f64().unwrap(); // by.is_number() checked above
        finite_number(t + b, i)?
    } else {
        return Err(format!(
            "changeset op #{i} (increment): target at path is {}, not a number",
            value_type_name(target)
        ));
    };
    *target = new_value;
    Ok(())
}

fn finite_number(x: f64, i: usize) -> Result<Value, String> {
    serde_json::Number::from_f64(x)
        .map(Value::Number)
        .ok_or_else(|| format!("changeset op #{i} (increment): result is not finite"))
}

fn apply_array_replace(doc: &mut Value, op: &Map<String, Value>, i: usize) -> Result<(), String> {
    let segments = resolve_path(op, i)?;
    let match_key = require_match_key(op, i)?;
    let match_value = require_match_value(op, i)?;
    let value = require_value(op, i)?;
    validate_depth(value, MAX_JSONB_DEPTH)?;

    let Some(node) = navigate_existing_mut(doc, &segments) else {
        return Ok(());
    };
    let Some(items) = node.as_array_mut() else {
        return Ok(());
    };
    if let Some(idx) = find_element_by_match(items, match_key, match_value) {
        items[idx] = value.clone();
    }
    Ok(())
}

fn apply_array_upsert(doc: &mut Value, op: &Map<String, Value>, i: usize) -> Result<(), String> {
    let segments = resolve_path(op, i)?;
    let match_key = require_match_key(op, i)?;
    let match_value = require_match_value(op, i)?;
    let value = require_value(op, i)?;
    validate_depth(value, MAX_JSONB_DEPTH)?;
    let Some(updates) = value.as_object() else {
        return Err(format!(
            "changeset op #{i} (array_upsert): value must be an object, got: {}",
            value_type_name(value)
        ));
    };

    // Upsert creates the array if it does not exist yet.
    let node = navigate_create_mut(doc, &segments)?;
    if node.is_null() {
        *node = Value::Array(Vec::new());
    }
    let Some(items) = node.as_array_mut() else {
        return Err(format!(
            "changeset op #{i} (array_upsert): path points to {}, not an array",
            value_type_name(node)
        ));
    };

    if let Some(idx) = find_element_by_match(items, match_key, match_value) {
        if let Some(elem) = items[idx].as_object_mut() {
            for (k, v) in updates {
                elem.insert(k.clone(), v.clone());
            }
        }
    } else {
        // Insert the value as a new element (caller includes match_key in it).
        insert_element(items, value.clone(), op);
    }
    Ok(())
}

fn apply_array_update(
    doc: &mut Value,
    op: &Map<String, Value>,
    i: usize,
    scope: MatchScope,
) -> Result<(), String> {
    let segments = resolve_path(op, i)?;
    let match_key = require_match_key(op, i)?;
    let match_value = require_match_value(op, i)?;
    let value = require_value(op, i)?;
    validate_depth(value, MAX_JSONB_DEPTH)?;
    let Some(updates) = value.as_object() else {
        return Err(format!(
            "changeset op #{i} (array_update): value must be an object, got: {}",
            value_type_name(value)
        ));
    };

    // No-op if the array is absent / not an array (cascade-robust).
    let Some(node) = navigate_existing_mut(doc, &segments) else {
        return Ok(());
    };
    let Some(items) = node.as_array_mut() else {
        return Ok(());
    };

    match scope {
        MatchScope::First => {
            if let Some(idx) = find_element_by_match(items, match_key, match_value) {
                merge_into_element(&mut items[idx], updates);
            }
        }
        MatchScope::All => {
            for elem in items.iter_mut() {
                if elem.get(match_key).is_some_and(|v| v == match_value) {
                    merge_into_element(elem, updates);
                }
            }
        }
    }
    Ok(())
}

/// Shallow-merge `updates` into `elem` if `elem` is an object (no-op otherwise).
fn merge_into_element(elem: &mut Value, updates: &Map<String, Value>) {
    if let Some(obj) = elem.as_object_mut() {
        for (k, v) in updates {
            obj.insert(k.clone(), v.clone());
        }
    }
}

/// Insert `element` into `items`, ordered by `sort_key`/`sort_order` if present, else appended.
fn insert_element(items: &mut Vec<Value>, element: Value, op: &Map<String, Value>) {
    match op.get("sort_key").and_then(Value::as_str) {
        Some(sort_key) => {
            let order = op
                .get("sort_order")
                .and_then(Value::as_str)
                .unwrap_or("ASC");
            let pos = find_insertion_point(items, element.get(sort_key), sort_key, order);
            items.insert(pos, element);
        }
        None => items.push(element),
    }
}

fn apply_array_delete(doc: &mut Value, op: &Map<String, Value>, i: usize) -> Result<(), String> {
    let segments = resolve_path(op, i)?;
    let match_key = require_match_key(op, i)?;
    let match_value = require_match_value(op, i)?;

    let Some(node) = navigate_existing_mut(doc, &segments) else {
        return Ok(());
    };
    let Some(items) = node.as_array_mut() else {
        return Ok(());
    };
    if let Some(idx) = find_element_by_match(items, match_key, match_value) {
        items.remove(idx);
    }
    Ok(())
}

fn apply_array_insert(doc: &mut Value, op: &Map<String, Value>, i: usize) -> Result<(), String> {
    let segments = resolve_path(op, i)?;
    let value = require_value(op, i)?;
    validate_depth(value, MAX_JSONB_DEPTH)?;
    let element = value.clone();

    // Create the array (and any intermediates) if absent.
    let node = navigate_create_mut(doc, &segments)?;
    if !node.is_array() {
        if node.is_null() {
            *node = Value::Array(Vec::new());
        } else {
            return Err(format!(
                "changeset op #{i} (array_insert): path points to {}, not an array",
                value_type_name(node)
            ));
        }
    }
    let items = node.as_array_mut().unwrap();
    insert_element(items, element, op);
    Ok(())
}

// ---- helpers ----------------------------------------------------------------------------

fn require_value(op: &Map<String, Value>, i: usize) -> Result<&Value, String> {
    op.get("value")
        .ok_or_else(|| format!("changeset op #{i} is missing required field \"value\""))
}

fn require_match_key(op: &Map<String, Value>, i: usize) -> Result<&str, String> {
    let key = op
        .get("match_key")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("changeset op #{i} is missing string \"match_key\""))?;
    validate_match_key(key).map_err(|e| format!("changeset op #{i}: {e}"))?;
    Ok(key)
}

fn require_match_value(op: &Map<String, Value>, i: usize) -> Result<&Value, String> {
    op.get("match_value")
        .ok_or_else(|| format!("changeset op #{i} is missing required field \"match_value\""))
}

/// Resolve an op's `"path"` (string dot-notation or array of segments) to path segments.
/// Absent / empty path means "root" (empty segment list).
fn resolve_path(op: &Map<String, Value>, i: usize) -> Result<Vec<PathSegment>, String> {
    let segments = match op.get("path") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) if s.is_empty() => Vec::new(),
        Some(Value::String(s)) => parse_path(s).map_err(|e| format!("changeset op #{i}: {e}"))?,
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|seg| match seg {
                Value::String(k) => Ok(PathSegment::Key(k.clone())),
                Value::Number(n) => n
                    .as_u64()
                    .and_then(|u| usize::try_from(u).ok().map(PathSegment::Index))
                    .ok_or_else(|| {
                        format!("changeset op #{i}: array index must be a non-negative integer")
                    }),
                other => Err(format!(
                    "changeset op #{i}: path segment must be a string or integer, got: {}",
                    value_type_name(other)
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(other) => {
            return Err(format!(
                "changeset op #{i}: path must be a string or array, got: {}",
                value_type_name(other)
            ))
        }
    };
    if segments.len() > MAX_PATH_SEGMENTS {
        return Err(format!(
            "changeset op #{i}: path has {} segments, exceeds maximum {MAX_PATH_SEGMENTS}",
            segments.len()
        ));
    }
    Ok(segments)
}

/// Navigate to the node at `segments`, creating intermediate objects/arrays as needed.
/// Returns a mutable reference to the final node.
fn navigate_create_mut<'a>(
    mut current: &'a mut Value,
    segments: &[PathSegment],
) -> Result<&'a mut Value, String> {
    for segment in segments {
        match segment {
            PathSegment::Key(key) => {
                if !current.is_object() {
                    *current = Value::Object(Map::new());
                }
                current = current
                    .as_object_mut()
                    .unwrap()
                    .entry(key.clone())
                    .or_insert(Value::Null);
            }
            PathSegment::Index(idx) => {
                validate_array_index(*idx, MAX_JSONB_ARRAY_SIZE)?;
                if !current.is_array() {
                    *current = Value::Array(Vec::new());
                }
                let arr = current.as_array_mut().unwrap();
                while arr.len() <= *idx {
                    arr.push(Value::Null);
                }
                current = &mut arr[*idx];
            }
        }
    }
    Ok(current)
}

/// Navigate to an EXISTING node at `segments` without creating anything.
/// Returns `None` if any segment is missing or type-mismatched.
fn navigate_existing_mut<'a>(
    mut current: &'a mut Value,
    segments: &[PathSegment],
) -> Option<&'a mut Value> {
    for segment in segments {
        current = match segment {
            PathSegment::Key(key) => current.as_object_mut()?.get_mut(key)?,
            PathSegment::Index(idx) => current.as_array_mut()?.get_mut(*idx)?,
        };
    }
    Some(current)
}

// ---- SQL entry point --------------------------------------------------------------------

/// Apply an ordered changeset to a JSONB document in a single parse/reserialize pass.
///
/// # Examples
/// ```sql
/// -- One call coalescing a feed row's changeset (vs. a chain of smart_patch calls):
/// SELECT jsonb_apply_changeset(
///     data,
///     '[
///        {"op": "array_upsert", "path": "posts", "match_key": "id",
///         "match_value": "3f2a...uuid",
///         "value": {"id": "3f2a...uuid", "title": "Edited"},
///         "sort_key": "created_at", "sort_order": "DESC"},
///        {"op": "array_delete", "path": "posts", "match_key": "id", "match_value": 7},
///        {"op": "deep_merge", "path": ["author"], "value": {"stats": {"posts": 12}}},
///        {"op": "increment", "path": "stats.post_count", "by": 1},
///        {"op": "remove", "path": "stats.stale_cache"}
///     ]'::jsonb
/// )
/// FROM tv_feed WHERE ...;
/// ```
// Retained only as the differential-test oracle for the binary implementation
// that replaced it; not built into the shipped extension.
#[cfg(any(test, feature = "pg_test"))]
#[allow(clippy::needless_pass_by_value)]
#[cfg_attr(
    any(test, feature = "pg_test"),
    pg_extern(
        immutable,
        parallel_safe,
        strict,
        name = "jsonb_apply_changeset_reference"
    )
)]
fn jsonb_apply_changeset_reference(doc: JsonB, ops: JsonB) -> JsonB {
    let mut root: Value = doc.0;

    let Some(ops_arr) = ops.0.as_array() else {
        error!(
            "ops argument must be a JSONB array, got: {}",
            value_type_name(&ops.0)
        );
    };
    if ops_arr.len() > MAX_CHANGESET_OPS {
        error!(
            "changeset has {} ops, exceeds maximum {}",
            ops_arr.len(),
            MAX_CHANGESET_OPS
        );
    }

    apply_changeset(&mut root, ops_arr).unwrap_or_else(|e| error!("{}", e));

    JsonB(root)
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use serde_json::json;

    #[allow(clippy::needless_pass_by_value)]
    fn run(mut doc: Value, ops: Value) -> Value {
        apply_changeset(&mut doc, ops.as_array().unwrap()).unwrap();
        doc
    }

    #[test]
    fn test_set_nested_creates_intermediates() {
        let out = run(json!({}), json!([{"op":"set","path":"a.b.c","value":1}]));
        assert_eq!(out, json!({"a":{"b":{"c":1}}}));
    }

    #[test]
    fn test_merge_at_path() {
        let out = run(
            json!({"author":{"name":"Old","city":"NYC"}}),
            json!([{"op":"merge","path":["author"],"value":{"name":"New"}}]),
        );
        assert_eq!(out, json!({"author":{"name":"New","city":"NYC"}}));
    }

    #[test]
    fn test_array_update_uuid_key() {
        // UUID (string) match values must work — jsonb_array_update_where_batch cannot do this.
        let out = run(
            json!({"posts":[{"id":"aaa","t":"x"},{"id":"bbb","t":"y"}]}),
            json!([{"op":"array_update","path":"posts","match_key":"id",
                    "match_value":"bbb","value":{"t":"z"}}]),
        );
        assert_eq!(
            out,
            json!({"posts":[{"id":"aaa","t":"x"},{"id":"bbb","t":"z"}]})
        );
    }

    #[test]
    fn test_array_delete_int_key() {
        let out = run(
            json!({"posts":[{"id":1},{"id":2},{"id":3}]}),
            json!([{"op":"array_delete","path":"posts","match_key":"id","match_value":2}]),
        );
        assert_eq!(out, json!({"posts":[{"id":1},{"id":3}]}));
    }

    #[test]
    fn test_array_insert_sorted_desc() {
        let out = run(
            json!({"posts":[{"id":9,"c":9},{"id":5,"c":5}]}),
            json!([{"op":"array_insert","path":"posts","value":{"id":7,"c":7},
                    "sort_key":"c","sort_order":"DESC"}]),
        );
        assert_eq!(
            out,
            json!({"posts":[{"id":9,"c":9},{"id":7,"c":7},{"id":5,"c":5}]})
        );
    }

    #[test]
    fn test_mixed_changeset_applied_in_order() {
        let out = run(
            json!({"posts":[{"id":1,"t":"a"},{"id":2,"t":"b"}],"author":{"name":"Old"}}),
            json!([
                {"op":"array_update","path":"posts","match_key":"id","match_value":1,"value":{"t":"A"}},
                {"op":"array_delete","path":"posts","match_key":"id","match_value":2},
                {"op":"merge","path":["author"],"value":{"name":"New"}},
                {"op":"array_insert","path":"posts","value":{"id":3,"t":"c"}}
            ]),
        );
        assert_eq!(
            out,
            json!({"posts":[{"id":1,"t":"A"},{"id":3,"t":"c"}],"author":{"name":"New"}})
        );
    }

    #[test]
    fn test_array_update_missing_path_is_noop() {
        let out = run(
            json!({"other":1}),
            json!([{"op":"array_update","path":"posts","match_key":"id","match_value":1,"value":{"t":"x"}}]),
        );
        assert_eq!(out, json!({"other":1}));
    }

    #[test]
    fn test_remove_key_and_index() {
        let out = run(
            json!({"a":{"b":1,"c":2},"arr":[10,20,30]}),
            json!([{"op":"remove","path":"a.b"},{"op":"remove","path":"arr[1]"}]),
        );
        assert_eq!(out, json!({"a":{"c":2},"arr":[10,30]}));
    }

    #[test]
    fn test_remove_missing_path_is_noop() {
        let out = run(json!({"a":1}), json!([{"op":"remove","path":"x.y.z"}]));
        assert_eq!(out, json!({"a":1}));
    }

    #[test]
    fn test_deep_merge_recurses() {
        let out = run(
            json!({"author":{"name":"A","stats":{"posts":10,"likes":5}}}),
            json!([{"op":"deep_merge","path":["author"],"value":{"stats":{"posts":11}}}]),
        );
        // shallow merge would drop "likes"; deep merge keeps it
        assert_eq!(
            out,
            json!({"author":{"name":"A","stats":{"posts":11,"likes":5}}})
        );
    }

    #[test]
    fn test_increment_creates_and_adds() {
        let out = run(
            json!({"stats":{"n":41}}),
            json!([
                {"op":"increment","path":"stats.n","by":1},
                {"op":"increment","path":"stats.fresh","by":5}
            ]),
        );
        assert_eq!(out, json!({"stats":{"n":42,"fresh":5}}));
    }

    #[test]
    fn test_increment_overflow_errors() {
        let mut doc = json!({"n": i64::MAX});
        let err = apply_changeset(
            &mut doc,
            json!([{"op":"increment","path":"n","by":1}])
                .as_array()
                .unwrap(),
        )
        .unwrap_err();
        assert!(err.contains("overflow"));
    }

    #[test]
    fn test_array_upsert_update_branch() {
        let out = run(
            json!({"posts":[{"id":"u1","t":"old"}]}),
            json!([{"op":"array_upsert","path":"posts","match_key":"id",
                    "match_value":"u1","value":{"id":"u1","t":"new"}}]),
        );
        assert_eq!(out, json!({"posts":[{"id":"u1","t":"new"}]}));
    }

    #[test]
    fn test_array_upsert_insert_branch_creates_array() {
        let out = run(
            json!({}),
            json!([{"op":"array_upsert","path":"posts","match_key":"id",
                    "match_value":"u2","value":{"id":"u2","t":"fresh"}}]),
        );
        assert_eq!(out, json!({"posts":[{"id":"u2","t":"fresh"}]}));
    }

    #[test]
    fn test_array_replace_whole_element() {
        let out = run(
            json!({"posts":[{"id":1,"a":1,"b":2}]}),
            json!([{"op":"array_replace","path":"posts","match_key":"id",
                    "match_value":1,"value":{"id":1,"c":3}}]),
        );
        // replace drops a and b, unlike array_update which would keep them
        assert_eq!(out, json!({"posts":[{"id":1,"c":3}]}));
    }

    #[test]
    fn test_array_update_all_matches() {
        let out = run(
            json!({"posts":[{"id":1,"s":"draft"},{"id":2,"s":"draft"},{"id":3,"s":"live"}]}),
            json!([{"op":"array_update_all","path":"posts","match_key":"s",
                    "match_value":"draft","value":{"reviewed":true}}]),
        );
        assert_eq!(
            out,
            json!({"posts":[
                {"id":1,"s":"draft","reviewed":true},
                {"id":2,"s":"draft","reviewed":true},
                {"id":3,"s":"live"}
            ]})
        );
    }

    #[test]
    fn test_unknown_op_errors() {
        let mut doc = json!({});
        let err = apply_changeset(&mut doc, json!([{"op":"frobnicate"}]).as_array().unwrap())
            .unwrap_err();
        assert!(err.contains("unknown op type"));
    }

    #[test]
    fn test_too_many_path_segments_rejected() {
        let long = vec![Value::String("k".into()); MAX_PATH_SEGMENTS + 1];
        let mut doc = json!({});
        let err = apply_changeset(
            &mut doc,
            json!([{"op":"set","path":long,"value":1}])
                .as_array()
                .unwrap(),
        )
        .unwrap_err();
        assert!(err.contains("exceeds maximum"));
    }
}

/// End-to-end tests that exercise the real `#[pg_extern]` through SQL: `JsonB`
/// (de)serialization at the pgrx boundary, and the `error!` unwind path.
#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;
    use pgrx::JsonB;

    #[pg_test]
    fn apply_changeset_mixed() {
        let result = Spi::get_one::<JsonB>(
            r#"SELECT jsonb_apply_changeset(
                 '{"posts":[{"id":"a","t":"x"},{"id":2,"t":"y"}],"stats":{"n":1}}'::jsonb,
                 '[{"op":"array_update","path":"posts","match_key":"id","match_value":"a","value":{"t":"X"}},
                   {"op":"array_delete","path":"posts","match_key":"id","match_value":2},
                   {"op":"increment","path":"stats.n","by":41}]'::jsonb)"#,
        )
        .expect("SPI ok")
        .expect("non-null");
        assert_eq!(
            result.0,
            serde_json::json!({"posts":[{"id":"a","t":"X"}],"stats":{"n":42}})
        );
    }

    #[pg_test]
    fn apply_changeset_upsert_insert_creates_array() {
        let result = Spi::get_one::<JsonB>(
            r#"SELECT jsonb_apply_changeset(
                 '{}'::jsonb,
                 '[{"op":"array_upsert","path":"posts","match_key":"id","match_value":"u2",
                    "value":{"id":"u2","t":"fresh"}}]'::jsonb)"#,
        )
        .expect("SPI ok")
        .expect("non-null");
        assert_eq!(
            result.0,
            serde_json::json!({"posts":[{"id":"u2","t":"fresh"}]})
        );
    }

    #[pg_test(error = "changeset op #0 has unknown op type: nope")]
    fn apply_changeset_unknown_op_errors() {
        Spi::run(r#"SELECT jsonb_apply_changeset('{}'::jsonb, '[{"op":"nope"}]'::jsonb)"#).unwrap();
    }
}
