-- Benchmark harness: repeatable, verified A/B timing for jsonb_delta.
--
-- Every performance number this project publishes should come out of here, for
-- three reasons the previous benchmarks did not satisfy:
--
--   1. A single `EXPLAIN ANALYZE` is one sample of a noisy variable. This runs
--      warm-up trials, then N>=10 measured ones, and reports order statistics
--      (median and p95) rather than whichever number came up first.
--   2. A ratio between two arms that compute different things is not a speedup.
--      Each arm's resulting state is captured and compared, and a scenario whose
--      arms disagree gets NO ratio reported — the mismatch is the finding.
--   3. A benchmark that mutates the database measures something different on
--      trial 2 than on trial 1. Each trial runs in a subtransaction that is
--      rolled back, so every trial starts from the same state.
--
-- Load it, define scenarios, run them:
--
--   \i test/bench/harness.sql
--   SELECT bench.define(name => 'my_case', native_sql => '...', delta_sql => '...',
--                       verify_sql => '...');
--   SELECT * FROM bench.run_all();
--
-- Calibration tests live in test/bench/harness_test.sql and should pass before
-- any number produced here is believed.

\set ON_ERROR_STOP on

DROP SCHEMA IF EXISTS bench CASCADE;
CREATE SCHEMA bench;

COMMENT ON SCHEMA bench IS
    'Benchmark harness. Transient: safe to drop and recreate at any time.';

-- ---------------------------------------------------------------------------
-- Scenario definitions
--
-- A scenario is four pieces of SQL. Adding a case is one bench.define() call,
-- not a new script.
-- ---------------------------------------------------------------------------
CREATE TABLE bench.scenario (
    name        text PRIMARY KEY,
    description text NOT NULL DEFAULT '',

    -- Run once before the trials, outside timing and outside the per-trial
    -- rollback. Use it to put the database in the state the arms expect.
    setup_sql   text NOT NULL DEFAULT 'SELECT 1',

    -- The two arms under comparison. Each is timed on its own.
    native_sql  text NOT NULL,
    delta_sql   text NOT NULL,

    -- The correctness check: if the two arms produce different output, no ratio
    -- is reported. Two modes, see bench.capture():
    --
    --   non-empty  a query returning one row and one column, reading back the
    --              state the arms were supposed to change (stored-table arms).
    --   ''         the arm's own return value is its output (in-memory arms,
    --              which write nothing for a verify query to read).
    verify_sql  text NOT NULL,

    -- N >= 10 is a floor, not a default to be lowered: below it the median is
    -- not meaningfully more robust than a single sample, which is the failure
    -- the harness exists to correct.
    n_trials    int NOT NULL DEFAULT 10 CHECK (n_trials >= 10),
    n_warmup    int NOT NULL DEFAULT 3  CHECK (n_warmup  >= 1),

    -- Ordering key so a matrix reports in a stable, readable sequence.
    sort_key    text NOT NULL DEFAULT ''
);

-- ---------------------------------------------------------------------------
-- Results
--
-- Kept as rows rather than printed, so a run can be re-run and compared, and so
-- the published artifact is generated from data rather than transcribed.
-- ---------------------------------------------------------------------------
CREATE TABLE bench.result (
    id               bigserial PRIMARY KEY,
    run_at           timestamptz NOT NULL DEFAULT now(),
    scenario         text NOT NULL,
    description      text NOT NULL DEFAULT '',
    n_trials         int NOT NULL,
    n_warmup         int NOT NULL,

    native_min_ms    double precision,
    native_median_ms double precision,
    native_p95_ms    double precision,

    delta_min_ms     double precision,
    delta_median_ms  double precision,
    delta_p95_ms     double precision,

    -- native_median / delta_median. Greater than 1 means jsonb_delta is faster.
    -- NULL whenever the arms disagree: an unverified ratio is not published.
    speedup          numeric,

    outputs_match    boolean NOT NULL,
    native_digest    text,
    delta_digest     text,

    -- Raw per-trial timings, kept so a reader can recompute the statistics
    -- instead of taking the summary on trust. That is the whole complaint in
    -- issue #15, and summarising away the evidence would reproduce it.
    native_trials_ms double precision[],
    delta_trials_ms  double precision[]
);

