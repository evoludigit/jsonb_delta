# Contributing to jsonb_delta

Thank you for your interest in contributing to jsonb_delta! This document provides guidelines for contributing to the project.

---

## Table of Contents

- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Code Style](#code-style)
- [Testing](#testing)
- [Pull Request Process](#pull-request-process)
- [Commit Message Guidelines](#commit-message-guidelines)
- [Reporting Bugs](#reporting-bugs)
- Suggesting Enhancements

---

## Getting Started

jsonb_delta is a PostgreSQL extension written in Rust using the [pgrx](https://github.com/pgcentralfoundation/pgrx) framework. Before contributing, familiarize yourself with:

- **Rust**: Basic Rust programming knowledge
- **PostgreSQL**: Understanding of PostgreSQL datatypes (especially JSONB)
- **pgrx**: How to build PostgreSQL extensions with Rust

---

## Development Setup

### Prerequisites

- **Rust**: 1.70+ (stable toolchain)

  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

- **PostgreSQL**: 13-17 (development headers required)

  ```bash
  # Debian/Ubuntu
  sudo apt-get install postgresql-server-dev-17 build-essential libclang-dev

  # Arch Linux
  sudo pacman -S postgresql-libs base-devel clang
  ```

- **cargo-pgrx**: 0.12.8

  ```bash
  cargo install --locked cargo-pgrx
  cargo pgrx init  # One-time setup
  ```

### Clone and Build

```bash
git clone https://github.com/fraiseql/jsonb_delta.git
cd jsonb_delta

# Build in debug mode
cargo pgrx run pg17

# Build in release mode
cargo build --release

# Install to local PostgreSQL
cargo pgrx install --release
```

### Running Tests

```bash
# Run Rust unit tests
cargo test

# Run pgrx integration tests
cargo pgrx test --release

# Run SQL regression tests
psql -d postgres -f test/sql/01_merge_shallow.sql
psql -d postgres -f test/sql/02_array_update_where.sql
# ... etc

# Run benchmarks
psql -d postgres -f test/benchmark_pg_tview_helpers.sql
```

---

## Code Style

### Rust Code

We use standard Rust formatting tools:

```bash
# Format code (REQUIRED before PR)
cargo fmt

# Lint code (REQUIRED before PR)
cargo clippy -- -D warnings

# Check for common issues
cargo audit
```

**Style Guidelines:**

1. **Follow Rust conventions**: Use `snake_case` for functions/variables, `PascalCase` for types
2. **Document public functions**: Add rustdoc comments (`///`) for all exported functions
3. **Handle errors gracefully**: Return `Result` types where appropriate
4. **Keep functions focused**: Each function should do one thing well
5. **Avoid unsafe**: Only use `unsafe` when absolutely necessary and document why

**Example:**

```rust
/// Updates a single element in a JSONB array by matching a key-value predicate.
///
/// # Arguments
/// * `target` - JSONB document containing the array
/// * `array_path` - Path to the array within the document
/// * `match_key` - Key to match on
/// * `match_value` - Value to match
/// * `updates` - JSONB object to merge into the matched element
///
/// # Returns
/// Updated JSONB document with the matched element merged
///
/// # Example
/// ```sql
/// SELECT jsonb_array_update_where(
///     '{"posts": [{"id": 1, "title": "Old"}]}'::jsonb,
///     'posts',
///     'id',
///     '1'::jsonb,
///     '{"title": "New"}'::jsonb
/// );
/// -- Result: {"posts": [{"id": 1, "title": "New"}]}
/// ```
#[pg_extern(immutable, parallel_safe, strict)]
fn jsonb_array_update_where(
    target: JsonB,
    array_path: &str,
    match_key: &str,
    match_value: JsonB,
    updates: JsonB,
) -> JsonB {
    // Implementation...
}
```

### SQL Code

1. **Lowercase keywords**: `SELECT`, `UPDATE`, `WHERE` → `select`, `update`, `where`
2. **Indent 4 spaces**: Use consistent indentation
3. **One statement per line**: For readability
4. **Add comments**: Explain complex queries

---

## Testing

### Test Requirements

**All contributions MUST include tests.**

1. **Unit tests** (Rust): Test individual functions
2. **Integration tests** (SQL): Test real-world scenarios
3. **Benchmark tests** (SQL): Validate performance claims

### Adding New Tests

**For new functions:**

1. Add Rust unit tests in `src/lib.rs`:

   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn test_my_new_function() {
           // Test logic here
       }
   }
   ```

2. Add SQL integration tests in `test/sql/`:

   ```bash
   # Create new test file
   touch test/sql/07_my_feature.sql
   ```

3. Add benchmark in `test/benchmark_my_feature.sql` if performance-critical

### Test Coverage

We aim for:
- **90%+ code coverage** for core functions
- **100% coverage** for edge cases (NULL, empty arrays, type mismatches)
- **Performance validation** for all optimization claims

---

## Pull Request Process

### Before Submitting

1. **Run all checks:**

   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   cargo test
   cargo pgrx test --release
   ```

2. **Update documentation:**
   - Add/update function docs in readme.md
   - Update changelog.md
   - Add examples to integration guide if applicable

3. **Verify performance claims:**
   - Run benchmarks before and after changes
   - Document performance impact in PR description

### PR Guidelines

1. **One feature per PR**: Keep PRs focused and reviewable
2. **Write clear titles**: E.g., "feat: add jsonb_array_delete_where function"
3. **Describe changes**: Explain what, why, and how
4. **Include test results**: Paste benchmark output or test results
5. **Link issues**: Reference any related issues (#123)

### PR Template

```markdown
## Description
Brief description of changes

## Motivation
Why is this change needed?

## Changes
- Added feature X
- Fixed bug Y
- Updated docs for Z

## Testing
- [ ] Unit tests added/updated
- [ ] Integration tests added/updated
- [ ] Benchmarks run (paste results below)
- [ ] All tests passing

## Performance Impact
Before: X ops/sec
After: Y ops/sec
Improvement: Z%

## Checklist
- [ ] Code formatted (`cargo fmt`)
- [ ] Linting passed (`cargo clippy`)
- [ ] Tests passing (`cargo test`)
- [ ] Documentation updated
- [ ] changelog.md updated
```

---

## Commit Message Guidelines

We follow [Conventional Commits](https://www.conventionalcommits.org/):

### Format

```text
<type>(<scope>): <description>

[optional body]

[optional footer]
```

### Types

- `feat`: New feature
- `fix`: Bug fix
- `perf`: Performance improvement
- `refactor`: Code refactoring (no functional change)
- `test`: Adding/updating tests
- `docs`: Documentation changes
- `chore`: Maintenance tasks
- `ci`: CI/CD changes

### Examples

```bash
# Adding a new function
feat(api): add jsonb_array_delete_where function

Implements surgical deletion of array elements by key-value match.
Performance: 5× faster than re-aggregation for 100-element arrays.

Closes #42

# Fixing a bug
fix(array_update): handle NULL match_value correctly

Previously returned error, now returns original JSONB unchanged.

# Performance improvement
perf(array_update): add loop unrolling for large arrays

8-way loop unrolling improves performance by 3× for arrays > 32 elements.

# Documentation
docs(readme): add NULL handling section to API reference
```

---

## Reporting Bugs

### Before Reporting

1. **Search existing issues**: Check if already reported
2. **Test with latest version**: Update to latest release
3. **Isolate the problem**: Create minimal reproduction case

### Bug Report Template

```markdown
Describe the bug
Clear description of what's wrong

To Reproduce
    -- Minimal SQL example that reproduces the issue
    SELECT jsonb_array_update_where(...);

Expected behavior
What you expected to happen

Actual behavior
What actually happened

Environment
- jsonb_delta version: v0.3.0
- PostgreSQL version: 17.2
- OS: Ubuntu 24.04
- Rust version: 1.83.0

Additional context
Any other relevant information
```

---

## Suggesting Enhancements

### Enhancement Template

```markdown
Feature Description
Clear description of the proposed feature

Motivation
Why is this feature needed? What problem does it solve?

Proposed API
    -- Example SQL syntax
    SELECT new_function(...);

Use Case
Real-world scenario where this would be useful

Performance Considerations
Expected performance characteristics

Alternatives Considered
What other approaches did you consider?
```

---

## Development Workflow

### 1. Create Feature Branch

```bash
git checkout -b feat/my-feature
```

### 2. Make Changes

```bash
# Edit code
vim src/lib.rs

# Test locally
cargo test
cargo pgrx test
```

### 3. Commit Changes

```bash
git add .
git commit -m "feat(api): add my_feature function"
```

### 4. Push and Create PR

```bash
git push origin feat/my-feature
# Open PR on GitHub
```

### 5. Address Review Comments

```bash
# Make changes
git add .
git commit -m "fix: address review comments"
git push
```

---

## Code Review Checklist

Reviewers will check for:

- [ ] Code follows Rust conventions
- [ ] All tests pass
- [ ] Performance claims validated
- [ ] Documentation updated
- [ ] Commit messages follow guidelines
- [ ] No compiler warnings
- [ ] No clippy warnings
- [ ] Code is well-documented
- [ ] Edge cases handled (NULL, empty, etc.)
- [ ] Security considerations addressed
- [ ] changelog.md updated

---

## Performance Benchmarking

### Running Benchmarks

```bash
# Create test database
createdb jsonb_delta_bench

# Install extension
psql -d jsonb_delta_bench -c "CREATE EXTENSION jsonb_delta;"

# Run benchmarks
psql -d jsonb_delta_bench -f test/benchmark_pg_tview_helpers.sql > results.txt

# Parse results
grep "Execution time" results.txt
```

### Benchmark Requirements

For performance-critical changes:

1. **Baseline comparison**: Test before and after
2. **Multiple runs**: Average of 5+ runs
3. **Different data sizes**: Small (10), medium (100), large (1000+) arrays
4. **Document results**: Include in PR description

---

## Release Process

For maintainers only

1. Update version in `Cargo.toml`
2. Update version in `jsonb_delta.control`
3. Generate SQL files: `cargo pgrx schema`
4. Update changelog.md
5. Create git tag: `git tag -a v0.X.0 -m "Release v0.X.0"`
6. Push tag: `git push origin v0.X.0`
7. Create GitHub release with changelog

---

## Getting Help

- **Documentation**: See readme.md and docs/
- **Issues**: [GitHub Issues](https://github.com/fraiseql/jsonb_delta/issues)
- **Discussions**: [GitHub Discussions](https://github.com/fraiseql/jsonb_delta/discussions)

---

## License

By contributing, you agree that your contributions will be licensed under the PostgreSQL License.

---

## Code of Conduct

### Our Standards

- Be respectful and inclusive
- Welcome newcomers
- Focus on constructive feedback
- Assume good intentions

### Unacceptable Behavior

- Harassment or discrimination
- Trolling or inflammatory comments
- Personal attacks
- Publishing private information

Violations will result in temporary or permanent bans.

---

Thank you for contributing to jsonb_delta! 🚀
