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

// Reason: mirrors pgrx's own `UnboxDatum for JsonB`, which is what lets a type be
// an array element (`Array<RawJsonb>`). The GAT carries the source datum's
// lifetime; the conversion defers to `FromDatum`, which detoasts. `RawJsonb`
// holds only a raw pointer, so it satisfies `Self: 'src` for any `'src`.
unsafe impl pgrx::datum::UnboxDatum for RawJsonb {
    type As<'src> = Self;
    unsafe fn unbox<'src>(d: pgrx::datum::Datum<'src>) -> Self::As<'src>
    where
        Self: 'src,
    {
        Self::from_datum(d.sans_lifetime(), false).unwrap()
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
// Ordered array insert
//
// Insert one element into a top-level array, either appended or placed to keep
// the array sorted by a field. Non-matching keys of the document, and every
// element the insert does not sit between, pass through as binary values -- so
// the whole array is not decoded, only walked.
// ---------------------------------------------------------------------------

/// Total-ordering rank of a jsonb value's type, mirroring `crate::compare_values`:
/// null < bool < number < string < (array/object, which compare equal).
const fn jbv_rank(t: pg_sys::jbvType::Type) -> u8 {
    match t {
        pg_sys::jbvType::jbvNull => 0,
        pg_sys::jbvType::jbvBool => 1,
        pg_sys::jbvType::jbvNumeric => 2,
        pg_sys::jbvType::jbvString => 3,
        _ => 4,
    }
}

/// Order two jsonb values exactly as `crate::compare_values` orders their serde
/// equivalents: by type rank first, then within a type. Numbers compare by
/// value through `AnyNumeric` (`PostgreSQL`'s own numeric comparison), which for
/// the integer and string sort keys these functions actually use gives the same
/// order as serde's integer-then-float path; containers compare equal.
///
/// # Safety
///
/// Both must be initialized `JsonbValue`s whose payloads outlive the call.
unsafe fn binary_compare(a: &pg_sys::JsonbValue, b: &pg_sys::JsonbValue) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (ra, rb) = (jbv_rank(a.type_), jbv_rank(b.type_));
    if ra != rb {
        return ra.cmp(&rb);
    }
    unsafe {
        match a.type_ {
            pg_sys::jbvType::jbvBool => a.val.boolean.cmp(&b.val.boolean),
            pg_sys::jbvType::jbvNumeric => {
                let an = pgrx::AnyNumeric::from_datum(pg_sys::Datum::from(a.val.numeric), false);
                let bn = pgrx::AnyNumeric::from_datum(pg_sys::Datum::from(b.val.numeric), false);
                match (an, bn) {
                    (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
                    _ => Ordering::Equal,
                }
            }
            pg_sys::jbvType::jbvString => {
                let na = usize::try_from(a.val.string.len).unwrap_or(0);
                let nb = usize::try_from(b.val.string.len).unwrap_or(0);
                let sa = std::slice::from_raw_parts(a.val.string.val.cast::<u8>(), na);
                let sb = std::slice::from_raw_parts(b.val.string.val.cast::<u8>(), nb);
                sa.cmp(sb)
            }
            // Both null, or both containers: equal, as compare_values has it.
            _ => Ordering::Equal,
        }
    }
}

/// The value of object field `key` in `elem`, or `None` when `elem` is not an
/// object or lacks the key -- matching serde's `elem.get(key)`.
///
/// # Safety
///
/// `elem`'s payload must outlive the call.
unsafe fn field_of(elem: &pg_sys::JsonbValue, key: &str) -> Option<pg_sys::JsonbValue> {
    unsafe {
        let c = object_container(elem);
        if c.is_null() {
            return None;
        }
        let mut k = key_value(key);
        let found = pg_sys::findJsonbValueFromContainer(c, pg_sys::JB_FOBJECT, &raw mut k);
        if found.is_null() {
            None
        } else {
            Some(*found)
        }
    }
}

/// Insertion index keeping `elements` sorted by `sort_key`, mirroring
/// `crate::find_insertion_point`: a `partition_point` over the same predicate,
/// so the position is identical for a sorted input and identically arbitrary for
/// an unsorted one. An element without the key sorts before keyed ones.
///
/// # Safety
///
/// Every value referenced must outlive the call.
unsafe fn insertion_point(
    elements: &[pg_sys::JsonbValue],
    new_val: &pg_sys::JsonbValue,
    sort_key: &str,
    is_asc: bool,
) -> usize {
    use std::cmp::Ordering;
    elements.partition_point(|elem| unsafe {
        // None (keyless) sorts before keyed elements, hence the `true` default.
        field_of(elem, sort_key).is_none_or(|ev| {
            let ord = binary_compare(&ev, new_val);
            if is_asc {
                ord == Ordering::Less
            } else {
                ord == Ordering::Greater
            }
        })
    })
}

/// Push an array as `elements` with `new_elem` inserted at `pos` (before the
/// element currently there); `pos == elements.len()` appends.
///
/// # Safety
///
/// Every pointer must outlive the call.
unsafe fn push_array_with_insert(
    elements: &[pg_sys::JsonbValue],
    new_elem: &pg_sys::JsonbValue,
    pos: usize,
    state: *mut *mut pg_sys::JsonbParseState,
) {
    unsafe {
        pg_sys::pushJsonbValue(
            state,
            pg_sys::JsonbIteratorToken::WJB_BEGIN_ARRAY,
            std::ptr::null_mut(),
        );
        for (j, e) in elements.iter().enumerate() {
            if j == pos {
                let mut ne = *new_elem;
                pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_ELEM, &raw mut ne);
            }
            let mut ev = *e;
            pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_ELEM, &raw mut ev);
        }
        if pos >= elements.len() {
            let mut ne = *new_elem;
            pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_ELEM, &raw mut ne);
        }
        pg_sys::pushJsonbValue(
            state,
            pg_sys::JsonbIteratorToken::WJB_END_ARRAY,
            std::ptr::null_mut(),
        );
    }
}

/// Collect an array container's elements into a `Vec`, each borrowing into the
/// source. Bounded by `MAX_JSONB_ARRAY_SIZE`, so the allocation is bounded.
///
/// # Safety
///
/// `array` must be a valid jsonb array container outliving the returned values.
unsafe fn collect_elements(array: *mut pg_sys::JsonbContainer) -> Vec<pg_sys::JsonbValue> {
    unsafe {
        let mut out = Vec::new();
        let mut it = pg_sys::JsonbIteratorInit(array);
        let mut ev = std::mem::zeroed::<pg_sys::JsonbValue>();
        pg_sys::JsonbIteratorNext(&raw mut it, &raw mut ev, true);
        loop {
            let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut ev, true);
            if tok != pg_sys::JsonbIteratorToken::WJB_ELEM {
                break;
            }
            out.push(ev);
        }
        out
    }
}

/// Insert `new_element` into the array at top-level key `array_path`, ordered by
/// `sort_key` if given (else appended), without materializing the document.
///
/// Behaviourally identical to `jsonb_array_insert_where`: the array is created if
/// the key is absent, a non-object target and a non-array value at the key each
/// raise, and the ordering is the same `partition_point` placement.
// Reason: `#[pg_extern]` requires owned arguments, as above. Deliberately NOT
// strict, matching the serde original -- sort_key / sort_order are optional.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe)]
fn jsonb_array_insert_where(
    target: RawJsonb,
    array_path: &str,
    new_element: RawJsonb,
    sort_key: Option<&str>,
    sort_order: Option<&str>,
) -> RawJsonb {
    // Reason: pointers come from detoasted datums and the palloc'ing builder.
    unsafe {
        if !is_object(&target) {
            let t = root_as_value(&target);
            error!(
                "target must be a JSONB object, got: {}",
                jsonb_type_name(&t)
            );
        }
        let new_elem = root_as_value(&new_element);

        let mut state: *mut pg_sys::JsonbParseState = std::ptr::null_mut();
        pg_sys::pushJsonbValue(
            &raw mut state,
            pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
            std::ptr::null_mut(),
        );

        let mut it = pg_sys::JsonbIteratorInit(target.container());
        let mut key = std::mem::zeroed::<pg_sys::JsonbValue>();
        let mut val = std::mem::zeroed::<pg_sys::JsonbValue>();
        pg_sys::JsonbIteratorNext(&raw mut it, &raw mut key, true);

        let mut found = false;
        loop {
            let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut key, true);
            if tok != pg_sys::JsonbIteratorToken::WJB_KEY {
                break;
            }
            let is_target = key.type_ == pg_sys::jbvType::jbvString
                && usize::try_from(key.val.string.len).unwrap_or(0) == array_path.len()
                && std::slice::from_raw_parts(key.val.string.val.cast::<u8>(), array_path.len())
                    == array_path.as_bytes();
            pg_sys::JsonbIteratorNext(&raw mut it, &raw mut val, true);
            pg_sys::pushJsonbValue(
                &raw mut state,
                pg_sys::JsonbIteratorToken::WJB_KEY,
                &raw mut key,
            );

            if !is_target {
                pg_sys::pushJsonbValue(
                    &raw mut state,
                    pg_sys::JsonbIteratorToken::WJB_VALUE,
                    &raw mut val,
                );
                continue;
            }
            found = true;
            if val.type_ != pg_sys::jbvType::jbvBinary
                || (*val.val.binary.data).header & pg_sys::JB_FARRAY == 0
            {
                error!(
                    "path '{}' must point to an array or not exist, got: {}",
                    array_path,
                    jsonb_type_name(&val)
                );
            }
            insert_into_array(
                val.val.binary.data,
                &new_elem,
                sort_key,
                sort_order,
                &raw mut state,
            );
        }

        if !found {
            // Absent key: create the array holding just the new element.
            let mut k = key_value(array_path);
            pg_sys::pushJsonbValue(
                &raw mut state,
                pg_sys::JsonbIteratorToken::WJB_KEY,
                &raw mut k,
            );
            push_array_with_insert(&[], &new_elem, 0, &raw mut state);
        }

        let result = pg_sys::pushJsonbValue(
            &raw mut state,
            pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
            std::ptr::null_mut(),
        );
        RawJsonb(pg_sys::JsonbValueToJsonb(result))
    }
}

