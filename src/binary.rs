//! Spike: operate on `PostgreSQL`'s binary JSONB directly, without materializing
//! the document.
//!
//! # Why this exists
//!
//! Measurement (`benchmarks/2026-07-20-issue15-ccx13.md`) established that
//! essentially 100% of this extension's cost is a document round trip, and that
//! the element matching and mutation the extension actually does is free by
//! comparison. On a 1000-element document, a no-op that parses and re-serializes
//! costs 2.45 ms while a real update costs 2.25 ms -- the work is not measurable
//! next to the conversion.
//!
//! The conversion is worse than "serde is slow". `pgrx::JsonB` converts by
//! calling `PostgreSQL`'s `jsonb_out` to render the document as *text*, parsing
//! that text with `serde_json`, and reversing both steps on the way out. That is
//! four passes over the document, two of them through a text representation that
//! nothing needs.
//!
//! `PostgreSQL`'s own operators do none of this: `||` merges two documents in
//! 0.25 ms on the same input, ~10x faster than our no-op.
//!
//! # Why `unsafe` is justified here
//!
//! There is no safe route to this. `pgrx` models `jsonb` only as
//! `JsonB(serde_json::Value)`, so the binary representation is reachable only
//! through the C API (`JsonbIteratorInit`, `JsonbIteratorNext`, `pushJsonbValue`,
//! `JsonbValueToJsonb`). This module is the only place in the crate that uses
//! `unsafe`, and it is confined to walking and rebuilding a container.
//!
//! Correctness is not asserted on the strength of this reasoning: the benchmark
//! harness compares this function's output byte-for-byte against the native `||`
//! operator across the whole size sweep and refuses to report a ratio if they
//! ever differ.

use pgrx::pg_sys;
use pgrx::pgrx_sql_entity_graph::metadata::{
    ArgumentError, Returns, ReturnsError, SqlMapping, SqlTranslatable,
};
use pgrx::prelude::*;

/// A `jsonb` argument kept in `PostgreSQL`'s binary form.
///
/// Deliberately does *not* implement `Deref` to `serde_json::Value`: the entire
/// point is that the document is never materialized.
pub struct RawJsonb(*mut pg_sys::Jsonb);

impl RawJsonb {
    /// The container at the root of the document.
    fn container(&self) -> *mut pg_sys::JsonbContainer {
        // Reason: `self.0` came from `pg_detoast_datum` in `from_polymorphic_datum`
        // and is a fully detoasted, aligned `Jsonb`, so `root` is in bounds.
        unsafe { &raw mut (*self.0).root }
    }
}

impl FromDatum for RawJsonb {
    unsafe fn from_polymorphic_datum(
        datum: pg_sys::Datum,
        is_null: bool,
        _: pg_sys::Oid,
    ) -> Option<Self> {
        if is_null {
            return None;
        }
        // `pg_detoast_datum` rather than `..._packed`: the packed form may carry a
        // 1-byte varlena header, and the `Jsonb` layout assumes the 4-byte one.
        let detoasted = unsafe { pg_sys::pg_detoast_datum(datum.cast_mut_ptr()) };
        Some(Self(detoasted.cast()))
    }
}

impl IntoDatum for RawJsonb {
    fn into_datum(self) -> Option<pg_sys::Datum> {
        Some(pg_sys::Datum::from(self.0))
    }

    fn type_oid() -> pg_sys::Oid {
        pg_sys::JSONBOID
    }
}

// Reason: the safety contract is that the SQL type named here matches the Rust
// type's datum representation. `RawJsonb` is a `*mut Jsonb`, which is exactly
// what a `jsonb` Datum is.
unsafe impl SqlTranslatable for RawJsonb {
    fn argument_sql() -> Result<SqlMapping, ArgumentError> {
        Ok(SqlMapping::literal("jsonb"))
    }
    fn return_sql() -> Result<Returns, ReturnsError> {
        Ok(Returns::One(SqlMapping::literal("jsonb")))
    }
}

