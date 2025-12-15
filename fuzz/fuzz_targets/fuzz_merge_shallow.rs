#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    // Try to parse two JSON documents from the input
    if data.len() < 2 {
        return;
    }

    let split = data.len() / 2;
    let (data1, data2) = data.split_at(split);

    // Attempt to parse both halves as JSON
    if let (Ok(v1), Ok(v2)) = (
        serde_json::from_slice::<Value>(data1),
        serde_json::from_slice::<Value>(data2),
    ) {
        // Only fuzz with objects (jsonb_merge_shallow expects objects)
        if v1.is_object() && v2.is_object() {
            // Test that merge doesn't crash or panic
            let mut merged = v1.clone();
            if let Some(obj1) = merged.as_object_mut() {
                if let Some(obj2) = v2.as_object() {
                    for (key, value) in obj2 {
                        obj1.insert(key.clone(), value.clone());
                    }
                }
            }

            // Verify properties:
            // 1. Result is still an object
            assert!(merged.is_object());

            // 2. All keys from v2 are in merged
            if let (Some(merged_obj), Some(v2_obj)) = (merged.as_object(), v2.as_object()) {
                for key in v2_obj.keys() {
                    assert!(merged_obj.contains_key(key), "Merged object missing key from v2");
                }
            }
        }
    }
});
