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
fn jsonb_merge_shallow(target: RawJsonb, source: RawJsonb) -> RawJsonb {
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
fn jsonb_array_update_where(
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
fn jsonb_array_delete_where(
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

// ---------------------------------------------------------------------------
// The pg_tviews-facing wrappers
//
// These are thin: `smart_patch_scalar` is a shallow merge and `smart_patch_array`
// is a first-match element merge, so both reduce to machinery already above. The
// argument ORDER differs from `jsonb_array_update_where` (source comes second
// here), which is the kind of detail that makes a hand-written duplicate a
// liability -- hence the delegation.
// ---------------------------------------------------------------------------

/// Root-level shallow merge, without materializing either document.
///
/// Behaviourally identical to `jsonb_smart_patch_scalar`, which is itself defined
/// as `jsonb_merge_shallow`.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_smart_patch_scalar(target: RawJsonb, source: RawJsonb) -> RawJsonb {
    jsonb_merge_shallow(target, source)
}

/// Merge `source` into the first array element matching `match_key`.
///
/// Behaviourally identical to `jsonb_smart_patch_array`, including its contract
/// of erroring rather than no-oping when the path is absent or is not an array.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_smart_patch_array(
    target: RawJsonb,
    source: RawJsonb,
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
            &ElementAction::Merge(source.container()),
            false,
        ))
    }
}

/// Apply many keyed updates to one array in a single pass.
///
/// Behaviourally identical to `jsonb_array_update_where_batch`: specs are
/// `{"match_value": ..., "updates": {...}}`, malformed specs are skipped, a
/// missing path / non-array path / non-array spec list each raise, and *every*
/// element matching a spec is updated rather than only the first.
///
/// One deliberate extension: the serde version reads `match_value` with `as_i64`
/// and silently drops anything else, so text and UUID keys could not be batched
/// at all. Matching here goes through the same scalar comparison as the other
/// functions, so those keys now work. Every previously-working call behaves
/// identically; only cases that used to match nothing have changed.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where_batch(
    target: RawJsonb,
    array_path: &str,
    match_key: &str,
    updates_array: RawJsonb,
) -> RawJsonb {
    crate::array_ops::validate_match_key(match_key).unwrap_or_else(|e| error!("{}", e));
    if !is_object(&target) {
        error!("target argument must be a JSONB object");
    }

    // Reason: pointers come from detoasted datums and the palloc'ing builder.
    unsafe {
        // The serde version raises on all three of these rather than no-oping.
        match array_at(target.container(), array_path) {
            None => error!("Path '{}' does not exist in document", array_path),
            Some(false) => error!("Path '{}' does not point to an array", array_path),
            Some(true) => {}
        }
        if (*updates_array.container()).header & pg_sys::JB_FARRAY == 0 {
            error!("updates_array must be a JSONB array");
        }

        // Collect the specs once. Each entry borrows into `updates_array`, which
        // outlives the rebuild below.
        let mut specs: Vec<(pg_sys::JsonbValue, *mut pg_sys::JsonbContainer)> = Vec::new();
        let specs_root = updates_array.container();
        {
            let mut it = pg_sys::JsonbIteratorInit(specs_root);
            let mut sv = std::mem::zeroed::<pg_sys::JsonbValue>();
            pg_sys::JsonbIteratorNext(&raw mut it, &raw mut sv, true);
            loop {
                let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut sv, true);
                if tok != pg_sys::JsonbIteratorToken::WJB_ELEM {
                    break;
                }
                if sv.type_ != pg_sys::jbvType::jbvBinary {
                    continue; // malformed spec, skipped as before
                }
                let spec = sv.val.binary.data;
                if (*spec).header & pg_sys::JB_FOBJECT == 0 {
                    continue;
                }
                let mut mv_key = key_value("match_value");
                let mv =
                    pg_sys::findJsonbValueFromContainer(spec, pg_sys::JB_FOBJECT, &raw mut mv_key);
                let mut up_key = key_value("updates");
                let up =
                    pg_sys::findJsonbValueFromContainer(spec, pg_sys::JB_FOBJECT, &raw mut up_key);
                if mv.is_null() || up.is_null() {
                    continue;
                }
                if (*up).type_ != pg_sys::jbvType::jbvBinary
                    || (*(*up).val.binary.data).header & pg_sys::JB_FOBJECT == 0
                {
                    continue;
                }
                specs.push((*mv, (*up).val.binary.data));
            }
        }

        RawJsonb(rebuild_with_batch_updates(
            target.container(),
            array_path,
            match_key,
            &specs,
        ))
    }
}

