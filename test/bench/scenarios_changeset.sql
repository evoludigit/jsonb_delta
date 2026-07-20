-- Scenarios: what jsonb_apply_changeset is actually worth, against two baselines.
--
-- The mechanism under test is narrow: both arms are serde, and both do the same
-- element matching. The only structural difference is how many times the whole
-- document is parsed and re-serialized -- N times for a chain of N calls, once
-- for a changeset of N ops. So against a chain, any speedup should
--
--   * grow with the number of ops,
--   * grow with document size,
--   * and collapse to parity at N = 1.
--
-- The N = 1 row is therefore a control, not filler: if the changeset arm still
-- looks fast when it does exactly one parse/serialize like the chain does, the
-- measurement is picking up something other than coalescing and none of the
-- other rows should be believed.
--
-- TWO BASELINES, DELIBERATELY
--
-- Measuring only against the chain would flatter the feature. For the specific
-- case these scenarios use -- N updates, one array, integer match key --
-- jsonb_array_update_where_batch already does the whole job in a single
-- parse/serialize pass, and it is the baseline a fair reader would demand. It is
-- also, on paper, the better algorithm: it builds a HashMap and makes one pass
-- over the array, where a changeset rescans the array once per op.
--
-- So the `batch_*` scenarios exist to try to make the feature look bad. That
-- they come out at parity is the finding: the changeset's advantage over batch
-- is not speed, it is coverage (heterogeneous ops, several paths, and non-integer
-- match keys -- batch reads match_value via as_i64 and silently skips anything
-- else, so UUID and text keys cannot use it at all).
--
-- Read the two families together. Quoting the chained ratio on its own would
-- reproduce, in a new place, exactly the overclaiming that issue #15 is about.
--
-- Both arms are in-memory expressions that write nothing, so they use the
-- harness's verify_sql => '' mode: the arm's own return value is its output.

\set ON_ERROR_STOP on

-- ---------------------------------------------------------------------------
-- Fixture documents
--
-- Each post carries a ~200-byte body so the document has enough bulk for
-- parse/serialize to be the dominant cost rather than a rounding error. A
-- 1000-post document lands around 250 KB, which is a realistic CQRS read-model
-- row and comfortably past the point where PostgreSQL TOASTs it.
-- ---------------------------------------------------------------------------
DROP TABLE IF EXISTS bench_doc;
CREATE TABLE bench_doc (n_posts int PRIMARY KEY, doc jsonb);

INSERT INTO bench_doc (n_posts, doc)
SELECT k,
       jsonb_build_object(
           'id',     1,
           'status', 'draft',
           'author', jsonb_build_object('name', 'Old', 'city', 'NYC'),
           'stats',  jsonb_build_object('post_count', k),
           'posts',  (SELECT jsonb_agg(jsonb_build_object(
                                 'id',   i,
                                 'title', 'Post ' || i,
                                 'body',  repeat('x', 200),
                                 'tags',  jsonb_build_array('a', 'b', 'c')))
                        FROM generate_series(1, k) AS i)
       )
  FROM (VALUES (10), (100), (200), (500), (1000), (5000)) AS v(k);

-- ---------------------------------------------------------------------------
-- Arm builders
--
-- The two arms are generated from the same (n_posts, n_ops) pair rather than
-- written out by hand, so they cannot drift apart: op i edits post i to title
-- 'E<i>' on both sides, by construction. Hand-nesting eight function calls and
-- an eight-element JSON array and hoping they match is exactly the kind of
-- transcription error that produces a spurious ratio.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION bench_chain_sql(n_posts int, n_ops int)
RETURNS text LANGUAGE plpgsql AS $$
DECLARE
    expr text;
    i    int;
BEGIN
    expr := format('(SELECT doc FROM bench_doc WHERE n_posts = %s)', n_posts);
    FOR i IN 1 .. n_ops LOOP
        expr := format(
            'jsonb_smart_patch_array(%s, %L::jsonb, %L, %L, %L::jsonb)',
            expr, format('{"title":"E%s"}', i), 'posts', 'id', i::text);
    END LOOP;
    RETURN 'SELECT ' || expr;
END;
$$;