-- ---------------------------------------------------------------------------
-- bench.define — register or replace a scenario
-- ---------------------------------------------------------------------------
CREATE FUNCTION bench.define(
    name        text,
    native_sql  text,
    delta_sql   text,
    verify_sql  text,
    description text DEFAULT '',
    setup_sql   text DEFAULT 'SELECT 1',
    n_trials    int  DEFAULT 10,
    n_warmup    int  DEFAULT 3,
    sort_key    text DEFAULT ''
) RETURNS text
LANGUAGE sql
AS $$
    INSERT INTO bench.scenario AS s
        (name, description, setup_sql, native_sql, delta_sql, verify_sql,
         n_trials, n_warmup, sort_key)
    VALUES
        (define.name, define.description, define.setup_sql, define.native_sql,
         define.delta_sql, define.verify_sql, define.n_trials, define.n_warmup,
         COALESCE(NULLIF(define.sort_key, ''), define.name))
    ON CONFLICT (name) DO UPDATE SET
        description = EXCLUDED.description,
        setup_sql   = EXCLUDED.setup_sql,
        native_sql  = EXCLUDED.native_sql,
        delta_sql   = EXCLUDED.delta_sql,
        verify_sql  = EXCLUDED.verify_sql,
        n_trials    = EXCLUDED.n_trials,
        n_warmup    = EXCLUDED.n_warmup,
        sort_key    = EXCLUDED.sort_key
    RETURNING s.name;
$$;

COMMENT ON FUNCTION bench.define IS
    'Register a benchmark scenario. Re-defining an existing name replaces it.';

-- ---------------------------------------------------------------------------
-- bench.trial — run one arm once, timed, and discard its writes
--
-- The trial runs inside a subtransaction that is deliberately aborted, so the
-- arm's writes never persist and every trial sees the same starting state. Only
-- the arm itself lies between the two clock reads: the subtransaction is opened
-- before the first read, and verification happens after the second.
--
-- plpgsql variables are not transactional, so the timing and the digest survive
-- the rollback that discards the arm's data changes.
-- ---------------------------------------------------------------------------
CREATE FUNCTION bench.trial(arm_sql text) RETURNS double precision
LANGUAGE plpgsql
AS $$
DECLARE
    started_at timestamptz;
    ended_at   timestamptz;
BEGIN
    BEGIN
        started_at := clock_timestamp();
        EXECUTE arm_sql;
        ended_at := clock_timestamp();

        -- Unwind the trial. This is the only exit from the block; the handler
        -- below is the intended path, not an error case.
        RAISE EXCEPTION USING ERRCODE = 'ZB001', MESSAGE = 'bench trial rollback';
    EXCEPTION
        WHEN SQLSTATE 'ZB001' THEN
            NULL;
    END;

    RETURN EXTRACT(EPOCH FROM (ended_at - started_at)) * 1000.0;
END;
$$;

COMMENT ON FUNCTION bench.trial IS
    'Run one arm once inside a rolled-back subtransaction; return elapsed ms. Timing only.';

-- ---------------------------------------------------------------------------
-- bench.capture — record what an arm produces, without timing it
--
-- Correctness is established once per arm rather than on every trial: it is a
-- property of the SQL, not of the run, and keeping it out of the timed path
-- means the reported milliseconds contain nothing but the arm.
--
-- Two modes, chosen by whether verify_sql is supplied:
--
--   stored-table  verify_sql reads back the state the arm was supposed to
--                 change. Use for scenarios whose arms UPDATE/INSERT/DELETE.
--   in-memory     verify_sql is '' and the arm's own return value is the
--                 output. Use for scenarios that evaluate an expression and
--                 write nothing — there is no state to read back.
--
-- Both run inside a rolled-back subtransaction, so verification cannot leak
-- state into the trials that follow.
-- ---------------------------------------------------------------------------
CREATE FUNCTION bench.capture(arm_sql text, verify_sql text) RETURNS text
LANGUAGE plpgsql
AS $$
DECLARE
    observed text;
