# CI/CD Remaining Fixes - Phase Plans

## Overview

This directory contains phase plans for fixing the remaining CI/CD issues after the initial fix round. The first fix round (in `.phases/ci-fix/`) successfully fixed schema validation and Docker packaging, and added pgrx initialization. However, two issues remain:

1. **Property-Based Tests**: Compilation fails due to missing PostgreSQL feature flags
2. **Load Tests**: PostgreSQL not accepting TCP connections

## Problem Summary

After the first fix round (commits 2fca056, 57facce, b2e3bf2, f512327):

**✅ Working (6 items):**
- PostgreSQL 13 integration tests
- PostgreSQL 14 integration tests
- PostgreSQL 15 integration tests
- PostgreSQL 16 integration tests
- PostgreSQL 17 integration tests
- PostgreSQL 18 integration tests

**❌ Still Failing (2 items):**
- Property-Based Tests - `$PGRX_HOME does not exist` during compilation
- Load Tests - PostgreSQL configuration timeout

**✅ Passing:**
- Benchmark workflow
- Lint workflow (usually)

## Root Causes Identified

### Property-Based Tests
**Error**: `error: failed to run custom build command for pgrx-pg-sys v0.16.1`
**Root Cause**: The script `run_property_tests.sh` runs `cargo test --release` without specifying PostgreSQL version feature flags. Even though pgrx is initialized, the compiler doesn't know which PostgreSQL version bindings to use.
**Solution**: Add `--no-default-features --features pg17` to all cargo test commands.

### Load Tests
**Error**: `Configure PostgreSQL` step times out waiting for pg_isready
**Root Cause**: PostgreSQL cluster is created but not configured to listen on TCP port 5432, or there's a configuration issue preventing connections.
**Solution**: Explicitly configure listen_addresses, add comprehensive diagnostics, test both Unix socket and TCP.

## Phase Execution Order

Execute these phases sequentially:

### Phase 1: Fix Property Tests Feature Flags
**File:** `phase-1-property-tests-feature-flags.md`
**Objective:** Add PostgreSQL feature flags to property test script
**Time Estimate:** 30 minutes
**Impact:** HIGH - Enables property-based testing in CI
**Complexity:** LOW - Simple script modification

### Phase 2: Fix Load Tests PostgreSQL Configuration
**File:** `phase-2-load-tests-postgresql-diagnostics.md`
**Objective:** Fix PostgreSQL TCP listening and add diagnostics
**Time Estimate:** 1 hour
**Impact:** HIGH - Enables load testing in CI
**Complexity:** MEDIUM - PostgreSQL configuration + debugging

### Phase 3: Verify and Document
**File:** `phase-3-verify-and-document.md`
**Objective:** Comprehensive verification and final documentation
**Time Estimate:** 45 minutes
**Impact:** CRITICAL - Ensures all fixes work together
**Complexity:** LOW - Verification and documentation

## Total Time Estimate

- **Development**: 2-2.5 hours
- **CI/CD verification**: 30-60 minutes
- **Total**: 2.5-3.5 hours

## Success Criteria

All phases complete when:

- [ ] All GitHub Actions workflows show green checkmarks
- [ ] Property-based tests run with 10000 iterations in CI
- [ ] Load tests complete all benchmarks in CI
- [ ] All 6 PostgreSQL versions pass integration tests
- [ ] No HIGH or CRITICAL security vulnerabilities
- [ ] Comprehensive documentation of fixes created
- [ ] Phase plans archived for future reference

## Difference from First Fix Round

**First Round** (`.phases/ci-fix/`):
- Added missing infrastructure (pgrx initialization, Docker files)
- Fixed schema validation
- Set up PostgreSQL clusters
- ~60% of the problem

**This Round** (`.phases/ci-fix-remaining/`):
- Fine-tuning existing setup (feature flags, configuration)
- Enhanced diagnostics
- Final verification
- ~40% of the problem

## Files to Modify

### Scripts
- `scripts/run_property_tests.sh` - Add feature flags

### Workflows
- `.github/workflows/test.yml` - Enhance PostgreSQL setup in load-tests job

### Documentation (Phase 3)
- `.phases/ci-fix-complete/SUMMARY.md` - Create final summary
- `README.md` - Update badges (if needed)
- `.phases/completed/` - Archive phase plans

### No Changes Needed
- Source code (`src/**`) - All fixes are infrastructure/configuration
- Tests (`tests/**`) - Test code is correct
- `Cargo.toml` - Feature configuration is correct
- `Dockerfile` - Already fixed in first round

## Quick Start

