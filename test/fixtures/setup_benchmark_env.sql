-- Idempotent setup for the whole benchmark suite.
--
-- Run this once before any test/benchmark_*.sql. It is safe to re-run.
--
-- Everything is rebuilt from scratch on each run (the full fixture set costs well
-- under a second), so the suite always starts from a deterministic state and a
-- benchmark that commits cannot leak into the next run.
--
-- Two tiers, deliberately kept separate:
--
--   Base fixtures (bench_*, v_*, tv_*)  — read-only during a benchmark run.
--   Working copies (test_*)             — what the benchmarks actually mutate.
--
-- The `test_` prefix is not an accident and must not be "unified" away: it is the
-- boundary between the read-only fixture and the mutable copy under test. This is
-- why issue #13's suggested rename would have been the wrong fix.

\set ON_ERROR_STOP on

\echo '→ Generating base fixtures'

-- CQRS fixtures: bench_*, v_dns_server, tv_network_configuration, tv_allocation
\i test/fixtures/generate_cqrs_data.sql

-- Tree-composition fixtures: bench_tree_*, v_tree_user_profile
\i test/fixtures/generate_tree_composition_data.sql

-- UUID fixtures: bench_uuid_*, v_uuid_dns_server, tv_uuid_network_configuration
\i test/fixtures/generate_uuid_test_data.sql

\echo '→ Recreating mutable working copies'

DROP TABLE IF EXISTS test_v_dns_server CASCADE;
DROP TABLE IF EXISTS test_tv_network_configuration CASCADE;
DROP TABLE IF EXISTS test_tv_allocation CASCADE;

CREATE TABLE test_v_dns_server AS SELECT * FROM v_dns_server;
CREATE TABLE test_tv_network_configuration AS SELECT * FROM tv_network_configuration;
CREATE TABLE test_tv_allocation AS SELECT * FROM tv_allocation;

CREATE INDEX idx_test_v_dns_server_id ON test_v_dns_server(id);
CREATE INDEX idx_test_tv_network_configuration_id ON test_tv_network_configuration(id);
CREATE INDEX idx_test_tv_allocation_id ON test_tv_allocation(id);
CREATE INDEX idx_test_tv_allocation_nc_id
    ON test_tv_allocation((data->'network_configuration'->>'id'));

ANALYZE test_v_dns_server;
ANALYZE test_tv_network_configuration;
ANALYZE test_tv_allocation;

\echo '✓ Benchmark environment ready'