/// Push the array at `array` with `new_elem` inserted, ordered per `sort_key` /
/// `sort_order`. Appends when there is no sort key or the new element carries no
/// value for it, matching `find_insertion_point`.
///
/// # Safety
///
/// `array` must be a valid jsonb array container; every pointer must outlive it.
unsafe fn insert_into_array(
    array: *mut pg_sys::JsonbContainer,
    new_elem: &pg_sys::JsonbValue,
    sort_key: Option<&str>,
    sort_order: Option<&str>,
    state: *mut *mut pg_sys::JsonbParseState,
) {
    unsafe {
        // Append path: no sort key, or the new element has no value at it.
        let sorted_pos = sort_key.and_then(|sk| {
            field_of(new_elem, sk).map(|nv| {
                let is_asc = sort_order.unwrap_or("ASC").eq_ignore_ascii_case("ASC");
                let elements = collect_elements(array);
                let pos = insertion_point(&elements, &nv, sk, is_asc);
                (elements, pos)
            })
        });

        if let Some((elements, pos)) = sorted_pos {
            push_array_with_insert(&elements, new_elem, pos, state);
        } else {
            // Append: stream existing elements through, then the new one.
            pg_sys::pushJsonbValue(
                state,
                pg_sys::JsonbIteratorToken::WJB_BEGIN_ARRAY,
                std::ptr::null_mut(),
            );
            let mut ait = pg_sys::JsonbIteratorInit(array);
            let mut ev = std::mem::zeroed::<pg_sys::JsonbValue>();
            pg_sys::JsonbIteratorNext(&raw mut ait, &raw mut ev, true);
            loop {
                let tok = pg_sys::JsonbIteratorNext(&raw mut ait, &raw mut ev, true);
                if tok != pg_sys::JsonbIteratorToken::WJB_ELEM {
                    break;
                }
                pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_ELEM, &raw mut ev);
            }
            let mut ne = *new_elem;
            pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_ELEM, &raw mut ne);
            pg_sys::pushJsonbValue(
                state,
                pg_sys::JsonbIteratorToken::WJB_END_ARRAY,
                std::ptr::null_mut(),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Path set
//
// The general nested setter: descend a path of object keys and array indices,
// creating intermediate containers and padding arrays with nulls exactly as the
// serde `set_path` does, and rebuild only the spine. Off-path keys and elements
// pass through as binary values. The rebuild recurses one level per path
// segment, so recursion depth is the path length -- bounded below.
// ---------------------------------------------------------------------------

/// The array container behind a value, or null when it is not an array.
///
/// # Safety
///
/// `v` must be an initialized `JsonbValue` whose payload outlives the call.
unsafe fn array_container(v: &pg_sys::JsonbValue) -> *mut pg_sys::JsonbContainer {
    unsafe {
        if v.type_ == pg_sys::jbvType::jbvBinary
            && (*v.val.binary.data).header & pg_sys::JB_FARRAY != 0
        {
            v.val.binary.data
        } else {
            std::ptr::null_mut()
        }
    }
}

/// Build the value for one path level: the existing `node` (or `None` when the
/// key/index was absent or the wrong type) with `value` set at `segs`, and
/// return the value from the closing push.
///
/// Mirrors `crate::path::set_path` exactly, including its destructive create:
/// a level whose existing value is the wrong container type is replaced by a
/// fresh empty one (its contents dropped), and array indexing pads with JSON
/// nulls up to the index.
///
/// # Safety
///
/// `segs` must be non-empty; `node`, `value`, and every container reached
/// through them must outlive the call.
// Reason: a single recursive spine-rebuild whose Key/Index cases must stay in
// one place to be reviewable against the JsonbValue stream; splitting it would
// scatter the unsafe pointer handling for no correctness gain.
#[allow(clippy::too_many_lines)]
unsafe fn push_value_for_level(
    node: Option<&pg_sys::JsonbValue>,
    segs: &[crate::path::PathSegment],
    value: &pg_sys::JsonbValue,
    state: *mut *mut pg_sys::JsonbParseState,
) -> *mut pg_sys::JsonbValue {
    use crate::path::PathSegment;
    let is_last = segs.len() == 1;
    unsafe {
        match &segs[0] {
            PathSegment::Key(key) => {
                pg_sys::pushJsonbValue(
                    state,
                    pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
                    std::ptr::null_mut(),
                );
                // Copy every off-path key of the existing object; remember the
                // on-path child if it is there.
                let mut child: Option<pg_sys::JsonbValue> = None;
                let container = node.map_or(std::ptr::null_mut(), |v| object_container(v));
                if !container.is_null() {
                    let mut it = pg_sys::JsonbIteratorInit(container);
                    let mut k = std::mem::zeroed::<pg_sys::JsonbValue>();
                    let mut v = std::mem::zeroed::<pg_sys::JsonbValue>();
                    pg_sys::JsonbIteratorNext(&raw mut it, &raw mut k, true);
                    loop {
                        let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut k, true);
                        if tok != pg_sys::JsonbIteratorToken::WJB_KEY {
                            break;
                        }
                        pg_sys::JsonbIteratorNext(&raw mut it, &raw mut v, true);
                        let is_key = k.type_ == pg_sys::jbvType::jbvString
                            && usize::try_from(k.val.string.len).unwrap_or(0) == key.len()
                            && std::slice::from_raw_parts(k.val.string.val.cast::<u8>(), key.len())
                                == key.as_bytes();
                        if is_key {
                            child = Some(v);
                        } else {
                            pg_sys::pushJsonbValue(
                                state,
                                pg_sys::JsonbIteratorToken::WJB_KEY,
                                &raw mut k,
                            );
                            pg_sys::pushJsonbValue(
                                state,
                                pg_sys::JsonbIteratorToken::WJB_VALUE,
                                &raw mut v,
                            );
                        }
                    }
                }
                let mut kk = key_value(key);
                pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_KEY, &raw mut kk);
                if is_last {
                    let mut val = *value;
                    pg_sys::pushJsonbValue(
                        state,
                        pg_sys::JsonbIteratorToken::WJB_VALUE,
                        &raw mut val,
                    );
                } else {
                    push_value_for_level(child.as_ref(), &segs[1..], value, state);
                }
                pg_sys::pushJsonbValue(
                    state,
                    pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
                    std::ptr::null_mut(),
                )
            }
            PathSegment::Index(idx) => {
                let idx = *idx;
                pg_sys::pushJsonbValue(
                    state,
                    pg_sys::JsonbIteratorToken::WJB_BEGIN_ARRAY,
                    std::ptr::null_mut(),
                );
                let arr = node.map_or(std::ptr::null_mut(), |v| array_container(v));
                let elements = if arr.is_null() {
                    Vec::new()
                } else {
                    collect_elements(arr)
                };
                let existing_len = elements.len();

                // Elements before the target index: the existing ones, then
                // JSON-null padding to reach the index (`ensure_array_capacity`).
                for e in elements.iter().take(idx.min(existing_len)) {
                    let mut e = *e;
                    pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_ELEM, &raw mut e);
                }
                for _ in existing_len..idx {
                    let mut n = std::mem::zeroed::<pg_sys::JsonbValue>();
                    n.type_ = pg_sys::jbvType::jbvNull;
                    pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_ELEM, &raw mut n);
                }
                // The target index itself.
                if is_last {
                    let mut val = *value;
                    pg_sys::pushJsonbValue(
                        state,
                        pg_sys::JsonbIteratorToken::WJB_ELEM,
                        &raw mut val,
                    );
                } else {
                    let child = elements.get(idx);
                    push_value_for_level(child, &segs[1..], value, state);
                }
                // Trailing elements the set did not disturb.
                for e in elements.iter().skip(idx + 1) {
                    let mut e = *e;
                    pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_ELEM, &raw mut e);
                }
                pg_sys::pushJsonbValue(
                    state,
                    pg_sys::JsonbIteratorToken::WJB_END_ARRAY,
                    std::ptr::null_mut(),
                )
            }
        }
    }
}