BEGIN
    BEGIN
        IF COALESCE(verify_sql, '') = '' THEN
            EXECUTE arm_sql INTO observed;
        ELSE
            EXECUTE arm_sql;
            EXECUTE verify_sql INTO observed;
        END IF;

        RAISE EXCEPTION USING ERRCODE = 'ZB001', MESSAGE = 'bench capture rollback';
    EXCEPTION
        WHEN SQLSTATE 'ZB001' THEN
            NULL;
    END;

    RETURN observed;
END;
$$;

COMMENT ON FUNCTION bench.capture IS
    'Return an arm''s output for correctness comparison, untimed and rolled back.';

-- ---------------------------------------------------------------------------
-- bench.measure — time both arms, interleaved
--
-- The arms alternate trial by trial rather than running as two consecutive
-- blocks. This matters because the deliverable is a *ratio*: a host that drifts
-- mid-run (CPU frequency scaling, cache warming, a neighbouring tenant waking
-- up) would otherwise charge the entire drift to whichever arm happened to be
-- running, and that bias lands directly in the published number. Alternating
-- makes both arms serve the same sentence.
-- ---------------------------------------------------------------------------
CREATE FUNCTION bench.measure(
    native_sql text,
    delta_sql  text,
    n_warmup   int,
    n_trials   int,
    OUT native_durations_ms double precision[],
    OUT delta_durations_ms  double precision[]
)
LANGUAGE plpgsql
AS $$
DECLARE
    i          int;
    native_ms  double precision;
    delta_ms   double precision;
BEGIN
    native_durations_ms := ARRAY[]::double precision[];
    delta_durations_ms  := ARRAY[]::double precision[];

    FOR i IN 1 .. (n_warmup + n_trials) LOOP
        native_ms := bench.trial(native_sql);
        delta_ms  := bench.trial(delta_sql);

        IF i > n_warmup THEN
            native_durations_ms := native_durations_ms || native_ms;
            delta_durations_ms  := delta_durations_ms  || delta_ms;
        END IF;
    END LOOP;
END;
$$;

COMMENT ON FUNCTION bench.measure IS
    'Time both arms over n_warmup + n_trials interleaved trials, so host drift affects both equally.';

-- ---------------------------------------------------------------------------
-- bench.run — measure one scenario, both arms, and record the result
-- ---------------------------------------------------------------------------
CREATE FUNCTION bench.run(scenario_name text) RETURNS bench.result
LANGUAGE plpgsql
AS $$
DECLARE
    s        bench.scenario;
    m        record;
    n_digest text;
    d_digest text;
    n_min    double precision;
    n_med    double precision;
    n_p95    double precision;
    d_min    double precision;
    d_med    double precision;
    d_p95    double precision;
    matched  boolean;
    ratio    numeric;
    out_row  bench.result;
