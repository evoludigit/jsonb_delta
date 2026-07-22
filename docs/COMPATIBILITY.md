# PostgreSQL Version Compatibility

## Supported Versions

| PostgreSQL | jsonb_delta | Status | Notes |
|------------|-----------|--------|-------|
| 13.x       | 0.1.0 – 0.2.x | ⛔ Dropped in 0.3.0 | EOL (Nov 2025); 0.3.x requires PG 14+ |
| 14.x       | 0.1.0+    | ✅ Tested | Minimum for 0.3.x |
| 15.x       | 0.1.0+    | ✅ Tested | Full support |
| 16.x       | 0.1.0+    | ✅ Tested | Full support |
| 17.x       | 0.1.0+    | ✅ Tested | Full support (primary) |
| 18.x       | 0.2.0+    | ⚠️ Beta  | Testing in progress |

> **jsonb_delta 0.3.x requires PostgreSQL 14 or newer.** The binary-JSONB
> rewrite uses `JsonbToJsonbValue`, which was added to PostgreSQL's C API in
> version 14. PostgreSQL 13 (end-of-life since November 2025) is supported only
> through jsonb_delta 0.2.x.

## Feature Availability

| Feature | PG 13-17 | PG 18 | Notes |
|---------|----------|-------|-------|
| Basic merge | ✅ | ✅ | All versions |
| Deep merge | ✅ | ✅ | All versions |
| Array operations | ✅ | ✅ | All versions |
| Nested paths | ✅ (v0.2.0+) | ✅ | Requires jsonb_delta 0.2.0+ |
| Depth limits | ✅ (v0.2.0+) | ✅ | Security hardening |

## Consumer Compatibility

### pg_tviews

[pg_tviews](https://github.com/fraiseql/pg_tviews) uses jsonb_delta to accelerate
incremental view maintenance. The compatibility contract is deliberately small:

- **One runtime symbol.** pg_tviews depends on exactly one function at runtime,
  `jsonb_smart_patch_scalar(jsonb, jsonb) -> jsonb`. Its full signature — arguments,
  result, and `IMMUTABLE STRICT PARALLEL SAFE` — is frozen against `pg_proc` by the
  contract test in `src/contract.rs`, so it cannot drift under a consumer (this is the
  guard for jsonb_delta [#12](https://github.com/evoludigit/jsonb_delta/issues/12) /
  `fraiseql/pg_tviews#50`).
- **Optional at runtime.** pg_tviews degrades gracefully to full-row recomputation when
  jsonb_delta is absent, so a missing extension is never a hard failure.
- **Any 0.3.x works.** The SQL contract is unchanged across `0.2.0 → 0.3.0 → 0.3.1` — the
  0.3.x line is a binary-representation rewrite and a packaging release, not an API change —
  so pg_tviews needs no code change to move between them.

pg_tviews does **not** call `jsonb_smart_patch_array` in any signature; array- and
nested-object tviews use full-row recomputation.

## Testing Matrix

All versions tested with:
- ✅ Unit tests (pgrx test framework)
- ✅ SQL integration tests
- ✅ Performance benchmarks
- ✅ Fuzzing (24h runs)
- ✅ Load testing (100 concurrent clients)

## Platform Support

| OS | Architecture | Status |
|----|--------------|--------|
| Linux | x86_64 | ✅ Primary |
| Linux | ARM64 | ✅ Tested (CI) |
| macOS | x86_64 | ❌ Not supported |
| macOS | ARM64 (M1/M2) | ❌ Not supported |
| Windows | x86_64 | ⚠️ Untested |

**Note**: macOS excluded per project requirements. Windows untested but may work.

## Installation by PostgreSQL Version

### PostgreSQL 14-17

```bash
# Install from source
git clone https://github.com/evoludigit/jsonb_delta.git
cd jsonb_delta
cargo pgrx install --pg-config=/usr/lib/postgresql/17/bin/pg_config --release

# Or use pre-built packages (when available)
# Download from GitHub releases
```

### PostgreSQL 18 (Beta)

```bash
# PostgreSQL 18 support requires jsonb_delta v0.2.0+
cargo pgrx install --pg-config=/usr/lib/postgresql/18/bin/pg_config --release
```

## Known Limitations

### PostgreSQL 14-15

- No JIT compilation support (available in PG 16+)
- Slightly slower performance on complex queries

### PostgreSQL 18 Beta Support

- New features may have compatibility issues
- Report bugs to [GitHub Issues](https://github.com/evoludigit/jsonb_delta/issues)

## Migration Guide

### Upgrading from v0.1.0 to v0.2.0

```sql
-- Drop old extension
DROP EXTENSION jsonb_delta;

-- Install new version
-- (follow installation steps above)

-- Recreate extension
CREATE EXTENSION jsonb_delta;

-- Recreate dependent views/materialized views
-- (your application-specific migration scripts)
```

**Breaking Changes in v0.2.0**:
- New nested path functions require explicit opt-in
- Depth validation enabled by default (configurable limit: 1000 levels)

## Compatibility Testing

To verify compatibility with your PostgreSQL version:

```bash
# Run full test suite
cargo pgrx test pg17

# Run smoke tests
psql -d test_db -f test/smoke_test_v0.1.0.sql

# Run performance benchmarks
psql -d test_db -f test/benchmark_comparison.sql
```

## Support

- **Documentation**: [docs/README.md](../docs/README.md)
- **API Reference**: [docs/API.md](../docs/API.md)
- **Issues**: [GitHub Issues](https://github.com/evoludigit/jsonb_delta/issues)
- **Discussions**: [GitHub Discussions](https://github.com/evoludigit/jsonb_delta/discussions)

## Version Policy

- **Current**: PostgreSQL 14-17 (stable)
- **Beta**: PostgreSQL 18 (testing)
- **Future**: PostgreSQL 19+ (when released)

We support the current maintained PostgreSQL major versions (14-17, with 18 in beta) to ensure broad adoption while keeping maintenance overhead manageable. PostgreSQL 13 reached end-of-life in November 2025 and is supported only through jsonb_delta 0.2.x.
