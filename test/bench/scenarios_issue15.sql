-- Scenarios: the published 2-7x claims, re-measured against the baselines the
-- documentation actually names.
--
-- This is the matrix issue #15 asks for. Every "native" arm below is transcribed
-- from docs/PERFORMANCE.md rather than invented, because the most likely reason
-- our published table and the reporter's measurements disagree is that we were
-- not comparing against the same thing. Pinning the baseline in executable form
-- is the point: whatever the ratios turn out to be, the next person can read
-- exactly what was on the other side of them.
--
-- Where the documentation offers more than one native approach, the FASTEST one
-- is used. docs/PERFORMANCE.md quotes the batch case as "~32ms (10 separate
-- updates)" and "~18ms (complex CTE)" and then claims 3-5x against the pair.
-- Quoting a speedup against the slower of two approaches you already know about
-- is how a claim becomes indefensible, so the single-pass re-aggregation is used
-- throughout.
--
-- Claims under test (docs/PERFORMANCE.md "Summary", README.md:123-137):
--
--   jsonb_array_update_where        2-3x   (2.9x quoted at 50 elements)
--   jsonb_array_update_where_batch  3-5x   (10 items in a 100-element array)
--   jsonb_array_delete_where        5-7x   (6.8x quoted at 50 elements)
--   jsonb_merge_shallow             1.5-2x
--   array-size sweep                2.0x / 3.2x / 3.6x at 10 / 100 / 1000
--
-- Both scenario families the reporter distinguishes are covered, because they can
-- give different answers: a stored-table UPDATE pays row rewrite, WAL, and TOAST
-- on both arms and dilutes any ratio, while an in-memory expression does not.
-- Reporting only the flattering one would be the same error as above.

\set ON_ERROR_STOP on

-- ---------------------------------------------------------------------------
-- Fixture
--
-- One row per (array size, key type). Orders carry a filler field so an element
-- is not a trivially small object; the target is always the middle element, so
-- matching cost is average rather than best or worst case.
-- ---------------------------------------------------------------------------
DROP TABLE IF EXISTS bench_cust;
CREATE TABLE bench_cust (
    n_orders int  NOT NULL,
    key_type text NOT NULL,
    data     jsonb NOT NULL,
    PRIMARY KEY (n_orders, key_type)
);

INSERT INTO bench_cust (n_orders, key_type, data)
SELECT k, kt,
       jsonb_build_object(
           'id',     'cust_001',
           'tier',   'gold',
           'orders', (SELECT jsonb_agg(jsonb_build_object(
                                 'id',     CASE WHEN kt = 'int'
                                                THEN to_jsonb(i)
                                                ELSE to_jsonb('ord-' || lpad(i::text, 6, '0')) END,
                                 'status', 'pending',
                                 'total',  (i * 7) % 500,
                                 'note',   repeat('n', 120)))
                        FROM generate_series(1, k) AS i)
       )
  FROM (VALUES (10), (50), (100), (1000)) AS v(k),
       (VALUES ('int'), ('text')) AS t(kt);

-- Mutable copy for the stored-table arms. The harness rolls each trial back, so
-- this is restored to its starting state between measurements.
DROP TABLE IF EXISTS bench_cust_t;
CREATE TABLE bench_cust_t AS SELECT * FROM bench_cust;
ALTER TABLE bench_cust_t ADD PRIMARY KEY (n_orders, key_type);

-- ---------------------------------------------------------------------------
-- Helpers
--
-- mid() picks the target element; as_jsonb() renders a match value in the form
-- each arm needs (jsonb literal for jsonb_delta, text for the ->> comparison the
-- native SQL uses).
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION bench_mid(n int) RETURNS int
LANGUAGE sql IMMUTABLE AS $$ SELECT GREATEST(1, n / 2) $$;

-- Returns a quoted SQL literal, not a bare value: PostgreSQL has no integer ->
-- jsonb cast, so the match value has to reach `::jsonb` as '5' rather than 5.
CREATE OR REPLACE FUNCTION bench_key_jsonb(kt text, i int) RETURNS text
LANGUAGE sql IMMUTABLE AS $$
    SELECT quote_literal(
        CASE WHEN kt = 'int' THEN i::text
             ELSE format('"ord-%s"', lpad(i::text, 6, '0')) END)
