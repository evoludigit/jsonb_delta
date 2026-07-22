/* jsonb_delta 0.3.0 -> 0.3.1 upgrade script.

   0.3.1 is a packaging-only release: it exists so the fixed release automation
   (issue #24) produces a GitHub Release with prebuilt pg13-17 packages, which
   the manually-cut 0.3.0 Release lacked. There is NO change to the extension
   itself — the shared object, every function signature, volatility, strictness
   and parallel-safety are byte-for-byte those of 0.3.0
   (sql/jsonb_delta--0.3.1.sql is identical to sql/jsonb_delta--0.3.0.sql).

   The catalog therefore needs no alteration, so this upgrade is intentionally a
   no-op beyond advancing pg_extension.extversion to 0.3.1. */
