# Phase 4: Verify All Fixes and Commit

## Objective

Verify that all CI/CD fixes work correctly both locally and in GitHub Actions, then commit the changes with proper documentation.

## Context

**Current State:**
After implementing Phases 1-3, we should have:
- ✅ Property-based tests with pgrx initialization
- ✅ Load tests with PostgreSQL properly configured
- ✅ Docker packaging working correctly
- Schema validation already fixed in previous commit

**This Phase:**
- Run comprehensive local verification
- Push changes and monitor CI/CD
- Document any remaining issues or known limitations
- Update project documentation if needed

## Files to Verify

1. `.github/workflows/test.yml` - All workflow changes
2. `Dockerfile` - Docker build fixes
3. `scripts/run_load_tests.sh` - Any script modifications
4. `.dockerignore` - Ensure it exists and is correct
5. All source files remain unchanged (fixes are infrastructure only)

## Verification Steps

### Step 1: Local Verification of Property Tests

```bash
# Clean pgrx environment to simulate CI
rm -rf ~/.pgrx ~/.cargo/bin/cargo-pgrx

# Install cargo-pgrx (simulating CI)
cargo install --locked cargo-pgrx --version 0.16.1

# Initialize pgrx (simulating CI)
cargo pgrx init --pg17=$(which pg_config)

# Run property tests
./scripts/run_property_tests.sh 10000

# Expected output:
# ✅ All property tests passed!
# Should see: 5 tests passed with 10000 QuickCheck iterations each
```

**Acceptance:**
- All 5 property tests pass
- No compilation errors
- No pgrx initialization errors
- Completes in reasonable time (< 3 minutes locally)

### Step 2: Local Verification of Load Tests

```bash
# Ensure PostgreSQL is running
sudo systemctl status postgresql
# If not running: sudo systemctl start postgresql

# Run load tests
./scripts/run_load_tests.sh

# Expected output:
# 🚀 Starting PostgreSQL load tests...
# ✅ Test data prepared (1000 rows)
# 🔄 Running concurrent merge test...
# ✅ Concurrent merge test passed (XXX ops/sec)
# ... more tests ...
# ✅ All load tests completed successfully!
```

**Acceptance:**
- PostgreSQL connectivity check passes
- All load tests complete successfully
- Performance metrics are reasonable
- No connection errors or timeouts

### Step 3: Local Docker Build Verification

```bash
# Build Docker image
docker build -t jsonb_ivm:verify .

# Should complete without errors

# Verify extension files are in the image
docker run --rm jsonb_ivm:verify ls -la /usr/share/postgresql/17/extension/
# Should see: jsonb_ivm.control, jsonb_ivm--0.1.0.sql

docker run --rm jsonb_ivm:verify ls -la /usr/lib/postgresql/17/lib/
# Should see: jsonb_ivm.so

# Test extension actually works
docker run --rm -d --name pg-verify -e POSTGRES_PASSWORD=test jsonb_ivm:verify
sleep 5

docker exec pg-verify psql -U postgres -c "CREATE EXTENSION jsonb_ivm;"
docker exec pg-verify psql -U postgres -c "SELECT jsonb_deep_merge('{\"a\":1}'::jsonb, '{\"b\":2}'::jsonb);"
# Should return: {"a": 1, "b": 2}

# Cleanup
docker stop pg-verify
docker rm pg-verify
```

**Acceptance:**
- Docker build completes successfully
- Extension files are present in correct locations
- Extension can be created in PostgreSQL
- Extension functions work correctly

### Step 4: Review All Changes

```bash
# Review all modified files
git status

# Check diff for workflow changes
git diff .github/workflows/test.yml

# Check diff for Dockerfile
git diff Dockerfile

# Check diff for any script changes
git diff scripts/

# Ensure no unintended changes
git diff src/
# Should show no changes (fixes are infrastructure only)
```

**Acceptance:**
- Only infrastructure files modified (.github/, Dockerfile, scripts/)
- No changes to source code (src/)
- No changes to tests (tests/)
- Changes match the phase plans exactly

### Step 5: Commit with Descriptive Message