$$;

CREATE OR REPLACE FUNCTION bench_key_text(kt text, i int) RETURNS text
LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE WHEN kt = 'int' THEN i::text
                ELSE 'ord-' || lpad(i::text, 6, '0') END
$$;

CREATE OR REPLACE FUNCTION bench_src(n int, kt text) RETURNS text
LANGUAGE sql IMMUTABLE AS $$
    SELECT format('(SELECT data FROM bench_cust WHERE n_orders = %s AND key_type = %L)', n, kt)
$$;

-- ---------------------------------------------------------------------------
-- update: single element by id
--   native  jsonb_set + jsonb_agg re-aggregation  (docs/PERFORMANCE.md:32-49)
--   delta   jsonb_array_update_where
-- ---------------------------------------------------------------------------
SELECT bench.define(
           name        => format('update_%s_%s_inmem', kt, lpad(k::text, 4, '0')),
           native_sql  => format($f$
               SELECT jsonb_set(%s, '{orders}', (
                   SELECT jsonb_agg(CASE WHEN elem->>'id' = %L
                                         THEN jsonb_set(elem, '{status}', '"shipped"')
                                         ELSE elem END)
                     FROM jsonb_array_elements((%s)->'orders') AS elem))$f$,
               bench_src(k, kt), bench_key_text(kt, bench_mid(k)), bench_src(k, kt)),
           delta_sql   => format($f$
               SELECT jsonb_array_update_where(%s, 'orders', 'id', %s::jsonb, '{"status":"shipped"}'::jsonb)$f$,
               bench_src(k, kt), bench_key_jsonb(kt, bench_mid(k))),
           verify_sql  => '',
           description => format('update 1 of %s (%s key), in-memory', k, kt),
           sort_key    => format('a_update_%s_%s_1inmem', kt, lpad(k::text, 4, '0')))
  FROM (VALUES (10), (50), (100), (1000)) AS v(k),
       (VALUES ('int'), ('text')) AS t(kt);

SELECT bench.define(
           name        => format('update_%s_%s_stored', kt, lpad(k::text, 4, '0')),
           native_sql  => format($f$
               UPDATE bench_cust_t SET data = jsonb_set(data, '{orders}', (
                   SELECT jsonb_agg(CASE WHEN elem->>'id' = %L
                                         THEN jsonb_set(elem, '{status}', '"shipped"')
                                         ELSE elem END)
                     FROM jsonb_array_elements(data->'orders') AS elem))
                 WHERE n_orders = %s AND key_type = %L$f$,
               bench_key_text(kt, bench_mid(k)), k, kt),
           delta_sql   => format($f$
               UPDATE bench_cust_t SET data =
                   jsonb_array_update_where(data, 'orders', 'id', %s::jsonb, '{"status":"shipped"}'::jsonb)
                 WHERE n_orders = %s AND key_type = %L$f$,
               bench_key_jsonb(kt, bench_mid(k)), k, kt),
           verify_sql  => format(
               'SELECT data::text FROM bench_cust_t WHERE n_orders = %s AND key_type = %L', k, kt),
           description => format('update 1 of %s (%s key), stored table', k, kt),
           sort_key    => format('a_update_%s_%s_2stored', kt, lpad(k::text, 4, '0')))
  FROM (VALUES (10), (50), (100), (1000)) AS v(k),
       (VALUES ('int'), ('text')) AS t(kt);

-- ---------------------------------------------------------------------------
-- delete: remove one element by id
--   native  jsonb_agg with a filtering WHERE  (docs/PERFORMANCE.md:157-170)
--   delta   jsonb_array_delete_where
-- This is the 5-7x claim, the largest published figure and the one most worth
-- checking independently of update.
-- ---------------------------------------------------------------------------
SELECT bench.define(
           name        => format('delete_%s_%s_inmem', kt, lpad(k::text, 4, '0')),
           native_sql  => format($f$
               SELECT jsonb_set(%s, '{orders}', (
                   SELECT jsonb_agg(elem)
                     FROM jsonb_array_elements((%s)->'orders') AS elem
                    WHERE elem->>'id' <> %L))$f$,
               bench_src(k, kt), bench_src(k, kt), bench_key_text(kt, bench_mid(k))),
           delta_sql   => format($f$
               SELECT jsonb_array_delete_where(%s, 'orders', 'id', %s::jsonb)$f$,
               bench_src(k, kt), bench_key_jsonb(kt, bench_mid(k))),
           verify_sql  => '',
           description => format('delete 1 of %s (%s key), in-memory', k, kt),
           sort_key    => format('b_delete_%s_%s_1inmem', kt, lpad(k::text, 4, '0')))
  FROM (VALUES (10), (50), (100), (1000)) AS v(k),
       (VALUES ('int'), ('text')) AS t(kt);