BEGIN
    SELECT * INTO s FROM bench.scenario WHERE bench.scenario.name = scenario_name;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'bench: no scenario named %', scenario_name;
    END IF;

    EXECUTE s.setup_sql;

    -- Correctness first: establish what each arm produces before spending time
    -- measuring it. A ratio between arms that disagree is not worth taking.
    n_digest := bench.capture(s.native_sql, s.verify_sql);
    d_digest := bench.capture(s.delta_sql,  s.verify_sql);

    SELECT * INTO m
      FROM bench.measure(s.native_sql, s.delta_sql, s.n_warmup, s.n_trials);

    SELECT min(v),
           percentile_cont(0.50) WITHIN GROUP (ORDER BY v),
           percentile_cont(0.95) WITHIN GROUP (ORDER BY v)
      INTO n_min, n_med, n_p95
      FROM unnest(m.native_durations_ms) AS v;

    SELECT min(v),
           percentile_cont(0.50) WITHIN GROUP (ORDER BY v),
           percentile_cont(0.95) WITHIN GROUP (ORDER BY v)
      INTO d_min, d_med, d_p95
      FROM unnest(m.delta_durations_ms) AS v;

    matched := n_digest IS NOT DISTINCT FROM d_digest;

    -- A ratio is only meaningful between arms that computed the same thing.
    IF matched AND d_med > 0 THEN
        ratio := round((n_med / d_med)::numeric, 4);
    ELSE
        ratio := NULL;
    END IF;

    INSERT INTO bench.result (
        scenario, description, n_trials, n_warmup,
        native_min_ms, native_median_ms, native_p95_ms,
        delta_min_ms, delta_median_ms, delta_p95_ms,
        speedup, outputs_match, native_digest, delta_digest,
        native_trials_ms, delta_trials_ms
    ) VALUES (
        s.name, s.description, s.n_trials, s.n_warmup,
        n_min, n_med, n_p95,
        d_min, d_med, d_p95,
        ratio, matched, n_digest, d_digest,
        m.native_durations_ms, m.delta_durations_ms
    )
    RETURNING * INTO out_row;

    RETURN out_row;
END;
$$;

COMMENT ON FUNCTION bench.run IS
    'Measure one scenario end to end and record a row in bench.result.';

-- ---------------------------------------------------------------------------
-- bench.run_all — measure every registered scenario
-- ---------------------------------------------------------------------------
CREATE FUNCTION bench.run_all() RETURNS SETOF bench.result
LANGUAGE plpgsql
AS $$
DECLARE
    s bench.scenario;
BEGIN
    FOR s IN SELECT * FROM bench.scenario ORDER BY sort_key, name LOOP
        RAISE NOTICE 'bench: running %', s.name;
        RETURN NEXT bench.run(s.name);
    END LOOP;
END;
$$;

COMMENT ON FUNCTION bench.run_all IS
    'Run every registered scenario in sort_key order.';

-- ---------------------------------------------------------------------------
-- bench.report — human-readable view of the most recent run of each scenario
--
-- Unverified rows are labelled rather than dropped: a scenario whose arms
-- disagree is a finding that must stay visible.
-- ---------------------------------------------------------------------------
CREATE VIEW bench.report AS
SELECT
    r.scenario,
    r.n_trials                                       AS n,
    round(r.native_median_ms::numeric, 4)            AS native_median_ms,
    round(r.native_p95_ms::numeric, 4)               AS native_p95_ms,
    round(r.delta_median_ms::numeric, 4)             AS delta_median_ms,
    round(r.delta_p95_ms::numeric, 4)                AS delta_p95_ms,
    CASE WHEN r.outputs_match THEN r.speedup END     AS speedup,
    CASE
        WHEN NOT r.outputs_match       THEN 'UNVERIFIED: arms disagree'
        WHEN r.speedup IS NULL         THEN 'no ratio'
        WHEN r.speedup >= 1.10         THEN 'faster'
        WHEN r.speedup <= 0.90         THEN 'SLOWER'
        ELSE                                'parity'
    END                                              AS verdict,
    r.run_at
FROM bench.result r
JOIN (
    SELECT scenario, max(id) AS id FROM bench.result GROUP BY scenario
) latest ON latest.id = r.id
LEFT JOIN bench.scenario sc ON sc.name = r.scenario
ORDER BY COALESCE(sc.sort_key, r.scenario), r.scenario;

COMMENT ON VIEW bench.report IS
    'Latest result per scenario, with a verdict. "parity" is a legitimate outcome.';
