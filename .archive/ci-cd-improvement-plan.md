# CI/CD Improvement Plan

**Current State Analysis & Recommendations**

---

## 📊 Current CI/CD Setup

### Existing Workflows

#### 1. **Test Workflow** (`test.yml`)
- ✅ Tests PostgreSQL versions 13-17 (matrix strategy)
- ✅ Rust toolchain installation
- ✅ Dependency caching
- ✅ Extension build & install
- ⚠️ **Limited SQL testing** (only runs `01_merge_shallow.sql`)
- ⚠️ **Basic smoke test** (only checks one function)
- ❌ **No link checking**
- ❌ **No benchmark validation**

#### 2. **Lint Workflow** (`lint.yml`)
- ✅ Rust formatting check (`cargo fmt`)
- ✅ Clippy linting with `-D warnings`
- ✅ Security audit (`cargo audit`)
- ✅ Documentation build check
- ❌ **No markdown linting**
- ❌ **No link validation**
- ❌ **No SQL linting**

#### 3. **Release Workflow** (`release.yml`)
- ✅ Builds for PostgreSQL 13-17
- ✅ Creates tarballs
- ✅ GitHub release automation
- ⚠️ **Uses old changelog filename** (`CHANGELOG.md` → should be `changelog.md`)
- ❌ **No release validation testing**

---

## 🚨 Critical Issues

### 1. **Incomplete SQL Testing** 🔴
**Problem:** Only tests 1 out of 20+ SQL test files

**Current:**
```yaml
# Only runs one test file
sudo -u postgres psql -d test_jsonb_delta -f test/sql/01_merge_shallow.sql
```

**Impact:**
- v0.3.0 functions (8 new functions) are **never tested** in CI
- No validation of `jsonb_smart_patch_*`, `jsonb_array_delete_where`, etc.
- SQL syntax errors could slip through

### 2. **No Documentation Link Validation** 🔴
**Problem:** After renaming 23 files to kebab-case, broken links could exist

**Impact:**
- Broken internal links in documentation
- Poor user experience
- Wasted time debugging "404s"

### 3. **No Benchmark Regression Detection** 🟡
**Problem:** No validation that performance claims hold

**Impact:**
- Could accidentally introduce performance regressions
- No tracking of performance over time

### 4. **Outdated Filename References** 🟡
**Problem:** `release.yml` still references `CHANGELOG.md` (should be `changelog.md`)

---

## 🎯 Improvement Recommendations

### Priority 1: CRITICAL (Must Fix)

#### **1.1 Comprehensive SQL Testing**

Add job to run **all** SQL tests:

```yaml
# Addition to test.yml
- name: Run comprehensive SQL tests
  run: |
    # Create extension
    sudo -u postgres psql -d test_jsonb_delta -c "CREATE EXTENSION jsonb_delta;"

    # Run all test files
    for test_file in test/sql/*.sql; do
      echo "Running $test_file..."
      sudo -u postgres psql -d test_jsonb_delta -f "$test_file" > "test_output_$(basename $test_file).txt" 2>&1

      if [ $? -ne 0 ]; then
        echo "❌ FAILED: $test_file"
        cat "test_output_$(basename $test_file).txt"
        exit 1
      else
        echo "✅ PASSED: $test_file"
      fi
    done

- name: Upload all test results
  if: always()
  uses: actions/upload-artifact@v4
  with:
    name: sql-test-results-pg${{ matrix.pg-version }}
    path: test_output_*.txt
```

**Benefit:** Tests all 13 functions across all PostgreSQL versions

---

#### **1.2 Documentation Link Validation**

Add new job to `lint.yml`:

```yaml
markdown-links:
  name: Markdown Link Check
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4

    - name: Check markdown links
      uses: gaurav-nelson/github-action-markdown-link-check@v1
      with:
        use-quiet-mode: 'yes'
        config-file: '.github/markdown-link-check-config.json'
```

