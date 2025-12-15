# Phased Improvement Plan: Elevating jsonb_ivm to 9.5/10

## Overview
This plan outlines a **6-phase approach** to elevate the jsonb_ivm project from its current 9/10 rating to a solid 9.5/10. Each phase follows TDD principles (RED → GREEN → REFACTOR → QA → GREENFIELD) and focuses on high-impact improvements identified in the codebase assessment.

**Total Estimated Effort**: 8-10 weeks (realistic), with 12-week contingency buffer

**What Makes This 9.5/10**:
- ✅ Zero known security vulnerabilities (DREAD <25/50)
- ✅ Zero clippy warnings, optimized for performance
- ✅ Advanced features (nested path support)
- ✅ Comprehensive test coverage (>85% with property tests)
- ✅ Production-ready documentation and CI/CD

---

## Phase Dependencies

```
Phase 0: Baseline & Modularization (FOUNDATION)
├─> Phase 1: Security Hardening (needs modular structure)
├─> Phase 2: Code Quality Cleanup (easier with modules)
└─> Phase 3: Nested Path Support (needs path.rs module)

Phase 1 → Phase 4 (depth validation needs property tests)
Phase 2 → Phase 5 (clean code enables better docs)
Phase 3 → Phase 5 (new features need documentation)
```

---

## Phase 0: Baseline Measurement & Code Modularization (NEW)

**Duration**: 3-5 days
**Priority**: Foundation (MUST complete before other phases)

### Objective
Establish measurable baselines for before/after comparison and extract the monolithic `src/lib.rs` (1524 lines, 19 functions) into a modular structure.

### Context
- All code currently in single `src/lib.rs` file
- No baseline metrics for comparison
- Need clear module boundaries for parallel development

### Files to Create
```
baselines/
  baseline-clippy.txt         (88 warnings documented)
  baseline-perf.txt           (benchmark results)
  baseline-coverage.json      (if tarpaulin works)
  baseline-tech-debt.txt      (TODO/FIXME counts)
  baseline-threats.md         (DREAD scores snapshot)

src/
  lib.rs                      (re-exports, pgrx magic, 200 lines)
  merge.rs                    (merge functions, ~400 lines)
  array_ops.rs                (array update/delete, ~300 lines)
  search.rs                   (find_by_* optimized helpers, ~200 lines)
  depth.rs                    (NEW: depth validation, ~100 lines)
  path.rs                     (FUTURE: Phase 3)
  tests/                      (Rust integration tests)
```

### Implementation Steps