```bash
git add .github/workflows/test.yml Dockerfile scripts/ .dockerignore

git commit -m "$(cat <<'EOF'
fix(ci): resolve all CI/CD workflow failures

Fixes three categories of CI failures identified after Phase 5:

**1. Property-Based Tests (Phase 1)**
- Add PostgreSQL 17 installation to property-tests job
- Install cargo-pgrx and initialize pgrx environment
- Fixes: "$PGRX_HOME does not exist" compilation error
- Property tests now compile and run successfully in CI

**2. Load Tests (Phase 2)**
- Ensure PostgreSQL cluster is created and started
- Configure trust authentication for CI environment
- Add pg_isready timeout and verification
- Update run_load_tests.sh with CI-aware connection params
- Fixes: "PostgreSQL is not running" error

**3. Docker Packaging (Phase 3)**
- Copy all required files (sql/, *.control) into Docker build
- Fix cargo pgrx package step with correct file paths
- Add debugging output for packaging failures
- Ensure multi-stage build preserves all artifacts
- Fixes: "No such file or directory" packaging error

**Schema Validation**
- Already fixed in commit 2fca056 (regenerated SQL schema)
- Line numbers and module paths now correct after refactoring

**Testing:**
- All fixes verified locally before commit
- Property tests: ✅ Pass with 10000 iterations
- Load tests: ✅ Pass with concurrent load
- Docker build: ✅ Builds and runs successfully

Closes the CI/CD issues preventing test and security workflows from passing.
EOF
)"
```

### Step 6: Push and Monitor CI/CD

```bash
# Push to GitHub
git push origin main

# Monitor the CI/CD runs
gh run list --limit 1

# Watch the test workflow
gh run watch <test-run-id>

# In separate terminal, watch security workflow
gh run watch <security-run-id>
```

**Monitor for:**
- ✅ Property-Based Tests job completes successfully
- ✅ Load Tests job completes successfully
- ✅ All PostgreSQL version tests pass
- ✅ Schema validation passes
- ✅ Docker build succeeds
- ✅ Trivy security scan completes
- ✅ Lint workflow passes
- ✅ Benchmark workflow passes

### Step 7: Verify CI Results

```bash
# After workflows complete, check results
gh run list --limit 5

# Should see all workflows with "completed success" status

# Check test details
gh run view <test-run-id>

# Check security scan results
gh run view <security-run-id> --log | grep -A 5 "Trivy"

# Check for any new issues in GitHub Security tab
gh api repos/:owner/:repo/code-scanning/alerts
```

**Acceptance:**
- All workflow jobs show ✓ (green checkmark)
- No new security vulnerabilities introduced
- All tests passing across all PostgreSQL versions (13-18)
- Code coverage metrics are reasonable (>80% if available)

## Final Verification Checklist

### Local Verification
- [ ] Property tests pass locally with clean pgrx environment
- [ ] Load tests pass locally with PostgreSQL running
- [ ] Docker image builds successfully
- [ ] Extension works in Docker container
- [ ] No unintended source code changes
- [ ] Git commit message is descriptive and accurate

### CI/CD Verification
- [ ] Property-Based Tests job: ✅ PASSED
- [ ] Load Tests job: ✅ PASSED
- [ ] PostgreSQL 13 integration tests: ✅ PASSED
- [ ] PostgreSQL 14 integration tests: ✅ PASSED
- [ ] PostgreSQL 15 integration tests: ✅ PASSED
- [ ] PostgreSQL 16 integration tests: ✅ PASSED
- [ ] PostgreSQL 17 integration tests: ✅ PASSED
- [ ] PostgreSQL 18 integration tests: ✅ PASSED
- [ ] Schema validation: ✅ PASSED
- [ ] Container Security Scan: ✅ PASSED
- [ ] Trivy security scan: ✅ PASSED
- [ ] Lint workflow: ✅ PASSED
- [ ] Benchmark workflow: ✅ PASSED

### Security & Compliance
- [ ] No HIGH or CRITICAL vulnerabilities in Trivy scan
- [ ] No secrets detected in codebase
- [ ] License compliance checks pass
- [ ] Dependency audit passes (cargo audit)
- [ ] SARIF results uploaded to GitHub Security