-- The strongest baseline available today for this shape of edit: one call, one
-- parse/serialize, one HashMap-driven pass over the array. Produces the same
-- document as the other two builders, so the harness can compare all three.
CREATE OR REPLACE FUNCTION bench_batch_sql(n_posts int, n_ops int)
RETURNS text LANGUAGE plpgsql AS $$
DECLARE
    ups jsonb := '[]'::jsonb;
    i   int;
BEGIN
    FOR i IN 1 .. n_ops LOOP
        ups := ups || jsonb_build_object(
            'match_value', i,
            'updates',     jsonb_build_object('title', 'E' || i));
    END LOOP;
    RETURN format(
        'SELECT jsonb_array_update_where_batch((SELECT doc FROM bench_doc WHERE n_posts = %s), %L, %L, %L::jsonb)',
        n_posts, 'posts', 'id', ups::text);
END;
$$;

CREATE OR REPLACE FUNCTION bench_changeset_sql(n_posts int, n_ops int)
RETURNS text LANGUAGE plpgsql AS $$
DECLARE
    ops jsonb := '[]'::jsonb;
    i   int;
BEGIN
    FOR i IN 1 .. n_ops LOOP
        ops := ops || jsonb_build_object(
            'op',          'array_update',
            'path',        'posts',
            'match_key',   'id',
            'match_value', i,
            'value',       jsonb_build_object('title', 'E' || i));
    END LOOP;
    RETURN format(
        'SELECT jsonb_apply_changeset((SELECT doc FROM bench_doc WHERE n_posts = %s), %L::jsonb)',
        n_posts, ops::text);
END;
$$;

-- ---------------------------------------------------------------------------
-- Registration
--
-- Two sweeps that vary one thing each:
--   ops_*   holds the document at 200 posts and varies N (1 -> 8)
--   size_*  holds N at 4 and varies the document (10 -> 1000 posts)
-- ---------------------------------------------------------------------------
SELECT bench.define(
           name        => format('ops_n%s_posts200', n),
           native_sql  => bench_chain_sql(200, n),
           delta_sql   => bench_changeset_sql(200, n),
           verify_sql  => '',
           description => format('%s edit(s), 200-post doc: chained smart_patch vs one changeset', n),
           sort_key    => format('a_ops_%s', lpad(n::text, 2, '0')))
  FROM (VALUES (1), (2), (4), (8)) AS v(n);

SELECT bench.define(
           name        => format('size_posts%s_n4', k),
           native_sql  => bench_chain_sql(k, 4),
           delta_sql   => bench_changeset_sql(k, 4),
           verify_sql  => '',
           description => format('4 edits, %s-post doc: chained smart_patch vs one changeset', k),
           sort_key    => format('b_size_%s', lpad(k::text, 5, '0')))
  FROM (VALUES (10), (100), (1000)) AS v(k);

-- The four configurations behind the "4.8x-40x" claim that was published and then
-- withdrawn as unsourced. Re-run here under the harness, in a release build, so
-- the claim is either reproduced or retired on evidence rather than on suspicion.
SELECT bench.define(
           name        => format('repro_%s_n%s', p, n),
           native_sql  => bench_chain_sql(p, n),
           delta_sql   => bench_changeset_sql(p, n),
           verify_sql  => '',
           description => format('withdrawn-claim config: %s-element array, N=%s', p, n),
           sort_key    => format('c_repro_%s_%s', lpad(p::text, 5, '0'), lpad(n::text, 3, '0')))
  FROM (VALUES (500, 5), (500, 20), (500, 50), (5000, 50)) AS v(p, n);

-- Same three configurations against the baseline that is actually hard to beat.
SELECT bench.define(
           name        => format('batch_%s_n%s', p, n),
           native_sql  => bench_batch_sql(p, n),
           delta_sql   => bench_changeset_sql(p, n),
           verify_sql  => '',
           description => format('strongest baseline: batch vs changeset, %s-element array, N=%s', p, n),
           sort_key    => format('d_batch_%s_%s', lpad(p::text, 5, '0'), lpad(n::text, 3, '0')))
  FROM (VALUES (500, 5), (500, 50), (5000, 50)) AS v(p, n);