/// Set `value` at `path` in `target`, creating intermediates, without
/// materializing the document.
///
/// Behaviourally identical to `jsonb_delta_set_path`, error text included. One
/// deliberate divergence: the rebuild recurses one level per path segment, so a
/// path longer than `MAX_JSONB_DEPTH` segments is rejected rather than risking
/// the backend stack -- the serde version had the same latent unbounded
/// recursion at output-serialization time and simply never guarded it.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_delta_set_path(target: RawJsonb, path: &str, value: RawJsonb) -> RawJsonb {
    let segments =
        crate::path::parse_path(path).unwrap_or_else(|e| error!("Invalid path '{}': {}", path, e));
    if segments.len() > crate::MAX_JSONB_DEPTH {
        error!(
            "Failed to set path '{}': path has {} segments, exceeds maximum depth {}",
            path,
            segments.len(),
            crate::MAX_JSONB_DEPTH
        );
    }
    // Reason: pointers come from detoasted datums and the palloc'ing builder.
    unsafe {
        let vroot = root_as_value(&value);
        if !depth_within(&vroot, 0, crate::MAX_JSONB_DEPTH) {
            error!(
                "JSONB nesting too deep (max {}, found depth {})",
                crate::MAX_JSONB_DEPTH,
                crate::MAX_JSONB_DEPTH + 1
            );
        }
        // Every index is validated (serde does this lazily while navigating; the
        // first over-limit index errors first either way, since the path is one
        // chain).
        for seg in &segments {
            if let crate::path::PathSegment::Index(idx) = seg {
                crate::depth::validate_array_index(*idx, crate::depth::MAX_JSONB_ARRAY_SIZE)
                    .unwrap_or_else(|e| error!("Failed to set path '{}': {}", path, e));
            }
        }
        let troot = root_as_value(&target);
        let mut state: *mut pg_sys::JsonbParseState = std::ptr::null_mut();
        let built = push_value_for_level(Some(&troot), &segments, &vroot, &raw mut state);
        RawJsonb(pg_sys::JsonbValueToJsonb(built))
    }
}

// ---------------------------------------------------------------------------
// Nested field update inside a matched array element
//
// Find the first element of a top-level array matching a key, then set a nested
// field inside it. The nested set reuses push_value_for_level, except for the
// serde quirk that the final value is set only when the last path segment is a
// key: a trailing array index is a silent no-op that still creates the parent
// intermediates. Non-matching elements and off-array keys pass through as binary.
// ---------------------------------------------------------------------------

/// Read the value at `segs` under `start`, or `None` if any step is missing or
/// type-mismatched -- exactly `crate::path::navigate_path`'s reading semantics.
///
/// # Safety
///
/// `start`'s payload and everything reached through it must outlive the call.
unsafe fn navigate_read(
    start: &pg_sys::JsonbValue,
    segs: &[crate::path::PathSegment],
) -> Option<pg_sys::JsonbValue> {
    use crate::path::PathSegment;
    unsafe {
        let mut current = *start;
        for seg in segs {
            if current.type_ != pg_sys::jbvType::jbvBinary {
                return None;
            }
            let container = current.val.binary.data;
            match seg {
                PathSegment::Key(k) => {
                    if (*container).header & pg_sys::JB_FOBJECT == 0 {
                        return None;
                    }
                    let mut kk = key_value(k);
                    let found = pg_sys::findJsonbValueFromContainer(
                        container,
                        pg_sys::JB_FOBJECT,
                        &raw mut kk,
                    );
                    if found.is_null() {
                        return None;
                    }
                    current = *found;
                }
                PathSegment::Index(i) => {
                    if (*container).header & pg_sys::JB_FARRAY == 0 {
                        return None;
                    }
                    let idx = u32::try_from(*i).unwrap_or(u32::MAX);
                    let found = pg_sys::getIthJsonbValueFromContainer(container, idx);
                    if found.is_null() {
                        return None;
                    }
                    current = *found;
                }
            }
        }
        Some(current)
    }
}

/// A `JsonbValue` for an empty object `{}`, the intermediate serde's parent
/// navigation synthesizes for an absent object key (`or_insert(Object)`).
///
/// # Safety
///
/// Uses the palloc'ing builder in the current memory context.
unsafe fn empty_object_value() -> pg_sys::JsonbValue {
    unsafe {
        let mut state: *mut pg_sys::JsonbParseState = std::ptr::null_mut();
        pg_sys::pushJsonbValue(
            &raw mut state,
            pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
            std::ptr::null_mut(),
        );
        let r = pg_sys::pushJsonbValue(
            &raw mut state,
            pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
            std::ptr::null_mut(),
        );
        let j = pg_sys::JsonbValueToJsonb(r);
        let mut v = std::mem::zeroed::<pg_sys::JsonbValue>();
        pg_sys::JsonbToJsonbValue(j, &raw mut v);
        v
    }
}

/// Push the matched `element` with `value` set at `segments`, as one array
/// element. Reproduces the serde rule that a path ending in an array index sets
/// nothing (a no-op that still creates the parent intermediates).
///
/// # Safety
///
/// `segments` is non-empty; all pointers must outlive the call.
unsafe fn push_updated_element(
    element: &pg_sys::JsonbValue,
    segments: &[crate::path::PathSegment],
    value: &pg_sys::JsonbValue,
    state: *mut *mut pg_sys::JsonbParseState,
) {
    use crate::path::PathSegment;
    unsafe {
        match segments.last().expect("segments is non-empty") {
            // The common case: a field name. Identical to set_path.
            PathSegment::Key(_) => {
                push_value_for_level(Some(element), segments, value, state);
            }
            PathSegment::Index(_) => {
                if segments.len() == 1 {
                    // A lone trailing index sets nothing: the element is untouched.
                    let mut e = *element;
                    pg_sys::pushJsonbValue(state, pg_sys::JsonbIteratorToken::WJB_ELEM, &raw mut e);
                } else {
                    // Navigate/create the parent path but leave its leaf as the
                    // navigation found or synthesized it -- serde skips the final
                    // index set. set_path over the parent with the leaf's own
                    // value reproduces exactly this (it overwrites the leaf with
                    // itself, or with the type-appropriate default when absent).
                    let parent = &segments[..segments.len() - 1];
                    let leaf = navigate_read(element, parent).unwrap_or_else(|| {
                        match parent.last().expect("parent is non-empty") {
                            PathSegment::Key(_) => empty_object_value(),
                            PathSegment::Index(_) => {
                                let mut n = std::mem::zeroed::<pg_sys::JsonbValue>();
                                n.type_ = pg_sys::jbvType::jbvNull;
                                n
                            }
                        }
                    });
                    push_value_for_level(Some(element), parent, &leaf, state);
                }
            }
        }
    }
}

