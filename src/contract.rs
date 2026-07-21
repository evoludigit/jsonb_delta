//! Exported-signature contract test (issue #12).
//!
//! `pg_tviews` called a `jsonb_smart_patch_array` signature (4-arg `TEXT[]`) that
//! this extension never exported. The mismatch went unnoticed for months because
//! it lived only in a downstream test stub, not against the real extension
//! (`fraiseql/pg_tviews#50`). The durable fix is on this side: freeze `jsonb_delta`'s
//! entire exported surface against `pg_proc` so any signature drift fails *here*,
//! in this repo's own suite, instead of surfacing as a `function ... does not
//! exist` error in a consumer at runtime.
//!
//! If you intentionally change a signature, update [`tests::EXPECTED`] — and
//! treat that edit as a contract change that needs a CHANGELOG entry and, for a
//! released version, an upgrade script in `sql/`.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    /// Every function `jsonb_delta` exports, rendered as
    /// `name(args) -> result [<volatility><strict?> <parallel>]` and sorted by
    /// name then identity arguments. Volatility `i` = immutable; parallel `s` =
    /// safe; `strict` is present unless the function is invoked on NULL input
    /// (only `jsonb_array_insert_where`, whose optional sort args are nullable).
    ///
    /// This string is the published contract. It was captured from a real
    /// `CREATE EXTENSION jsonb_delta` install, not hand-written.
    const EXPECTED: &str = "\
jsonb_apply_changeset(doc jsonb, ops jsonb) -> jsonb [i strict s]
jsonb_array_contains_id(data jsonb, array_path text, id_key text, id_value jsonb) -> boolean [i strict s]
jsonb_array_delete_where(target jsonb, array_path text, match_key text, match_value jsonb) -> jsonb [i strict s]
jsonb_array_insert_where(target jsonb, array_path text, new_element jsonb, sort_key text, sort_order text) -> jsonb [i - s]
jsonb_array_update_multi_row(targets jsonb[], array_path text, match_key text, match_value jsonb, updates jsonb) -> TABLE(result jsonb) [i strict s]
jsonb_array_update_where(target jsonb, array_path text, match_key text, match_value jsonb, updates jsonb) -> jsonb [i strict s]
jsonb_array_update_where_batch(target jsonb, array_path text, match_key text, updates_array jsonb) -> jsonb [i strict s]
jsonb_deep_merge(target jsonb, source jsonb) -> jsonb [i strict s]
jsonb_delta_array_update_where_path(target jsonb, array_key text, match_key text, match_value jsonb, update_path text, update_value jsonb) -> jsonb [i strict s]
jsonb_delta_set_path(target jsonb, path text, value jsonb) -> jsonb [i strict s]
jsonb_extract_id(data jsonb, key text DEFAULT 'id'::text) -> text [i strict s]
jsonb_merge_at_path(target jsonb, source jsonb, path text[]) -> jsonb [i strict s]
jsonb_merge_shallow(target jsonb, source jsonb) -> jsonb [i strict s]
jsonb_smart_patch_array(target jsonb, source jsonb, array_path text, match_key text, match_value jsonb) -> jsonb [i strict s]
jsonb_smart_patch_nested(target jsonb, source jsonb, path text[]) -> jsonb [i strict s]
jsonb_smart_patch_scalar(target jsonb, source jsonb) -> jsonb [i strict s]";

    /// The exported surface of the currently-installed extension, in the exact
    /// shape [`EXPECTED`] pins.
    ///
    /// The two filters describe the *shipped* surface. Under `cargo pgrx test`
    /// the extension is built with the `pg_test` feature, which also exposes the
    /// serde `_reference` oracles (public schema, `_reference` suffix) and the
    /// `#[pg_test]` functions (in the `tests` schema). Neither ships in a release
    /// build, so both filters are no-ops there; here they exclude the test-only
    /// artifacts so the assertion is about the real contract, not the harness.
    fn exported_surface() -> String {
        Spi::get_one::<String>(
            "SELECT string_agg(
               p.proname || '(' || pg_get_function_arguments(p.oid) || ') -> '
                 || pg_get_function_result(p.oid) || ' [' || p.provolatile::text
                 || CASE WHEN p.proisstrict THEN ' strict' ELSE ' -' END
                 || ' ' || p.proparallel::text || ']',
               E'\n' ORDER BY p.proname, pg_get_function_identity_arguments(p.oid))
             FROM pg_depend d
             JOIN pg_proc p ON p.oid = d.objid
             JOIN pg_extension e ON e.oid = d.refobjid
             JOIN pg_namespace n ON n.oid = p.pronamespace
             WHERE d.deptype = 'e' AND e.extname = 'jsonb_delta'
               AND n.nspname = 'public'
               AND right(p.proname, 10) <> '_reference'",
        )
        .expect("introspection query failed")
        .expect("jsonb_delta exports no functions — is the extension installed?")
    }

    #[pg_test]
    fn exported_signatures_match_the_frozen_contract() {
        let actual = exported_surface();
        assert_eq!(
            actual, EXPECTED,
            "\n\njsonb_delta's exported SQL surface drifted from the frozen contract.\n\
             If this change is intentional, update EXPECTED in src/contract.rs and record \
             the contract change in CHANGELOG.md (ship an upgrade script for a released \
             version).\n\n--- actual ---\n{actual}\n\n--- expected ---\n{EXPECTED}\n",
        );
    }

    /// `jsonb_smart_patch_scalar(jsonb, jsonb) -> jsonb` is the single symbol
    /// `pg_tviews`' runtime actually depends on (issue #12, `fraiseql/pg_tviews#50`).
    /// It is pinned on its own so a change to it is unmissable, independent of
    /// the full-surface diff above.
    #[pg_test]
    fn smart_patch_scalar_signature_is_stable_for_pg_tviews() {
        let sig = Spi::get_one::<String>(
            "SELECT pg_get_function_arguments(p.oid) || ' -> ' || pg_get_function_result(p.oid)
             FROM pg_proc p
             JOIN pg_namespace n ON n.oid = p.pronamespace
             WHERE p.proname = 'jsonb_smart_patch_scalar' AND n.nspname = 'public'",
        )
        .expect("query failed")
        .expect("jsonb_smart_patch_scalar is not exported — pg_tviews depends on it");
        assert_eq!(
            sig, "target jsonb, source jsonb -> jsonb",
            "jsonb_smart_patch_scalar(jsonb, jsonb) -> jsonb is the one signature pg_tviews \
             calls; changing it breaks the consumer (jsonb_delta #12).",
        );
    }
}
