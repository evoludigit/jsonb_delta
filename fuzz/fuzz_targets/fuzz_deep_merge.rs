#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fn deep_merge(base: &mut Value, update: &Value) {
    match (base, update) {
        (Value::Object(base_obj), Value::Object(update_obj)) => {
            for (key, value) in update_obj {
                if let Some(base_value) = base_obj.get_mut(key) {
                    deep_merge(base_value, value);
                } else {
                    base_obj.insert(key.clone(), value.clone());
                }
            }
        }
        (base, update) => {
            *base = update.clone();
        }
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }

    let split = data.len() / 2;
    let (data1, data2) = data.split_at(split);

    if let (Ok(mut v1), Ok(v2)) = (
        serde_json::from_slice::<Value>(data1),
        serde_json::from_slice::<Value>(data2),
    ) {
        // Test deep merge doesn't crash
        deep_merge(&mut v1, &v2);

        // Verify result is valid JSON
        let _ = serde_json::to_string(&v1);

        // Test with nested structures
        if v1.is_object() && v2.is_object() {
            let serialized = serde_json::to_string(&v1).unwrap();
            let deserialized: Value = serde_json::from_str(&serialized).unwrap();
            assert_eq!(v1, deserialized, "Round-trip serialization failed");
        }
    }
});
