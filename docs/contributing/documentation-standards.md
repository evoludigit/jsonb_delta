# Documentation Standards

This document outlines the standards and conventions for documenting the jsonb_delta PostgreSQL extension.

## API Documentation in README.md

### Function Documentation Format

All public functions must be documented in the README.md API Reference section with the following format:

```markdown
### `function_name(parameters)` [⭐ NEW]

Brief description of what the function does.

**Parameters:**
- `param1` (type) - Description of parameter
- `param2` (type) - Description of parameter

**Returns:** Description of return value

**Performance:** Performance characteristics if applicable

**Example:**
```sql
SELECT function_name(...);
-- Result: expected output
```

### Heading Level Requirements

**CRITICAL**: All function documentation MUST use exactly 3 hashes (`###`) for the heading level.

```markdown
### `jsonb_function_name(parameters)`  ✅ CORRECT
#### `jsonb_function_name(parameters)` ❌ WRONG - Will not be counted by CI
```

**Why level 3?**
- Matches the CI check pattern: `^### \`jsonb_`
- Consistent with semantic document structure
- Proper hierarchy: `##` for sections, `###` for functions

### CI Enforcement

The CI system automatically validates documentation coverage:

```bash
# Counts functions in Rust code
RUST_FUNCTIONS=$(grep -c "#\[pg_extern" src/lib.rs)

# Counts documented functions in README
README_FUNCTIONS=$(grep -c "^### \`jsonb_" README.md)

# Must be equal or CI fails
[ "$RUST_FUNCTIONS" -eq "$README_FUNCTIONS" ]
```

**Failure behavior:**
- If functions are added without documentation: ❌ CI fails
- If documentation is added without functions: ❌ CI fails
- If heading levels are wrong: ❌ CI fails (functions not counted)

### Adding New Functions

When adding a new function:

1. **Implement the function** in `src/lib.rs` with `#[pg_extern]`
2. **Add comprehensive documentation** to README.md API Reference
3. **Use level 3 heading**: `###`function_name(parameters)``
4. **Include all required sections**: Parameters, Returns, Example
5. **Test locally**:

   ```bash
   RUST_FUNCTIONS=$(grep -c "#\[pg_extern" src/lib.rs)
   README_FUNCTIONS=$(grep -c "^### \`jsonb_" README.md)
   echo "Code: $RUST_FUNCTIONS, Docs: $README_FUNCTIONS"
   ```

### Function Organization

Functions are organized by version in the README:

```markdown
## 📖 API Reference

### v0.3.0 Functions
### `jsonb_smart_patch_*` functions...

---

### v0.1.0 & v0.2.0 Functions
### `jsonb_array_update_where*` functions...
### `jsonb_merge_*` functions...
```

**Version ordering**: Newest versions first (v0.3.0, then v0.2.0/v0.1.0)

## Code Documentation (Rust)

### Rustdoc Comments

All public functions must have comprehensive rustdoc comments:

```rust
/// Brief description of the function.
///
/// # Arguments
/// * `target` - Description of the target parameter
/// * `source` - Description of the source parameter
///
/// # Returns
/// Description of the return value
///
/// # Example
/// ```sql
/// SELECT function_name(...);
/// -- Result: expected output
/// ```
#[pg_extern(immutable, parallel_safe, strict)]
pub fn function_name(target: JsonB, source: JsonB) -> JsonB {
    // implementation
}
```

### Documentation Requirements

- **Brief description**: One sentence summary
- **Parameters**: Document all parameters with types and descriptions
- **Return value**: Describe what is returned
- **Examples**: Include SQL examples showing usage
- **Performance**: Note performance characteristics for optimization functions

## Changelog Documentation

### Format

All changes must be documented in `changelog.md`:

```markdown
## [v0.3.0] - 2024-12-XX

### Added
- `jsonb_smart_patch_scalar()` - Intelligent shallow merge for top-level updates
- `jsonb_array_delete_where()` - Surgical array element deletion

### Performance
- 3-5× faster array operations with SIMD optimizations
- Improved cascade throughput: 167 → 357 ops/sec (+114%)

### Fixed
- NULL handling in array update functions
```

### Categories

- **Added**: New features and functions
- **Changed**: Breaking changes or API modifications
- **Fixed**: Bug fixes
- **Performance**: Performance improvements
- **Deprecated**: Features marked for removal
- **Removed**: Removed features

## Testing Documentation

### Test File Organization

Tests are organized in `test/` directory:

```text
test/
├── sql/           # SQL integration tests
│   ├── 01_merge_shallow.sql
│   ├── 02_array_update_where.sql
│   └── ...
├── fixtures/      # Test data setup
├── expected/      # Expected test outputs
└── benchmark_*.sql # Performance benchmarks
```

### Test Documentation Requirements

Each test file must include:
- **Purpose comment**: What the test validates
- **Setup**: Required test data
- **Assertions**: Expected vs actual results
- **Edge cases**: NULL values, empty arrays, type mismatches

## Troubleshooting Documentation

### Error Handling Documentation

Document common error scenarios in `docs/troubleshooting.md`:

- **NULL parameter handling**
- **Missing paths/keys**
- **Type mismatches**
- **Performance issues**

### Best Practices

Include usage best practices:
- When to use each function
- Performance optimization tips
- Common pitfalls to avoid

## Markdown Standards

### Linting Rules

The project enforces markdown standards via CI:

- **MD031**: Blanks around fenced code blocks
- **MD032**: Blanks around lists
- **MD034**: No bare URLs (use `<url>` or `[text](url)`)
- **MD036**: No emphasis used as headings

### Common Violations

```markdown
❌ Wrong: Bare URL
See https://github.com/fraiseql/jsonb_delta for details

✅ Correct: Angle brackets
See <https://github.com/fraiseql/jsonb_delta> for details

✅ Correct: Link text
See [the repository](https://github.com/fraiseql/jsonb_delta) for details
```

```markdown
❌ Wrong: Emphasis as heading
**Installation Steps**

✅ Correct: Proper heading
### Installation Steps
```

## Documentation Maintenance

### Regular Reviews

- **Monthly**: Review documentation completeness
- **Per release**: Update changelog and version numbers
- **Per PR**: Ensure documentation is updated for code changes

### Validation Commands

```bash
# Check documentation coverage
RUST_FUNCTIONS=$(grep -c "#\[pg_extern" src/lib.rs)
README_FUNCTIONS=$(grep -c "^### \`jsonb_" README.md)
echo "Coverage: $README_FUNCTIONS/$RUST_FUNCTIONS"

# Validate markdown formatting
npm install -g markdownlint-cli2
markdownlint-cli2 "**/*.md"

# Check for broken links
npm install -g markdown-link-check
find docs -name "*.md" -exec markdown-link-check {} \;
```

## Contributing to Documentation

### Documentation PRs

Documentation-only PRs are welcome:

- Fix typos and grammar
- Improve examples and explanations
- Add missing function documentation
- Update outdated information

### Documentation Standards for Contributors

When contributing:

1. **Follow existing patterns**: Match style of existing documentation
2. **Test examples**: Ensure SQL examples work
3. **Update all relevant files**: README, changelog, troubleshooting
4. **Validate locally**: Run the CI checks before pushing

---

**Last Updated**: 2024-12-XX
**Maintained by**: Contributors</content>
<parameter name="filePath">docs/contributing/documentation-standards.md
