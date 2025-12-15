# Development Guide

## Prerequisites

- Rust 1.83+ (install via [rustup](https://rustup.rs))
- PostgreSQL 13-17 with dev headers
- cargo-pgrx 0.12.8+

## Setup

```bash
# Install pgrx
cargo install --locked cargo-pgrx

# Initialize pgrx
cargo pgrx init
```

## Development Workflow

### Build and Test

```bash
# Run all tests
./run_tests.sh

# Format code
cargo fmt

# Build release
cargo build --release

# Install extension locally
cargo pgrx install --release
```

### Interactive Development

```bash
# Start PostgreSQL with extension loaded
cargo pgrx run pg17

# This opens psql with:
# - Extension installed
# - Auto-reload on code changes (in dev mode)
```

### Testing Against Multiple PostgreSQL Versions

```bash
# Test all supported versions
for ver in 13 14 15 16 17; do
    echo "Testing PostgreSQL $ver..."
    cargo pgrx test pg$ver || break
done
```

## Code Structure

```text
src/
└── lib.rs              # Main extension code
    ├── jsonb_merge_shallow  # Core merge function
    └── tests                # Rust unit tests

src/bin/
└── pgrx_embed.rs       # SQL generation binary

test/
├── sql/                # SQL integration tests
├── expected/           # Expected test output
└── benchmark_simple.sql # Performance benchmarks
```

## Adding New Functions

1. Add function to `src/lib.rs` with `#[pg_extern]` attribute
2. Write Rust unit tests with `#[pgrx::pg_test]`
3. Add SQL integration tests to `test/sql/`
4. Update expected output in `test/expected/`
5. Update README and CHANGELOG
6. Run `./run_tests.sh` to verify

## Performance Profiling

```bash
# Run benchmarks
psql -h localhost -p 28817 -d postgres -f test/benchmark_simple.sql

# Build with profiling symbols
cargo build --release --profile profiling
```

## Debugging

```bash
# Run with debug output
RUST_LOG=debug cargo pgrx run pg17

# Check logs
tail -f ~/.pgrx/data-17/logfile
```

## CI/CD

All commits are tested against PostgreSQL 13-17 via GitHub Actions.

Workflows:
- `.github/workflows/test.yml` - Multi-version PostgreSQL testing
- `.github/workflows/lint.yml` - Code quality (rustfmt, clippy, security audit)
- `.github/workflows/release.yml` - Automated releases on tags

## Quality Gates

Every commit must pass:

1. **Compilation**: `cargo build --release`
2. **Tests**: `./run_tests.sh`
3. **Format**: `cargo fmt --check`
4. **Benchmarks**: Performance within acceptable range

## Release Process

```bash
# 1. Update version in Cargo.toml
vim Cargo.toml

# 2. Update CHANGELOG.md
vim CHANGELOG.md

# 3. Commit changes
git add .
git commit -m "chore: prepare release v0.1.0"

# 4. Create and push tag
git tag -a v0.1.0 -m "Release v0.1.0"
git push origin main
git push origin v0.1.0

# 5. GitHub Actions will build and create release automatically
```

## Tips

- Use `cargo pgrx run pg17` for interactive development
- Run `./run_tests.sh` before committing
- Keep functions small and well-documented
- Write benchmarks for performance-critical code
- Use `#[pgrx::pg_test]` for database-dependent tests

## Getting Help

- pgrx docs: [pgrx repository](https://github.com/pgcentralfoundation/pgrx)
- Project issues: [GitHub Issues](https://github.com/evoludigit/jsonb_delta/issues)
- PostgreSQL docs: [PostgreSQL Documentation](https://www.postgresql.org/docs/)