/// Set a nested field in the first matching element of a top-level array, without
/// materializing the document.
///
/// Behaviourally identical to `jsonb_delta_array_update_where_path`, error text
/// and the trailing-index no-op included.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
// Reason: the navigate-read then set-path-with-leaf flow, plus the trailing-Index
// quirk handling, reads as one procedure; extracting fragments would obscure it.
#[allow(clippy::too_many_lines)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_delta_array_update_where_path(
    target: RawJsonb,
    array_key: &str,
    match_key: &str,
    match_value: RawJsonb,
    update_path: &str,
    update_value: RawJsonb,
) -> RawJsonb {
    crate::array_ops::validate_match_key(match_key).unwrap_or_else(|e| error!("{}", e));
    let segments = crate::path::parse_path(update_path)
        .unwrap_or_else(|e| error!("Invalid update path '{}': {}", update_path, e));
    if segments.len() > crate::MAX_JSONB_DEPTH {
        error!(
            "Invalid update path '{}': path has {} segments, exceeds maximum {}",
            update_path,
            segments.len(),
            crate::MAX_JSONB_DEPTH
        );
    }
    // Reason: pointers come from detoasted datums and the palloc'ing builder.
    unsafe {
        // The array must be present at the top-level key and be an array. A
        // non-object target reads as "does not exist", matching `get_mut`.
        let arr_val = if is_object(&target) {
            let mut k = key_value(array_key);
            let f = pg_sys::findJsonbValueFromContainer(
                target.container(),
                pg_sys::JB_FOBJECT,
                &raw mut k,
            );
            if f.is_null() {
                None
            } else {
                Some(*f)
            }
        } else {
            None
        };
        match arr_val {
            None => error!("Array path '{}' does not exist in document", array_key),
            Some(v) if array_container(&v).is_null() => error!(
                "Path '{}' does not point to an array, found: {}",
                array_key,
                jsonb_type_name(&v)
            ),
            Some(_) => {}
        }

        // Depth is validated before any match is sought, exactly as serde does.
        let uroot = root_as_value(&update_value);
        if !depth_within(&uroot, 0, crate::MAX_JSONB_DEPTH) {
            error!(
                "JSONB nesting too deep (max {}, found depth {})",
                crate::MAX_JSONB_DEPTH,
                crate::MAX_JSONB_DEPTH + 1
            );
        }
        let mv = root_as_value(&match_value);

        // Rebuild the document, transforming the first matching element only.
        let mut state: *mut pg_sys::JsonbParseState = std::ptr::null_mut();
        pg_sys::pushJsonbValue(
            &raw mut state,
            pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
            std::ptr::null_mut(),
        );
        let mut it = pg_sys::JsonbIteratorInit(target.container());
        let mut key = std::mem::zeroed::<pg_sys::JsonbValue>();
        let mut val = std::mem::zeroed::<pg_sys::JsonbValue>();
        pg_sys::JsonbIteratorNext(&raw mut it, &raw mut key, true);
        let mut done = false;
        loop {
            let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut key, true);
            if tok != pg_sys::JsonbIteratorToken::WJB_KEY {
                break;
            }
            let is_target = key.type_ == pg_sys::jbvType::jbvString
                && usize::try_from(key.val.string.len).unwrap_or(0) == array_key.len()
                && std::slice::from_raw_parts(key.val.string.val.cast::<u8>(), array_key.len())
                    == array_key.as_bytes();
            pg_sys::JsonbIteratorNext(&raw mut it, &raw mut val, true);
            pg_sys::pushJsonbValue(
                &raw mut state,
                pg_sys::JsonbIteratorToken::WJB_KEY,
                &raw mut key,
            );

            if !is_target {
                pg_sys::pushJsonbValue(
                    &raw mut state,
                    pg_sys::JsonbIteratorToken::WJB_VALUE,
                    &raw mut val,
                );
                continue;
            }

            pg_sys::pushJsonbValue(
                &raw mut state,
                pg_sys::JsonbIteratorToken::WJB_BEGIN_ARRAY,
                std::ptr::null_mut(),
            );
            let mut ait = pg_sys::JsonbIteratorInit(val.val.binary.data);
            let mut ev = std::mem::zeroed::<pg_sys::JsonbValue>();
            pg_sys::JsonbIteratorNext(&raw mut ait, &raw mut ev, true);
            loop {
                let etok = pg_sys::JsonbIteratorNext(&raw mut ait, &raw mut ev, true);
                if etok != pg_sys::JsonbIteratorToken::WJB_ELEM {
                    break;
                }
                if !done && element_matches(&ev, match_key, &mv) {
                    done = true;
                    // Parent index segments are bounds-checked, only on a match,
                    // exactly as serde validates them while navigating.
                    for seg in &segments[..segments.len() - 1] {
                        if let crate::path::PathSegment::Index(idx) = seg {
                            crate::depth::validate_array_index(
                                *idx,
                                crate::depth::MAX_JSONB_ARRAY_SIZE,
                            )
                            .unwrap_or_else(|e| error!("{}", e));
                        }
                    }
                    push_updated_element(&ev, &segments, &uroot, &raw mut state);
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
        RawJsonb(pg_sys::JsonbValueToJsonb(result))
    }
}

// ---------------------------------------------------------------------------
// Multi-row array update
//
// Apply one keyed update to a top-level array in each of many documents, as a
// set-returning function. Each row's work is the single-row update above, so no
// document is round-tripped; only the changed element of each is rebuilt.
// ---------------------------------------------------------------------------

/// Apply the shared update to one document's array, or raise with the serde
/// single-row function's exact text. Mirrors `jsonb_array_update_where_reference`:
/// a missing key or a non-array value at the key raises rather than no-ops.
///
/// # Safety
///
/// `target` must wrap a live document; `mv` and `updates` must outlive the call.
unsafe fn update_one_row(
    target: &RawJsonb,
    array_path: &str,
    match_key: &str,
    mv: &pg_sys::JsonbValue,
    updates: *mut pg_sys::JsonbContainer,
) -> RawJsonb {
    unsafe {
        let mut k = key_value(array_path);
        let found =
            pg_sys::findJsonbValueFromContainer(target.container(), pg_sys::JB_FOBJECT, &raw mut k);
        if found.is_null() {
            error!("Path '{}' does not exist in document", array_path);
        }
        let fv = *found;
        if array_container(&fv).is_null() {
            error!(
                "Path '{}' does not point to an array, found: {}",
                array_path,
                jsonb_type_name(&fv)
            );
        }
        RawJsonb(rebuild_with_array_transform(
            target.container(),
            array_path,
            match_key,
            mv,
            &ElementAction::Merge(updates),
            false,
        ))
    }
}

/// Update the first matching element of one array across many documents, without
/// materializing any of them.
///
/// Behaviourally identical to `jsonb_array_update_multi_row`: `updates` must be an
/// object (checked once), each document's first matching element is shallow-merged,
/// and a document missing the array or holding a non-array there raises with the
/// single-row function's text.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_multi_row(
    targets: pgrx::Array<RawJsonb>,
    array_path: &str,
    match_key: &str,
    match_value: RawJsonb,
    updates: RawJsonb,
) -> TableIterator<'static, (name!(result, RawJsonb),)> {
    crate::array_ops::validate_match_key(match_key).unwrap_or_else(|e| error!("{}", e));
    // Reason: pointers come from detoasted datums and the palloc'ing builder.
    //
    // TableIterator is a value-per-call SRF: its `next` runs across several
    // PostgreSQL calls, and the argument datums (targets, match_value, updates)
    // live only for the first. A lazy closure over them would dangle on the
    // second row. So every row is computed here, on the first call, while the
    // arguments are valid; pgrx runs this body in the multi-call memory context,
    // so the result documents outlive the iteration. Only the finished set is
    // handed to the iterator, which captures nothing borrowed.
    unsafe {
        if !is_object(&updates) {
            error!("updates argument must be a JSONB object");
        }
        let mv = root_as_value(&match_value);
        let updates_c = updates.container();
        // NULL elements are skipped, matching `targets.iter().flatten()`.
        let results: Vec<(RawJsonb,)> = targets
            .iter()
            .flatten()
            .map(|target| {
                (update_one_row(
                    &target, array_path, match_key, &mv, updates_c,
                ),)
            })
            .collect();
        TableIterator::new(results)
    }
}

// ---------------------------------------------------------------------------
// Coalesced changeset
//
// Unlike every function above, applying an ordered changeset of many
// heterogeneous ops needs a mutable document: op i+1 sees op i's effect, and the
// ops touch arbitrary paths in arbitrary order. That is what serde's Value tree
// is for, and its op logic is already audited and tested. What is wasteful is
// how pgrx's `JsonB` gets there and back -- jsonb -> text (jsonb_out) -> Value
// (serde parse) on the way in, and the reverse on the way out, four passes, two
// through a text form nothing needs.
//
// This replaces the input half with a direct binary -> Value walk and reuses the
// existing op logic unchanged. Numbers go through their canonical text so the
// integer-vs-float choice and the f64 rounding match serde_json's parser exactly
// -- which matters because serde_json is built without arbitrary_precision, so
// this path is deliberately as lossy as the serde original (1.50 -> 1.5), not
// more precise. Being more precise would *diverge* from the function it replaces.
// ---------------------------------------------------------------------------

