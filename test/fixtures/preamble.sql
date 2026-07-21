-- Shared preamble for every test/benchmark_*.sql script.
--
-- Included via `\i test/fixtures/preamble.sql` — psql resolves \i relative to
-- the current working directory, so benchmarks are run from the repository root.

\timing on
\set ON_ERROR_STOP on

CREATE EXTENSION IF NOT EXISTS jsonb_delta;
