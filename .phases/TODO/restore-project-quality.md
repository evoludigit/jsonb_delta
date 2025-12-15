# Restore & Improve Project Quality Standards

## Context

Recent CI/CD fixes lowered quality standards to get tests passing quickly:
1. **Markdown linting**: Disabled 6 rules (MD031, MD032, MD034, MD036, etc.)
2. **Documentation coverage**: Changed from fail → warning (13 functions vs 5 documented)
3. **PostgreSQL 17**: Downgraded benchmark workflow to PG 16

**Goal**: Restore these standards while maintaining CI stability and improving overall quality.

---

## Phase 1: Documentation Coverage (PRIORITY 1)

### Objective
Achieve 100% API documentation coverage for all 13 public functions.

### Current State
- **Code**: 13 functions (marked with `#[pg_extern]`)
- **README.md**: Only 5 documented under "### `jsonb_`" headings
- **CI Check**: Currently just warns, doesn't fail

### Missing Documentation (8 functions)

Need to document in README.md:
1. `jsonb_merge_shallow` ✓ (exists)
2. `jsonb_array_update_where` ✓ (exists)
3. `jsonb_array_update_where_batch` ✓ (exists)
4. `jsonb_array_update_multi_row` ✓ (exists)
5. `jsonb_merge_at_path` ✓ (exists)
6. `jsonb_smart_patch_scalar` ⚠️ (documented but may not match pattern)
7. `jsonb_smart_patch_nested` ⚠️ (documented but may not match pattern)
8. `jsonb_smart_patch_array` ⚠️ (documented but may not match pattern)
9. `jsonb_array_delete_where` ⚠️ (documented but may not match pattern)
10. `jsonb_array_insert_where` ⚠️ (documented but may not match pattern)
11. `jsonb_deep_merge` ⚠️ (documented but may not match pattern)
12. `jsonb_extract_id` ⚠️ (documented but may not match pattern)
13. `jsonb_array_contains_id` ⚠️ (documented but may not match pattern)

### Root Cause Analysis

The CI check searches for:
```bash
grep -c "^### \`jsonb_" README.md
```

This pattern requires:
- Line starts with `###` (3 hashes, NOT 4)
- Space after `###`
- Backtick
- Function name starting with `jsonb_`

**Issue**: Current README uses `####` (4 hashes) for some functions, which don't match the pattern.

### Implementation Plan

#### Step 1: Audit README structure
```bash
# Count all function headings (any level)
grep -E "^#{3,4} \`jsonb_" README.md | wc -l

# Find which heading level is used
grep -E "^### \`jsonb_" README.md    # Level 3 (matches CI)
grep -E "^#### \`jsonb_" README.md   # Level 4 (doesn't match)
```

#### Step 2: Standardize heading levels
**Decision**: Use `###` (level 3) for ALL function documentation