/// Rebuild `doc`, applying the first matching spec to each element of the array
/// at `array_path`.
///
/// # Safety
///
/// `doc` must be a valid object container, and every pointer in `specs` must
/// outlive the call.
unsafe fn rebuild_with_batch_updates(
    doc: *mut pg_sys::JsonbContainer,
    array_path: &str,
    match_key: &str,
    specs: &[(pg_sys::JsonbValue, *mut pg_sys::JsonbContainer)],
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

            if !is_target || v.type_ != pg_sys::jbvType::jbvBinary {
                pg_sys::pushJsonbValue(
                    &raw mut state,
                    pg_sys::JsonbIteratorToken::WJB_VALUE,
                    &raw mut v,
                );
                continue;
            }

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
                let hit = specs
                    .iter()
                    .find(|(mv, _)| element_matches(&ev, match_key, mv));
                if let Some((_, updates)) = hit {
                    pg_sys::pushJsonbValue(
                        &raw mut state,
                        pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
                        std::ptr::null_mut(),
                    );
                    push_object_pairs(ev.val.binary.data, &raw mut state);
                    push_object_pairs(*updates, &raw mut state);
                    pg_sys::pushJsonbValue(
                        &raw mut state,
                        pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
                        std::ptr::null_mut(),
                    );
                } else {
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

// ---------------------------------------------------------------------------
// Nested-path merge
//
// The first port here that is not a flat rebuild. `jsonb_merge_at_path` descends
// a path and *creates missing intermediate objects*, so the rebuild has to
// recurse and be able to synthesize a chain that was never in the document.
//
// Only the spine is rebuilt: at each level every key except the one on the path
// is handed through as a binary value, so a wide object costs no more than a
// narrow one.
// ---------------------------------------------------------------------------

/// Type name matching `crate::value_type_name`, so error text is identical.
///
/// # Safety
///
/// `v` must be an initialized `JsonbValue` whose payload outlives the call.
unsafe fn jsonb_type_name(v: &pg_sys::JsonbValue) -> &'static str {
    unsafe {
        match v.type_ {
            pg_sys::jbvType::jbvNull => "null",
            pg_sys::jbvType::jbvBool => "boolean",
            pg_sys::jbvType::jbvNumeric => "number",
            pg_sys::jbvType::jbvString => "string",
            pg_sys::jbvType::jbvArray => "array",
            pg_sys::jbvType::jbvObject => "object",
            pg_sys::jbvType::jbvBinary => {
                if (*v.val.binary.data).header & pg_sys::JB_FARRAY == 0 {
                    "object"
                } else {
                    "array"
                }
            }
            _ => "unknown",
        }
    }
}

/// The object container behind a value, or null when it is not an object.
///
/// # Safety
///
/// `v` must be an initialized `JsonbValue` whose payload outlives the call.
unsafe fn object_container(v: &pg_sys::JsonbValue) -> *mut pg_sys::JsonbContainer {
    unsafe {
        if v.type_ == pg_sys::jbvType::jbvBinary
            && (*v.val.binary.data).header & pg_sys::JB_FOBJECT != 0
        {
            v.val.binary.data
        } else {
            std::ptr::null_mut()
        }
    }
}

/// The object container for a level of the path, or null when the key was absent.
///
/// Raises with the serde version's exact wording when the level exists but is not
/// an object. The message and the slice of the path it quotes both depend on
/// whether this is the last segment, which is reproduced rather than tidied.
///
/// # Safety
///
/// `node`'s payload must outlive the call; `path` must be non-empty.
unsafe fn resolve_path_object(
    node: Option<&pg_sys::JsonbValue>,
    path: &[&str],
    full: &[&str],
    depth: usize,
) -> *mut pg_sys::JsonbContainer {
    unsafe {
        let Some(v) = node else {
            return std::ptr::null_mut();
        };
        let c = object_container(v);
        if c.is_null() {
            if path.len() == 1 {
                error!(
                    "Path navigation failed: expected object at {:?}, got: {}",
                    &full[..depth],
                    jsonb_type_name(v)
                );
            } else {
                error!(
                    "Path navigation failed at {:?}, expected object, got: {}",
                    &full[..=depth],
                    jsonb_type_name(v)
                );
            }
        }
        c
    }
}

/// Push a complete object value: `node`, with `source` shallow-merged in at `path`.
///
/// `node` of `None` means the key was absent, which the serde version handles by
/// inserting an empty object -- so a path can be created wholesale.
///
/// The two "path navigation failed" messages differ in wording *and* in which
/// slice of the path they quote, depending on whether the level being indexed is
/// the last one. That is faithfully reproduced rather than tidied, because the
/// text is observable.
///
/// # Safety
///
/// `path` must be non-empty; all pointers must outlive the call.
unsafe fn push_merged_at_path(
    node: Option<&pg_sys::JsonbValue>,
    path: &[&str],
    full: &[&str],
    depth: usize,
    source: *mut pg_sys::JsonbContainer,
    state: *mut *mut pg_sys::JsonbParseState,
) -> *mut pg_sys::JsonbValue {
    unsafe {
        let container = resolve_path_object(node, path, full, depth);

        pg_sys::pushJsonbValue(
            state,
            pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
            std::ptr::null_mut(),
        );

        let mut found = false;
        if !container.is_null() {
            let mut it = pg_sys::JsonbIteratorInit(container);
            let mut v = std::mem::zeroed::<pg_sys::JsonbValue>();
            pg_sys::JsonbIteratorNext(&raw mut it, &raw mut v, true);

            loop {
                let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut v, true);
                if tok != pg_sys::JsonbIteratorToken::WJB_KEY {
                    break;
                }
                let n = usize::try_from(v.val.string.len).unwrap_or(0);
                let is_target = v.type_ == pg_sys::jbvType::jbvString
                    && n == path[0].len()
                    && std::slice::from_raw_parts(v.val.string.val.cast::<u8>(), n)
                        == path[0].as_bytes();

                pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_KEY, &raw mut v);
                let mut child = std::mem::zeroed::<pg_sys::JsonbValue>();
                pg_sys::JsonbIteratorNext(&raw mut it, &raw mut child, true);

                if !is_target {
                    pg_sys::pushJsonbValue(
                        state,
                        pg_sys::JsonbIteratorToken::WJB_VALUE,
                        &raw mut child,
                    );
                    continue;
                }
                found = true;

                if path.len() == 1 {
                    let target = object_container(&child);
                    if target.is_null() {
                        error!(
                            "Cannot merge into non-object at path {:?}, found: {}",
                            full,
                            jsonb_type_name(&child)
                        );
                    }
                    pg_sys::pushJsonbValue(
                        state,
                        pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
                        std::ptr::null_mut(),
                    );
                    push_object_pairs(target, state);
                    push_object_pairs(source, state);
                    pg_sys::pushJsonbValue(
                        state,
                        pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
                        std::ptr::null_mut(),
                    );
                } else {
                    push_merged_at_path(Some(&child), &path[1..], full, depth + 1, source, state);
                }
            }
        }

        if !found {
            // The key was absent: synthesize it, and any remaining path below it.
            let mut k = key_value(path[0]);
            pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_KEY, &raw mut k);
            if path.len() == 1 {
                pg_sys::pushJsonbValue(
                    state,
                    pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
                    std::ptr::null_mut(),
                );
                push_object_pairs(source, state);
                pg_sys::pushJsonbValue(
                    state,
                    pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
                    std::ptr::null_mut(),
                );
            } else {
                push_merged_at_path(None, &path[1..], full, depth + 1, source, state);
            }
        }

        // The push that closes the outermost object returns the finished value.
        pg_sys::pushJsonbValue(
            state,
            pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
            std::ptr::null_mut(),
        )
    }
}