// pgrx generates the C entry point from these two traits. Both are normally
// supplied by macros private to pgrx, so they are written out here; each simply
// defers to the `FromDatum` / `IntoDatum` impls above, exactly as pgrx's own
// `argue_from_datum!` and `impl_repackage_into_datum!` do for `JsonB`.
//
// Reason: `unsafe impl` is required by the traits themselves. The obligation is
// that the datum handed over really is of the SQL type declared in
// `SqlTranslatable`, which holds because both sides say `jsonb`.
unsafe impl<'fcx> pgrx::callconv::ArgAbi<'fcx> for RawJsonb {
    unsafe fn unbox_arg_unchecked(arg: pgrx::callconv::Arg<'_, 'fcx>) -> Self {
        let index = arg.index();
        unsafe {
            arg.unbox_arg_using_from_datum()
                .unwrap_or_else(|| panic!("argument {index} must not be null"))
        }
    }
}

unsafe impl pgrx::callconv::BoxRet for RawJsonb {
    unsafe fn box_into<'fcx>(
        self,
        fcinfo: &mut pgrx::callconv::FcInfo<'fcx>,
    ) -> pgrx::datum::Datum<'fcx> {
        match self.into_datum() {
            Some(datum) => unsafe { fcinfo.return_raw_datum(datum) },
            None => fcinfo.return_null(),
        }
    }
}

/// Copy every key/value pair of an object container into `state`.
///
/// `skipNested` is true, so a nested object or array arrives as a single
/// `jbvBinary` value pointing into the source document and is pushed through
/// without being walked. That is what makes this shallow merge O(top-level keys)
/// rather than O(document).
///
/// # Safety
///
/// `container` must point to a valid jsonb object container that outlives the
/// call, and `state` must be a valid parse state positioned inside an object.
unsafe fn push_object_pairs(
    container: *mut pg_sys::JsonbContainer,
    state: *mut *mut pg_sys::JsonbParseState,
) {
    unsafe {
        let mut it = pg_sys::JsonbIteratorInit(container);
        let mut v = std::mem::zeroed::<pg_sys::JsonbValue>();

        // Consume the opening WJB_BEGIN_OBJECT.
        pg_sys::JsonbIteratorNext(&raw mut it, &raw mut v, true);

        loop {
            let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut v, true);
            if tok != pg_sys::JsonbIteratorToken::WJB_KEY {
                break;
            }
            pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_KEY, &raw mut v);

            pg_sys::JsonbIteratorNext(&raw mut it, &raw mut v, true);
            pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_VALUE, &raw mut v);
        }
    }
}

/// True when the container at the root of `j` is a JSONB object.
fn is_object(j: &RawJsonb) -> bool {
    // Reason: JB_FOBJECT is a bit in the container header; reading it requires
    // dereferencing the container pointer obtained above.
    unsafe { (*j.container()).header & pg_sys::JB_FOBJECT != 0 }
}

/// Shallow-merge two JSONB objects without materializing either one.
///
/// Semantically identical to `jsonb_merge_shallow`, and to the native `||`
/// operator for two objects: keys from `source` replace keys from `target`,
/// nested values are replaced wholesale rather than merged.
// Reason: `#[pg_extern]` requires owned arguments -- pgrx builds the C entry
// point from the by-value signature, so a reference is not expressible here.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_merge_shallow_fast(target: RawJsonb, source: RawJsonb) -> RawJsonb {
    if !is_object(&target) {
        error!("target argument must be a JSONB object");
    }
    if !is_object(&source) {
        error!("source argument must be a JSONB object");
    }

    // Reason: every pointer below is either freshly obtained from a detoasted
    // datum or produced by the palloc'ing jsonb builder, and none escape the
    // current memory context.
    unsafe {
        let mut state: *mut pg_sys::JsonbParseState = std::ptr::null_mut();
        pg_sys::pushJsonbValue(
            &raw mut state,
            pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
            std::ptr::null_mut(),
        );

        // Target first, then source. Duplicate keys are resolved when the object
        // is closed, and the later push wins -- the same rule `||` follows, and
        // the reason source overrides target without an explicit lookup.
        push_object_pairs(target.container(), &raw mut state);
        push_object_pairs(source.container(), &raw mut state);

        let result = pg_sys::pushJsonbValue(
            &raw mut state,
            pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
            std::ptr::null_mut(),
        );

        RawJsonb(pg_sys::JsonbValueToJsonb(result))
    }
}

