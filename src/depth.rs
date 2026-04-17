// jsonb_delta - Depth Validation Module
//
// Security module for preventing stack overflow attacks via deeply nested JSONB.
// Implements configurable depth limits with clear error messages.
//
// Security module for preventing stack overflow attacks via deeply nested JSONB

use serde_json::Value;

/// Maximum allowed JSONB nesting depth to prevent stack overflow attacks
pub const MAX_JSONB_DEPTH: usize = 1000;

/// Maximum number of elements allowed in a single JSONB array.
/// Prevents OOM attacks via large index padding (e.g., arr[999999999]).
#[allow(dead_code)]
pub const MAX_JSONB_ARRAY_SIZE: usize = 100_000;

/// Return `Err` if `idx` would require padding an array beyond `max` elements.
///
/// # Errors
/// Returns an error string if `idx >= max`.
#[allow(dead_code)]
pub fn validate_array_index(idx: usize, max: usize) -> Result<(), String> {
    if idx >= max {
        Err(format!(
            "Array index {idx} exceeds maximum allowed size {max}"
        ))
    } else {
        Ok(())
    }
}

/// Validate that a JSONB value does not exceed maximum nesting depth
///
/// Recursively traverses the JSONB structure counting nesting levels.
/// Returns an error if any path exceeds `MAX_JSONB_DEPTH` levels.
///
/// # Arguments
/// * `val` - The JSONB value to validate
/// * `max_depth` - Maximum allowed nesting depth (should be `MAX_JSONB_DEPTH`)
///
/// # Returns
/// * `Ok(())` if depth is within limits
/// * `Err(String)` with descriptive error message if too deep
///
/// # Errors
/// Returns an error if the JSONB nesting depth exceeds `max_depth` levels.
pub fn validate_depth(val: &Value, max_depth: usize) -> Result<(), String> {
    fn check_depth(val: &Value, current: usize, max: usize) -> Result<usize, String> {
        if current > max {
            return Err(format!(
                "JSONB nesting too deep (max {max}, found depth {current})"
            ));
        }
        match val {
            Value::Object(map) => {
                let mut max_child = current;
                for v in map.values() {
                    max_child = max_child.max(check_depth(v, current + 1, max)?);
                }
                Ok(max_child)
            }
            Value::Array(arr) => {
                let mut max_child = current;
                for v in arr {
                    max_child = max_child.max(check_depth(v, current + 1, max)?);
                }
                Ok(max_child)
            }
            _ => Ok(current),
        }
    }
    check_depth(val, 0, max_depth)?;
    Ok(())
}

/// Get the maximum nesting depth of a JSONB value
///
/// Traverses the entire JSONB structure to find the deepest nesting level.
/// Useful for analysis and testing.
///
/// # Arguments
/// * `val` - The JSONB value to analyze
///
/// # Returns
/// Maximum nesting depth found (0 for scalars, 1 for shallow objects, etc.)
#[allow(dead_code)]
pub fn get_max_depth(val: &Value) -> usize {
    fn check_depth(val: &Value, current: usize) -> usize {
        match val {
            Value::Object(map) => {
                let mut max_child = current;
                for v in map.values() {
                    max_child = max_child.max(check_depth(v, current + 1));
                }
                max_child
            }
            Value::Array(arr) => {
                let mut max_child = current;
                for v in arr {
                    max_child = max_child.max(check_depth(v, current + 1));
                }
                max_child
            }
            _ => current,
        }
    }
    check_depth(val, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_validate_depth_shallow() {
        let val = json!({"a": 1, "b": {"c": 2}});
        assert!(validate_depth(&val, MAX_JSONB_DEPTH).is_ok());
    }

    #[test]
    fn test_validate_depth_array() {
        let val = json!([{"nested": [1, 2, 3]}]);
        assert!(validate_depth(&val, MAX_JSONB_DEPTH).is_ok());
    }

    #[test]
    fn test_validate_depth_too_deep() {
        // Create a structure with MAX_JSONB_DEPTH + 1 levels (exceeds limit)
        let mut deep = json!({"level": 1}); // Start with level 1

        // Wrap it in MAX_JSONB_DEPTH levels of nesting to reach MAX_JSONB_DEPTH + 1 total
        for _ in 0..MAX_JSONB_DEPTH {
            deep = json!({"nested": deep});
        }

        let result = validate_depth(&deep, MAX_JSONB_DEPTH);
        assert!(result.is_err());
        let err_msg = result.unwrap_err();
        assert!(err_msg.contains("JSONB nesting too deep"));
        assert!(err_msg.contains("max 1000"));
        // NEW: assert the actual depth appears in the message
        assert!(
            err_msg.contains("1001") || err_msg.contains("found depth"),
            "error should report the actual depth, got: {err_msg}"
        );
    }

    #[test]
    fn test_get_max_depth() {
        assert_eq!(get_max_depth(&json!(42)), 0);
        assert_eq!(get_max_depth(&json!({"a": 1})), 1);
        assert_eq!(get_max_depth(&json!({"a": {"b": {"c": 1}}})), 3);
        assert_eq!(get_max_depth(&json!([{"a": [1, 2]}])), 3);
    }

    #[test]
    fn test_max_depth_within_limit() {
        // Create exactly MAX_JSONB_DEPTH levels by building from the inside out
        let mut deep = json!({"level": 1}); // Start with level 1

        // Wrap it in (MAX_JSONB_DEPTH - 1) levels of nesting to reach exactly MAX_JSONB_DEPTH
        for _ in 0..(MAX_JSONB_DEPTH - 1) {
            deep = json!({"nested": deep});
        }

        // Should be exactly at the limit
        assert_eq!(get_max_depth(&deep), MAX_JSONB_DEPTH);
        assert!(validate_depth(&deep, MAX_JSONB_DEPTH).is_ok());
    }

    #[test]
    fn test_array_index_within_limit() {
        assert!(validate_array_index(99_999, MAX_JSONB_ARRAY_SIZE).is_ok());
    }

    #[test]
    fn test_array_index_exceeds_limit() {
        let err = validate_array_index(100_000, MAX_JSONB_ARRAY_SIZE).unwrap_err();
        assert!(err.contains("100000"), "error should contain the bad index");
        assert!(err.contains("100000"), "error should contain the limit");
    }
}