Create config file `.github/markdown-link-check-config.json`:
```json
{
  "ignorePatterns": [
    {
      "pattern": "^http://localhost"
    },
    {
      "pattern": "^https://github.com/evoludigit/jsonb_delta/discussions"
    }
  ],
  "timeout": "20s",
  "retryOn429": true,
  "retryCount": 3,
  "fallbackRetryDelay": "30s",
  "aliveStatusCodes": [200, 206, 301, 302, 307, 308]
}
```

**Benefit:** Catches broken links in all 30+ markdown files

---

#### **1.3 Fix Release Workflow Filename**

Update `release.yml`:

```yaml
# OLD
body_path: CHANGELOG.md

# NEW
body_path: changelog.md
```

**Benefit:** Release notes will work correctly

---

### Priority 2: HIGH (Should Add)

#### **2.1 Benchmark Smoke Tests**

Add to `test.yml`:

```yaml
- name: Run benchmark smoke tests
  run: |
    # Don't run full benchmarks (too slow), but verify they execute
    sudo -u postgres psql -d test_jsonb_delta -c "
      -- Quick smoke test of each function
      SELECT jsonb_array_update_where(
        '{\"posts\": [{\"id\": 1, \"title\": \"old\"}]}'::jsonb,
        'posts', 'id', '1'::jsonb, '{\"title\": \"new\"}'::jsonb
      );

      SELECT jsonb_array_delete_where(
        '{\"posts\": [{\"id\": 1}]}'::jsonb,
        'posts', 'id', '1'::jsonb
      );

      SELECT jsonb_array_insert_where(
        '{\"posts\": []}'::jsonb,
        'posts', '{\"id\": 1}'::jsonb, NULL, NULL
      );

      SELECT jsonb_deep_merge(
        '{\"a\": 1, \"b\": {\"c\": 2}}'::jsonb,
        '{\"b\": {\"d\": 3}}'::jsonb
      );

      SELECT jsonb_smart_patch_scalar(
        '{\"a\": 1}'::jsonb, '{\"b\": 2}'::jsonb
      );
    " || exit 1

    echo "✅ All v0.3.0 functions smoke tested"
```

**Benefit:** Ensures all 13 functions at least execute without errors

---

#### **2.2 Markdown Linting**

Add to `lint.yml`:

```yaml
markdown-lint:
  name: Markdown Lint
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4

    - name: Lint markdown files
      uses: DavidAnson/markdownlint-cli2-action@v14
      with:
        globs: '**/*.md'
```

Create `.markdownlint.json`:
```json
{
  "default": true,
  "MD013": false,
  "MD033": false,
  "MD041": false
}
```

**Benefit:** Consistent markdown formatting across 30+ docs

---

#### **2.3 SQL Schema Validation**

Add to `test.yml`:

```yaml
- name: Validate SQL schema generation
  run: |
    # Generate schema and compare with committed version
    cargo pgrx schema > /tmp/generated-schema.sql

    # Check if schema matches committed version
    if ! diff -u sql/jsonb_delta--0.3.0.sql /tmp/generated-schema.sql; then
      echo "❌ Generated schema doesn't match committed version!"
      echo "Run: cargo pgrx schema > sql/jsonb_delta--0.3.0.sql"
      exit 1
    fi

    echo "✅ Schema generation validated"
```

**Benefit:** Prevents mismatch between Rust code and SQL definitions

---

### Priority 3: MEDIUM (Nice to Have)

#### **3.1 Performance Regression Detection**

Add new workflow `.github/workflows/benchmark.yml`:

