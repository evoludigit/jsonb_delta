//! Path parsing and navigation for nested JSONB operations
//!
//! This module provides functionality to parse dot notation and array indexing
//! paths like `user.profile.name` or `orders[0].items[1].price`.

use serde_json::Value;

/// Represents a single segment in a JSONB path
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PathSegment {
    /// Object key access (e.g., `.field`)
    Key(String),
    /// Array index access (e.g., `[0]`)
    Index(usize),
}

/// Parse a path string into a sequence of path segments
///
/// # Supported Syntax
/// - Dot notation: `a.b.c` → access nested objects
/// - Array indexing: `a[0]` → access array element by index
/// - Mixed paths: `orders[0].items[1].price` → combined access
/// - Backward compatibility: Single keys `user` still work
///
/// # Errors
///
/// Returns an error if:
/// - The path is empty
/// - The path contains consecutive dots (e.g., `a..b`)
/// - Array index is empty (e.g., `a[]`)
/// - Array index is not a valid number
/// - Unexpected closing bracket appears (e.g., `a]`)
///
/// # Examples
/// ```
/// use jsonb_ivm::path::{parse_path, PathSegment};
///
/// assert_eq!(
///     parse_path("a.b.c").unwrap(),
///     vec![
///         PathSegment::Key("a".into()),
///         PathSegment::Key("b".into()),
///         PathSegment::Key("c".into()),
///     ]
/// );
///
/// assert_eq!(
///     parse_path("a[0].b[1]").unwrap(),
///     vec![
///         PathSegment::Key("a".into()),
///         PathSegment::Index(0),
///         PathSegment::Key("b".into()),
///         PathSegment::Index(1),
///     ]
/// );
/// ```
pub fn parse_path(path: &str) -> Result<Vec<PathSegment>, String> {
    let mut segments = Vec::new();
    let mut current_key = String::new();
    let mut chars = path.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '.' => {
                if !current_key.is_empty() {
                    segments.push(PathSegment::Key(current_key.clone()));
                    current_key.clear();
                }
                // Skip consecutive dots or leading dots
                if chars.peek() == Some(&'.') {
                    return Err("Invalid path: consecutive dots".into());
                }
            }
            '[' => {
                if !current_key.is_empty() {
                    segments.push(PathSegment::Key(current_key.clone()));
                    current_key.clear();
                }
                // Parse index
                let index_str: String = chars.by_ref().take_while(|&c| c != ']').collect();

                if index_str.is_empty() {
                    return Err("Invalid path: empty array index".into());
                }

                let index = index_str
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid array index: {index_str}"))?;
                segments.push(PathSegment::Index(index));
            }
            ']' => {
                return Err("Invalid path: unexpected closing bracket".into());
            }
            _ => {
                current_key.push(ch);
            }
        }
    }

    if !current_key.is_empty() {
        segments.push(PathSegment::Key(current_key));
    }

    if segments.is_empty() {
        return Err("Invalid path: empty path".into());
    }

    Ok(segments)
}

/// Navigate to a value in a JSONB document using a parsed path
///
/// Returns `Some(&Value)` if the path exists, `None` if any segment doesn't exist.
///
/// # Examples
/// ```
/// use serde_json::json;
/// use jsonb_ivm::path::{parse_path, navigate_path};
///
/// let data = json!({
///     "user": {
///         "profile": {
///             "name": "Alice"
///         }
///     }
/// });
///
/// let path = parse_path("user.profile.name").unwrap();
/// assert_eq!(navigate_path(&data, &path), Some(&json!("Alice")));
///
/// let invalid_path = parse_path("user.profile.age").unwrap();
/// assert_eq!(navigate_path(&data, &invalid_path), None);
/// ```
#[must_use]
pub fn navigate_path<'a>(json: &'a Value, path: &[PathSegment]) -> Option<&'a Value> {
    let mut current = json;

    for segment in path {
        match segment {
            PathSegment::Key(key) => {
                if let Some(obj) = current.as_object() {
                    current = obj.get(key)?;
                } else {
                    return None;
                }
            }
            PathSegment::Index(idx) => {
                if let Some(arr) = current.as_array() {
                    current = arr.get(*idx)?;
                } else {
                    return None;
                }
            }
        }
    }

    Some(current)
}