/// Merge `source` into the object at `path`, creating it if absent.
///
/// Behaviourally identical to `jsonb_merge_at_path`, error messages included.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_merge_at_path(target: RawJsonb, source: RawJsonb, path: pgrx::Array<&str>) -> RawJsonb {
    // Reason: pointers come from detoasted datums and the palloc'ing builder.
    unsafe {
        let source_root = root_as_value(&source);
        if object_container(&source_root).is_null() {
            error!(
                "source argument must be a JSONB object, got: {}",
                jsonb_type_name(&source_root)
            );
        }
        let source_c = source_root.val.binary.data;

        // NULL elements are skipped, matching `path.iter().flatten()`.
        let segments: Vec<&str> = path.iter().flatten().collect();

        if segments.is_empty() {
            if !is_object(&target) {
                let t = root_as_value(&target);
                error!(
                    "target argument must be a JSONB object when path is empty, got: {}",
                    jsonb_type_name(&t)
                );
            }
            return jsonb_merge_shallow(target, source);
        }

        let root = root_as_value(&target);
        let mut state: *mut pg_sys::JsonbParseState = std::ptr::null_mut();
        let built = push_merged_at_path(
            Some(&root),
            &segments,
            &segments,
            0,
            source_c,
            &raw mut state,
        );
        RawJsonb(pg_sys::JsonbValueToJsonb(built))
    }
}

/// Merge `source` into the nested object at `path`.
///
/// Behaviourally identical to `jsonb_smart_patch_nested`, which is defined as
/// `jsonb_merge_at_path`.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_smart_patch_nested(
    target: RawJsonb,
    source: RawJsonb,
    path: pgrx::Array<&str>,
) -> RawJsonb {
    jsonb_merge_at_path(target, source, path)
}

// ---------------------------------------------------------------------------
// Deep (recursive) merge
//
// Unlike the shallow merge -- where a nested object on either side is handed
// through as an opaque binary value -- deep merge descends into keys present in
// BOTH documents as objects and merges them recursively. Every other key is
// copied wholesale (present on one side only) or replaced (source wins), so
// only the overlapping object spine is walked; disjoint subtrees still pass
// through as binary pointers and are never decoded.
// ---------------------------------------------------------------------------

/// Whether `v` nests no deeper than `max`, bailing as soon as it does not.
///
/// Mirrors `crate::validate_depth`: a scalar or empty container is depth 0, and
/// each level of nesting that holds a value adds one. Object keys are strings,
/// so only values are descended -- exactly what `validate_depth` does with
/// `map.values()`. Returns `false` at the first scalar found at level `max + 1`,
/// which is the point where `validate_depth` raises, so the reported depth is
/// always `max + 1` regardless of how much deeper the document goes.
///
/// # Safety
///
/// `v` must be an initialized `JsonbValue` whose payload outlives the call.
unsafe fn depth_within(v: &pg_sys::JsonbValue, current: usize, max: usize) -> bool {
    unsafe {
        if current > max {
            return false;
        }
        if v.type_ != pg_sys::jbvType::jbvBinary {
            return true; // a scalar sits at `current`, which is <= max here
        }
        let mut it = pg_sys::JsonbIteratorInit(v.val.binary.data);
        let mut child = std::mem::zeroed::<pg_sys::JsonbValue>();
        pg_sys::JsonbIteratorNext(&raw mut it, &raw mut child, true);
        loop {
            let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut child, true);
            match tok {
                // An object key: the value follows and is the thing to descend.
                pg_sys::JsonbIteratorToken::WJB_KEY => {
                    pg_sys::JsonbIteratorNext(&raw mut it, &raw mut child, true);
                    if !depth_within(&child, current + 1, max) {
                        return false;
                    }
                }
                pg_sys::JsonbIteratorToken::WJB_ELEM => {
                    if !depth_within(&child, current + 1, max) {
                        return false;
                    }
                }
                _ => return true, // WJB_END_OBJECT / WJB_END_ARRAY
            }
        }
    }
}