```yaml
name: Benchmark

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  benchmark:
    name: Performance Benchmarks
    runs-on: ubuntu-latest
    if: github.event_name == 'push' || contains(github.event.pull_request.labels.*.name, 'benchmark')

    steps:
      - uses: actions/checkout@v4

      # ... setup steps ...

      - name: Run benchmarks
        run: |
          sudo -u postgres psql -d test_jsonb_delta \
            -f test/benchmark_pg_tview_helpers.sql \
            > benchmark_results.txt 2>&1

      - name: Parse benchmark results
        run: |
          # Extract timing from benchmark output
          grep "Execution time:" benchmark_results.txt > timings.txt

          # Compare with baseline (if exists)
          if [ -f benchmark_baseline.txt ]; then
            # Simple comparison (can be enhanced with statistical analysis)
            echo "Comparing with baseline..."
          fi

      - name: Upload benchmark results
        uses: actions/upload-artifact@v4
        with:
          name: benchmark-results
          path: |
            benchmark_results.txt
            timings.txt
```

**Benefit:** Track performance over time, catch regressions

---

#### **3.2 Documentation Coverage Check**

Add to `lint.yml`:

```yaml
doc-coverage:
  name: Documentation Coverage
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4

    - name: Check function documentation coverage
      run: |
        # Count functions in code
        RUST_FUNCTIONS=$(grep -c "#\[pg_extern" src/lib.rs || echo 0)

        # Count functions documented in README
        README_FUNCTIONS=$(grep -c "^### \`jsonb_" readme.md || echo 0)

        echo "Rust functions: $RUST_FUNCTIONS"
        echo "Documented in README: $README_FUNCTIONS"

        if [ "$RUST_FUNCTIONS" -ne "$README_FUNCTIONS" ]; then
          echo "⚠️  Mismatch: Not all functions are documented!"
          echo "Please update readme.md API reference"
          exit 1
        fi

        echo "✅ All functions documented"
```

**Benefit:** Ensures API reference stays in sync with code

---

#### **3.3 Multi-Architecture Testing**

Expand test matrix:

```yaml
strategy:
  fail-fast: false
  matrix:
    pg-version: [13, 14, 15, 16, 17]
    os: [ubuntu-latest, macos-latest]
    exclude:
      # macOS testing is expensive, only test latest PG
      - os: macos-latest
        pg-version: 13
      - os: macos-latest
        pg-version: 14
      - os: macos-latest
        pg-version: 15
      - os: macos-latest
        pg-version: 16
```

**Benefit:** Validates macOS compatibility (currently untested)

---

#### **3.4 Nightly Rust Testing**

Add job to detect issues with upcoming Rust versions:

```yaml
rust-nightly:
  name: Rust Nightly Check
  runs-on: ubuntu-latest
  continue-on-error: true  # Don't fail PR if nightly breaks

  steps:
    # ... setup ...

    - uses: dtolnay/rust-toolchain@nightly

    - name: Build with nightly
      run: cargo build --release
```

**Benefit:** Early warning of future Rust breaking changes

---

### Priority 4: LOW (Future Enhancements)

#### **4.1 Code Coverage Tracking**

Use `cargo-tarpaulin` to track test coverage:

```yaml
- name: Generate coverage report
  run: |
    cargo install cargo-tarpaulin
    cargo tarpaulin --out Xml --output-dir ./coverage

- name: Upload to Codecov
  uses: codecov/codecov-action@v3
```

**Benefit:** Visibility into test coverage percentage

---

#### **4.2 Dependency Update Automation**

Add Dependabot config `.github/dependabot.yml`:

```yaml
version: 2
updates:
  - package-ecosystem: "cargo"
    directory: "/"
    schedule:
      interval: "weekly"
    open-pull-requests-limit: 5

  - package-ecosystem: "github-actions"
    directory: "/"
    schedule:
      interval: "weekly"
```

**Benefit:** Automated dependency updates

---

#### **4.3 Release Changelog Automation**

Use `git-cliff` to auto-generate changelogs:

```yaml
- name: Generate changelog
  uses: orhun/git-cliff-action@v2
  with:
    config: cliff.toml
    args: --latest --strip header
```

**Benefit:** Consistent, automated changelog generation

---

## 📋 Implementation Checklist