/// Set a value at a specific path in a JSONB document
///
/// This is a mutable version of navigation that can create intermediate objects/arrays
/// as needed. Used internally by the path-based update functions.
///
/// # Errors
///
/// Returns an error if the path is empty.
///
/// # Panics
///
/// Panics if internal unwrap operations fail after successful type checks (should not happen in normal operation).
///
/// # Examples
/// ```
/// use serde_json::json;
/// use jsonb_ivm::path::{parse_path, set_path};
///
/// let mut data = json!({"user": {"profile": {}}});
/// let path = parse_path("user.profile.name").unwrap();
/// set_path(&mut data, &path, json!("Alice"));
///
/// assert_eq!(data, json!({"user": {"profile": {"name": "Alice"}}}));
/// ```
pub fn set_path(json: &mut Value, path: &[PathSegment], value: Value) -> Result<(), String> {
    if path.is_empty() {
        return Err("Cannot set empty path".into());
    }

    // Navigate to the parent of the final segment
    let parent_path = &path[..path.len() - 1];
    let final_segment = &path[path.len() - 1];

    let mut current = json;
    for segment in parent_path {
        match segment {
            PathSegment::Key(key) => {
                if !current.is_object() {
                    *current = Value::Object(serde_json::Map::new());
                }
                let obj = current.as_object_mut().unwrap();
                current = obj
                    .entry(key.clone())
                    .or_insert(Value::Object(serde_json::Map::new()));
            }
            PathSegment::Index(idx) => {
                if !current.is_array() {
                    *current = Value::Array(Vec::new());
                }
                let arr = current.as_array_mut().unwrap();
                // Extend array if necessary
                while arr.len() <= *idx {
                    arr.push(Value::Null);
                }
                current = &mut arr[*idx];
            }
        }
    }

    // Set the final value
    match final_segment {
        PathSegment::Key(key) => {
            if !current.is_object() {
                *current = Value::Object(serde_json::Map::new());
            }
            let obj = current.as_object_mut().unwrap();
            obj.insert(key.clone(), value);
        }
        PathSegment::Index(idx) => {
            if !current.is_array() {
                *current = Value::Array(Vec::new());
            }
            let arr = current.as_array_mut().unwrap();
            // Extend array if necessary
            while arr.len() <= *idx {
                arr.push(Value::Null);
            }
            arr[*idx] = value;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_simple_key() {
        assert_eq!(
            parse_path("user").unwrap(),
            vec![PathSegment::Key("user".into())]
        );
    }

    #[test]
    fn test_parse_dot_notation() {
        assert_eq!(
            parse_path("a.b.c").unwrap(),
            vec![
                PathSegment::Key("a".into()),
                PathSegment::Key("b".into()),
                PathSegment::Key("c".into()),
            ]
        );
    }

    #[test]
    fn test_parse_array_index() {
        assert_eq!(
            parse_path("a[0]").unwrap(),
            vec![PathSegment::Key("a".into()), PathSegment::Index(0),]
        );
    }

    #[test]
    fn test_parse_mixed_path() {
        assert_eq!(
            parse_path("orders[0].items[1].price").unwrap(),
            vec![
                PathSegment::Key("orders".into()),
                PathSegment::Index(0),
                PathSegment::Key("items".into()),
                PathSegment::Index(1),
                PathSegment::Key("price".into()),
            ]
        );
    }

    #[test]
    fn test_parse_invalid_empty_index() {
        assert!(parse_path("a[]").is_err());
    }

    #[test]
    fn test_parse_invalid_consecutive_dots() {
        assert!(parse_path("a..b").is_err());
    }

    #[test]
    fn test_parse_invalid_unexpected_bracket() {
        assert!(parse_path("a]").is_err());
    }

    #[test]
    fn test_navigate_simple_path() {
        let data = json!({"user": {"name": "Alice"}});
        let path = parse_path("user.name").unwrap();
        assert_eq!(navigate_path(&data, &path), Some(&json!("Alice")));
    }

    #[test]
    fn test_navigate_array_path() {
        let data = json!({"items": ["a", "b", "c"]});
        let path = parse_path("items[1]").unwrap();
        assert_eq!(navigate_path(&data, &path), Some(&json!("b")));
    }

    #[test]
    fn test_navigate_nonexistent_path() {
        let data = json!({"user": {"name": "Alice"}});
        let path = parse_path("user.age").unwrap();
        assert_eq!(navigate_path(&data, &path), None);
    }

    #[test]
    fn test_navigate_invalid_array_access() {
        let data = json!({"user": {"name": "Alice"}});
        let path = parse_path("user[0]").unwrap();
        assert_eq!(navigate_path(&data, &path), None);
    }

    #[test]
    fn test_set_simple_path() {
        let mut data = json!({"user": {}});
        let path = parse_path("user.name").unwrap();
        set_path(&mut data, &path, json!("Alice")).unwrap();
        assert_eq!(data, json!({"user": {"name": "Alice"}}));
    }

    #[test]
    fn test_set_array_path() {
        let mut data = json!({"items": []});
        let path = parse_path("items[0]").unwrap();
        set_path(&mut data, &path, json!("first")).unwrap();
        assert_eq!(data, json!({"items": ["first"]}));
    }

    #[test]
    fn test_set_nested_path() {
        let mut data = json!({});
        let path = parse_path("user.profile.settings.theme").unwrap();
        set_path(&mut data, &path, json!("dark")).unwrap();
        assert_eq!(
            data,
            json!({"user": {"profile": {"settings": {"theme": "dark"}}}})
        );
    }
}