SELECT bench.define(
           name        => format('delete_%s_%s_stored', kt, lpad(k::text, 4, '0')),
           native_sql  => format($f$
               UPDATE bench_cust_t SET data = jsonb_set(data, '{orders}', (
                   SELECT jsonb_agg(elem)
                     FROM jsonb_array_elements(data->'orders') AS elem
                    WHERE elem->>'id' <> %L))
                 WHERE n_orders = %s AND key_type = %L$f$,
               bench_key_text(kt, bench_mid(k)), k, kt),
           delta_sql   => format($f$
               UPDATE bench_cust_t SET data =
                   jsonb_array_delete_where(data, 'orders', 'id', %s::jsonb)
                 WHERE n_orders = %s AND key_type = %L$f$,
               bench_key_jsonb(kt, bench_mid(k)), k, kt),
           verify_sql  => format(
               'SELECT data::text FROM bench_cust_t WHERE n_orders = %s AND key_type = %L', k, kt),
           description => format('delete 1 of %s (%s key), stored table', k, kt),
           sort_key    => format('b_delete_%s_%s_2stored', kt, lpad(k::text, 4, '0')))
  FROM (VALUES (10), (50), (100), (1000)) AS v(k),
       (VALUES ('int'), ('text')) AS t(kt);

-- ---------------------------------------------------------------------------
-- batch: 10 elements updated at once, 100-element array (the documented case)
--   native  ONE re-aggregation pass handling all 10 ids -- the "complex CTE"
--           arm, not the "10 separate updates" arm
--   delta   jsonb_array_update_where_batch
-- ---------------------------------------------------------------------------
SELECT bench.define(
           name        => 'batch10_int_0100_inmem',
           native_sql  => format($f$
               SELECT jsonb_set(%s, '{orders}', (
                   SELECT jsonb_agg(CASE WHEN (elem->>'id')::int BETWEEN 1 AND 10
                                         THEN elem || '{"status":"shipped"}'::jsonb
                                         ELSE elem END)
                     FROM jsonb_array_elements((%s)->'orders') AS elem))$f$,
               bench_src(100, 'int'), bench_src(100, 'int')),
           delta_sql   => format($f$
               SELECT jsonb_array_update_where_batch(%s, 'orders', 'id', %L::jsonb)$f$,
               bench_src(100, 'int'),
               (SELECT jsonb_agg(jsonb_build_object('match_value', i,
                                                    'updates', '{"status":"shipped"}'::jsonb))::text
                  FROM generate_series(1, 10) AS i)),
           verify_sql  => '',
           description => 'batch: 10 updates in a 100-element array, in-memory',
           sort_key    => 'c_batch_1inmem');

-- ---------------------------------------------------------------------------
-- merge_shallow: top-level key merge
--   native  the `||` operator, which is what a competent author would write
--   delta   jsonb_merge_shallow
-- ---------------------------------------------------------------------------
SELECT bench.define(
           name        => format('merge_int_%s_inmem', lpad(k::text, 4, '0')),
           native_sql  => format($f$SELECT %s || '{"tier":"platinum","vip":true,"region":"eu","segment":"a","score":9}'::jsonb$f$,
                                 bench_src(k, 'int')),
           delta_sql   => format($f$SELECT jsonb_merge_shallow(%s, '{"tier":"platinum","vip":true,"region":"eu","segment":"a","score":9}'::jsonb)$f$,
                                 bench_src(k, 'int')),
           verify_sql  => '',
           description => format('merge 5 top-level keys, %s-element doc, in-memory', k),
           sort_key    => format('d_merge_%s', lpad(k::text, 4, '0')))
  FROM (VALUES (50), (1000)) AS v(k);