/// Push the deep merge of two object containers as a single object value.
///
/// Keys present in both, whose values are both objects, are merged recursively;
/// every other key is copied (present on one side only) or replaced (source
/// wins). Returns the value produced by the closing `WJB_END_OBJECT`, so the
/// top-level caller can hand it to `JsonbValueToJsonb`.
///
/// # Safety
///
/// Both containers must be valid jsonb object containers outliving the call.
unsafe fn push_deep_merged(
    target: *mut pg_sys::JsonbContainer,
    source: *mut pg_sys::JsonbContainer,
    state: *mut *mut pg_sys::JsonbParseState,
) -> *mut pg_sys::JsonbValue {
    unsafe {
        pg_sys::pushJsonbValue(
            state,
            pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
            std::ptr::null_mut(),
        );

        // Pass 1: every target key, in target order. If source carries the same
        // key, merge (both objects) or replace (source wins); else copy it.
        let mut it = pg_sys::JsonbIteratorInit(target);
        let mut key = std::mem::zeroed::<pg_sys::JsonbValue>();
        let mut tv = std::mem::zeroed::<pg_sys::JsonbValue>();
        pg_sys::JsonbIteratorNext(&raw mut it, &raw mut key, true);
        loop {
            let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut key, true);
            if tok != pg_sys::JsonbIteratorToken::WJB_KEY {
                break;
            }
            pg_sys::JsonbIteratorNext(&raw mut it, &raw mut tv, true);

            pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_KEY, &raw mut key);
            // `findJsonbValueFromContainer` reads the key for comparison only and
            // does not mutate it, so reusing `key` after the push above is sound.
            let sv = pg_sys::findJsonbValueFromContainer(source, pg_sys::JB_FOBJECT, &raw mut key);
            if sv.is_null() {
                pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_VALUE, &raw mut tv);
                continue;
            }
            let t_obj = object_container(&tv);
            let s_obj = object_container(&*sv);
            if !t_obj.is_null() && !s_obj.is_null() {
                push_deep_merged(t_obj, s_obj, state);
            } else {
                pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_VALUE, sv);
            }
        }

        // Pass 2: source keys absent from target, in source order.
        let mut sit = pg_sys::JsonbIteratorInit(source);
        let mut skey = std::mem::zeroed::<pg_sys::JsonbValue>();
        let mut sval = std::mem::zeroed::<pg_sys::JsonbValue>();
        pg_sys::JsonbIteratorNext(&raw mut sit, &raw mut skey, true);
        loop {
            let tok = pg_sys::JsonbIteratorNext(&raw mut sit, &raw mut skey, true);
            if tok != pg_sys::JsonbIteratorToken::WJB_KEY {
                break;
            }
            pg_sys::JsonbIteratorNext(&raw mut sit, &raw mut sval, true);
            let found =
                pg_sys::findJsonbValueFromContainer(target, pg_sys::JB_FOBJECT, &raw mut skey);
            if found.is_null() {
                pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_KEY, &raw mut skey);
                pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_VALUE, &raw mut sval);
            }
        }

        pg_sys::pushJsonbValue(
            state,
            pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
            std::ptr::null_mut(),
        )
    }
}

/// Recursively merge `source` into `target`, descending into shared object keys.
///
/// Behaviourally identical to `jsonb_deep_merge`: `source` depth is validated
/// first (so a too-deep source raises even when `target` is not an object), and
/// when either operand is not an object the result is `source`, unchanged.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_deep_merge(target: RawJsonb, source: RawJsonb) -> RawJsonb {
    // Reason: pointers come from detoasted datums and the palloc'ing builder.
    unsafe {
        let sroot = root_as_value(&source);
        if !depth_within(&sroot, 0, crate::MAX_JSONB_DEPTH) {
            error!(
                "JSONB nesting too deep (max {}, found depth {})",
                crate::MAX_JSONB_DEPTH,
                crate::MAX_JSONB_DEPTH + 1
            );
        }
        // deep_merge_recursive replaces with `source` unless BOTH are objects.
        if !is_object(&target) || !is_object(&source) {
            return source;
        }
        let mut state: *mut pg_sys::JsonbParseState = std::ptr::null_mut();
        let built = push_deep_merged(target.container(), source.container(), &raw mut state);
        RawJsonb(pg_sys::JsonbValueToJsonb(built))
    }
}

// ---------------------------------------------------------------------------
// Read-only probes
//
// These never rebuild anything, so they shed the *entire* round trip rather than
// half of it. The serde versions still pay a full parse to answer a question
// about one key.
// ---------------------------------------------------------------------------

