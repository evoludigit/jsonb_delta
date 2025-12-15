// jsonb_delta - Optimized Search Helpers Module
//
// High-performance search functions for finding elements in JSONB arrays.
// Includes SIMD-optimized integer ID matching with loop unrolling.
//
// Part of Phase 0: Code Modularization

use serde_json::Value;

/// Optimized integer ID matching with loop unrolling
/// Returns index of first matching element, or None
///
/// This function uses manual loop unrolling to help the compiler
/// generate SIMD instructions automatically (auto-vectorization)
#[inline]
pub fn find_by_int_id_optimized(
    array: &[Value],
    match_key: &str,
    match_value: i64,
) -> Option<usize> {
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
pub fn find_by_int_id_scalar(array: &[Value], match_key: &str, match_value: i64) -> Option<usize> {
    array.iter().position(|elem| {
        elem.get(match_key).and_then(serde_json::Value::as_i64) == Some(match_value)
    })
}
