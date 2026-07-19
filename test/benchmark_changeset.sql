-- Benchmark: jsonb_apply_changeset (coalesced, ONE parse/serialize pass)
--            vs. chained jsonb_smart_patch_array() calls (the pattern IVM callers emit today).
--
-- Both apply the SAME N array-element updates to the same document. The coalesced call pays
-- the whole-document (de)serialization once; the chain pays it once per edit.
--
-- Run:  psql -f test/benchmark_changeset.sql
--
-- This script reports a coalescing SPEEDUP RATIO; it is a tool, not a source of
-- published numbers. Build the extension in RELEASE for any figure you intend to
-- cite (debug builds inflate serde cost, and because the chained arm pays that cost
-- N times vs. once for the coalesced arm, a debug build overstates the ratio).
-- The ratio grows with N (edits coalesced) because the coalesced arm pays the
-- whole-document (de)serialization once while the chain pays it once per edit.

CREATE EXTENSION IF NOT EXISTS jsonb_delta;

-- Server-side best-of-N stopwatch (ms).
CREATE OR REPLACE FUNCTION pg_temp.bench(q text, trials int DEFAULT 6) RETURNS double precision AS $$
DECLARE t0 timestamptz; best double precision := 1e18; w double precision; i int;
BEGIN
  FOR i IN 1..trials LOOP
    t0 := clock_timestamp(); EXECUTE q; w := extract(epoch FROM clock_timestamp() - t0) * 1000.0;
    IF w < best THEN best := w; END IF;
  END LOOP;
  RETURN best;
END $$ LANGUAGE plpgsql;

-- Build a chain of N nested jsonb_smart_patch_array() calls over `data`.
CREATE OR REPLACE FUNCTION pg_temp.chained_sql(n int, size int) RETURNS text AS $$
DECLARE expr text := 'data'; k int; eid int;
BEGIN
  FOR k IN 0..n-1 LOOP
    eid := 1 + k * (size / n);
    expr := format('jsonb_smart_patch_array(%s, %L::jsonb, %L, %L, %L::jsonb)',
                   expr, json_build_object('v', 900000 + k)::text, 'posts', 'id', eid::text);
  END LOOP;
  RETURN expr;
END $$ LANGUAGE plpgsql;

-- Build an equivalent changeset (N array_update ops) for jsonb_apply_changeset.
CREATE OR REPLACE FUNCTION pg_temp.changeset_ops(n int, size int) RETURNS jsonb AS $$
  SELECT jsonb_agg(jsonb_build_object(
    'op', 'array_update', 'path', 'posts', 'match_key', 'id',
    'match_value', 1 + k * (size / n), 'value', jsonb_build_object('v', 900000 + k)))
  FROM generate_series(0, n - 1) k;
$$ LANGUAGE sql;

DO $$
DECLARE size int := 1000; iters int := 20; n int; ch double precision; cs double precision;
BEGIN
  DROP TABLE IF EXISTS bench_doc;
  CREATE TEMP TABLE bench_doc AS
    SELECT jsonb_build_object('posts',
      (SELECT jsonb_agg(jsonb_build_object('id', g, 'v', g, 'pad', repeat('x', 20)))
       FROM generate_series(1, size) g)) AS data;

  -- Correctness: the two approaches must produce identical output.
  IF (SELECT (SELECT data FROM bench_doc) IS NOT NULL) THEN
    EXECUTE format(
      'DO $chk$ BEGIN IF (SELECT %s FROM bench_doc) <> jsonb_apply_changeset((SELECT data FROM bench_doc), %L::jsonb) '
      'THEN RAISE EXCEPTION ''chained and changeset disagree''; END IF; END $chk$;',
      pg_temp.chained_sql(20, size), pg_temp.changeset_ops(20, size)::text);
    RAISE NOTICE 'equivalence check (chained == changeset, N=20): OK';
  END IF;

  RAISE NOTICE '% posts, per-call ms (best of 6 over % iters):', size, iters;
  RAISE NOTICE '  N | chained (smart_patch) | changeset (apply) | speedup';
  FOREACH n IN ARRAY ARRAY[5, 20, 50] LOOP
    ch := pg_temp.bench(format('SELECT %s FROM bench_doc, generate_series(1,%s)',
                               pg_temp.chained_sql(n, size), iters)) / iters;
    cs := pg_temp.bench(format('SELECT jsonb_apply_changeset(data, %L::jsonb) FROM bench_doc, generate_series(1,%s)',
                               pg_temp.changeset_ops(n, size)::text, iters)) / iters;
    RAISE NOTICE '  % |          % |         % |  %x',
      lpad(n::text, 2), to_char(ch, 'FM9990.0000'), to_char(cs, 'FM9990.0000'), to_char(ch / cs, 'FM990.0');
  END LOOP;
END $$;