/// Convert a jsonb value to a `serde_json::Value` exactly as pgrx's `JsonB`
/// (`jsonb_out` then `serde_json::from_str`) would, but without the text form.
///
/// # Safety
///
/// `v`'s payload and everything reached through it must outlive the call.
// Reason: the `jbvNull` arm is enumerated explicitly so the JsonbValue type
// dispatch reads exhaustively, even though the `_` fallback handles it identically.
#[allow(clippy::match_same_arms)]
unsafe fn value_from_jbv(v: &pg_sys::JsonbValue, depth: usize) -> serde_json::Value {
    use serde_json::Value;
    unsafe {
        // Bound the recursion off the backend stack. The serde original cannot
        // reach this: its JsonB parse trips serde_json's ~128-level limit first.
        if depth > crate::MAX_JSONB_DEPTH {
            error!(
                "JSONB nesting too deep (max {}, found depth {})",
                crate::MAX_JSONB_DEPTH,
                crate::MAX_JSONB_DEPTH + 1
            );
        }
        match v.type_ {
            pg_sys::jbvType::jbvNull => Value::Null,
            pg_sys::jbvType::jbvBool => Value::Bool(v.val.boolean),
            pg_sys::jbvType::jbvString => {
                let n = usize::try_from(v.val.string.len).unwrap_or(0);
                let bytes = std::slice::from_raw_parts(v.val.string.val.cast::<u8>(), n);
                Value::String(String::from_utf8_lossy(bytes).into_owned())
            }
            pg_sys::jbvType::jbvNumeric => {
                // The canonical numeric text is what jsonb_out emits, so parsing
                // it through serde reproduces the exact Number pgrx would build.
                pgrx::AnyNumeric::from_datum(pg_sys::Datum::from(v.val.numeric), false)
                    .and_then(|n| serde_json::from_str::<Value>(&n.to_string()).ok())
                    .unwrap_or(Value::Null)
            }
            pg_sys::jbvType::jbvBinary => {
                let c = v.val.binary.data;
                if (*c).header & pg_sys::JB_FARRAY != 0 {
                    let mut arr = Vec::new();
                    let mut it = pg_sys::JsonbIteratorInit(c);
                    let mut e = std::mem::zeroed::<pg_sys::JsonbValue>();
                    pg_sys::JsonbIteratorNext(&raw mut it, &raw mut e, true);
                    loop {
                        let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut e, true);
                        if tok != pg_sys::JsonbIteratorToken::WJB_ELEM {
                            break;
                        }
                        arr.push(value_from_jbv(&e, depth + 1));
                    }
                    Value::Array(arr)
                } else {
                    let mut map = serde_json::Map::new();
                    let mut it = pg_sys::JsonbIteratorInit(c);
                    let mut k = std::mem::zeroed::<pg_sys::JsonbValue>();
                    let mut val = std::mem::zeroed::<pg_sys::JsonbValue>();
                    pg_sys::JsonbIteratorNext(&raw mut it, &raw mut k, true);
                    loop {
                        let tok = pg_sys::JsonbIteratorNext(&raw mut it, &raw mut k, true);
                        if tok != pg_sys::JsonbIteratorToken::WJB_KEY {
                            break;
                        }
                        pg_sys::JsonbIteratorNext(&raw mut it, &raw mut val, true);
                        let n = usize::try_from(k.val.string.len).unwrap_or(0);
                        let key = String::from_utf8_lossy(std::slice::from_raw_parts(
                            k.val.string.val.cast::<u8>(),
                            n,
                        ))
                        .into_owned();
                        map.insert(key, value_from_jbv(&val, depth + 1));
                    }
                    Value::Object(map)
                }
            }
            _ => Value::Null,
        }
    }
}

/// A scalar `serde_json::Value` as a `JsonbValue`, matching what `jsonb_in` builds
/// from that value's canonical text. Numbers go through `numeric_in` (via
/// `AnyNumeric`) on their serde text, so the result is exactly `jsonb_in`'s numeric.
///
/// # Safety
///
/// The returned value borrows `v`'s string bytes / a freshly palloc'd numeric,
/// both of which must outlive the `JsonbValueToJsonb` that consumes it.
// Reason: the `Value::Null` arm is enumerated explicitly so the serde-value
// dispatch reads exhaustively, even though the `_` fallback handles it identically.
#[allow(clippy::match_same_arms)]
unsafe fn scalar_to_jbv(v: &serde_json::Value) -> pg_sys::JsonbValue {
    use serde_json::Value;
    unsafe {
        let mut jbv = std::mem::zeroed::<pg_sys::JsonbValue>();
        match v {
            Value::Null => jbv.type_ = pg_sys::jbvType::jbvNull,
            Value::Bool(b) => {
                jbv.type_ = pg_sys::jbvType::jbvBool;
                jbv.val.boolean = *b;
            }
            Value::String(s) => {
                jbv.type_ = pg_sys::jbvType::jbvString;
                jbv.val.string.len = i32::try_from(s.len()).unwrap_or(i32::MAX);
                jbv.val.string.val = s.as_ptr().cast::<std::ffi::c_char>().cast_mut();
            }
            Value::Number(n) => {
                jbv.type_ = pg_sys::jbvType::jbvNumeric;
                let numeric = pgrx::AnyNumeric::try_from(n.to_string().as_str())
                    .ok()
                    .and_then(pgrx::IntoDatum::into_datum)
                    .expect("a serde number is valid numeric text");
                jbv.val.numeric = numeric.cast_mut_ptr();
            }
            // containers are never passed here
            _ => jbv.type_ = pg_sys::jbvType::jbvNull,
        }
        jbv
    }
}

/// Push one member (`WJB_ELEM` in an array, `WJB_VALUE` in an object) of any
/// serde value: a scalar directly, a container by recursing.
///
/// # Safety
///
/// `v` and everything reached through it must outlive the call.
unsafe fn push_serde_member(
    v: &serde_json::Value,
    state: *mut *mut pg_sys::JsonbParseState,
    elem: bool,
) {
    use serde_json::Value;
    unsafe {
        match v {
            Value::Object(_) | Value::Array(_) => {
                push_serde_container(v, state);
            }
            scalar => {
                let mut jbv = scalar_to_jbv(scalar);
                let tok = if elem {
                    pg_sys::JsonbIteratorToken::WJB_ELEM
                } else {
                    pg_sys::JsonbIteratorToken::WJB_VALUE
                };
                pg_sys::pushJsonbValue(state, tok, &raw mut jbv);
            }
        }
    }
}

/// Push a serde object or array as a complete jsonb value, recursing into
/// members. Returns the value from the closing push (non-null only at the root).
///
/// # Safety
///
/// `v` must be an object or array, and outlive the call.
unsafe fn push_serde_container(
    v: &serde_json::Value,
    state: *mut *mut pg_sys::JsonbParseState,
) -> *mut pg_sys::JsonbValue {
    use serde_json::Value;
    unsafe {
        match v {
            Value::Object(map) => {
                pg_sys::pushJsonbValue(
                    state,
                    pg_sys::JsonbIteratorToken::WJB_BEGIN_OBJECT,
                    std::ptr::null_mut(),
                );
                for (k, val) in map {
                    let mut kjbv = key_value(k);
                    pg_sys::pushJsonbValue(
                        state,
                        pg_sys::JsonbIteratorToken::WJB_KEY,
                        &raw mut kjbv,
                    );
                    push_serde_member(val, state, false);
                }
                pg_sys::pushJsonbValue(
                    state,
                    pg_sys::JsonbIteratorToken::WJB_END_OBJECT,
                    std::ptr::null_mut(),
                )
            }
            Value::Array(arr) => {
                pg_sys::pushJsonbValue(
                    state,
                    pg_sys::JsonbIteratorToken::WJB_BEGIN_ARRAY,
                    std::ptr::null_mut(),
                );
                for val in arr {
                    push_serde_member(val, state, true);
                }
                pg_sys::pushJsonbValue(
                    state,
                    pg_sys::JsonbIteratorToken::WJB_END_ARRAY,
                    std::ptr::null_mut(),
                )
            }
            _ => unreachable!("push_serde_container only takes containers"),
        }
    }
}