```bash
# Phase 1: Fix property tests
# Edit scripts/run_property_tests.sh to add feature flags
vim scripts/run_property_tests.sh
git commit -m "fix(ci): add PostgreSQL feature flags to property tests"

# Phase 2: Fix load tests
# Edit .github/workflows/test.yml to enhance PostgreSQL setup
vim .github/workflows/test.yml
git commit -m "fix(ci): enhance PostgreSQL configuration for load tests"

# Phase 3: Verify and document
git push origin main
gh run watch <run-id>
# Create summary documentation
# Archive phase plans

# Done!
```

## Testing Strategy

### Local Testing (Before Commit)
```bash
# Property tests
./scripts/run_property_tests.sh 100

# Load tests (with PostgreSQL running)
./scripts/run_load_tests.sh

# Regular tests
cargo test --no-default-features --features pg17
```

### CI Testing (After Commit)
```bash
# Monitor all workflows
gh run list --limit 1
gh run watch <test-run-id>

# Check specific jobs
gh run view <run-id> --job=<job-id>

# Get logs if failing
gh run view <run-id> --log-failed
```

## Rollback Plan

If fixes don't work:

```bash
# Revert commits
git revert HEAD~2..HEAD

# Push revert
git push origin main

# Investigate further
gh run view <failed-run-id> --log > debug.log
cat debug.log | grep -A 20 "error\|Error\|FAIL"

# Update phase plans with new findings
vim .phases/ci-fix-remaining/phase-X-*.md

# Try again with updated approach
```

## Common Pitfalls to Avoid

1. **Don't forget feature flags everywhere**
   - Every `cargo test` and `cargo build` needs flags
   - Check both scripts and workflows

2. **Don't assume PostgreSQL defaults**
   - Explicitly configure listen_addresses
   - Explicitly specify ports
   - Verify with pg_lsclusters

3. **Don't skip local testing**
   - CI failures are slow to debug
   - Local testing catches issues early
   - Docker can simulate CI environment

4. **Don't ignore diagnostic output**
   - Enhanced error messages save debugging time
   - Log everything that might be useful
   - Show cluster status, processes, and configuration

5. **Don't commit without verification**
   - Run tests locally first
   - Check git diff carefully
   - Verify commit messages are descriptive

## Expected Final State

```
GitHub Actions - All Workflows Passing ✅

Test Workflow (8/8 jobs):
  ✓ PostgreSQL 13 - Integration tests
  ✓ PostgreSQL 14 - Integration tests
  ✓ PostgreSQL 15 - Integration tests
  ✓ PostgreSQL 16 - Integration tests
  ✓ PostgreSQL 17 - Integration tests
  ✓ PostgreSQL 18 - Integration tests
  ✓ Property-Based Tests - 8 properties × 10K iterations
  ✓ Load Tests - Performance benchmarks

Benchmark Workflow (1/1 jobs):
  ✓ Benchmark Tests - Performance metrics

Lint Workflow (1/1 jobs):
  ✓ Lint and Format - Code quality checks

Security & Compliance Workflow (4/4 checks):
  ✓ Container Security Scan - Trivy results
  ✓ Dependency Audit - No vulnerabilities
  ✓ License Compliance - All licenses approved
  ✓ Supply Chain Metadata - SBOM generated
```

## References

- First fix round: `.phases/ci-fix/`
- pgrx documentation: https://github.com/pgcentralfoundation/pgrx
- PostgreSQL cluster management: https://www.postgresql.org/docs/current/app-pg-ctl.html
- GitHub Actions workflow syntax: https://docs.github.com/en/actions/using-workflows/workflow-syntax-for-github-actions
- Troubleshooting PostgreSQL: https://wiki.postgresql.org/wiki/Troubleshooting

## Notes

**Why separate phase rounds?**

The first round focused on adding missing infrastructure. This round fine-tunes that infrastructure. Separating them:
- Makes each phase plan more focused
- Easier to understand the progression
- Clear separation between "adding stuff" and "configuring stuff"
- Historical record shows the iterative process

**Why such detailed phase plans?**

CI/CD issues are:
- Time-consuming to debug (wait for runners, check logs)
- Easy to miss subtle configuration issues
- Hard to reproduce locally
- Critical for project (broken CI blocks development)

Detailed phase plans:
- Reduce debugging time (clear instructions)
- Serve as troubleshooting guides
- Document decisions for future maintainers
- Follow PrintOptim CI/CD methodology

**Success depends on:**

1. Accurate root cause analysis
2. Systematic fixes (one thing at a time)
3. Comprehensive verification
4. Good documentation

This phase plan structure ensures all four.
