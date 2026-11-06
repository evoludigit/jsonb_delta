/* jsonb_delta 0.3.1 -> 0.3.2 upgrade script.

   0.3.2 is a packaging-only release: it adds PostgreSQL 18 to the release
   matrix so a prebuilt pg18 package exists (issue #34). There is NO behavior
   change — every function signature, volatility, strictness and
   parallel-safety are those of 0.3.1 (sql/jsonb_delta--0.3.2.sql is identical
   to sql/jsonb_delta--0.3.1.sql).

   The catalog therefore needs no alteration, so this upgrade is intentionally a
   no-op beyond advancing pg_extension.extversion to 0.3.2. */