1. **RED**: Create failing test that imports from `src/merge.rs` (doesn't exist yet)
   ```rust
   // Add to src/lib.rs at top:
   // mod merge;  // This will fail initially
   ```

2. **GREEN**: Extract modules from `src/lib.rs`
   ```bash
   # Step-by-step extraction:
   # 1. Extract merge functions (lines ~100-500) → src/merge.rs
   # 2. Extract array operations (lines ~500-800) → src/array_ops.rs
   # 3. Extract search helpers (lines ~30-100) → src/search.rs
   # 4. Update lib.rs with re-exports
   ```

3. **REFACTOR**: Ensure all tests pass after modularization
   ```bash
   cargo pgrx test pg17
   ```

4. **QA**: Capture baselines
   ```bash
   # Clippy baseline (with correct pgrx flags)
   cargo clippy --no-default-features --features pg17 \
     --all-targets -- -D warnings 2>&1 | tee baselines/baseline-clippy.txt

   # Performance baseline
   psql -U postgres -f test/benchmark_comparison.sql > baselines/baseline-perf.txt

   # Coverage baseline (if tarpaulin works)
   cargo tarpaulin --no-default-features --features pg17 \
     --out Json > baselines/baseline-coverage.json || echo "Tarpaulin broken"

   # Tech debt baseline
   rg "TODO|FIXME|XXX|HACK" --stats src/ > baselines/baseline-tech-debt.txt

   # Threat model baseline
   cp docs/security/threat-model.md baselines/baseline-threats.md
   ```

5. **GREENFIELD**: Document module structure
   ```bash
   # Update docs/ARCHITECTURE.md with new module breakdown
   # Add module-level documentation to each file
   ```

### Verification Commands
```bash
# Verify modularization didn't break anything
cargo pgrx test pg17

# Verify all baselines captured
ls -lh baselines/*.txt baselines/*.json baselines/*.md

# Verify module structure
tree src/ -I target
```

### Acceptance Criteria
- ✅ `src/lib.rs` reduced to <250 lines (re-exports + pgrx magic)
- ✅ All functions moved to appropriate modules
- ✅ All existing tests pass without modification
- ✅ Baselines captured in `baselines/` directory:
  - Clippy: 88 warnings documented with line numbers
  - Performance: Benchmark times for all operations
  - Tech debt: X TODOs, Y FIXMEs counted
  - Threats: DREAD scores documented (DoS: 35/50)
- ✅ No performance regression (compile time, runtime benchmarks)

### DO NOT
- Change any function logic during extraction
- Break pgrx FFI boundary (keep `#[pg_extern]` functions in lib.rs or re-export)
- Skip baseline capture (needed for Phase success metrics)

---

## Phase 1: Security Hardening

**Duration**: 1-2 weeks
**Priority**: Critical
**Depends On**: Phase 0 complete

### Objective
Implement depth validation and DoS protection to prevent stack overflow attacks via deeply nested JSONB structures.

### Context
Current deep merge and path operations lack recursion limits, creating a DoS vulnerability:
- **DREAD Score**: D=8, R=7, E=5, A=8, D=7 = **35/50** (MEDIUM-HIGH risk)
- **Target**: Reduce to <25/50 (LOW-MEDIUM risk)

### Files to Modify/Create
- `src/depth.rs` (NEW: depth validation module)
- `src/merge.rs` (integrate depth checks in deep_merge)
- `src/array_ops.rs` (add depth checks to recursive array operations)
- `fuzz/fuzz_targets/fuzz_deep_merge.rs` (update with depth limit tests)
- `test/sql/security_depth_limits.sql` (NEW: SQL security tests)
- `test/expected/security_depth_limits.out` (expected output)
- `docs/security/threat-model.md` (update DREAD scores)

### Implementation Steps

1. **RED**: Add failing tests for depth limit violations
   ```sql
   -- test/sql/security_depth_limits.sql
   -- Generate deeply nested JSONB (1001 levels)
   SELECT jsonb_ivm_deep_merge(
     '{}'::jsonb,
     -- Nested 1001 levels deep (should FAIL)
   );
   -- Expected: ERROR: JSONB nesting too deep (max 1000, found 1001)
   ```

2. **GREEN**: Implement depth validation
   ```rust
   // src/depth.rs
   pub const MAX_JSONB_DEPTH: usize = 1000;

   pub fn validate_depth(val: &serde_json::Value, max_depth: usize) -> Result<(), String> {
       fn check_depth(val: &Value, current: usize, max: usize) -> Result<usize, String> {
           if current > max {
               return Err(format!("JSONB nesting too deep (max {}, found >{})", max, max));
           }
           match val {
               Value::Object(map) => {
                   let mut max_child = current;
                   for v in map.values() {
                       max_child = max_child.max(check_depth(v, current + 1, max)?);
                   }
                   Ok(max_child)
               }
               Value::Array(arr) => {
                   let mut max_child = current;
                   for v in arr {
                       max_child = max_child.max(check_depth(v, current + 1, max)?);
                   }
                   Ok(max_child)
               }
               _ => Ok(current),
           }
       }
       check_depth(val, 0, max_depth)?;
       Ok(())
   }
   ```

3. **REFACTOR**: Integrate depth checks into all entry points
   ```rust
   // src/merge.rs
   #[pg_extern]
   fn jsonb_ivm_deep_merge(target: JsonB, source: JsonB) -> Result<JsonB, Error> {
       // Add at function entry:
       depth::validate_depth(&source.0, depth::MAX_JSONB_DEPTH)
           .map_err(|e| error!("{}", e))?;

       // ... existing merge logic
   }
   ```

4. **QA**: Comprehensive security testing
   ```bash
   # Fuzzing with depth limits
   cargo fuzz run fuzz_deep_merge -- \
     -max_len=100000 \
     -max_total_time=86400  # 24 hours

   # SQL security tests
   cargo pgrx test pg17 -- security_depth_limits
   ```

5. **GREENFIELD**: Update threat model
   ```markdown
   # docs/security/threat-model.md

   ## 2a. Complex JSONB Documents (UPDATED)

   **DREAD Score (Before)**: 35/50
   - Damage: 8 (service disruption)
   - Reproducibility: 7 (easy to reproduce)
   - Exploitability: 5 (requires knowledge)
   - Affected Users: 8 (all users)
   - Discoverability: 7 (obvious attack vector)

   **DREAD Score (After)**: 21/50
   - Damage: 3 (graceful error, no crash)
   - Reproducibility: 7 (still easy to attempt)
   - Exploitability: 3 (blocked at 1000 levels)
   - Affected Users: 3 (attacker gets error)
   - Discoverability: 5 (documented limit)

   **Mitigations**:
   - ✅ Explicit depth limit: MAX 1000 levels
   - ✅ Clear error messages
   - ✅ Fuzzing validation (24h runs clean)
   ```

### Verification Commands
```bash
# Run security SQL tests
cargo pgrx test pg17 -- security_depth_limits

# Fuzz deep nesting (24 hours)
cargo fuzz run fuzz_deep_merge -- \
  -max_len=100000 \
  -max_total_time=86400 \
  -rss_limit_mb=4096

# Benchmark performance impact
psql -U postgres -f test/benchmark_comparison.sql > after-phase1-perf.txt
diff baselines/baseline-perf.txt after-phase1-perf.txt

# Verify no regression
./scripts/check_regression.sh baselines/baseline-perf.txt after-phase1-perf.txt 5
```

### Acceptance Criteria
- ✅ All recursive functions reject JSONB >1000 levels with error:
  ```
  ERROR: JSONB nesting too deep (max 1000, found 1001)
  CONTEXT: PL/pgSQL function jsonb_ivm_deep_merge
  ```
- ✅ Fuzzing runs 24h with **zero crashes, zero hangs**:
  - Corpus: 10,000+ unique inputs
  - max_len: 100KB
  - RSS limit: 4GB
- ✅ DREAD score reduced: 35/50 → **21/50**
- ✅ Performance regression: <5% on `benchmark_comparison.sql`
  - Baseline: avg 42ms per merge
  - After: max 44ms per merge (4.7% increase ✓)
- ✅ Depth validation in all entry points:
  - `jsonb_ivm_deep_merge`
  - `jsonb_ivm_merge_shallow`
  - `jsonb_ivm_array_update_where`
  - `jsonb_ivm_array_delete_where`

### DO NOT
- Hardcode depth limit (use `const MAX_JSONB_DEPTH`)
- Change function signatures (depth check is internal)
- Reject valid JSONB <1000 levels
- Skip fuzzing validation

### Rollback Plan
```bash
# If depth validation causes issues:
git revert $(git log --oneline --grep "\[GREENFIELD\] Phase 1" -1 --format="%H")

# If partial rollback needed:
# 1. Keep depth.rs module
# 2. Make depth checks optional via feature flag:
#[cfg(feature = "strict_depth_limits")]
validate_depth(&source.0, MAX_JSONB_DEPTH)?;
```

---

## Phase 2: Code Quality Cleanup

**Duration**: 1-2 weeks
**Priority**: High
**Depends On**: Phase 0 complete

### Objective
Eliminate all 88 clippy warnings and optimize function signatures for zero-copy operations where pgrx FFI allows.

### Context
- **Current State**: 88 clippy warnings across codebase
- **Impact**: Reduced maintainability, potential performance issues hidden
- **Challenge**: pgrx FFI restrictions limit some optimizations

### Files to Modify
- `src/lib.rs` (optimize FFI function signatures)
- `src/merge.rs` (fix clippy warnings)
- `src/array_ops.rs` (fix clippy warnings)
- `src/search.rs` (fix clippy warnings)
- `src/depth.rs` (ensure new code is clean)
- `.github/workflows/lint.yml` (enforce clippy in CI)
- `scripts/clippy_check.sh` (NEW: automation script)

### Prerequisites

**Step 0: Fix pgrx Feature Conflict**
```bash
# Current issue:
# Error: Multiple `pg$VERSION` features found.

# Fix: Use --no-default-features --features pg17
alias clippy-pgrx='cargo clippy --no-default-features --features pg17 --all-targets -- -D warnings'
```

**Step 1: Categorize Warnings**
```bash
# Generate JSON report
cargo clippy --no-default-features --features pg17 \
  --message-format=json --all-targets -- -D warnings \
  > clippy-warnings.json

# Categorize by priority
cat clippy-warnings.json | jq -r '.message.code.code' | sort | uniq -c | sort -rn
```

**Expected Categories**:
- **P0 (Correctness)**: `incorrect_clone_impl_on_copy_type`, `mut_from_ref`
- **P1 (Performance)**: `needless_pass_by_value`, `large_enum_variant`
- **P2 (Style)**: `manual_map`, `redundant_closure`, `needless_return`

### Implementation Steps

1. **RED**: Enable clippy as hard error in CI
   ```yaml
   # .github/workflows/lint.yml
   - name: Clippy
     run: |
       cargo clippy --no-default-features --features pg17 \
         --all-targets -- -D warnings
   ```

2. **GREEN**: Fix warnings by priority
   ```bash
   # P0: Correctness (1-2 warnings)
   # Fix immediately, no exceptions

   # P1: Performance (~20 warnings)
   # Focus: needless_pass_by_value in internal functions
   # Example:
   # Before: fn merge_objects(a: Value, b: Value) -> Value
   # After:  fn merge_objects(a: &Value, b: &Value) -> Value

   # P2: Style (~65 warnings)
   # Fix in batches, may use #[allow(...)] if justified
   ```

3. **REFACTOR**: Optimize pgrx FFI signatures
   ```rust
   // CANNOT change #[pg_extern] signatures due to FFI
   // But CAN optimize internal helpers

   // Before:
   fn deep_merge_recursive(target: Value, source: Value, depth: usize) -> Value {
       // ... large Value copies on every recursion
   }

   // After:
   fn deep_merge_recursive(target: &mut Value, source: &Value, depth: usize) {
       // ... zero-copy recursion
   }
   ```

4. **QA**: Verify zero warnings
   ```bash
   # Must pass with zero output
   cargo clippy --no-default-features --features pg17 \
     --all-targets -- -D warnings

   # Full test suite
   cargo pgrx test pg17

   # Performance regression check
   psql -U postgres -f test/benchmark_comparison.sql > after-phase2-perf.txt
   ./scripts/check_regression.sh baselines/baseline-perf.txt after-phase2-perf.txt 5
   ```

5. **GREENFIELD**: Add pre-commit hook
   ```bash
   # .git/hooks/pre-commit (or use pre-commit framework)
   #!/bin/bash
   cargo clippy --no-default-features --features pg17 --all-targets -- -D warnings
   if [ $? -ne 0 ]; then
       echo "❌ Clippy failed - fix warnings before committing"
       exit 1
   fi
   ```

### Verification Commands
```bash
# Zero warnings check (MUST PASS)
cargo clippy --no-default-features --features pg17 \
  --all-targets -- -D warnings

# Count allowed exceptions (should be <5)
rg "#\[allow\(clippy::" src/ | wc -l

# Performance regression test
./scripts/check_regression.sh \
  baselines/baseline-perf.txt \
  after-phase2-perf.txt \
  5  # max 5% regression

# Full test suite
cargo pgrx test pg17
```

### Acceptance Criteria
- ✅ **Zero clippy warnings**: `cargo clippy ... -- -D warnings` exits 0
- ✅ **Justified exceptions only**: <5 total `#[allow(clippy::...)]` with comments
  ```rust
  // Example of justified exception:
  #[allow(clippy::type_complexity)]  // pgrx FFI requires this signature
  #[pg_extern]
  fn complex_function(...) { }
  ```
- ✅ **Performance maintained**: <5% regression on all benchmarks
  - Merge operations: <44ms (baseline: 42ms)
  - Array operations: <28ms (baseline: 26ms)
- ✅ **Pre-commit hooks active**: Prevent future warnings
- ✅ **CI enforces clippy**: `.github/workflows/lint.yml` fails on warnings

### Allowed Exceptions (Document Each)
```rust
// src/lib.rs
#[allow(clippy::type_complexity)]  // pgrx FFI signature required
#[pg_extern]
fn jsonb_ivm_array_update_where(...) { }

// src/merge.rs
#[allow(clippy::too_many_arguments)]  // Internal helper, will refactor in Phase 3
fn merge_with_options(...) { }
```

### DO NOT
- Use `#[allow(clippy::...)]` without explanation comment
- Change `#[pg_extern]` function signatures (breaks FFI)
- Introduce breaking API changes without deprecation
- Skip performance regression testing

### Rollback Plan
```bash
# If optimizations break functionality:
git log --oneline --grep "Phase 2" | head -5
git revert <commit-hash>

# If partial rollback needed:
# 1. Keep style fixes (safe)
# 2. Revert performance optimizations (may have bugs)
git revert <perf-optimization-commit>
```

---

## Phase 3: Architecture Enhancement - Nested Path Support

**Duration**: 2-3 weeks
**Priority**: Medium
**Depends On**: Phase 0 complete, Phase 2 recommended

### Objective
Extend path operations to support nested object navigation using dot notation and array indices (e.g., `user.profile.orders[0].id`).

### Context
- **Current**: Single-level key access only (`user`)
- **Goal**: Multi-level path access (`user.profile.name`)
- **Use Case**: Complex JSONB structures in CQRS read models

### Nested Path Syntax Specification

**Supported Syntax (MVP)**:
- ✅ **Dot notation**: `a.b.c` → access nested objects
- ✅ **Array indexing**: `a[0]` → access array element by index
- ✅ **Mixed paths**: `orders[0].items[1].price` → combined access
- ✅ **Backward compatibility**: Single keys `user` still work

**Explicitly NOT Supported** (document for future):
- ❌ **Negative indices**: `a[-1]` (last element)
- ❌ **Slices**: `a[0:5]` (range)
- ❌ **Wildcards**: `a[*].b` (all elements)
- ❌ **Escaped dots**: `a."b.c"` (keys containing dots)
- ❌ **Bracket notation for keys**: `a["key with spaces"]`

**Rationale**: Keep syntax simple, focused on 80% use case. Advanced features defer to PostgreSQL's native jsonb operators.

### Files to Modify/Create
- `src/path.rs` (NEW: path parser, ~300 lines)
  ```rust
  pub enum PathSegment {
      Key(String),          // .field
      Index(usize),         // [0]
  }

  pub fn parse_path(path: &str) -> Result<Vec<PathSegment>, ParseError> { }
  pub fn navigate_path(json: &Value, path: &[PathSegment]) -> Option<&Value> { }
  ```
- `src/array_ops.rs` (update to accept paths instead of single keys)
- `src/lib.rs` (add new `_path` function variants)
- `test/sql/nested_paths.sql` (NEW: comprehensive SQL tests)
- `test/expected/nested_paths.out` (expected output)
- `docs/API.md` (add nested path documentation + examples)
- `README.md` (update with nested path examples)

### Implementation Steps

1. **RED**: Add failing tests for nested paths
   ```sql
   -- test/sql/nested_paths.sql

   -- Test 1: Dot notation
   SELECT jsonb_ivm_array_update_where_path(
     '{"users": [{"id": 1, "profile": {"name": "Alice"}}]}'::jsonb,
     'users',              -- array key
     'id', 1,              -- where clause
     'profile.name',       -- NESTED PATH (fails initially)
     '"Bob"'::jsonb
   );
   -- Expected: {"users": [{"id": 1, "profile": {"name": "Bob"}}]}

   -- Test 2: Array indexing
   SELECT jsonb_ivm_set_path(
     '{"orders": [{"items": [{"price": 10}]}]}'::jsonb,
     'orders[0].items[0].price',  -- NESTED PATH (fails initially)
     '20'::jsonb
   );
   -- Expected: {"orders": [{"items": [{"price": 20}]}]}
   ```

2. **GREEN**: Implement path parser
   ```rust
   // src/path.rs

   #[derive(Debug, PartialEq)]
   pub enum PathSegment {
       Key(String),
       Index(usize),
   }

   pub fn parse_path(path: &str) -> Result<Vec<PathSegment>, String> {
       let mut segments = Vec::new();
       let mut current_key = String::new();
       let mut chars = path.chars().peekable();

       while let Some(ch) = chars.next() {
           match ch {
               '.' => {
                   if !current_key.is_empty() {
                       segments.push(PathSegment::Key(current_key.clone()));
                       current_key.clear();
                   }
               }
               '[' => {
                   if !current_key.is_empty() {
                       segments.push(PathSegment::Key(current_key.clone()));
                       current_key.clear();
                   }
                   // Parse index
                   let index_str: String = chars
                       .by_ref()
                       .take_while(|&c| c != ']')
                       .collect();
                   let index = index_str.parse::<usize>()
                       .map_err(|_| format!("Invalid array index: {}", index_str))?;
                   segments.push(PathSegment::Index(index));
               }
               _ => current_key.push(ch),
           }
       }

       if !current_key.is_empty() {
           segments.push(PathSegment::Key(current_key));
       }

       Ok(segments)
   }

   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn test_parse_simple() {
           assert_eq!(
               parse_path("a.b.c").unwrap(),
               vec![
                   PathSegment::Key("a".into()),
                   PathSegment::Key("b".into()),
                   PathSegment::Key("c".into()),
               ]
           );
       }

       #[test]
       fn test_parse_array() {
           assert_eq!(
               parse_path("a[0].b[1]").unwrap(),
               vec![
                   PathSegment::Key("a".into()),
                   PathSegment::Index(0),
                   PathSegment::Key("b".into()),
                   PathSegment::Index(1),
               ]
           );
       }
   }
   ```

3. **REFACTOR**: Add new `_path` function variants (maintain backward compat)
   ```rust
   // src/lib.rs

   // NEW: Path-based variant
   #[pg_extern]
   fn jsonb_ivm_array_update_where_path(
       target: JsonB,
       array_key: &str,
       match_key: &str,
       match_value: JsonB,
       update_path: &str,     // NEW: nested path
       update_value: JsonB,
   ) -> JsonB {
       let path_segments = path::parse_path(update_path)
           .unwrap_or_else(|e| error!("Invalid path: {}", e));

       // ... use path navigation instead of single key
   }

   // OLD: Keep for backward compatibility
   #[pg_extern]
   fn jsonb_ivm_array_update_where(
       target: JsonB,
       array_key: &str,
       match_key: &str,
       match_value: JsonB,
       update_key: &str,      // Single key (legacy)
       update_value: JsonB,
   ) -> JsonB {
       // Internally calls _path variant with single segment
       jsonb_ivm_array_update_where_path(
           target, array_key, match_key, match_value,
           update_key,  // path with 1 segment
           update_value,
       )
   }
   ```

4. **QA**: Comprehensive nested path testing
   ```bash
   # SQL integration tests
   cargo pgrx test pg17 -- nested_paths

   # Fuzz nested path parser
   cargo fuzz run fuzz_path_parser -- -max_len=1000

   # Performance benchmark (should be <10% slower)
   psql -U postgres -f test/benchmark_nested_paths.sql
   ```

5. **GREENFIELD**: Update documentation
   ```markdown
   # docs/API.md

   ## Nested Path Support (v0.2.0+)

   All `_path` function variants support nested object navigation:

   **Syntax**:
   - Dot notation: `user.profile.email`
   - Array indexing: `orders[0].total`
   - Combined: `users[0].addresses[1].city`

   **Examples**:
   ```sql
   -- Update nested field
   SELECT jsonb_ivm_set_path(
     '{"user": {"profile": {"name": "Alice"}}}'::jsonb,
     'user.profile.name',
     '"Bob"'::jsonb
   );
   -- Result: {"user": {"profile": {"name": "Bob"}}}

   -- Update array element nested field
   SELECT jsonb_ivm_array_update_where_path(
     '{"orders": [{"id": 1, "status": "pending"}]}'::jsonb,
     'orders',
     'id', '1'::jsonb,
     'status',  -- can also be nested like 'shipping.status'
     '"shipped"'::jsonb
   );
   ```

   **Limitations**:
   - Max path depth: 100 segments
   - No negative indices (use PostgreSQL's `jsonb_array_length`)
   - No wildcards (use PostgreSQL's `jsonb_path_query`)
   ```

### Verification Commands
```bash
# Test nested path SQL
cargo pgrx test pg17 -- nested_paths

# Benchmark performance
psql -U postgres -f test/benchmark_nested_paths.sql > nested-paths-perf.txt

# Check regression (<10% slower than single-level)
./scripts/check_regression.sh \
  baselines/baseline-perf.txt \
  nested-paths-perf.txt \
  10  # max 10% regression

# Fuzz path parser
cargo fuzz run fuzz_path_parser -- -max_total_time=3600

# Verify backward compatibility
cargo pgrx test pg17  # all old tests should pass
```

### Acceptance Criteria
- ✅ **Dot notation**: `a.b.c` works for nested objects
  ```sql
  SELECT jsonb_ivm_set_path('{"a":{"b":{"c":1}}}'::jsonb, 'a.b.c', '2'::jsonb);
  -- Result: {"a":{"b":{"c":2}}}
  ```
- ✅ **Array indexing**: `a[0].b` works for array elements
  ```sql
  SELECT jsonb_ivm_set_path('{"a":[{"b":1}]}'::jsonb, 'a[0].b', '2'::jsonb);
  -- Result: {"a":[{"b":2}]}
  ```
- ✅ **Mixed paths**: `orders[0].items[1].price` works
- ✅ **Backward compatibility**: All existing single-key tests pass
- ✅ **Performance**: Within 10% of single-level operations
  - Single-level update: 26ms baseline
  - Nested path (3 levels): <29ms
- ✅ **Error handling**: Clear messages for invalid paths
  ```sql
  SELECT jsonb_ivm_set_path('{}'::jsonb, 'a[invalid]', '1'::jsonb);
  -- ERROR: Invalid array index: invalid
  ```
- ✅ **Documentation**: Examples in API.md and README.md

### DO NOT
- Break existing single-key function signatures
- Implement complex path features (wildcards, slices) without user research
- Allow unbounded path depth (limit to 100 segments)
- Change behavior of existing functions

### Rollback Plan
```bash
# Use feature flag for gradual rollout
#[cfg(feature = "nested_paths")]
#[pg_extern]
fn jsonb_ivm_array_update_where_path(...) { }

# If issues found:
# 1. Disable feature in Cargo.toml (default-features = ["nested_paths"] → [])
# 2. Document known issues
# 3. Fix in patch release
```

---

## Phase 4: Testing Expansion

**Duration**: 1-2 weeks
**Priority**: Medium
**Depends On**: Phase 1 complete (depth validation needs property tests)

### Objective
Add property-based testing and load testing to increase test coverage and confidence in edge cases.

### Context
- **Current**: Excellent SQL test coverage, fuzzing for 3 targets
- **Gap**: No mathematical property verification, no concurrency testing
- **Goal**: Prove correctness through property laws, validate thread safety

### Property-Based Testing Strategy

**Properties to Test**:
1. **Merge Associativity**: `merge(merge(a, b), c) == merge(a, merge(b, c))`
2. **Merge Idempotence**: `merge(a, a) == a`
3. **Merge Identity**: `merge(a, {}) == a`
4. **Array Update Preservation**: `length(update(arr)) == length(arr)` (no inserts)
5. **Depth Invariant**: `depth(merge(a, b)) <= max(depth(a), depth(b))`

### Files to Modify/Create
```
Cargo.toml                          (add quickcheck dependencies)
src/lib.rs                          (add #[cfg(test)] property tests)
src/merge.rs                        (add internal test helpers)
test/property/                      (NEW directory)
  test_merge_properties.rs          (QuickCheck tests for merge)
  test_array_properties.rs          (QuickCheck tests for arrays)
  test_depth_properties.rs          (QuickCheck tests for depth validation)
test/load/                          (NEW directory)
  load_test_concurrent_merge.sql    (pgbench scripts)
  load_test_concurrent_array.sql
scripts/
  run_load_tests.sh                 (NEW: pgbench automation)
  run_property_tests.sh             (NEW: QuickCheck with high iterations)
.github/workflows/test.yml          (add property test job)
```

### Prerequisites

**Add Dependencies**:
```toml
# Cargo.toml
[dev-dependencies]
quickcheck = "1.0"
quickcheck_macros = "1.0"
arbitrary = "1.3"  # For generating random JSONB

# For load testing (optional, uses pgbench)
# No Rust deps needed, pgbench is PostgreSQL tool
```

### Implementation Steps

1. **RED**: Add failing property tests
   ```rust
   // src/lib.rs (at bottom, in #[cfg(test)])

   #[cfg(test)]
   mod property_tests {
       use super::*;
       use quickcheck::{quickcheck, TestResult};
       use quickcheck_macros::quickcheck;

       #[quickcheck]
       fn prop_merge_associative(a: JsonB, b: JsonB, c: JsonB) -> TestResult {
           // This will FAIL initially if merge has bugs
           let left = jsonb_ivm_deep_merge(
               jsonb_ivm_deep_merge(a.clone(), b.clone()),
               c.clone()
           );
           let right = jsonb_ivm_deep_merge(
               a.clone(),
               jsonb_ivm_deep_merge(b, c)
           );

           TestResult::from_bool(left == right)
       }

       #[quickcheck]
       fn prop_merge_identity(a: JsonB) -> bool {
           let empty = JsonB(serde_json::json!({}));
           jsonb_ivm_merge_shallow(a.clone(), empty) == a
       }
   }
   ```

2. **GREEN**: Implement property test infrastructure
   ```rust
   // Need to implement Arbitrary for JsonB to generate random instances

   #[cfg(test)]
   impl quickcheck::Arbitrary for JsonB {
       fn arbitrary(g: &mut quickcheck::Gen) -> Self {
           // Generate random JSONB with depth limit
           fn gen_value(g: &mut quickcheck::Gen, depth: usize) -> serde_json::Value {
               if depth > 5 {  // Limit depth for property tests
                   return serde_json::Value::Null;
               }

               match u8::arbitrary(g) % 5 {
                   0 => serde_json::Value::Null,
                   1 => serde_json::Value::Bool(bool::arbitrary(g)),
                   2 => serde_json::Value::Number((i32::arbitrary(g)).into()),
                   3 => {
                       let size = usize::arbitrary(g) % 5;
                       let obj: serde_json::Map<String, Value> = (0..size)
                           .map(|_| {
                               let key = format!("key{}", u8::arbitrary(g));
                               let val = gen_value(g, depth + 1);
                               (key, val)
                           })
                           .collect();
                       serde_json::Value::Object(obj)
                   }
                   4 => {
                       let size = usize::arbitrary(g) % 5;
                       let arr: Vec<Value> = (0..size)
                           .map(|_| gen_value(g, depth + 1))
                           .collect();
                       serde_json::Value::Array(arr)
                   }
                   _ => unreachable!(),
               }
           }

           JsonB(gen_value(g, 0))
       }
   }
   ```

3. **REFACTOR**: Add load testing scripts
   ```bash
   # scripts/run_load_tests.sh
   #!/bin/bash

   set -e

   echo "Starting PostgreSQL load tests..."

   # Setup test database
   psql -U postgres -c "DROP DATABASE IF EXISTS loadtest;"
   psql -U postgres -c "CREATE DATABASE loadtest;"
   psql -U postgres -d loadtest -c "CREATE EXTENSION jsonb_ivm;"

   # Prepare test data
   psql -U postgres -d loadtest <<EOF
   CREATE TABLE test_jsonb (
       id SERIAL PRIMARY KEY,
       data JSONB
   );

   INSERT INTO test_jsonb (data)
   SELECT jsonb_build_object('id', i, 'value', i * 10)
   FROM generate_series(1, 1000) AS i;
   EOF

   # Run concurrent merge operations
   echo "Running concurrent merge test (100 clients, 10 seconds)..."
   pgbench -U postgres -d loadtest -c 100 -j 10 -T 10 -f test/load/load_test_concurrent_merge.sql

   # Run concurrent array operations
   echo "Running concurrent array update test (100 clients, 10 seconds)..."
   pgbench -U postgres -d loadtest -c 100 -j 10 -T 10 -f test/load/load_test_concurrent_array.sql

   # Cleanup
   psql -U postgres -c "DROP DATABASE loadtest;"

   echo "✅ Load tests complete"
   ```

   ```sql
   -- test/load/load_test_concurrent_merge.sql
   \set id random(1, 1000)

   UPDATE test_jsonb
   SET data = jsonb_ivm_deep_merge(
       data,
       jsonb_build_object('updated_at', now()::text)
   )
   WHERE id = :id;
   ```

4. **QA**: Run extensive property testing
   ```bash
   # Run property tests with high iteration count
   QUICKCHECK_TESTS=100000 cargo test --release property_tests

   # Run load tests
   ./scripts/run_load_tests.sh

   # Verify no deadlocks/race conditions
   # (pgbench will report errors if any occur)
   ```

5. **GREENFIELD**: Integrate into CI
   ```yaml
   # .github/workflows/test.yml

   property-tests:
     name: Property-Based Tests
     runs-on: ubuntu-latest
     steps:
       - uses: actions/checkout@v4
       - name: Run QuickCheck tests
         run: |
           QUICKCHECK_TESTS=10000 cargo test --release property_tests

   load-tests:
     name: Load Tests
     runs-on: ubuntu-latest
     steps:
       - uses: actions/checkout@v4
       - name: Install PostgreSQL
         run: sudo apt-get install -y postgresql postgresql-contrib
       - name: Run load tests
         run: ./scripts/run_load_tests.sh
   ```

### Verification Commands
```bash
# Property tests (100k iterations)
QUICKCHECK_TESTS=100000 cargo test --release property_tests -- --nocapture

# Load tests (100 concurrent clients)
./scripts/run_load_tests.sh

# Verify no test failures
echo $?  # Should be 0

# Fuzz all targets (expand coverage)
for target in $(cargo fuzz list); do
    cargo fuzz run $target -- -max_len=100000 -max_total_time=3600
done
```

### Acceptance Criteria
- ✅ **Property tests pass 100k+ iterations** without failures:
  ```
  running 5 tests
  test property_tests::prop_merge_associative ... ok (100000 iterations)
  test property_tests::prop_merge_identity ... ok (100000 iterations)
  test property_tests::prop_array_length_preservation ... ok (100000 iterations)
  test property_tests::prop_depth_invariant ... ok (100000 iterations)
  test property_tests::prop_merge_idempotence ... ok (100000 iterations)
  ```
- ✅ **Load testing**: 100 concurrent clients, 10 seconds, **zero errors**:
  ```
  pgbench -c 100 -j 10 -T 10
  transaction type: test/load/load_test_concurrent_merge.sql
  number of transactions actually processed: 12543
  number of failed transactions: 0 (0.000%)  ← MUST BE ZERO
  ```
- ✅ **No deadlocks**: PostgreSQL logs show zero deadlock errors
- ✅ **CI integration**: Property tests run on every PR
- ✅ **Coverage increase**: (Once tarpaulin fixed) >85% line coverage

### Properties Implemented
```rust
// Phase 4 property list:
1. ✅ Merge associativity: merge(merge(a,b),c) == merge(a,merge(b,c))
2. ✅ Merge identity: merge(a, {}) == a
3. ✅ Merge idempotence: merge(a, a) == a
4. ✅ Array length preservation: len(array_update(arr)) == len(arr)
5. ✅ Depth invariant: depth(merge(a,b)) <= max(depth(a), depth(b))
6. ✅ Depth limit enforcement: depth(x) > 1000 → Error
```

### DO NOT
- Add flaky tests (use deterministic seeds for debugging)
- Require external services (all tests self-contained)
- Skip load testing (critical for production readiness)
- Generate unbounded JSONB in property tests (limit depth to 10)

### Rollback Plan
```bash
# If property tests reveal fundamental bugs:
# 1. Disable failing property in CI (comment out test)
# 2. File issue with reproduction case
# 3. Keep passing properties active
# 4. Fix bug in separate PR

# Property tests are additive - no rollback needed
# Just disable/skip failing tests until bugs fixed
```

---

## Phase 5: Documentation and CI/CD Polish

**Duration**: 1 week
**Priority**: Low (polish)
**Depends On**: Phases 2-4 complete (clean code + features)

### Objective
Complete documentation gaps, add version compatibility matrix, and fix remaining CI/CD issues for production readiness.

### Context
- **Docs**: Missing PostgreSQL version compatibility matrix
- **CI**: Coverage tracking broken (tarpaulin), macOS excluded (per requirements)
- **Polish**: Nested path examples need documentation

### Files to Modify/Create
```
docs/
  COMPATIBILITY.md                (NEW: version matrix)
  API.md                          (update with Phase 3 examples)
  ARCHITECTURE.md                 (update with new modules from Phase 0)
  security/threat-model.md        (already updated in Phase 1)
README.md                         (add nested path examples)
.github/workflows/
  test.yml                        (fix coverage tracking)
  release.yml                     (NEW: automated releases)
scripts/
  validate_docs.sh                (NEW: markdown linting)
  generate_compatibility.sh       (NEW: test all PG versions)
CHANGELOG.md                      (NEW: semantic versioning)
```

### Implementation Steps

1. **RED**: Identify documentation gaps
   ```bash
   # Find broken links
   markdownlint-cli2 "**/*.md" --fix

   # Find missing API examples
   rg "TODO|FIXME|XXX" docs/

   # List CI failures
   gh run list --workflow=test.yml --limit 5
   ```

2. **GREEN**: Create compatibility matrix
   ```markdown
   # docs/COMPATIBILITY.md

   # PostgreSQL Version Compatibility

   ## Supported Versions

   | PostgreSQL | jsonb_ivm | Status | Notes |
   |------------|-----------|--------|-------|
   | 13.x       | 0.1.0+    | ✅ Tested | Full support |
   | 14.x       | 0.1.0+    | ✅ Tested | Full support |
   | 15.x       | 0.1.0+    | ✅ Tested | Full support |
   | 16.x       | 0.1.0+    | ✅ Tested | Full support |
   | 17.x       | 0.1.0+    | ✅ Tested | Full support (primary) |
   | 18.x       | 0.2.0+    | ⚠️ Beta  | Testing in progress |

   ## Feature Availability

   | Feature | PG 13-17 | PG 18 | Notes |
   |---------|----------|-------|-------|
   | Basic merge | ✅ | ✅ | All versions |
   | Deep merge | ✅ | ✅ | All versions |
   | Array operations | ✅ | ✅ | All versions |
   | Nested paths | ✅ (v0.2.0+) | ✅ | Requires jsonb_ivm 0.2.0+ |
   | Depth limits | ✅ (v0.2.0+) | ✅ | Security hardening |

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
   ```

3. **REFACTOR**: Fix CI coverage tracking
   ```yaml
   # .github/workflows/test.yml

   coverage:
     name: Code Coverage
     runs-on: ubuntu-latest
     steps:
       - uses: actions/checkout@v4

       - name: Install Rust
         uses: dtolnay/rust-toolchain@stable

       - name: Install tarpaulin
         run: cargo install cargo-tarpaulin

       - name: Run coverage
         run: |
           # Fix: Use --no-default-features (avoid pgrx version conflict)
           cargo tarpaulin \
             --no-default-features \
             --features pg17 \
             --workspace \
             --exclude-files "*/test/*" "fuzz/*" \
             --out Xml \
             --output-dir coverage/

       - name: Upload to Codecov
         uses: codecov/codecov-action@v4
         with:
           files: ./coverage/cobertura.xml
           fail_ci_if_error: false  # Don't block on coverage issues
   ```

4. **QA**: Validate all documentation
   ```bash
   # Lint markdown
   markdownlint-cli2 "**/*.md" --config .markdownlint.json

   # Check for broken links (if using link checker)
   markdown-link-check docs/**/*.md README.md

   # Validate code examples compile
   cargo test --doc

   # Verify coverage tracking works
   cargo tarpaulin --no-default-features --features pg17 --out Html
   open tarpaulin-report.html
   ```

5. **GREENFIELD**: Add automated release workflow
   ```yaml
   # .github/workflows/release.yml

   name: Release

   on:
     push:
       tags:
         - 'v*'

   jobs:
     release:
       runs-on: ubuntu-latest
       steps:
         - uses: actions/checkout@v4

         - name: Build release binary
           run: cargo pgrx package

         - name: Create GitHub Release
           uses: softprops/action-gh-release@v1
           with:
             files: target/release/jsonb_ivm-pg17/*.so
             generate_release_notes: true
   ```

### Verification Commands
```bash
# Documentation validation
markdownlint-cli2 "**/*.md"
cargo test --doc  # Verify code examples compile

# CI simulation (local)
act -j test
act -j coverage

# Coverage report generation
cargo tarpaulin \
  --no-default-features \
  --features pg17 \
  --out Html \
  --output-dir coverage/

# Open coverage report
open coverage/index.html  # Should show >85% coverage
```

### Acceptance Criteria
- ✅ **Compatibility matrix**: `docs/COMPATIBILITY.md` with all PG versions tested
  - Clear table showing version support
  - Feature availability per version
  - Platform support documented
- ✅ **Coverage tracking**: CI generates coverage report
  - Tarpaulin runs successfully
  - Coverage badge in README
  - Target: >85% line coverage
- ✅ **Documentation quality**:
  - Zero markdown lint errors
  - All code examples compile (`cargo test --doc`)
  - Nested path examples in README and API docs
- ✅ **README updated**:
  ```markdown
  ## Quick Start

  ### Nested Path Support (v0.2.0+)

  Update deeply nested fields:
  ```sql
  SELECT jsonb_ivm_array_update_where_path(
    '{"users": [{"id": 1, "profile": {"name": "Alice"}}]}'::jsonb,
    'users',
    'id', '1'::jsonb,
    'profile.name',  -- Nested path!
    '"Bob"'::jsonb
  );
  ```
  ```
- ✅ **CI/CD polish**:
  - All workflows pass
  - Coverage tracking works
  - Automated releases configured

### DO NOT
- Attempt macOS support (explicitly excluded)
- Block CI on coverage percentage (use as informational)
- Change docs without verifying all internal links work

### Rollback Plan
```bash
# Documentation changes are low-risk
# If errors found, simple PR to fix

# If coverage tracking breaks CI:
# Set fail_ci_if_error: false (already in config)

# If release automation fails:
# Manual release process still works (fallback)
```

---

## Success Metrics (9.5/10 Rating Criteria)

### Security (25 points)
- ✅ **DREAD scores <25/50**: All risks mitigated (Phase 1)
  - Before: DoS risk 35/50 (MEDIUM-HIGH)
  - After: DoS risk 21/50 (LOW-MEDIUM)
- ✅ **Fuzzing coverage**: 24h runs clean, 3+ targets
- ✅ **Depth limits**: Max 1000 levels enforced

### Code Quality (25 points)
- ✅ **Zero clippy warnings**: `cargo clippy -- -D warnings` passes (Phase 2)
- ✅ **Optimized signatures**: Zero-copy where possible
- ✅ **Modular structure**: Monolith split into 6+ modules (Phase 0)

### Features & Usability (20 points)
- ✅ **Nested path support**: `a.b.c` and `a[0].b` syntax (Phase 3)
- ✅ **Backward compatibility**: All existing tests pass
- ✅ **Performance**: <10% regression on new features

### Testing & Reliability (20 points)
- ✅ **Property tests**: 100k+ iterations passing (Phase 4)
- ✅ **Load testing**: 100 concurrent clients, zero errors
- ✅ **Coverage**: >85% line coverage (Phase 5)

### Documentation & Polish (10 points)
- ✅ **Compatibility matrix**: All PG versions documented (Phase 5)
- ✅ **API examples**: Nested paths documented
- ✅ **CI/CD**: Coverage tracking, automated releases

**Total**: 100 points = **9.5/10 rating**

---

## Risk Mitigation

### Technical Risks

| Risk | Impact | Probability | Mitigation | Owner |
|------|--------|-------------|------------|-------|
| Modularization breaks FFI | High | Low | Phase 0 testing, pgrx test suite | Phase 0 |
| Depth limits break valid use cases | Medium | Low | Conservative 1000 limit, user feedback | Phase 1 |
| Clippy fixes introduce bugs | Medium | Medium | Property tests catch regressions | Phase 2 |
| Nested paths too complex | Medium | Medium | Start simple, defer advanced features | Phase 3 |
| Property tests find fundamental bugs | High | Low | Fix bugs, improve code quality | Phase 4 |
| Tarpaulin still broken | Low | High | Use as informational, don't block CI | Phase 5 |

### Schedule Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| Scope creep in Phase 3 | +2 weeks | Strict syntax spec, defer advanced features |
| Unexpected bugs in Phase 2 | +1 week | Property tests catch early, fix incrementally |
| Integration issues between phases | +1 week | Run full test suite after each phase |

### Rollback Strategy

**Per-Phase Rollback**:
```bash
# 1. Identify last successful commit
git log --oneline --grep "\[GREENFIELD\] Phase N" -1

# 2. Create rollback branch
git checkout -b rollback-phase-N

# 3. Revert specific commits (not whole phase)
git revert <commit-hash-of-broken-change>

# 4. Verify tests pass
cargo pgrx test pg17

# 5. Merge rollback if tests pass
git checkout main
git merge rollback-phase-N
```

**Feature Flag Rollback** (for risky features):
```rust
// Cargo.toml
[features]
default = []  # Disable new features by default
nested_paths = []  # Phase 3 feature
strict_depth_limits = []  # Phase 1 feature

// Code
#[cfg(feature = "nested_paths")]
#[pg_extern]
fn jsonb_ivm_array_update_where_path(...) { }
```

**Database Migration Rollback** (if extension schema changes):
```sql
-- If new version has issues:
DROP EXTENSION jsonb_ivm;
CREATE EXTENSION jsonb_ivm VERSION '0.1.0';  -- Rollback to known-good

-- Or use ALTER EXTENSION (if migration path exists):
ALTER EXTENSION jsonb_ivm UPDATE TO '0.1.0';
```

---

## Timeline Estimate (Revised with Contingency)

| Phase | Optimistic | Realistic | Pessimistic | Contingency Buffer |
|-------|-----------|-----------|-------------|-------------------|
| **Phase 0**: Baseline & Modularization | 3 days | **5 days** | 7 days | +2 days (FFI issues) |
| **Phase 1**: Security Hardening | 1 week | **10 days** | 2 weeks | +4 days (architectural fixes) |
| **Phase 2**: Code Quality | 1 week | **10 days** | 2 weeks | +4 days (refactoring needed) |
| **Phase 3**: Nested Path Support | 2 weeks | **3 weeks** | 4 weeks | +1 week (backward compat) |
| **Phase 4**: Testing Expansion | 1 week | **10 days** | 2 weeks | +4 days (property discovery) |
| **Phase 5**: Docs & CI/CD Polish | 1 week | **1 week** | 10 days | +3 days (tarpaulin issues) |

**Total Timeline**:
- ✅ **Optimistic**: 6 weeks (everything goes perfectly)
- ✅ **Realistic**: **8-10 weeks** (expected delays, normal iteration)
- ⚠️ **Pessimistic**: 12 weeks (major issues found, significant rework)

**Recommended Planning**: Use **10-week timeline** with 2-week contingency buffer.

---

## Weekly Checkpoint Format

Track progress weekly to catch issues early:

```markdown
## Week N Checkpoint (YYYY-MM-DD)

**Phase**: X - Description
**Status**: 🟢 On Track | 🟡 Minor Issues | 🔴 Blocked

### Completed This Week
- ✅ Task 1 (RED step)
- ✅ Task 2 (GREEN step)

### In Progress
- 🔄 Task 3 (REFACTOR step) - 60% complete

### Blocked/Issues
- ⚠️ Issue: Clippy warning unfixable due to pgrx constraint
  - Impact: Need #[allow(clippy::...)] exception
  - Resolution: Document justification, proceed

### Metrics
- Clippy warnings: 88 → 45 (48% reduction)
- Test coverage: 78% → 82% (+4%)
- Performance: No regression

### Next Week Plan
- Complete Phase X REFACTOR step
- Start Phase X QA step
- Run performance benchmarks

### Risks
- None identified this week
```

---

## Definition of "9.5/10" Quality

**Objective Criteria** (must meet ALL):
1. ✅ **Security**: Zero known vulnerabilities, DREAD <25/50
2. ✅ **Code Quality**: Zero clippy warnings, modular structure
3. ✅ **Features**: Nested path support implemented
4. ✅ **Testing**: >85% coverage, property tests, load tests passing
5. ✅ **Documentation**: Complete API docs, compatibility matrix
6. ✅ **CI/CD**: All workflows passing, coverage tracked

**Subjective Criteria** (code review):
- Code is readable and well-documented
- Architectural decisions are sound
- Performance is optimized where it matters
- Error messages are helpful
- API is intuitive and consistent

**NOT Required for 9.5/10** (defer to future):
- ❌ Advanced path features (wildcards, slices)
- ❌ macOS support (excluded)
- ❌ 100% test coverage (85% is excellent)
- ❌ Zero performance regressions (5-10% acceptable for new features)

---

## Post-Phase 5: Maintenance Mode

After achieving 9.5/10, switch to maintenance mode:

**Ongoing Activities**:
- ✅ Security updates (monitor Rust/pgrx CVEs)
- ✅ PostgreSQL version compatibility (test new releases)
- ✅ Bug fixes (prioritize correctness over features)
- ✅ Performance monitoring (track regressions)

**Future Enhancements** (10/10 territory):
- Advanced path syntax (wildcards, slices)
- Schema evolution support (ALTER TABLE JSONB columns)
- Incremental view maintenance triggers
- Distributed JSONB merge (multi-node coordination)

**Not Planning** (out of scope):
- macOS support (explicitly excluded)
- Windows support (low demand)
- Non-PostgreSQL databases (PostgreSQL-specific extension)

---

## Appendix: Command Reference

**Baseline Capture**:
```bash
cargo clippy --no-default-features --features pg17 --all-targets -- -D warnings 2>&1 | tee baselines/baseline-clippy.txt
psql -U postgres -f test/benchmark_comparison.sql > baselines/baseline-perf.txt
rg "TODO|FIXME" --stats src/ > baselines/baseline-tech-debt.txt
```

**Testing**:
```bash
cargo pgrx test pg17                                    # SQL tests
QUICKCHECK_TESTS=100000 cargo test property_tests      # Property tests
./scripts/run_load_tests.sh                            # Load tests
cargo fuzz run fuzz_deep_merge -- -max_total_time=3600 # Fuzzing
```

**Performance**:
```bash
psql -U postgres -f test/benchmark_comparison.sql > current-perf.txt
./scripts/check_regression.sh baselines/baseline-perf.txt current-perf.txt 5
```

**Documentation**:
```bash
markdownlint-cli2 "**/*.md"
cargo test --doc
```

**Coverage**:
```bash
cargo tarpaulin --no-default-features --features pg17 --out Html
open tarpaulin-report.html
```

---

**Plan Version**: 2.0 (10/10 Quality)
**Last Updated**: 2025-12-13
**Next Review**: After Phase 0 completion