### Documentation
- [ ] TESTING.md updated with CI troubleshooting (if needed)
- [ ] README badges show passing status
- [ ] Phase plans archived in .phases/ci-fix/
- [ ] No TODOs left in workflow files

## Acceptance Criteria

- [ ] All CI/CD workflows pass successfully on main branch
- [ ] Changes committed with descriptive commit message
- [ ] No regressions in existing functionality
- [ ] CI/CD runs complete in reasonable time:
  - Test workflow: < 8 minutes per PostgreSQL version
  - Property tests: < 5 minutes
  - Load tests: < 6 minutes
  - Security scan: < 12 minutes
  - Lint: < 3 minutes
  - Benchmark: < 10 minutes
- [ ] GitHub Security tab shows no new alerts
- [ ] All phase plans documented and archived

## DO NOT

- Do NOT commit if local verification fails
- Do NOT skip monitoring the CI/CD run after pushing
- Do NOT ignore new security vulnerabilities
- Do NOT merge to main if any CI job fails
- Do NOT delete the phase plans - they're valuable documentation

## Rollback Plan

If CI/CD still fails after these fixes:

```bash
# Revert the commit
git revert HEAD

# Push the revert
git push origin main

# Investigate the specific failure
gh run view <failed-run-id> --log-failed > failure.log

# Review failure.log and update phase plans accordingly

# Re-run phases with updated fixes
```

## Post-Verification Tasks

### Update README Badges (if applicable)

```markdown
<!-- In README.md, ensure CI badges are present -->

[![Tests](https://github.com/fraiseql/jsonb_ivm/actions/workflows/test.yml/badge.svg)](https://github.com/fraiseql/jsonb_ivm/actions/workflows/test.yml)
[![Security](https://github.com/fraiseql/jsonb_ivm/actions/workflows/security.yml/badge.svg)](https://github.com/fraiseql/jsonb_ivm/actions/workflows/security.yml)
[![Lint](https://github.com/fraiseql/jsonb_ivm/actions/workflows/lint.yml/badge.svg)](https://github.com/fraiseql/jsonb_ivm/actions/workflows/lint.yml)
```

### Archive Phase Plans

```bash
# Create archive directory
mkdir -p .phases/completed/2025-12-ci-fix

# Move phase plans
mv .phases/ci-fix/* .phases/completed/2025-12-ci-fix/

# Commit archive
git add .phases/completed/
git commit -m "docs: archive CI fix phase plans"
git push origin main
```

### Update TESTING.md (if needed)

Add a troubleshooting section for CI:

```markdown
## CI/CD Troubleshooting

### Property Tests Failing
- Ensure pgrx is initialized: `cargo pgrx init`
- Check PostgreSQL headers are installed
- Verify cargo-pgrx version matches workflow (0.16.1)

### Load Tests Failing
- Verify PostgreSQL is running: `pg_isready`
- Check connection parameters (PGHOST, PGPORT, PGUSER)
- Ensure trust authentication is configured in CI

### Docker Build Failing
- Verify all required files are COPYed
- Check .dockerignore doesn't exclude necessary files
- Ensure pg_config path is correct
```

## Notes

**Success Metrics:**
- All 12+ CI jobs passing (7 PostgreSQL versions + property/load/security/lint/benchmark)
- Zero HIGH or CRITICAL security vulnerabilities
- Fast feedback loop (most jobs < 10 minutes)
- Clear error messages if something fails

**Known Limitations:**
- Property tests with 10000 iterations take ~2-3 minutes (acceptable)
- Load tests require PostgreSQL service (can't run in pure Rust tests)
- Docker build is slower due to multi-stage build (but produces smaller image)
- Trivy scan may flag dependencies (not our code)

**Future Improvements:**
- Consider caching Docker layers for faster builds
- Add code coverage reporting to GitHub
- Set up dependabot for automatic dependency updates
- Add performance regression detection in benchmarks

**Maintenance:**
- Review and update workflow dependencies quarterly
- Keep cargo-pgrx version in sync across all jobs
- Update PostgreSQL versions when new releases are available
- Monitor security advisories for Rust dependencies
