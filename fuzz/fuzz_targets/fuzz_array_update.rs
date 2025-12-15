#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    // Parse input as JSON
    if let Ok(mut value) = serde_json::from_slice::<Value>(data) {
        // Only fuzz arrays
        if let Some(arr) = value.as_array_mut() {
            if arr.is_empty() {
                return;
            }

            // Test various array operations
            let len_before = arr.len();

            // 1. Test iteration (shouldn't crash)
            for item in arr.iter() {
                let _ = item.is_object();
            }

            // 2. Test element access
            if !arr.is_empty() {
                let _ = &arr[0];
                let _ = &arr[arr.len() - 1];
            }

            // 3. Test modification
            if let Some(first) = arr.first_mut() {
                if first.is_object() {
                    if let Some(obj) = first.as_object_mut() {
                        obj.insert("_fuzz_test".to_string(), Value::Bool(true));
                    }
                }
            }

            // 4. Verify array integrity
            assert_eq!(arr.len(), len_before, "Array length changed unexpectedly");
        }
    }
});
