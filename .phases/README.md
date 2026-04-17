# Security & Performance Hardening Plan

## Context

Health audit of the `jsonb_delta` extension identified 8 actionable items across
security, performance, and test coverage. This plan addresses all of them in
dependency order.

## Phases

| Phase | Title | Priority | Scope |
|-------|-------|----------|-------|
| 1 | Array size bounds | HIGH | `src/depth.rs`, `src/path.rs`, `src/lib.rs` |
| 2 | Nested path integration tests | HIGH | `test/sql/nested_paths.sql` |
| 3 | Input validation hardening | MEDIUM | `src/array_ops.rs`, `src/path.rs`, `src/merge.rs`, `src/lib.rs` |
| 4 | Binary search for sorted insertion | MEDIUM | `src/array_ops.rs`, `src/property_tests.rs` |
| 5 | Error-path test coverage | MEDIUM | `test/sql/`, `src/property_tests.rs`, `src/depth.rs` |
| 6 | Finalize | LOW | Cleanup, deduplication, docs |

## Current Status

- [x] Audit complete
- [ ] Phase 1 — Not started
- [ ] Phase 2 — Not started
- [ ] Phase 3 — Not started
- [ ] Phase 4 — Not started
- [ ] Phase 5 — Not started
- [ ] Phase 6 — Not started

## Key Constraints

- Tests run via `cargo pgrx test pg18` (Rust unit/property tests) and
  `just test-sql` (SQL integration tests).
- The extension is loaded via `CREATE EXTENSION IF NOT EXISTS jsonb_delta` at
  the top of each SQL test file — no manual installation needed.
- `cargo clippy --all-targets --all-features -- -D warnings` must pass after
  every commit; treat any warning as a build failure.
- `cargo nextest run` is available as a faster alternative to `cargo test`.