/// Differential tests against the native `||` operator.
///
/// Every case asserts equality with `||` rather than against a hand-written
/// expected document. That is deliberate: `||` is `PostgreSQL`'s own
/// implementation of this exact operation, so it is a stronger oracle than any
/// literal a test author would write, and it keeps the tests honest about the
/// duplicate-key and key-ordering rules this code relies on rather than
/// restating this author's belief about them.
#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;

    /// Assert `jsonb_merge_shallow_fast(a, b)` matches `a || b`.
    fn assert_matches_concat(a: &str, b: &str) {
        let same = Spi::get_one::<bool>(&format!(
            "SELECT jsonb_merge_shallow_fast('{a}'::jsonb, '{b}'::jsonb) = '{a}'::jsonb || '{b}'::jsonb"
        ))
        .expect("SPI ok")
        .expect("not null");
        assert!(same, "binary merge disagreed with `||` for {a} || {b}");
    }

    #[pg_test]
    fn matches_concat_for_disjoint_keys() {
        assert_matches_concat(r#"{"a":1}"#, r#"{"b":2}"#);
    }

    #[pg_test]
    fn source_key_wins_on_collision() {
        assert_matches_concat(r#"{"a":1,"b":2}"#, r#"{"b":9,"c":3}"#);
    }

    #[pg_test]
    fn nested_values_are_replaced_not_merged() {
        assert_matches_concat(r#"{"a":{"x":1},"b":2}"#, r#"{"a":{"y":2}}"#);
    }

    #[pg_test]
    fn arrays_pass_through_untouched() {
        assert_matches_concat(r#"{"a":[1,2,3],"b":{"c":[4]}}"#, r#"{"d":[5]}"#);
    }

    #[pg_test]
    fn empty_operands() {
        assert_matches_concat(r#"{"a":1}"#, "{}");
        assert_matches_concat("{}", r#"{"a":1}"#);
        assert_matches_concat("{}", "{}");
    }

    #[pg_test]
    fn json_null_is_a_value_not_a_deletion() {
        assert_matches_concat(r#"{"a":1}"#, r#"{"a":null}"#);
    }

    #[pg_test]
    fn non_ascii_keys_and_values() {
        assert_matches_concat(r#"{"café":1,"日本":2}"#, r#"{"café":"newé"}"#);
    }

    /// Keys are stored in a length-then-bytes order internally, so a merge that
    /// introduces keys of varying length exercises the re-sort on close.
    #[pg_test]
    fn many_keys_of_differing_length() {
        let a: String = (0..64)
            .map(|i| format!(r#""k{}":{i}"#, "x".repeat(i % 17)))
            .collect::<Vec<_>>()
            .join(",");
        let b: String = (0..64)
            .map(|i| format!(r#""k{}":{}"#, "x".repeat(i % 13), i + 1000))
            .collect::<Vec<_>>()
            .join(",");
        assert_matches_concat(&format!("{{{a}}}"), &format!("{{{b}}}"));
    }

    #[pg_test]
    fn agrees_with_the_serde_implementation_it_replaces() {
        let same = Spi::get_one::<bool>(
            r#"SELECT jsonb_merge_shallow_fast('{"a":1,"b":{"n":1}}'::jsonb, '{"b":2,"c":3}'::jsonb)
                    = jsonb_merge_shallow('{"a":1,"b":{"n":1}}'::jsonb, '{"b":2,"c":3}'::jsonb)"#,
        )
        .expect("SPI ok")
        .expect("not null");
        assert!(same, "binary merge disagreed with the serde implementation");
    }
}
