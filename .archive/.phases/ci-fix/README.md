# CI/CD Fix Phase Plans

## Overview

This directory contains detailed phase plans for fixing all CI/CD workflow failures identified after the Phase 5 documentation and polish commit.

## Problem Summary

After pushing the Phase 5 completion to GitHub, multiple CI/CD workflows failed:

1. **Test Workflow** - 3 failures:
   - ✅ Schema validation (fixed in commit 2fca056)
   - ❌ Property-based tests (missing pgrx setup)
   - ❌ Load tests (PostgreSQL not running)

2. **Security & Compliance Workflow** - 1 failure:
   - ❌ Docker build (cargo pgrx package error)

3. **Benchmark Workflow** - ✅ Passing
4. **Lint Workflow** - ✅ Passing

## Phase Execution Order

Execute these phases sequentially:

### Phase 1: Fix Property-Based Tests
**File:** `phase-1-property-tests-fix.md`
**Objective:** Add pgrx initialization to property-tests CI job
**Time Estimate:** 30 minutes
**Impact:** Medium (enables QuickCheck property testing in CI)

### Phase 2: Fix Load Tests
**File:** `phase-2-load-tests-fix.md`
**Objective:** Ensure PostgreSQL is running in load-tests CI job
**Time Estimate:** 45 minutes
**Impact:** Medium (enables performance/load testing in CI)

### Phase 3: Fix Docker Packaging
**File:** `phase-3-docker-packaging-fix.md`
**Objective:** Fix cargo pgrx package in Docker build
**Time Estimate:** 1 hour
**Impact:** High (enables security scanning and container deployment)

### Phase 4: Verify and Commit
**File:** `phase-4-verify-and-commit.md`
**Objective:** Comprehensive verification and documentation
**Time Estimate:** 45 minutes
**Impact:** Critical (ensures all fixes work together)

## Total Time Estimate

- **Development:** 3-4 hours
- **CI/CD verification:** 30-60 minutes
- **Total:** 3.5-5 hours

## Success Criteria

All phases complete when:

- [ ] All GitHub Actions workflows show green checkmarks
- [ ] Property-based tests run successfully in CI (10000 iterations)
- [ ] Load tests complete with performance metrics
- [ ] Docker image builds and passes Trivy security scan
- [ ] No HIGH or CRITICAL vulnerabilities reported
- [ ] Changes committed with comprehensive commit message
- [ ] Documentation updated (if needed)

## Files Modified

### GitHub Workflows
- `.github/workflows/test.yml` - Property tests and load tests fixes

### Docker
- `Dockerfile` - Fix packaging step and file copying
- `.dockerignore` - Ensure required files are included

### Scripts (maybe)
- `scripts/run_load_tests.sh` - CI-aware connection parameters

### Source Code
- **NONE** - All fixes are infrastructure-only

## Testing Strategy

### Local Testing (before commit)
```bash
# Phase 1: Property tests
rm -rf ~/.pgrx
cargo install cargo-pgrx --version 0.16.1
cargo pgrx init --pg17=$(which pg_config)
./scripts/run_property_tests.sh 10000

# Phase 2: Load tests
sudo systemctl start postgresql
./scripts/run_load_tests.sh

# Phase 3: Docker build
docker build -t jsonb_ivm:test .
docker run --rm jsonb_ivm:test psql -U postgres -c "CREATE EXTENSION jsonb_ivm;"
```

### CI Testing (after commit)
```bash
git push origin main
gh run watch $(gh run list --limit 1 --json databaseId --jq '.[0].databaseId')
```

## Rollback Plan

If any phase fails:

1. **Immediate:** Revert the commit
   ```bash
   git revert HEAD
   git push origin main
   ```

2. **Investigate:** Download failure logs
   ```bash
   gh run view <run-id> --log-failed > failure.log
   ```

3. **Fix:** Update phase plan with correct solution

4. **Re-test:** Verify fix locally before re-committing

## Dependencies

### Required Tools (local)
- Rust 1.85+
- cargo-pgrx 0.16.1
- PostgreSQL 13-18 (any version for local testing)
- Docker (for Phase 3)
- GitHub CLI (`gh`) for monitoring

### CI Environment
- Ubuntu 22.04 (GitHub Actions runner)
- PostgreSQL from apt.postgresql.org repository
- Rust from dtolnay/rust-toolchain action
- Docker buildx

## Common Issues and Solutions

### Issue: Property tests timeout
**Solution:** Reduce QuickCheck iterations for CI (but keep 10000 for thorough testing)

### Issue: Load tests fail intermittently
**Solution:** Add retry logic or increase timeout in pg_isready check

### Issue: Docker build runs out of memory
**Solution:** Use Docker layer caching or reduce build parallelism

### Issue: Trivy finds vulnerabilities
**Solution:** Update dependencies or document accepted risks in security policy

## References

- pgrx documentation: https://github.com/pgcentralfoundation/pgrx
- GitHub Actions docs: https://docs.github.com/en/actions
- Trivy scanner: https://github.com/aquasecurity/trivy
- PostgreSQL CI setup: https://www.postgresql.org/docs/current/install-binaries.html

## Notes

**Why separate phases?**
- Each phase addresses a distinct failure mode
- Allows incremental progress and testing
- Easier to debug if one fix causes issues
- Clear separation of concerns (property tests vs load tests vs security)

**Why fix all at once?**
- All failures are related to CI infrastructure
- Single commit keeps git history clean
- Easier to revert if needed
- Demonstrates comprehensive solution

**Why detailed documentation?**
- CI failures are hard to debug without context
- Future developers will thank us
- Serves as troubleshooting guide
- Documents decisions and trade-offs

## Phase Plan Template Used

Each phase follows the PrintOptim CI/CD phase plan structure:

1. **Objective** - What we're trying to achieve
2. **Context** - Current state and root cause
3. **Files to Modify** - Exact files that will change
4. **Implementation Steps** - Step-by-step with code examples
5. **Verification Commands** - How to test locally and in CI
6. **Acceptance Criteria** - Checklist for completion
7. **DO NOT** - Anti-patterns to avoid
8. **Notes** - Additional context and alternatives

This structure ensures comprehensive, executable plans that can be run by AI or humans with high success rate.