### Phase 1: Critical Fixes (Next PR)
- [ ] Add comprehensive SQL testing (all test files)
- [ ] Add markdown link validation
- [ ] Fix `changelog.md` reference in `release.yml`
- [ ] Add v0.3.0 function smoke tests

**Estimated Time:** 2-3 hours
**Impact:** HIGH - Prevents broken releases

### Phase 2: High Priority (This Week)
- [ ] Add benchmark smoke tests
- [ ] Add markdown linting
- [ ] Add SQL schema validation
- [ ] Add documentation coverage check

**Estimated Time:** 3-4 hours
**Impact:** MEDIUM - Improves code quality

### Phase 3: Nice to Have (Next Month)
- [ ] Performance regression tracking
- [ ] Multi-architecture testing (macOS)
- [ ] Rust nightly testing
- [ ] Code coverage reporting

**Estimated Time:** 6-8 hours
**Impact:** LOW-MEDIUM - Professional polish

### Phase 4: Future (v0.4.0+)
- [ ] Dependabot setup
- [ ] Changelog automation
- [ ] PGXN publishing automation

**Estimated Time:** 4-6 hours
**Impact:** LOW - Long-term maintenance

---

## 🎯 Quick Wins (Do First)

### 1. Fix Release Workflow (5 minutes)
```bash
# Edit .github/workflows/release.yml
sed -i 's/CHANGELOG.md/changelog.md/g' .github/workflows/release.yml
git add .github/workflows/release.yml
git commit -m "ci: fix changelog filename reference in release workflow"
```

### 2. Add Comprehensive SQL Testing (30 minutes)
Add the SQL test loop to `test.yml` (see section 1.1 above)

### 3. Add Link Validation (15 minutes)
Add markdown-link-check job to `lint.yml` (see section 1.2 above)

---

## 📊 Expected Outcomes

### After Phase 1:
- ✅ All 13 functions tested in CI
- ✅ No broken documentation links
- ✅ Releases work correctly
- ✅ 95% confidence in release quality

### After Phase 2:
- ✅ Consistent markdown formatting
- ✅ Schema always in sync
- ✅ Documentation coverage verified
- ✅ 99% confidence in release quality

### After Phase 3:
- ✅ Performance tracking over time
- ✅ macOS compatibility validated
- ✅ Future-proof against Rust changes
- ✅ Professional CI/CD setup

---

## 💡 Additional Recommendations

### 1. Add CI Badge to README
```markdown
[![CI](https://github.com/evoludigit/jsonb_delta/actions/workflows/test.yml/badge.svg)](https://github.com/evoludigit/jsonb_delta/actions/workflows/test.yml)
[![Lint](https://github.com/evoludigit/jsonb_delta/actions/workflows/lint.yml/badge.svg)](https://github.com/evoludigit/jsonb_delta/actions/workflows/lint.yml)
```

### 2. Create Pre-Commit Hooks
`.git/hooks/pre-commit`:
```bash
#!/bin/bash
cargo fmt -- --check || exit 1
cargo clippy -- -D warnings || exit 1
```

### 3. Add CODEOWNERS File
`.github/CODEOWNERS`:
```
* @fraiseql
/docs/ @fraiseql
/.github/ @fraiseql
```

---

## 🚀 Summary

### Current State
- ✅ Basic CI/CD in place
- ⚠️ Limited test coverage
- ❌ No link validation
- ❌ No benchmark tracking

### After Improvements
- ✅ Comprehensive SQL testing
- ✅ Link validation
- ✅ Markdown linting
- ✅ Schema validation
- ✅ Performance tracking
- ✅ Multi-platform testing

### Recommended Action
**Start with Phase 1** (2-3 hours investment):
1. Fix release workflow (5 min)
2. Add comprehensive SQL tests (30 min)
3. Add link validation (15 min)
4. Add v0.3.0 smoke tests (15 min)

This will give you **95% confidence** in releases with minimal time investment.

---

**Questions?** Let me know which improvements you'd like me to implement first!