Rationale:
- Matches CI pattern
- Consistent structure
- Proper semantic hierarchy (## for sections, ### for functions)

#### Step 3: Update README.md structure
```markdown
## 📖 API Reference

### v0.3.0 Functions

### `jsonb_smart_patch_scalar(target, source)` ⭐ NEW
[existing content]

### `jsonb_smart_patch_nested(target, source, path)` ⭐ NEW
[existing content]

### `jsonb_smart_patch_array(target, source, array_path, match_key, match_value)` ⭐ NEW
[existing content]

### `jsonb_array_delete_where(target, array_path, match_key, match_value)` ⭐ NEW
[existing content]

### `jsonb_array_insert_where(target, array_path, new_element, sort_key, sort_order)` ⭐ NEW
[existing content]

### `jsonb_deep_merge(target, source)` ⭐ NEW
[existing content]

### `jsonb_extract_id(data, key)` ⭐ NEW
[existing content]

### `jsonb_array_contains_id(data, array_path, id_key, id_value)` ⭐ NEW
[existing content]

---

### v0.1.0 & v0.2.0 Functions

### `jsonb_array_update_where(target, array_path, match_key, match_value, updates)`
[existing content]

### `jsonb_array_update_where_batch(target, array_path, match_key, updates_array)` ⭐ NEW in v0.2.0
[existing content]

### `jsonb_array_update_multi_row(targets, array_path, match_key, match_value, updates)` ⭐ NEW in v0.2.0
[existing content]

### `jsonb_merge_at_path(target, source, path)`
[existing content]

### `jsonb_merge_shallow(target, source)`
[existing content]
```

#### Step 4: Re-enable CI fail
Once README has correct structure:

```yaml
# .github/workflows/lint.yml (line 146-152)
if [ "$RUST_FUNCTIONS" -ne "$README_FUNCTIONS" ]; then
  echo "❌ ERROR: Not all functions are documented!"
  echo "Found $RUST_FUNCTIONS functions in code but only $README_FUNCTIONS documented in README.md"
  echo "Please update README.md API reference"
  exit 1  # ← Re-enable failure
fi
```

#### Step 5: Add enforcement documentation
Create `docs/contributing/documentation-standards.md`:
- Explain heading level requirements
- Provide function documentation template
- Document CI check behavior

### Verification Commands
```bash
# Run locally before committing
RUST_FUNCTIONS=$(grep -c "#\[pg_extern" src/lib.rs)
README_FUNCTIONS=$(grep -c "^### \`jsonb_" README.md)
echo "Code: $RUST_FUNCTIONS, Docs: $README_FUNCTIONS"
[ "$RUST_FUNCTIONS" -eq "$README_FUNCTIONS" ] && echo "✅ PASS" || echo "❌ FAIL"
```

### Acceptance Criteria
- [ ] All 13 functions have `### ` (level 3) headings in README.md
- [ ] CI check passes: `RUST_FUNCTIONS == README_FUNCTIONS`
- [ ] CI fails if new functions added without docs
- [ ] Documentation standards documented in contributing guide

---

## Phase 2: Markdown Linting Standards (PRIORITY 2)

### Objective
Re-enable strict markdown linting while fixing existing violations.

### Current State (Relaxed Rules)
```json
{
  "default": true,
  "MD013": false,  // Line length
  "MD031": false,  // Blanks around fences  ← RELAXED
  "MD032": false,  // Blanks around lists   ← RELAXED
  "MD033": false,  // Inline HTML
  "MD034": false,  // Bare URLs             ← RELAXED
  "MD036": false,  // Emphasis as heading   ← RELAXED
  "MD041": false   // First line heading
}
```

### Rules to Re-enable

#### MD031: Blanks around fenced code blocks
**What it enforces**: Empty line before/after ` ``` ` blocks

**Why it matters**:
- Improves readability
- Consistent rendering across Markdown parsers
- Professional documentation standards

**Example violation**:
```markdown
Some text
```sql
SELECT * FROM table;
```
More text
```

**Fixed**:
```markdown
Some text

```sql
SELECT * FROM table;
```

More text
```

#### MD032: Blanks around lists
**What it enforces**: Empty line before/after list blocks

**Why it matters**:
- Clear visual separation
- Prevents parser confusion
- Better accessibility

**Example violation**:
```markdown
Some text
- Item 1
- Item 2
More text
```

**Fixed**:
```markdown
Some text

- Item 1
- Item 2

More text
```

#### MD034: No bare URLs
**What it enforces**: URLs must be in `<>` or `[text](url)` format

**Why it matters**:
- Consistent link rendering
- Better accessibility (screen readers need link text)
- Professional appearance

**Example violation**:
```markdown
See https://github.com/fraiseql/jsonb_ivm for details
```

**Fixed options**:
```markdown
See <https://github.com/fraiseql/jsonb_ivm> for details
See [the repository](https://github.com/fraiseql/jsonb_ivm) for details
```

#### MD036: No emphasis as heading
**What it enforces**: Don't use `**bold**` as section headers

**Why it matters**:
- Semantic correctness (headings are for structure)
- Better SEO
- Accessibility (screen readers recognize headings)
- Proper document outline

**Example violation**:
```markdown
**Installation Steps**

1. Clone repo
2. Build
```

**Fixed**:
```markdown
### Installation Steps

1. Clone repo
2. Build
```

### Implementation Strategy

#### Step 1: Identify violations (current files)
```bash
# Run markdownlint with verbose output
npx markdownlint-cli2 "**/*.md" --config .markdownlint.json

# Count violations per rule
npx markdownlint-cli2 "**/*.md" 2>&1 | grep -c "MD031"
npx markdownlint-cli2 "**/*.md" 2>&1 | grep -c "MD032"
npx markdownlint-cli2 "**/*.md" 2>&1 | grep -c "MD034"
npx markdownlint-cli2 "**/*.md" 2>&1 | grep -c "MD036"
```

#### Step 2: Re-enable rules ONE AT A TIME

**Rationale**: Incremental fixes prevent overwhelming changes

**Order** (easiest → hardest):
1. **MD031** (blanks around fences) - Usually auto-fixable
2. **MD032** (blanks around lists) - Usually auto-fixable
3. **MD034** (bare URLs) - Requires manual review (link text choice)
4. **MD036** (emphasis as heading) - Requires semantic judgment

#### Step 3: Fix violations for each rule

**Phase 2a: MD031 (Blanks around fences)**
```bash
# Files to check (likely violators)
- README.md
- docs/implementation/*.md
- docs/pg-tview-integration-examples.md
- UPGRADING.md
- development.md
- contributing.md

# Auto-fix where possible
npx markdownlint-cli2-fix "**/*.md" --config .markdownlint-md031-only.json

# Manual review
git diff
```

**Phase 2b: MD032 (Blanks around lists)**
```bash
# Similar process as MD031
npx markdownlint-cli2-fix "**/*.md" --config .markdownlint-md032-only.json
```

**Phase 2c: MD034 (Bare URLs)**
```bash
# Identify all bare URLs
grep -rn "http[s]*://[^ ]*" *.md docs/*.md | grep -v "\[.*\]("

# Manual fixes required - decide link text for each:
# - Use descriptive text: [PostgreSQL documentation](https://www.postgresql.org)
# - Or angle brackets: <https://www.postgresql.org>
```

**Phase 2d: MD036 (Emphasis as heading)**
```bash
# Find **Bold Text** patterns that should be headings
grep -rn "^\*\*[A-Z]" *.md docs/*.md

# Convert to proper headings (### or ####)
# Requires semantic judgment - is it really a heading?
```

#### Step 4: Update .markdownlint.json incrementally

After each phase, update config:

```json
// After Phase 2a
{
  "default": true,
  "MD013": false,
  "MD031": true,  // ✅ RE-ENABLED
  "MD032": false,
  "MD033": false,
  "MD034": false,
  "MD036": false,
  "MD041": false
}
```

### Rules to KEEP DISABLED (Justified)

#### MD013: Line length
**Decision**: Keep disabled

**Justification**:
- Code examples exceed 80 chars
- Long URLs break readability if wrapped
- SQL queries are more readable on one line
- Modern editors have soft wrap

#### MD033: Inline HTML
**Decision**: Keep disabled

**Justification**:
- Badges in README (`<img>` tags)
- Complex tables (HTML `<table>`)
- Collapsible sections (`<details>`)
- Legitimate use cases in documentation

#### MD041: First line must be heading
**Decision**: Keep disabled

**Justification**:
- Some docs have front-matter
- Some docs have explanatory paragraph first
- Not a critical quality issue

### Verification Commands
```bash
# Check all markdown files
npx markdownlint-cli2 "**/*.md" --config .markdownlint.json

# Run in CI (already configured)
npm install -g markdownlint-cli2
markdownlint-cli2 "**/*.md"
```

### Acceptance Criteria
- [ ] MD031 re-enabled: No blanks around fences violations
- [ ] MD032 re-enabled: No blanks around lists violations
- [ ] MD034 re-enabled: All URLs properly formatted
- [ ] MD036 re-enabled: No emphasis used as headings
- [ ] CI passes with strict rules
- [ ] Documentation updated with markdown standards

---

## Phase 3: PostgreSQL 17 Support (PRIORITY 3)

### Objective
Restore PostgreSQL 17 testing in benchmark workflow (currently downgraded to PG 16).

### Current State
```yaml
# .github/workflows/benchmark.yml
- name: Install PostgreSQL 16  # ← DOWNGRADED from 17
  run: |
    sudo apt-get install -y postgresql-16 postgresql-server-dev-16
```

### Problem Analysis

**Why was PG 17 downgraded?**
```
Error: E: Unable to locate package postgresql-17
```

**Root Cause**: GitHub Actions Ubuntu runners use PostgreSQL APT repository which may not have PG 17 packages yet, or package name differs.

### Investigation Required

#### Option 1: Check package availability
```bash
# In GitHub Actions runner (ubuntu-latest = 22.04)
apt-cache search postgresql-17
apt-cache policy postgresql-17

# Check PostgreSQL APT repository
cat /etc/apt/sources.list.d/pgdg.list
```

#### Option 2: Check PostgreSQL release timeline
- **PG 17 Release Date**: September 26, 2024
- **Ubuntu Package Availability**: Usually 1-2 weeks after release
- **GitHub Actions Runner Updates**: Monthly

**Hypothesis**: PG 17 packages may be available now (we're in late 2024/early 2025).

### Implementation Options

#### Option A: Re-test PG 17 availability (RECOMMENDED)

Try restoring PG 17 - it may work now:

```yaml
- name: Install PostgreSQL 17
  run: |
    # Add PostgreSQL APT repository (if not present)
    sudo sh -c 'echo "deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list'
    wget --quiet -O - https://www.postgresql.org/media/keys/ACCC4CF8.asc | sudo apt-key add -

    sudo apt-get update
    sudo apt-get install -y \
      postgresql-17 \
      postgresql-server-dev-17
```

**Test locally with Docker first**:
```bash
# Simulate GitHub Actions environment
docker run -it ubuntu:22.04 bash

# Inside container
apt-get update
apt-get install -y wget lsb-release gnupg
sh -c 'echo "deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list'
wget --quiet -O - https://www.postgresql.org/media/keys/ACCC4CF8.asc | apt-key add -
apt-get update
apt-cache search postgresql-17
```

#### Option B: Matrix approach (test both PG 16 & 17)

If PG 17 availability is uncertain:

```yaml
strategy:
  matrix:
    pg-version: [16, 17]
  fail-fast: false

- name: Install PostgreSQL ${{ matrix.pg-version }}
  run: |
    sudo apt-get update
    sudo apt-get install -y \
      postgresql-${{ matrix.pg-version }} \
      postgresql-server-dev-${{ matrix.pg-version }} || {
        echo "⚠️  PostgreSQL ${{ matrix.pg-version }} not available, skipping"
        exit 0
      }
```

#### Option C: Keep PG 16, document limitation (FALLBACK)

If PG 17 truly unavailable:

```yaml
# Benchmark uses PG 16 (PG 17 not available on GitHub Actions yet)
# Full PG 13-17 matrix testing happens in test.yml
```

**Document in README**:
```markdown
## CI/CD Notes

- **Full Testing**: PostgreSQL 13-17 (test.yml)
- **Benchmarks**: PostgreSQL 16 only (PG 17 packages not available on GitHub Actions runners yet)
```

### Recommended Approach

1. **Try Option A first** - PG 17 may be available now
2. If fails, **implement Option B** - matrix with graceful fallback
3. If still issues, **accept Option C** - document limitation

### Verification Commands
```bash
# Local test (requires Docker)
docker run -it ubuntu:22.04 bash
# [Run installation commands from Option A]

# CI test
git commit -m "test: attempt PostgreSQL 17 restoration in benchmark workflow"
git push
# Watch GitHub Actions workflow
```

### Acceptance Criteria
- [ ] PostgreSQL 17 packages successfully install in benchmark workflow
- [ ] Benchmark tests pass with PG 17
- [ ] OR: Matrix includes PG 17 with graceful fallback
- [ ] OR: Limitation documented if PG 17 unavailable

---

## Phase 4: Enhanced Linting & Quality Checks (FUTURE)

### Objective
Add additional quality checks beyond current standards.

### Proposed Enhancements

#### 1. Rust Code Quality

**Add clippy strict mode**:
```yaml
# .github/workflows/lint.yml
- name: Clippy (strict)
  run: |
    cargo clippy --all-targets --all-features -- \
      -W clippy::all \
      -W clippy::pedantic \
      -W clippy::nursery \
      -D warnings
```

**Rationale**: Catch more potential issues early

#### 2. SQL Schema Validation

**Already implemented** ✅:
```yaml
- name: Validate SQL schema generation
  run: |
    cargo pgrx schema > /tmp/generated-schema.sql
    diff -u sql/jsonb_ivm--0.3.0.sql /tmp/generated-schema.sql
```

#### 3. Benchmark Regression Detection

**Add performance regression checks**:
```yaml
- name: Compare benchmarks
  run: |
    # Store baseline results
    # Compare new results
    # Fail if >10% regression
```

**Challenge**: Need stable benchmark environment (dedicated hardware)

#### 4. Documentation Completeness

**Enhance function documentation checks**:
```bash
# Check for:
# - Parameter descriptions
# - Return value documentation
# - Examples in docs
# - Error behavior documented
```

#### 5. Dependency Auditing

**Add security vulnerability scanning**:
```yaml
- name: Security audit
  run: |
    cargo install cargo-audit
    cargo audit
```

#### 6. Code Coverage Enforcement

**Already started** ✅ (added in recent commits):
```yaml
- name: Generate coverage report
  if: matrix.pg-version == 17 && (github.event_name == 'push' || ...)
  run: |
    cargo install cargo-tarpaulin
    cargo tarpaulin --out Xml --output-dir ./coverage

- name: Upload to Codecov
  uses: codecov/codecov-action@v3
```

**Enhancement**: Add minimum coverage threshold
```yaml
- name: Check coverage threshold
  run: |
    COVERAGE=$(cargo tarpaulin --out Json | jq '.coverage')
    if (( $(echo "$COVERAGE < 80.0" | bc -l) )); then
      echo "❌ Coverage $COVERAGE% below 80% threshold"
      exit 1
    fi
```

### Priority Order

1. **Rust clippy strict mode** (Low effort, high value)
2. **Security audit** (Low effort, critical for prod)
3. **Coverage threshold** (Medium effort, good practice)
4. **Benchmark regression** (High effort, requires infra)

### Acceptance Criteria (Phase 4)
- [ ] Clippy strict mode enabled
- [ ] Security audit in CI
- [ ] Coverage threshold enforced (80%+)
- [ ] Documentation for all quality checks

---

## Summary: Quality Restoration Roadmap

### Immediate (This Week)
1. ✅ **Phase 1**: Fix documentation coverage (13 functions → README)
2. ✅ **Phase 2a-b**: Re-enable MD031, MD032 (auto-fixable)

### Short-term (This Month)
3. ✅ **Phase 2c-d**: Re-enable MD034, MD036 (manual fixes)
4. ✅ **Phase 3**: Restore PostgreSQL 17 support
5. ✅ **Phase 4.1-4.2**: Add clippy strict + security audit

### Long-term (Future)
6. ⏳ **Phase 4.3**: Coverage thresholds
7. ⏳ **Phase 4.4**: Benchmark regression detection

### Quality Metrics (Target)

| Metric | Current | Target | Status |
|--------|---------|--------|--------|
| **Documentation Coverage** | 38% (5/13) | 100% (13/13) | ❌ RED |
| **Markdown Linting** | 58% (7/12 rules) | 83% (10/12 rules) | ⚠️  AMBER |
| **PostgreSQL Testing** | PG 13-16 | PG 13-17 | ⚠️  AMBER |
| **Code Coverage** | Unknown | 80%+ | ⏳ N/A |
| **Security Audit** | None | Passing | ⏳ N/A |

### Success Criteria (Overall)

**Project is "quality restored" when**:
- ✅ All public functions documented in README
- ✅ CI fails on documentation gaps
- ✅ 10/12 markdown rules enforced (4 re-enabled)
- ✅ PostgreSQL 17 supported OR limitation documented
- ✅ Code coverage visible (Codecov integrated)
- ✅ Security audit in CI

**Project is "quality excellent" when** (Phase 4):
- ✅ Clippy strict mode passing
- ✅ 80%+ code coverage enforced
- ✅ No security vulnerabilities
- ✅ Benchmark regression detection

---

## Notes & Decisions

### Why Not Fix Everything At Once?

**Reason**: Incremental quality improvements are more sustainable than "big bang" fixes.

**Benefits**:
1. Each phase is independently testable
2. Easier to identify root causes if CI breaks
3. Can be done alongside feature work
4. Team learns quality standards gradually

### When to Compromise Quality?

**Never compromise on**:
- Security (vulnerabilities, unsafe code)
- Correctness (test failures, data corruption)
- Documentation (public APIs must be documented)

**Acceptable to compromise on** (temporarily):
- Style/formatting (can fix in bulk later)
- Non-critical linting rules (MD013 line length)
- Performance optimizations (premature optimization)

**Current compromises** (technical debt):
- MD034 (bare URLs): Will fix in Phase 2c
- PostgreSQL 17: Will attempt fix in Phase 3
- Coverage thresholds: Will add in Phase 4

### Quality Philosophy

> **"Quality is not an act, it is a habit."** - Aristotle

This project should exemplify PostgreSQL extension best practices:
- Comprehensive documentation (users)
- Strict linting (contributors)
- Full test coverage (maintainers)
- Performance benchmarks (production)

**The goal**: When someone looks at `jsonb_ivm`, they should see a model PostgreSQL extension.

---

## Implementation Plan (Step-by-step)

### Week 1: Documentation & Easy Wins

**Day 1-2**: Phase 1 (Documentation)
- [ ] Audit README.md heading levels
- [ ] Standardize all function headings to `###`
- [ ] Verify CI passes (13 == 13)
- [ ] Re-enable CI failure on mismatch
- [ ] Commit: "docs: restore 100% documentation coverage"

**Day 3-4**: Phase 2a-b (Auto-fixable linting)
- [ ] Create `.markdownlint-md031.json` (MD031 only)
- [ ] Run auto-fix: `markdownlint-cli2-fix`
- [ ] Review changes, commit
- [ ] Update `.markdownlint.json` (MD031: true)
- [ ] Repeat for MD032
- [ ] Commit: "style: restore MD031, MD032 markdown linting"

**Day 5**: Phase 3 (PostgreSQL 17)
- [ ] Test PG 17 availability in Docker
- [ ] Update benchmark.yml
- [ ] Test in CI
- [ ] Commit: "ci: restore PostgreSQL 17 in benchmark workflow"

### Week 2: Manual Linting Fixes

**Day 1-3**: Phase 2c (Bare URLs)
- [ ] Find all bare URLs: `grep -rn "http[s]*://" *.md`
- [ ] For each URL, choose descriptive link text
- [ ] Update markdown files
- [ ] Update `.markdownlint.json` (MD034: true)
- [ ] Commit: "docs: fix bare URLs (MD034)"

**Day 4-5**: Phase 2d (Emphasis as heading)
- [ ] Find all **Bold Text** that should be headings
- [ ] Convert to proper `###` or `####`
- [ ] Verify semantic correctness
- [ ] Update `.markdownlint.json` (MD036: true)
- [ ] Commit: "docs: convert emphasis to headings (MD036)"

### Week 3: Future Enhancements (Phase 4)

**Day 1**: Clippy strict
- [ ] Add clippy strict check to CI
- [ ] Fix any new warnings
- [ ] Commit: "ci: add clippy strict mode"

**Day 2**: Security audit
- [ ] Add `cargo audit` to CI
- [ ] Review and address any findings
- [ ] Commit: "ci: add security vulnerability scanning"

**Day 3-5**: Coverage threshold (optional)
- [ ] Analyze current coverage
- [ ] Add missing tests to reach 80%
- [ ] Add coverage threshold check
- [ ] Commit: "test: enforce 80% coverage threshold"

---

## Rollback Plan

If quality improvements break CI or take too long:

### Quick Rollback
```bash
# Revert to relaxed standards temporarily
git revert HEAD  # Last commit
git push

# Or restore old config
git checkout HEAD~1 -- .markdownlint.json
git checkout HEAD~1 -- .github/workflows/lint.yml
git commit -m "temp: rollback quality changes (debugging)"
```

### Partial Implementation
Can complete phases independently:
- Phase 1 (docs) without Phase 2 (linting)
- Phase 2a (MD031) without Phase 2b (MD032)
- Any phase without Phase 4 (enhancements)

### Quality Gate
Before merging each phase:
- ✅ CI passes
- ✅ No test regressions
- ✅ Documentation updated
- ✅ Team reviewed (if applicable)

---

## Success Metrics

### How We'll Know Quality Is Restored

**Quantitative**:
- Documentation coverage: 13/13 functions (100%)
- Markdown linting: 10/12 rules enabled (83%)
- CI stability: <5% false failure rate
- PostgreSQL versions: 13-17 tested (100%)

**Qualitative**:
- New contributors can understand standards from docs
- CI feedback is helpful, not noisy
- Quality checks catch real issues before merge
- Project documentation is professional and complete

### Monitoring

**Weekly Check**:
```bash
# Run locally
npm install -g markdownlint-cli2
markdownlint-cli2 "**/*.md"

RUST_FUNCTIONS=$(grep -c "#\[pg_extern" src/lib.rs)
README_FUNCTIONS=$(grep -c "^### \`jsonb_" README.md)
echo "Docs coverage: $README_FUNCTIONS/$RUST_FUNCTIONS"

cargo clippy --all-targets -- -W clippy::all
cargo audit
```

**Monthly Review**:
- Review CI failure patterns
- Identify new quality debt
- Update this plan

---

## Related Documentation

- **Implementation Details**: See each phase's commit messages
- **Markdown Standards**: (To be created in `docs/contributing/markdown-style.md`)
- **Documentation Standards**: (To be created in `docs/contributing/api-documentation.md`)
- **CI/CD Architecture**: `ci-cd-improvement-plan.md`

---

**Last Updated**: 2024-12-09
**Status**: Planning
**Owner**: Maintainer
