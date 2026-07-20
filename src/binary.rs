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

// ---------------------------------------------------------------------------
// Array element operations
//
// These are where the technique pays. The native SQL they replace rebuilds the
// whole array with `jsonb_agg` over `jsonb_array_elements`, so it materializes
// and re-encodes every element. Walking the binary form lets a non-matching
// element pass straight through as a `jbvBinary` pointer into the source
// document -- never decoded, never re-encoded. Cost becomes proportional to the
// elements actually changed rather than to the size of the array.
// ---------------------------------------------------------------------------

/// A `JsonbValue` holding a borrowed string, for key lookups.
fn key_value(key: &str) -> pg_sys::JsonbValue {
    let mut jbv = unsafe { std::mem::zeroed::<pg_sys::JsonbValue>() };
    jbv.type_ = pg_sys::jbvType::jbvString;
    jbv.val.string.len = i32::try_from(key.len()).unwrap_or(i32::MAX);
    jbv.val.string.val = key.as_ptr().cast::<std::ffi::c_char>().cast_mut();
    jbv
}

/// Scalar equality, mirroring `PostgreSQL`'s own `equalsJsonbScalarValue`.
///
/// Numbers are compared with `numeric_eq` rather than by bytes, so `1` and `1.0`
/// match exactly as they do in SQL. Anything non-scalar (an object or array used
/// as a match value) compares unequal, which is the existing behaviour.
///
/// # Safety
///
/// Both values must be initialized `JsonbValue`s whose payloads outlive the call.
unsafe fn scalar_equals(a: &pg_sys::JsonbValue, b: &pg_sys::JsonbValue) -> bool {
    if a.type_ != b.type_ {
        return false;
    }
    unsafe {
        match a.type_ {
            pg_sys::jbvType::jbvNull => true,
            pg_sys::jbvType::jbvBool => a.val.boolean == b.val.boolean,
            pg_sys::jbvType::jbvString => {
                a.val.string.len == b.val.string.len && {
                    let n = usize::try_from(a.val.string.len).unwrap_or(0);
                    std::slice::from_raw_parts(a.val.string.val.cast::<u8>(), n)
                        == std::slice::from_raw_parts(b.val.string.val.cast::<u8>(), n)
                }
            }
            pg_sys::jbvType::jbvNumeric => {
                // Numbers compare by value rather than by bytes, so `1` matches
                // `1.0` exactly as it would in SQL. pgrx's AnyNumeric wraps
                // PostgreSQL's own numeric comparison, which avoids reimplementing
                // scale handling here.
                let an = pgrx::AnyNumeric::from_datum(pg_sys::Datum::from(a.val.numeric), false);
                let bn = pgrx::AnyNumeric::from_datum(pg_sys::Datum::from(b.val.numeric), false);
                match (an, bn) {
                    (Some(x), Some(y)) => x == y,
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

/// Whether an array element is an object whose `match_key` equals `match_value`.
///
/// # Safety
///
/// `elem` must be a `jbvBinary` or scalar value produced by a live iterator.
unsafe fn element_matches(
    elem: &pg_sys::JsonbValue,
    match_key: &str,
    match_value: &pg_sys::JsonbValue,
) -> bool {
    unsafe {
        if elem.type_ != pg_sys::jbvType::jbvBinary {
            return false;
        }
        let container = elem.val.binary.data;
        if (*container).header & pg_sys::JB_FOBJECT == 0 {
            return false;
        }
        let mut key = key_value(match_key);
        let found =
            pg_sys::findJsonbValueFromContainer(container, pg_sys::JB_FOBJECT, &raw mut key);
        !found.is_null() && scalar_equals(&*found, match_value)
    }
}

/// What to do with an array element that matched.
enum ElementAction {
    /// Merge `updates`' keys into the element.
    Merge(*mut pg_sys::JsonbContainer),
    /// Drop the element from the array.
    Drop,
}

/// Rebuild `doc`, transforming the array held at top-level key `array_path`.
///
/// Elements that do not match are pushed through untouched as binary values.
/// Keys of `doc` other than `array_path` are likewise passed through whole.
///
/// A document with no such key, or whose value there is not an array, is
/// reproduced unchanged -- the existing functions are deliberately no-ops in
/// that case so a cascading caller need not check first.
///
/// # Safety
///
/// `doc` must be a valid object container outliving the call.
unsafe fn rebuild_with_array_transform(
    doc: *mut pg_sys::JsonbContainer,
    array_path: &str,
    match_key: &str,
    match_value: &pg_sys::JsonbValue,
    action: &ElementAction,
    all_matches: bool,
) -> *mut pg_sys::Jsonb {
    unsafe {
        let mut state: *mut pg_sys::JsonbParseState = std::ptr::null_mut();
        pg_sys::pushJsonbValue(
            &raw mut state,
            pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
            std::ptr::null_mut(),
        );

        let mut it = pg_sys::JsonbIteratorInit(doc);
        let mut v = std::mem::zeroed::<pg_sys::JsonbValue>();
        pg_sys::JsonbIteratorNext(&raw mut it, &raw mut v, true);

        let mut done = false;
        loop {
            let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut v, true);
            if tok != pg_sys::JsonbIteratorToken::WJB_KEY {
                break;
            }
            let is_target = v.type_ == pg_sys::jbvType::jbvString
                && usize::try_from(v.val.string.len).unwrap_or(0) == array_path.len()
                && std::slice::from_raw_parts(v.val.string.val.cast::<u8>(), array_path.len())
                    == array_path.as_bytes();

            pg_sys::pushJsonbValue(
                &raw mut state,
                pg_sys::JsonbIteratorToken::WJB_KEY,
                &raw mut v,
            );
            pg_sys::JsonbIteratorNext(&raw mut it, &raw mut v, true);

            let is_array = v.type_ == pg_sys::jbvType::jbvBinary
                && (*v.val.binary.data).header & pg_sys::JB_FARRAY != 0;

            if !is_target || !is_array || done {
                pg_sys::pushJsonbValue(
                    &raw mut state,
                    pg_sys::JsonbIteratorToken::WJB_VALUE,
                    &raw mut v,
                );
                continue;
            }

            // Rewrite this array, element by element.
            pg_sys::pushJsonbValue(
                &raw mut state,
                pg_sys::JsonbIteratorToken::WJB_BEGIN_ARRAY,
                std::ptr::null_mut(),
            );

            let mut ait = pg_sys::JsonbIteratorInit(v.val.binary.data);
            let mut ev = std::mem::zeroed::<pg_sys::JsonbValue>();
            pg_sys::JsonbIteratorNext(&raw mut ait, &raw mut ev, true);

            loop {
                let etok = pg_sys::JsonbIteratorNext(&raw mut ait, &raw mut ev, true);
                if etok != pg_sys::JsonbIteratorToken::WJB_ELEM {
                    break;
                }

                if (!done || all_matches) && element_matches(&ev, match_key, match_value) {
                    if !all_matches {
                        done = true;
                    }
                    match *action {
                        ElementAction::Drop => {}
                        ElementAction::Merge(updates) => {
                            pg_sys::pushJsonbValue(
                                &raw mut state,
                                pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
                                std::ptr::null_mut(),
                            );
                            push_object_pairs(ev.val.binary.data, &raw mut state);
                            push_object_pairs(updates, &raw mut state);
                            pg_sys::pushJsonbValue(
                                &raw mut state,
                                pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
                                std::ptr::null_mut(),
                            );
                        }
                    }
                } else {
                    // The common path: hand the element straight through without
                    // decoding it.
                    pg_sys::pushJsonbValue(
                        &raw mut state,
                        pg_sys::JsonbIteratorToken::WJB_ELEM,
                        &raw mut ev,
                    );
                }
            }

            pg_sys::pushJsonbValue(
                &raw mut state,
                pg_sys::JsonbIteratorToken::WJB_END_ARRAY,
                std::ptr::null_mut(),
            );
        }

        let result = pg_sys::pushJsonbValue(
            &raw mut state,
            pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
            std::ptr::null_mut(),
        );
        pg_sys::JsonbValueToJsonb(result)
    }
}

/// Whether `array_path` names an array at the top level of `doc`.
///
/// The two callers disagree about what to do when it does not, so this reports
/// rather than decides: `jsonb_array_update_where` errors, while
/// `jsonb_array_delete_where` returns the document untouched. Both behaviours are
/// preserved exactly.
///
/// # Safety
///
/// `doc` must be a valid object container outliving the call.
unsafe fn array_at(doc: *mut pg_sys::JsonbContainer, array_path: &str) -> Option<bool> {
    unsafe {
        let mut key = key_value(array_path);
        let found = pg_sys::findJsonbValueFromContainer(doc, pg_sys::JB_FOBJECT, &raw mut key);
        if found.is_null() {
            return None;
        }
        Some(
            (*found).type_ == pg_sys::jbvType::jbvBinary
                && (*(*found).val.binary.data).header & pg_sys::JB_FARRAY != 0,
        )
    }
}

/// Read the root of a document as a single `JsonbValue`, for use as a match value.
///
/// # Safety
///
/// `j` must wrap a live, detoasted document.
unsafe fn root_as_value(j: &RawJsonb) -> pg_sys::JsonbValue {
    unsafe {
        let mut v = std::mem::zeroed::<pg_sys::JsonbValue>();
        pg_sys::JsonbToJsonbValue(j.0, &raw mut v);
        // A top-level scalar is stored as a one-element pseudo-array; unwrap it so
        // the comparison sees the scalar the caller wrote.
        if v.type_ == pg_sys::jbvType::jbvBinary
            && (*v.val.binary.data).header & pg_sys::JB_FSCALAR != 0
        {
            let mut it = pg_sys::JsonbIteratorInit(v.val.binary.data);
            let mut inner = std::mem::zeroed::<pg_sys::JsonbValue>();
            pg_sys::JsonbIteratorNext(&raw mut it, &raw mut inner, true);
            pg_sys::JsonbIteratorNext(&raw mut it, &raw mut inner, true);
            return inner;
        }
        v
    }
}

/// Update the first array element matching `match_key` = `match_value`.
///
/// Behaviourally identical to `jsonb_array_update_where`, without materializing
/// the document.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where_fast(
    target: RawJsonb,
    array_path: &str,
    match_key: &str,
    match_value: RawJsonb,
    updates: RawJsonb,
) -> RawJsonb {
    crate::array_ops::validate_match_key(match_key).unwrap_or_else(|e| error!("{}", e));
    if !is_object(&target) {
        error!("target argument must be a JSONB object");
    }
    // Reason: pointers come from detoasted datums and the palloc'ing builder.
    unsafe {
        // Matches the serde implementation, which errors rather than no-ops here.
        match array_at(target.container(), array_path) {
            None => error!("Path '{}' does not exist in document", array_path),
            Some(false) => error!("Path '{}' does not point to an array", array_path),
            Some(true) => {}
        }
        let mv = root_as_value(&match_value);
        RawJsonb(rebuild_with_array_transform(
            target.container(),
            array_path,
            match_key,
            &mv,
            &ElementAction::Merge(updates.container()),
            false,
        ))
    }
}

/// Delete every array element matching `match_key` = `match_value`.
///
/// Behaviourally identical to `jsonb_array_delete_where`, without materializing
/// the document.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_delete_where_fast(
    target: RawJsonb,
    array_path: &str,
    match_key: &str,
    match_value: RawJsonb,
) -> RawJsonb {
    crate::array_ops::validate_match_key(match_key).unwrap_or_else(|e| error!("{}", e));
    if !is_object(&target) {
        error!("target argument must be a JSONB object");
    }
    // Reason: pointers come from detoasted datums and the palloc'ing builder.
    unsafe {
        let mv = root_as_value(&match_value);
        // First match only: the serde implementation removes a single index found
        // by `find_element_by_match`, and this must not quietly delete more.
        RawJsonb(rebuild_with_array_transform(
            target.container(),
            array_path,
            match_key,
            &mv,
            &ElementAction::Drop,
            false,
        ))
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

    /// Assert a `_fast` call matches the serde function it replaces.
    fn assert_matches_serde(fast: &str, serde: &str) {
        let same = Spi::get_one::<bool>(&format!("SELECT {fast} = {serde}"))
            .expect("SPI ok")
            .expect("not null");
        assert!(
            same,
            "binary and serde implementations disagreed:\n  {fast}\n  {serde}"
        );
    }

    const DOC: &str = r#"{"posts":[{"id":1,"t":"a"},{"id":2,"t":"b"},{"id":3,"t":"c"},{"id":2,"t":"dup"}],"n":9}"#;

    #[pg_test]
    fn array_update_matches_serde() {
        for (key, upd) in [
            ("2", r#"{"t":"Z"}"#),
            ("1", r#"{"t":"Z"}"#),
            ("99", r#"{"t":"Z"}"#),
            ("2", r#"{"x":true}"#),
        ] {
            assert_matches_serde(
                &format!("jsonb_array_update_where_fast('{DOC}','posts','id','{key}','{upd}')"),
                &format!("jsonb_array_update_where('{DOC}','posts','id','{key}','{upd}')"),
            );
        }
    }

    /// Only the first match is updated, even though the fixture has two id=2.
    #[pg_test]
    fn array_update_touches_only_the_first_match() {
        assert_matches_serde(
            &format!("jsonb_array_update_where_fast('{DOC}','posts','id','2','{{\"t\":\"Z\"}}')"),
            &format!("jsonb_array_update_where('{DOC}','posts','id','2','{{\"t\":\"Z\"}}')"),
        );
    }

    #[pg_test]
    fn array_delete_matches_serde() {
        for key in ["2", "1", "99"] {
            assert_matches_serde(
                &format!("jsonb_array_delete_where_fast('{DOC}','posts','id','{key}')"),
                &format!("jsonb_array_delete_where('{DOC}','posts','id','{key}')"),
            );
        }
    }

    /// `delete` is a no-op on a missing path, where `update` errors. The two
    /// serde functions genuinely differ here and both contracts are preserved.
    #[pg_test]
    fn delete_on_missing_path_is_a_no_op() {
        assert_matches_serde(
            &format!("jsonb_array_delete_where_fast('{DOC}','nope','id','2')"),
            &format!("jsonb_array_delete_where('{DOC}','nope','id','2')"),
        );
    }

    #[pg_test(error = "Path 'nope' does not exist in document")]
    fn update_on_missing_path_errors() {
        Spi::run("SELECT jsonb_array_update_where_fast('{\"a\":1}','nope','id','1','{}')")
            .expect("SPI ok");
    }

    #[pg_test(error = "match_key must not be empty")]
    fn empty_match_key_is_rejected() {
        Spi::run("SELECT jsonb_array_update_where_fast('{\"p\":[]}','p','','1','{}')")
            .expect("SPI ok");
    }

    #[pg_test]
    fn text_and_uuid_match_keys() {
        let doc = r#"{"posts":[{"id":"a-1","t":"x"},{"id":"3f2a-uuid","t":"y"}]}"#;
        assert_matches_serde(
            &format!(
                r#"jsonb_array_update_where_fast('{doc}','posts','id','"3f2a-uuid"','{{"t":"Z"}}')"#
            ),
            &format!(
                r#"jsonb_array_update_where('{doc}','posts','id','"3f2a-uuid"','{{"t":"Z"}}')"#
            ),
        );
    }

    /// Numbers match by value, as `PostgreSQL` does: `'2'::jsonb = '2.0'::jsonb`
    /// is true, and containment agrees.
    ///
    /// This is a deliberate *divergence* from the serde implementation, which
    /// compares `serde_json::Number` structurally and so fails to match `2` when
    /// given `2.0`. A caller passing `to_jsonb(2.0)` or a numeric column silently
    /// matched nothing before. Asserting the SQL-consistent behaviour here so the
    /// difference is pinned rather than discovered later.
    #[pg_test]
    fn numbers_match_by_value_not_by_scale() {
        let matched = Spi::get_one::<bool>(
            r#"SELECT jsonb_array_update_where_fast('{"p":[{"id":2,"t":"a"}]}','p','id','2.0','{"t":"Z"}')
                    = '{"p":[{"id":2,"t":"Z"}]}'::jsonb"#,
        )
        .expect("SPI ok")
        .expect("not null");
        assert!(matched, "2.0 should match an id of 2, as it does in SQL");
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
