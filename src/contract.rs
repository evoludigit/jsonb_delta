// jsonb_delta - Exported-signature contract test (Phase 3, Cycle 1 — #12)
//
// Pins the extension's complete exported surface (name + argument types) against a
// frozen inventory, so a signature drift can never again be discovered first by a
// downstream consumer (pg_tviews — see fraiseql/pg_tviews#50).
//
// The inventory mirrors degustation's frozen contract
// (degustation/contracts/specs/c4_tviews_jsonb_delta.toml, [[exports]]), measured
// against jsonb_delta control 0.1.0 / Cargo 0.2.0.
//
// This module is compiled only under `test` / `pg_test`; it is never part of the
// shipped extension.

/// jsonb_delta's complete exported surface: (function name, comma-joined argument
/// types in order). Any drift — a function added, removed, or with changed argument
/// types — fails `exported_signatures_match_frozen_contract`.
///
/// Keep this list sorted by name; the test compares it as an exact set.
#[cfg(any(test, feature = "pg_test"))]
pub(crate) const FROZEN_EXPORTS: &[(&str, &str)] = &[
    ("jsonb_array_contains_id", "jsonb, text, text, jsonb"),
    ("jsonb_array_delete_where", "jsonb, text, text, jsonb"),
    ("jsonb_array_insert_where", "jsonb, text, jsonb, text, text"),
    (
        "jsonb_array_update_multi_row",
        "jsonb[], text, text, jsonb, jsonb",
    ),
    (
        "jsonb_array_update_where",
        "jsonb, text, text, jsonb, jsonb",
    ),
    ("jsonb_array_update_where_batch", "jsonb, text, text, jsonb"),
    ("jsonb_deep_merge", "jsonb, jsonb"),
    (
        "jsonb_delta_array_update_where_path",
        "jsonb, text, text, jsonb, text, jsonb",
    ),
    ("jsonb_delta_set_path", "jsonb, text, jsonb"),
    ("jsonb_extract_id", "jsonb, text"),
    ("jsonb_merge_at_path", "jsonb, jsonb, text[]"),
    ("jsonb_merge_shallow", "jsonb, jsonb"),
    ("jsonb_smart_patch_array", "jsonb, jsonb, text, text, jsonb"),
    ("jsonb_smart_patch_nested", "jsonb, jsonb, text[]"),
    ("jsonb_smart_patch_scalar", "jsonb, jsonb"),
];

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use super::FROZEN_EXPORTS;
    use pgrx::prelude::*;

    /// One `name(argtypes)` line per exported function, sorted, newline-joined.
    fn canonical(sigs: impl Iterator<Item = (String, String)>) -> String {
        let mut lines: Vec<String> = sigs.map(|(n, a)| format!("{n}({a})")).collect();
        lines.sort();
        lines.join("\n")
    }

    /// The extension's actual exported surface from the catalog: every function owned
    /// by the `jsonb_delta` extension (`pg_depend` deptype 'e'), argument types built
    /// from `proargtypes` — name-independent, since arg names are not part of the
    /// callable contract; the types and their order are.
    fn actual_canonical() -> String {
        Spi::get_one::<String>(
            "SELECT string_agg(p.proname || '(' || \
                COALESCE((SELECT string_agg(format_type(t, NULL), ', ' ORDER BY ord) \
                          FROM unnest(p.proargtypes) WITH ORDINALITY AS a(t, ord)), '') \
                || ')', E'\\n' ORDER BY 1) \
             FROM pg_proc p \
             JOIN pg_depend d ON d.objid = p.oid AND d.deptype = 'e' \
             JOIN pg_extension e ON e.oid = d.refobjid \
             JOIN pg_namespace ns ON ns.oid = p.pronamespace \
             WHERE e.extname = 'jsonb_delta' \
               AND ns.nspname <> 'tests'",
        )
        .expect("SPI ok")
        .unwrap_or_default()
    }

    #[pg_test]
    fn exported_signatures_match_frozen_contract() {
        let expected = canonical(
            FROZEN_EXPORTS
                .iter()
                .map(|(n, a)| ((*n).to_string(), (*a).to_string())),
        );
        let actual = actual_canonical();
        assert_eq!(
            actual, expected,
            "\nexported-signature contract drift (a downstream consumer must never be the \
             first to find this):\n--- frozen ---\n{expected}\n--- actual ---\n{actual}\n"
        );
    }
}