/// Whether `array_path` holds an element whose `id_key` equals `id_value`.
///
/// Behaviourally identical to `jsonb_array_contains_id`: a non-object document,
/// a missing path, or a non-array at that path all answer `false` rather than
/// raising.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_contains_id(
    data: RawJsonb,
    array_path: &str,
    id_key: &str,
    id_value: RawJsonb,
) -> bool {
    crate::array_ops::validate_match_key(id_key).unwrap_or_else(|e| error!("{}", e));
    if !is_object(&data) {
        return false;
    }
    // Reason: pointers come from detoasted datums; nothing is allocated here.
    unsafe {
        let mut key = key_value(array_path);
        let found =
            pg_sys::findJsonbValueFromContainer(data.container(), pg_sys::JB_FOBJECT, &raw mut key);
        if found.is_null() || (*found).type_ != pg_sys::jbvType::jbvBinary {
            return false;
        }
        let array = (*found).val.binary.data;
        if (*array).header & pg_sys::JB_FARRAY == 0 {
            return false;
        }

        let target = root_as_value(&id_value);
        let mut it = pg_sys::JsonbIteratorInit(array);
        let mut ev = std::mem::zeroed::<pg_sys::JsonbValue>();
        pg_sys::JsonbIteratorNext(&raw mut it, &raw mut ev, true);
        loop {
            let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut ev, true);
            if tok != pg_sys::JsonbIteratorToken::WJB_ELEM {
                return false;
            }
            if element_matches(&ev, id_key, &target) {
                return true;
            }
        }
    }
}