/// Serialize a `serde_json::Value` straight to a jsonb datum, the inverse of
/// `value_from_jbv` and the exact output pgrx's `JsonB` would produce, without
/// the text form.
///
/// # Safety
///
/// `v` and everything reached through it must outlive the call.
unsafe fn value_to_jsonb(v: &serde_json::Value) -> *mut pg_sys::Jsonb {
    use serde_json::Value;
    unsafe {
        match v {
            Value::Object(_) | Value::Array(_) => {
                let mut state: *mut pg_sys::JsonbParseState = std::ptr::null_mut();
                let built = push_serde_container(v, &raw mut state);
                pg_sys::JsonbValueToJsonb(built)
            }
            scalar => {
                let mut jbv = scalar_to_jbv(scalar);
                pg_sys::JsonbValueToJsonb(&raw mut jbv)
            }
        }
    }
}

/// Apply an ordered changeset to a document in one parse/reserialize pass,
/// reading the document directly from its binary form and writing the result
/// straight back to binary.
///
/// Behaviourally identical to `jsonb_apply_changeset`: the op logic is the same
/// audited serde code, only the conversions on either side skip the text form.
// Reason: `#[pg_extern]` requires owned arguments, as above.
#[allow(clippy::needless_pass_by_value)]
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_apply_changeset(doc: RawJsonb, ops: RawJsonb) -> RawJsonb {
    // Reason: pointers come from detoasted datums; the Value tree is Rust-owned.
    unsafe {
        let doc_root = root_as_value(&doc);
        let mut root = value_from_jbv(&doc_root, 0);

        let ops_root = root_as_value(&ops);
        let ops_value = value_from_jbv(&ops_root, 0);
        let Some(ops_arr) = ops_value.as_array() else {
            error!(
                "ops argument must be a JSONB array, got: {}",
                crate::value_type_name(&ops_value)
            );
        };
        if ops_arr.len() > crate::changeset::MAX_CHANGESET_OPS {
            error!(
                "changeset has {} ops, exceeds maximum {}",
                ops_arr.len(),
                crate::changeset::MAX_CHANGESET_OPS
            );
        }

        crate::apply_changeset(&mut root, ops_arr).unwrap_or_else(|e| error!("{}", e));

        RawJsonb(value_to_jsonb(&root))
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
            (r"{}", r#"{"a":1}"#),
            (r#"{"a":1}"#, r"{}"),
            (r"{}", r"{}"),
            // non-ascii keys, with a recursion on one of them
            (
                r#"{"café":1,"日本":{"x":1}}"#,
                r#"{"café":2,"日本":{"y":2}}"#,
            ),
            // neither, or one, operand is an object: source wins wholesale
            (r"5", r#"{"a":1}"#),
            (r"[1,2]", r#"{"a":1}"#),
            (r#"{"a":1}"#, r"5"),
            (r#"{"a":1}"#, r"[1,2]"),
            (r"5", r"7"),
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

    /// Array insert across appends, array creation, and ordered insertion by
    /// integer / string / decimal keys -- including the placements that expose
    /// an off-by-one (front, back, between, equal, keyless).
    #[pg_test]
    fn array_insert_matches_serde() {
        // (target, array_path, element, sort_key_sql, sort_order_sql)
        let cases = [
            // append (no sort key)
            (
                r#"{"p":[{"id":1},{"id":2}]}"#,
                "p",
                r#"{"id":3}"#,
                "NULL",
                "NULL",
            ),
            (r#"{"p":[]}"#, "p", r#"{"id":1}"#, "NULL", "NULL"),
            // create the array when the key is absent
            (r"{}", "p", r#"{"id":1}"#, "NULL", "NULL"),
            (r#"{"a":1,"b":[9]}"#, "p", r#"{"id":1}"#, "NULL", "NULL"),
            // append a scalar element
            (r#"{"p":[1,2]}"#, "p", r"3", "NULL", "NULL"),
            // ordered insert by integer id: between, front, back, equal
            (
                r#"{"p":[{"id":1},{"id":3},{"id":5}]}"#,
                "p",
                r#"{"id":4}"#,
                "'id'",
                "'ASC'",
            ),
            (
                r#"{"p":[{"id":2},{"id":4}]}"#,
                "p",
                r#"{"id":1}"#,
                "'id'",
                "'ASC'",
            ),
            (
                r#"{"p":[{"id":2},{"id":4}]}"#,
                "p",
                r#"{"id":9}"#,
                "'id'",
                "'ASC'",
            ),
            (
                r#"{"p":[{"id":2},{"id":4}]}"#,
                "p",
                r#"{"id":4}"#,
                "'id'",
                "'ASC'",
            ),
            // descending
            (
                r#"{"p":[{"id":5},{"id":3},{"id":1}]}"#,
                "p",
                r#"{"id":4}"#,
                "'id'",
                "'DESC'",
            ),
            // string sort key (timestamps), and the NULL sort_order default (ASC)
            (
                r#"{"p":[{"c":"2025-01-01"},{"c":"2025-01-03"}]}"#,
                "p",
                r#"{"c":"2025-01-02"}"#,
                "'c'",
                "'ASC'",
            ),
            (
                r#"{"p":[{"id":1},{"id":3}]}"#,
                "p",
                r#"{"id":2}"#,
                "'id'",
                "NULL",
            ),
            // decimal sort key -- AnyNumeric orders these the same as serde's f64
            (
                r#"{"p":[{"id":1.5},{"id":3.5}]}"#,
                "p",
                r#"{"id":2.5}"#,
                "'id'",
                "'ASC'",
            ),
            // new element lacks the sort key -> append
            (
                r#"{"p":[{"id":1},{"id":3}]}"#,
                "p",
                r#"{"x":9}"#,
                "'id'",
                "'ASC'",
            ),
            // an existing element lacks the sort key -> it sorts before keyed ones
            (
                r#"{"p":[{"x":0},{"id":3}]}"#,
                "p",
                r#"{"id":2}"#,
                "'id'",
                "'ASC'",
            ),
        ];
        for (t, path, e, sk, so) in cases {
            assert_matches_serde(
                &format!("jsonb_array_insert_where('{t}','{path}','{e}',{sk},{so})"),
                &format!("jsonb_array_insert_where_reference('{t}','{path}','{e}',{sk},{so})"),
            );
        }
    }

    /// The two failure modes (non-object target, non-array value at the key)
    /// raise identical text to the serde original, across value types.
    #[pg_test]
    fn array_insert_errors_match_serde() {
        let cases = [
            (r"[1,2]", "p", r#"{"id":1}"#),
            (r"5", "p", r#"{"id":1}"#),
            (r#""s""#, "p", r#"{"id":1}"#),
            (r#"{"p":5}"#, "p", r#"{"id":1}"#),
            (r#"{"p":{"x":1}}"#, "p", r#"{"id":1}"#),
            (r#"{"p":"str"}"#, "p", r#"{"id":1}"#),
        ];
        for (t, path, e) in cases {
            assert_same_outcome(
                &format!("jsonb_array_insert_where('{t}','{path}','{e}',NULL,NULL)"),
                &format!("jsonb_array_insert_where_reference('{t}','{path}','{e}',NULL,NULL)"),
            );
        }
    }

    /// The general nested setter, compared as data across creation, padding,
    /// destructive type replacement, sibling/trailing preservation, root
    /// replacement, container values, and every parse/index error. Run through
    /// `assert_same_outcome` so a value result and an error message are both
    /// covered by one comparison.
    #[pg_test]
    fn set_path_matches_serde() {
        // (target, path, value)
        let cases = [
            // create / nested create
            (r#"{"user":{}}"#, "user.name", r#""Alice""#),
            (r"{}", "user.profile.settings.theme", r#""dark""#),
            (r#"{"a":1}"#, "b", r"2"),
            // overwrite an existing value wholesale
            (r#"{"a":{"b":1}}"#, "a", r"9"),
            (r#"{"a":{"b":1}}"#, "a.b", r"99"),
            // sibling preservation
            (r#"{"a":1,"b":{"x":1,"y":2}}"#, "b.x", r"99"),
            // destructive type replacement along the path
            (r#"{"a":5}"#, "a.b", r"9"),
            (r#"{"a":[1,2]}"#, "a.b", r"9"),
            (r#"{"a":{"x":1}}"#, "a[0]", r"9"),
            // array index: create, pad with nulls, preserve trailing
            (r#"{"items":[]}"#, "items[0]", r#""first""#),
            (r#"{"items":[]}"#, "items[3]", r"9"),
            (r#"{"a":[10,20,30]}"#, "a[1]", r"99"),
            (r#"{"a":[1,2,3,4,5]}"#, "a[2]", r"99"),
            // mixed key/index, with creation of the whole chain
            (r#"{"orders":[{}]}"#, "orders[0].id", r"5"),
            (r"{}", "a[0].b[1].c", r"7"),
            // container-typed values
            (r"{}", "a", r#"{"x":1}"#),
            (r"{}", "a", r"[1,2,3]"),
            // root itself is not the container the first segment needs
            (r"5", "a", r"9"),
            (r"[1,2]", "a", r"9"),
            (r"5", "[0]", r"9"),
            (r#"{"a":1}"#, "[0]", r"9"),
            // non-ascii key
            (r#"{"café":{}}"#, "café.日本", r"1"),
            // parse errors
            (r"{}", "a..b", r"1"),
            (r"{}", "a[]", r"1"),
            (r"{}", "a]", r"1"),
            (r"{}", "", r"1"),
            // index over the size limit
            (r"{}", "arr[200000]", r"1"),
        ];
        for (t, path, v) in cases {
            assert_same_outcome(
                &format!("jsonb_delta_set_path('{t}','{path}','{v}')"),
                &format!("jsonb_delta_set_path_reference('{t}','{path}','{v}')"),
            );
        }
    }

    /// Value depth is enforced at the documented 1000-level cap, the same
    /// deliberate divergence as deep merge (the serde original trips `serde_json`'s
    /// ~128 parse limit long before its own guard). Asserted directly.
    #[pg_test]
    fn set_path_enforces_documented_depth_limit() {
        let out = outcome(
            r#"jsonb_delta_set_path('{}','a',(repeat('{"a":',1001)||'1'||repeat('}',1001))::jsonb)"#,
        );
        assert_eq!(
            out,
            "ERROR: JSONB nesting too deep (max 1000, found depth 1001)"
        );
    }

    /// Nested field update inside the first matched array element: the common
    /// key-terminated paths, plus the trailing-index no-op quirk, first-match,
    /// intermediate creation, destructive type replacement, and no-match.
    // Reason: a table-driven differential test; its length is the case table, not
    // branching complexity, and it reads better as one list than split apart.
    #[allow(clippy::too_many_lines)]
    #[pg_test]
    fn array_update_where_path_matches_serde() {
        // (target, array_key, match_key, match_value, update_path, update_value)
        let cases = [
            (
                r#"{"users":[{"id":1,"profile":{"name":"Alice","city":"NY"}}]}"#,
                "users",
                "id",
                "1",
                "profile.name",
                r#""Bob""#,
            ),
            (
                r#"{"users":[{"id":1}]}"#,
                "users",
                "id",
                "1",
                "profile.name",
                r#""X""#,
            ),
            (r#"{"users":[{"id":1}]}"#, "users", "id", "1", "a.b.c", r"9"),
            // destructive type replacement along the path
            (
                r#"{"users":[{"id":1,"profile":5}]}"#,
                "users",
                "id",
                "1",
                "profile.name",
                r#""X""#,
            ),
            // mixed key/index path ending in a key
            (
                r#"{"users":[{"id":1,"orders":[{"id":9,"s":"new"}]}]}"#,
                "users",
                "id",
                "1",
                "orders[0].s",
                r#""shipped""#,
            ),
            // no match -> document unchanged
            (
                r#"{"users":[{"id":1}]}"#,
                "users",
                "id",
                "99",
                "profile.name",
                r#""X""#,
            ),
            // only the first of two matches is updated
            (
                r#"{"users":[{"id":1,"n":"a"},{"id":1,"n":"b"}]}"#,
                "users",
                "id",
                "1",
                "n",
                r#""Z""#,
            ),
            // container-typed update values
            (
                r#"{"users":[{"id":1}]}"#,
                "users",
                "id",
                "1",
                "meta",
                r#"{"x":1}"#,
            ),
            (
                r#"{"users":[{"id":1}]}"#,
                "users",
                "id",
                "1",
                "meta",
                r"[1,2,3]",
            ),
            // text match key
            (
                r#"{"users":[{"id":"a","x":1},{"id":"b","x":2}]}"#,
                "users",
                "id",
                r#""b""#,
                "x",
                r"9",
            ),
            // trailing-index no-op: lone index, and a nested index over absent /
            // existing-object / existing-array leaves
            (
                r#"{"users":[{"id":1,"n":"a"}]}"#,
                "users",
                "id",
                "1",
                "[0]",
                r#""ignored""#,
            ),
            (
                r#"{"users":[{"id":1}]}"#,
                "users",
                "id",
                "1",
                "a[0]",
                r#""ignored""#,
            ),
            (
                r#"{"users":[{"id":1,"a":{"z":1}}]}"#,
                "users",
                "id",
                "1",
                "a[0]",
                r#""ignored""#,
            ),
            (
                r#"{"users":[{"id":1,"a":[10,20]}]}"#,
                "users",
                "id",
                "1",
                "a[3]",
                r#""ignored""#,
            ),
            // non-ascii, and preservation of sibling document keys
            (
                r#"{"u":[{"id":1,"café":{}}]}"#,
                "u",
                "id",
                "1",
                "café.日本",
                r"1",
            ),
            (
                r#"{"n":9,"users":[{"id":1}],"m":[1,2]}"#,
                "users",
                "id",
                "1",
                "x",
                r"7",
            ),
        ];
        for (t, ak, mk, mv, up, uv) in cases {
            assert_same_outcome(
                &format!(
                    "jsonb_delta_array_update_where_path('{t}','{ak}','{mk}','{mv}','{up}','{uv}')"
                ),
                &format!(
                    "jsonb_delta_array_update_where_path_reference('{t}','{ak}','{mk}','{mv}','{up}','{uv}')"
                ),
            );
        }
    }

    /// Every failure mode: empty match key, bad update path, missing / non-array
    /// target key, non-object target, and an over-limit parent index (validated
    /// only on a match, exactly as serde does).
    #[pg_test]
    fn array_update_where_path_errors_match_serde() {
        let cases = [
            (r#"{"users":[{"id":1}]}"#, "users", "", "1", "x", r"1"),
            (r#"{"users":[{"id":1}]}"#, "users", "id", "1", "a..b", r"1"),
            (r#"{"users":[{"id":1}]}"#, "users", "id", "1", "", r"1"),
            (r#"{"x":1}"#, "users", "id", "1", "a", r"1"),
            (r#"{"users":5}"#, "users", "id", "1", "a", r"1"),
            (r#"{"users":{"x":1}}"#, "users", "id", "1", "a", r"1"),
            (r"[1,2]", "users", "id", "1", "a", r"1"),
            (r"5", "users", "id", "1", "a", r"1"),
            (
                r#"{"users":[{"id":1}]}"#,
                "users",
                "id",
                "1",
                "a[200000].x",
                r"1",
            ),
        ];
        for (t, ak, mk, mv, up, uv) in cases {
            assert_same_outcome(
                &format!(
                    "jsonb_delta_array_update_where_path('{t}','{ak}','{mk}','{mv}','{up}','{uv}')"
                ),
                &format!(
                    "jsonb_delta_array_update_where_path_reference('{t}','{ak}','{mk}','{mv}','{up}','{uv}')"
                ),
            );
        }
    }

    #[pg_test]
    fn array_update_where_path_enforces_depth_limit() {
        let out = outcome(
            r#"jsonb_delta_array_update_where_path('{"u":[{"id":1}]}','u','id','1','x',(repeat('{"a":',1001)||'1'||repeat('}',1001))::jsonb)"#,
        );
        assert_eq!(
            out,
            "ERROR: JSONB nesting too deep (max 1000, found depth 1001)"
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

    /// Multi-row update: the whole result set must agree with the serde original,
    /// row order included, across duplicates, no-match rows, skipped NULL
    /// elements, an empty input, and a text match key.
    #[pg_test]
    fn multi_row_matches_serde() {
        // (targets_sql, array_key, match_key, match_value, updates)
        let cases = [
            (
                r#"ARRAY['{"p":[{"id":1,"n":"a"}]}','{"p":[{"id":1,"n":"b"},{"id":2}]}']::jsonb[]"#,
                "p",
                "id",
                "1",
                r#"{"x":9}"#,
            ),
            // duplicate matches in one document: only the first changes
            (
                r#"ARRAY['{"p":[{"id":1},{"id":1}]}']::jsonb[]"#,
                "p",
                "id",
                "1",
                r#"{"x":9}"#,
            ),
            // a no-match document is returned unchanged
            (
                r#"ARRAY['{"p":[{"id":5}]}','{"p":[{"id":1}]}']::jsonb[]"#,
                "p",
                "id",
                "1",
                r#"{"x":9}"#,
            ),
            // NULL elements are skipped
            (
                r#"ARRAY['{"p":[{"id":1}]}',NULL,'{"p":[{"id":1}]}']::jsonb[]"#,
                "p",
                "id",
                "1",
                r#"{"x":9}"#,
            ),
            // empty input
            (r"ARRAY[]::jsonb[]", "p", "id", "1", r#"{"x":9}"#),
            // text match key
            (
                r#"ARRAY['{"p":[{"id":"a"},{"id":"b"}]}']::jsonb[]"#,
                "p",
                "id",
                r#""b""#,
                r#"{"x":9}"#,
            ),
        ];
        for (tg, ak, mk, mv, up) in cases {
            assert_same_outcome(
                &format!("(SELECT array_agg(result) FROM jsonb_array_update_multi_row({tg},'{ak}','{mk}','{mv}','{up}'))"),
                &format!("(SELECT array_agg(result) FROM jsonb_array_update_multi_row_reference({tg},'{ak}','{mk}','{mv}','{up}'))"),
            );
        }
    }

    /// The upfront errors (bad match key, non-object updates) and the per-row
    /// errors (a document missing the array, or holding a non-array there) all
    /// raise identically.
    #[pg_test]
    fn multi_row_errors_match_serde() {
        let cases = [
            (
                r#"ARRAY['{"p":[{"id":1}]}']::jsonb[]"#,
                "p",
                "id",
                "1",
                r"5",
            ),
            (
                r#"ARRAY['{"p":[{"id":1}]}']::jsonb[]"#,
                "p",
                "",
                "1",
                r#"{"x":9}"#,
            ),
            (
                r#"ARRAY['{"p":[{"id":1}]}','{"q":1}']::jsonb[]"#,
                "p",
                "id",
                "1",
                r#"{"x":9}"#,
            ),
            (r#"ARRAY['{"p":5}']::jsonb[]"#, "p", "id", "1", r#"{"x":9}"#),
        ];
        for (tg, ak, mk, mv, up) in cases {
            assert_same_outcome(
                &format!("(SELECT array_agg(result) FROM jsonb_array_update_multi_row({tg},'{ak}','{mk}','{mv}','{up}'))"),
                &format!("(SELECT array_agg(result) FROM jsonb_array_update_multi_row_reference({tg},'{ak}','{mk}','{mv}','{up}'))"),
            );
        }
    }

    /// The coalesced changeset, every op type against the serde original: set /
    /// remove / merge / `deep_merge` / increment / array_{`update,update_all,replace`,
    /// upsert,delete,insert}, both path forms, multi-op sequences, text keys, and
    /// -- critically -- number scale, which must be lost identically (`serde_json`
    /// has no `arbitrary_precision`, so 1.50 becomes 1.5 in both).
    #[pg_test]
    fn apply_changeset_matches_serde() {
        // (doc, ops)
        let cases = [
            // set: dot path, array-of-segments path, and an array index
            (r#"{"a":1}"#, r#"[{"op":"set","path":"b.c","value":9}]"#),
            (r#"{"a":1}"#, r#"[{"op":"set","path":["b","c"],"value":9}]"#),
            (
                r#"{"a":1}"#,
                r#"[{"op":"set","path":"items[0]","value":"x"}]"#,
            ),
            // remove: key, index, and an absent no-op
            (r#"{"a":1,"b":2}"#, r#"[{"op":"remove","path":"a"}]"#),
            (r#"{"a":[1,2,3]}"#, r#"[{"op":"remove","path":"a[1]"}]"#),
            (r#"{"a":1}"#, r#"[{"op":"remove","path":"nope"}]"#),
            // merge (shallow), deep_merge, and empty-path merge at root
            (
                r#"{"a":{"x":1,"y":2}}"#,
                r#"[{"op":"merge","path":"a","value":{"x":9}}]"#,
            ),
            (
                r#"{"a":{"x":1,"n":{"p":1,"q":2}}}"#,
                r#"[{"op":"deep_merge","path":"a","value":{"n":{"p":9}}}]"#,
            ),
            (r#"{"x":1}"#, r#"[{"op":"merge","value":{"y":2}}]"#),
            // increment: integer, float, and an absent counter (starts at 0)
            (r#"{"n":5}"#, r#"[{"op":"increment","path":"n","by":3}]"#),
            (r#"{"n":5}"#, r#"[{"op":"increment","path":"n","by":2.5}]"#),
            (r"{}", r#"[{"op":"increment","path":"c","by":1}]"#),
            // array ops
            (
                r#"{"p":[{"id":1,"n":"a"},{"id":2,"n":"b"},{"id":1,"n":"c"}]}"#,
                r#"[{"op":"array_update","path":"p","match_key":"id","match_value":1,"value":{"n":"Z"}}]"#,
            ),
            (
                r#"{"p":[{"id":1},{"id":1}]}"#,
                r#"[{"op":"array_update_all","path":"p","match_key":"id","match_value":1,"value":{"n":"Z"}}]"#,
            ),
            (
                r#"{"p":[{"id":1,"n":"a"}]}"#,
                r#"[{"op":"array_replace","path":"p","match_key":"id","match_value":1,"value":{"id":1,"x":9}}]"#,
            ),
            (
                r#"{"p":[{"id":1}]}"#,
                r#"[{"op":"array_upsert","path":"p","match_key":"id","match_value":2,"value":{"id":2,"new":true}}]"#,
            ),
            (
                r#"{"p":[{"id":1}]}"#,
                r#"[{"op":"array_upsert","path":"p","match_key":"id","match_value":1,"value":{"z":9}}]"#,
            ),
            (
                r#"{"p":[{"id":1},{"id":2}]}"#,
                r#"[{"op":"array_delete","path":"p","match_key":"id","match_value":1}]"#,
            ),
            (
                r#"{"p":[{"id":1}]}"#,
                r#"[{"op":"array_insert","path":"p","value":{"id":2}}]"#,
            ),
            (
                r#"{"p":[{"c":1},{"c":3}]}"#,
                r#"[{"op":"array_insert","path":"p","value":{"c":2},"sort_key":"c","sort_order":"ASC"}]"#,
            ),
            // multi-op coalescing in a single call
            (
                r#"{"stats":{"count":10},"posts":[{"id":1}]}"#,
                r#"[{"op":"increment","path":"stats.count","by":1},{"op":"array_insert","path":"posts","value":{"id":2}},{"op":"set","path":"updated","value":true}]"#,
            ),
            // number scale is lost identically (the correctness risk of this port)
            (r#"{"price":1.50,"qty":2}"#, r"[]"),
            (
                r#"{"price":1.50}"#,
                r#"[{"op":"set","path":"x","value":1}]"#,
            ),
            (
                r#"{"a":-5,"b":0,"big":9007199254740993}"#,
                r#"[{"op":"set","path":"c","value":1}]"#,
            ),
            // text match key
            (
                r#"{"p":[{"id":"a"},{"id":"b"}]}"#,
                r#"[{"op":"array_update","path":"p","match_key":"id","match_value":"b","value":{"x":9}}]"#,
            ),
            // a non-object document, replaced wholesale by a root set
            (r"5", r#"[{"op":"set","path":"","value":{"a":1}}]"#),
        ];
        for (doc, ops) in cases {
            assert_same_outcome(
                &format!("jsonb_apply_changeset('{doc}','{ops}')"),
                &format!("jsonb_apply_changeset_reference('{doc}','{ops}')"),
            );
        }
    }

    /// Every malformed changeset raises identically: unknown op, missing op /
    /// value fields, a non-object op, a non-array ops argument, a wrong-typed
    /// increment, a non-object merge value, and an integer overflow.
    #[pg_test]
    fn apply_changeset_errors_match_serde() {
        let cases = [
            (r#"{"a":1}"#, r#"[{"op":"nonsense","path":"a"}]"#),
            (r#"{"a":1}"#, r#"[{"path":"a"}]"#),
            (r#"{"a":1}"#, r#"[{"op":"set","path":"a"}]"#),
            (r#"{"a":1}"#, r"[5]"),
            (r#"{"a":1}"#, r"{}"),
            (r#"{"a":1}"#, r#"[{"op":"increment","path":"a","by":"x"}]"#),
            (
                r#"{"a":{"x":1}}"#,
                r#"[{"op":"merge","path":"a","value":5}]"#,
            ),
            (
                r#"{"a":9223372036854775807}"#,
                r#"[{"op":"increment","path":"a","by":1}]"#,
            ),
        ];
        for (doc, ops) in cases {
            assert_same_outcome(
                &format!("jsonb_apply_changeset('{doc}','{ops}')"),
                &format!("jsonb_apply_changeset_reference('{doc}','{ops}')"),
            );
        }
    }
}