/// Read a top-level `key` as text, for string and number values.
///
/// Behaviourally identical to `jsonb_extract_id`: anything else -- a boolean, an
/// object, an array, a missing key, a non-object document -- yields NULL.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe)]
fn jsonb_extract_id(data: RawJsonb, key: default!(&str, "'id'")) -> Option<String> {
    if !is_object(&data) {
        return None;
    }
    // Reason: pointers come from detoasted datums; the only allocation is the
    // returned String, which Rust owns.
    unsafe {
        let mut k = key_value(key);
        let found =
            pg_sys::findJsonbValueFromContainer(data.container(), pg_sys::JB_FOBJECT, &raw mut k);
        if found.is_null() {
            return None;
        }
        match (*found).type_ {
            pg_sys::jbvType::jbvString => {
                let n = usize::try_from((*found).val.string.len).unwrap_or(0);
                let bytes = std::slice::from_raw_parts((*found).val.string.val.cast::<u8>(), n);
                // jsonb strings are validated UTF-8 on the way in.
                Some(String::from_utf8_lossy(bytes).into_owned())
            }
            // Rendered by PostgreSQL's own numeric output, which is the canonical
            // spelling and matches what the serde version produced.
            pg_sys::jbvType::jbvNumeric => {
                pgrx::AnyNumeric::from_datum(pg_sys::Datum::from((*found).val.numeric), false)
                    .map(|n| n.to_string())
            }
            _ => None,
        }
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

    /// Assert `jsonb_merge_shallow(a, b)` matches `a || b`.
    fn assert_matches_concat(a: &str, b: &str) {
        let same = Spi::get_one::<bool>(&format!(
            "SELECT jsonb_merge_shallow('{a}'::jsonb, '{b}'::jsonb) = '{a}'::jsonb || '{b}'::jsonb"
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
                &format!("jsonb_array_update_where('{DOC}','posts','id','{key}','{upd}')"),
                &format!(
                    "jsonb_array_update_where_reference('{DOC}','posts','id','{key}','{upd}')"
                ),
            );
        }
    }

    /// Only the first match is updated, even though the fixture has two id=2.
    #[pg_test]
    fn array_update_touches_only_the_first_match() {
        assert_matches_serde(
            &format!("jsonb_array_update_where('{DOC}','posts','id','2','{{\"t\":\"Z\"}}')"),
            &format!(
                "jsonb_array_update_where_reference('{DOC}','posts','id','2','{{\"t\":\"Z\"}}')"
            ),
        );
    }

    #[pg_test]
    fn array_delete_matches_serde() {
        for key in ["2", "1", "99"] {
            assert_matches_serde(
                &format!("jsonb_array_delete_where('{DOC}','posts','id','{key}')"),
                &format!("jsonb_array_delete_where_reference('{DOC}','posts','id','{key}')"),
            );
        }
    }

    /// `delete` is a no-op on a missing path, where `update` errors. The two
    /// serde functions genuinely differ here and both contracts are preserved.
    #[pg_test]
    fn delete_on_missing_path_is_a_no_op() {
        assert_matches_serde(
            &format!("jsonb_array_delete_where('{DOC}','nope','id','2')"),
            &format!("jsonb_array_delete_where_reference('{DOC}','nope','id','2')"),
        );
    }

    #[pg_test(error = "Path 'nope' does not exist in document")]
    fn update_on_missing_path_errors() {
        Spi::run("SELECT jsonb_array_update_where('{\"a\":1}','nope','id','1','{}')")
            .expect("SPI ok");
    }

    #[pg_test(error = "match_key must not be empty")]
    fn empty_match_key_is_rejected() {
        Spi::run("SELECT jsonb_array_update_where('{\"p\":[]}','p','','1','{}')").expect("SPI ok");
    }

    #[pg_test]
    fn text_and_uuid_match_keys() {
        let doc = r#"{"posts":[{"id":"a-1","t":"x"},{"id":"3f2a-uuid","t":"y"}]}"#;
        assert_matches_serde(
            &format!(
                r#"jsonb_array_update_where('{doc}','posts','id','"3f2a-uuid"','{{"t":"Z"}}')"#
            ),
            &format!(
                r#"jsonb_array_update_where_reference('{doc}','posts','id','"3f2a-uuid"','{{"t":"Z"}}')"#
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
            r#"SELECT jsonb_array_update_where('{"p":[{"id":2,"t":"a"}]}','p','id','2.0','{"t":"Z"}')
                    = '{"p":[{"id":2,"t":"Z"}]}'::jsonb"#,
        )
        .expect("SPI ok")
        .expect("not null");
        assert!(matched, "2.0 should match an id of 2, as it does in SQL");
    }

    #[pg_test]
    fn smart_patch_scalar_matches_serde() {
        assert_matches_serde(
            r#"jsonb_smart_patch_scalar('{"a":1,"b":{"n":1}}','{"b":2,"c":3}')"#,
            r#"jsonb_smart_patch_scalar_reference('{"a":1,"b":{"n":1}}','{"b":2,"c":3}')"#,
        );
    }

    #[pg_test]
    fn smart_patch_array_matches_serde() {
        for key in ["2", "1", "99"] {
            assert_matches_serde(
                &format!(
                    "jsonb_smart_patch_array('{DOC}','{{\"t\":\"Z\"}}','posts','id','{key}')"
                ),
                &format!("jsonb_smart_patch_array_reference('{DOC}','{{\"t\":\"Z\"}}','posts','id','{key}')"),
            );
        }
    }

    #[pg_test(error = "Path 'nope' does not exist in document")]
    fn smart_patch_array_errors_on_missing_path() {
        Spi::run("SELECT jsonb_smart_patch_array('{\"a\":1}','{}','nope','id','1')")
            .expect("SPI ok");
    }

    #[pg_test]
    fn contains_id_matches_serde() {
        for (path, key, val) in [
            ("posts", "id", "2"),
            ("posts", "id", "99"),
            ("nope", "id", "2"),
        ] {
            let same = Spi::get_one::<bool>(&format!(
                "SELECT jsonb_array_contains_id('{DOC}','{path}','{key}','{val}')
                      = jsonb_array_contains_id_reference('{DOC}','{path}','{key}','{val}')"
            ))
            .expect("SPI ok")
            .expect("not null");
            assert!(same, "contains_id disagreed for {path}/{key}/{val}");
        }
    }

    #[pg_test]
    fn extract_id_matches_serde() {
        for doc in [
            r#"{"id":"abc","x":1}"#,
            r#"{"id":123}"#,
            r#"{"id":true}"#,
            r#"{"id":{"n":1}}"#,
            r#"{"id":[1]}"#,
            r#"{"id":null}"#,
            r#"{"other":1}"#,
        ] {
            let same = Spi::get_one::<bool>(&format!(
                "SELECT jsonb_extract_id('{doc}','id') IS NOT DISTINCT FROM
                        jsonb_extract_id_reference('{doc}','id')"
            ))
            .expect("SPI ok")
            .expect("not null");
            assert!(same, "extract_id disagreed for {doc}");
        }
    }

    #[pg_test]
    fn batch_update_matches_serde() {
        let specs =
            r#"[{"match_value":1,"updates":{"t":"X"}},{"match_value":3,"updates":{"t":"Y"}}]"#;
        assert_matches_serde(
            &format!("jsonb_array_update_where_batch('{DOC}','posts','id','{specs}')"),
            &format!("jsonb_array_update_where_batch_reference('{DOC}','posts','id','{specs}')"),
        );
    }

    /// Every element matching a spec is updated, including duplicates -- the
    /// fixture carries two id=2 and both must change.
    #[pg_test]
    fn batch_update_hits_every_match() {
        let specs = r#"[{"match_value":2,"updates":{"t":"X"}}]"#;
        assert_matches_serde(
            &format!("jsonb_array_update_where_batch('{DOC}','posts','id','{specs}')"),
            &format!("jsonb_array_update_where_batch_reference('{DOC}','posts','id','{specs}')"),
        );
    }

    #[pg_test]
    fn batch_update_skips_malformed_specs() {
        let specs = r#"[{"match_value":1},{"nope":true},7,{"match_value":3,"updates":{"t":"Y"}}]"#;
        assert_matches_serde(
            &format!("jsonb_array_update_where_batch('{DOC}','posts','id','{specs}')"),
            &format!("jsonb_array_update_where_batch_reference('{DOC}','posts','id','{specs}')"),
        );
    }

    #[pg_test(error = "Path 'nope' does not exist in document")]
    fn batch_update_errors_on_missing_path() {
        Spi::run("SELECT jsonb_array_update_where_batch('{\"p\":[]}','nope','id','[]')")
            .expect("SPI ok");
    }

    #[pg_test(error = "updates_array must be a JSONB array")]
    fn batch_update_errors_on_non_array_specs() {
        Spi::run("SELECT jsonb_array_update_where_batch('{\"p\":[]}','p','id','{}')")
            .expect("SPI ok");
    }

    /// `jsonb_extract_id` round-trips numbers through `serde_json`, which parses
    /// into `f64` and so renders `1.50` as `1.5`. Reading the stored numeric
    /// directly preserves the scale, which is what `->>` gives:
    /// `'{"id":1.50}'::jsonb ->> 'id'` is `1.50`. Asserting the SQL-consistent
    /// answer, and noting the divergence rather than hiding it.
    #[pg_test]
    fn extract_id_preserves_numeric_scale() {
        let got = Spi::get_one::<String>(r#"SELECT jsonb_extract_id('{"id":1.50}','id')"#)
            .expect("SPI ok")
            .expect("not null");
        assert_eq!(got, "1.50");
        let pg = Spi::get_one::<String>(r#"SELECT '{"id":1.50}'::jsonb ->> 'id'"#)
            .expect("SPI ok")
            .expect("not null");
        assert_eq!(got, pg, "should agree with the ->> operator");
    }

    /// The serde version reads `match_value` with `as_i64`, so text keys silently
    /// matched nothing. This is the one case where the binary version is
    /// deliberately more capable, so it is asserted directly rather than
    /// differentially.
    #[pg_test]
    fn batch_update_now_supports_text_keys() {
        let doc = r#"{"p":[{"id":"a","t":"x"},{"id":"b","t":"y"}]}"#;
        let specs = r#"[{"match_value":"b","updates":{"t":"Z"}}]"#;
        let got = Spi::get_one::<bool>(&format!(
            r#"SELECT jsonb_array_update_where_batch('{doc}','p','id','{specs}')
                    = '{{"p":[{{"id":"a","t":"x"}},{{"id":"b","t":"Z"}}]}}'::jsonb"#
        ))
        .expect("SPI ok")
        .expect("not null");
        assert!(got, "text match_value should now batch-update");
    }

    /// Run a fragment and return either its value as text or its error message.
    ///
    /// The catch happens in `plpgsql` rather than in Rust because a `PostgreSQL`
    /// error caught without a surrounding subtransaction leaves the transaction
    /// aborted, so the second case in a loop would fail for the wrong reason. A
    /// plpgsql `EXCEPTION` block opens a subtransaction, making this repeatable.
    fn outcome(sql: &str) -> String {
        Spi::run(
            "CREATE OR REPLACE FUNCTION pg_temp.outcome(q text) RETURNS text
             LANGUAGE plpgsql AS $fn$
             DECLARE r text;
             BEGIN
                 EXECUTE 'SELECT (' || q || ')::text' INTO r;
                 RETURN coalesce(r, 'NULL');
             EXCEPTION WHEN OTHERS THEN RETURN 'ERROR: ' || SQLERRM;
             END $fn$;",
        )
        .expect("helper created");
        Spi::get_one_with_args::<String>("SELECT pg_temp.outcome($1)", &[sql.into()])
            .expect("SPI ok")
            .expect("not null")
    }

    /// Compare a `_fast` call with its serde original on *both* the value and the
    /// error paths. `jsonb_merge_at_path` has three distinct failure messages that
    /// quote different slices of the path, so parity is established by running
    /// both rather than by reading the source.
    fn assert_same_outcome(fast: &str, serde: &str) {
        let (a, b) = (outcome(fast), outcome(serde));
        assert_eq!(a, b, "diverged:\n  fast:  {fast}\n  serde: {serde}");
    }

    #[pg_test]
    fn merge_at_path_matches_serde() {
        let cases = [
            (
                r#"'{"a":{"b":{"x":1}}}'"#,
                r#"'{"y":2}'"#,
                r"ARRAY['a','b']",
            ),
            (
                r#"'{"a":{"b":{"x":1}}}'"#,
                r#"'{"x":9}'"#,
                r"ARRAY['a','b']",
            ),
            (r#"'{"a":1,"u":{"n":1}}'"#, r#"'{"m":2}'"#, r"ARRAY['u']"),
            // path absent end to end: intermediates must be created
            (r"'{}'", r#"'{"x":1}'"#, r"ARRAY['a','b','c']"),
            (r#"'{"a":{}}'"#, r#"'{"x":1}'"#, r"ARRAY['a','b']"),
            // empty path merges at the root
            (r#"'{"a":1}'"#, r#"'{"b":2}'"#, r"ARRAY[]::text[]"),
            // wide objects: every off-path key must survive untouched
            (
                r#"'{"k1":1,"k2":[1,2],"a":{"z":0},"k3":{"n":1}}'"#,
                r#"'{"w":1}'"#,
                r"ARRAY['a']",
            ),
        ];
        for (t, src, path) in cases {
            assert_same_outcome(
                &format!("jsonb_merge_at_path({t},{src},{path})"),
                &format!("jsonb_merge_at_path_reference({t},{src},{path})"),
            );
        }
    }

    /// The three failure modes, compared as data. Each quotes a different slice
    /// of the path, which is exactly the kind of detail a reimplementation gets
    /// subtly wrong.
    #[pg_test]
    fn merge_at_path_error_text_matches_serde() {
        let cases = [
            // target is not an object, empty path
            (r"'[1,2]'", r#"'{"x":1}'"#, r"ARRAY[]::text[]"),
            // source is not an object
            (r#"'{"a":{}}'"#, r"'[1]'", r"ARRAY['a']"),
            // scalar blocking the last segment
            (r#"'{"a":5}'"#, r#"'{"x":1}'"#, r"ARRAY['a']"),
            // scalar blocking an intermediate segment
            (r#"'{"a":5}'"#, r#"'{"x":1}'"#, r"ARRAY['a','b']"),
            // array blocking the last segment
            (r#"'{"a":[1]}'"#, r#"'{"x":1}'"#, r"ARRAY['a']"),
            // root is a scalar with a non-empty path
            (r#"'"s"'"#, r#"'{"x":1}'"#, r"ARRAY['a']"),
            (r#"'"s"'"#, r#"'{"x":1}'"#, r"ARRAY['a','b']"),
            // deeper: scalar two levels down
            (r#"'{"a":{"b":7}}'"#, r#"'{"x":1}'"#, r"ARRAY['a','b','c']"),
        ];
        for (t, src, path) in cases {
            assert_same_outcome(
                &format!("jsonb_merge_at_path({t},{src},{path})"),
                &format!("jsonb_merge_at_path_reference({t},{src},{path})"),
            );
        }
    }

    #[pg_test]
    fn smart_patch_nested_matches_serde() {
        assert_same_outcome(
            r#"jsonb_smart_patch_nested('{"u":{"c":{"n":"A","city":"NY"}}}','{"n":"B"}',ARRAY['u','c'])"#,
            r#"jsonb_smart_patch_nested_reference('{"u":{"c":{"n":"A","city":"NY"}}}','{"n":"B"}',ARRAY['u','c'])"#,
        );
    }

    /// Deep merge across the cases that distinguish it from a shallow merge:
    /// shared object keys recurse, everything else is copied or replaced, and a
    /// non-object operand makes `source` win outright.
    #[pg_test]
    fn deep_merge_matches_serde() {
        let cases = [
            // disjoint keys
            (r#"{"a":1}"#, r#"{"b":2}"#),
            // overlapping scalar: source replaces
            (r#"{"a":1,"b":2}"#, r#"{"b":9}"#),
            // the recursion that shallow merge cannot do: "likes" must survive
            (
                r#"{"author":{"name":"A","stats":{"posts":10,"likes":5}}}"#,
                r#"{"author":{"stats":{"posts":11}}}"#,
            ),
            // object replaced by scalar, and the reverse
            (r#"{"a":{"x":1}}"#, r#"{"a":5}"#),
            (r#"{"a":5}"#, r#"{"a":{"x":1}}"#),
            // object vs array, and arrays are replaced not merged
            (r#"{"a":{"x":1}}"#, r#"{"a":[1,2]}"#),
            (r#"{"a":[1,2]}"#, r#"{"a":[3]}"#),
            // source-only key carrying a nested object through untouched
            (r#"{"a":1}"#, r#"{"b":{"c":2}}"#),
            // three levels of shared-object recursion
            (
                r#"{"a":{"b":{"c":1,"d":2}}}"#,
                r#"{"a":{"b":{"c":9,"e":3}}}"#,
            ),
            // empty operands
            (r#"{}"#, r#"{"a":1}"#),
            (r#"{"a":1}"#, r#"{}"#),
            (r#"{}"#, r#"{}"#),
            // non-ascii keys, with a recursion on one of them
            (
                r#"{"café":1,"日本":{"x":1}}"#,
                r#"{"café":2,"日本":{"y":2}}"#,
            ),
            // neither, or one, operand is an object: source wins wholesale
            (r#"5"#, r#"{"a":1}"#),
            (r#"[1,2]"#, r#"{"a":1}"#),
            (r#"{"a":1}"#, r#"5"#),
            (r#"{"a":1}"#, r#"[1,2]"#),
            (r#"5"#, r#"7"#),
        ];
        for (t, s) in cases {
            assert_matches_serde(
                &format!("jsonb_deep_merge('{t}','{s}')"),
                &format!("jsonb_deep_merge_reference('{t}','{s}')"),
            );
        }
    }

    /// The binary version is the first to actually enforce the documented
    /// 1000-level depth cap. The serde original cannot reach its own guard: the
    /// argument is parsed through pgrx's `JsonB`, whose `serde_json` parse trips
    /// its ~128-level recursion limit ("recursion limit exceeded") long before
    /// `validate_depth` runs. Walking the binary form has no such parse limit,
    /// so the cap here is both the documented contract and the bound that keeps
    /// `push_deep_merged`'s recursion off the backend stack. A deliberate,
    /// pinned divergence, in the spirit of the numeric-scale ones above.
    #[pg_test]
    fn deep_merge_enforces_documented_depth_limit() {
        let out = outcome(
            r#"jsonb_deep_merge('{}',(repeat('{"a":',1001)||'1'||repeat('}',1001))::jsonb)"#,
        );
        assert_eq!(
            out,
            "ERROR: JSONB nesting too deep (max 1000, found depth 1001)"
        );
    }

    /// The flip side of the divergence: a 300-level document is past serde's
    /// parse limit but within the 1000-level cap, so the serde original errors
    /// where the binary version merges. `deep_merge(a, a) == a` for any all-object
    /// document, which is what the recursion must produce.
    #[pg_test]
    fn deep_merge_handles_depth_serde_cannot_parse() {
        let ok = Spi::get_one::<bool>(
            r#"WITH a(v) AS (SELECT (repeat('{"a":',300)||'1'||repeat('}',300))::jsonb)
               SELECT jsonb_deep_merge(v, v) = v FROM a"#,
        )
        .expect("SPI ok")
        .expect("not null");
        assert!(
            ok,
            "binary deep merge should handle depth serde cannot parse"
        );
    }

    #[pg_test]
    fn agrees_with_the_serde_implementation_it_replaces() {
        let same = Spi::get_one::<bool>(
            r#"SELECT jsonb_merge_shallow('{"a":1,"b":{"n":1}}'::jsonb, '{"b":2,"c":3}'::jsonb)
                    = jsonb_merge_shallow_reference('{"a":1,"b":{"n":1}}'::jsonb, '{"b":2,"c":3}'::jsonb)"#,
        )
        .expect("SPI ok")
        .expect("not null");
        assert!(same, "binary merge disagreed with the serde implementation");
    }
}
