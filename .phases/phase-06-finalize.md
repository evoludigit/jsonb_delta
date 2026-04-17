# Phase 6: Finalize

## Objective

Transform the completed implementation into a clean, production-ready repository.
Remove development artifacts, eliminate duplicated code, verify all quality
gates, and update documentation.

## Success Criteria (all boxes must be ticked before merging)

- [ ] Zero output from `git grep -iE "phase|todo|fixme|hack"` in `src/`
- [ ] `cargo fmt --check` passes (no formatting changes needed)
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes (zero warnings)
- [ ] `cargo pgrx test pg18` passes (all Rust unit + property + pgrx tests)
- [ ] `just test-sql` passes (all SQL integration tests)
- [ ] `cargo pgrx schema pg18` output matches `sql/jsonb_delta--0.1.0.sql`
- [ ] `.phases/` directory removed from the branch
- [ ] `docs/API.md` updated with new validation behaviour
- [ ] `CHANGELOG.md` has a security hardening entry

---

## Steps

### 1. Quality Control Review

Read through all changed files as a senior engineer reviewing a PR. Check:

- [ ] Error messages are consistent in tone and format across all validation
      helpers (`validate_array_index`, `validate_match_key`, key-length check)
- [ ] `MAX_JSONB_ARRAY_SIZE` and `MAX_KEY_LENGTH` are both documented with a
      `///` doc comment explaining *why* the specific value was chosen
- [ ] The `find_insertion_point` doc comment clearly states the sorted-input
      precondition and O(log n) complexity
- [ ] No public API surface was accidentally changed (check `sql/jsonb_delta--0.1.0.sql`)
- [ ] The binary search (`partition_point`) handles elements with no `sort_key`
      the same way the old linear scan did (they should sort consistently)

---

### 2. Security Audit

Review as an attacker would:

- [ ] **Array bounds bypass**: Can the index cap be circumvented via the
      `jsonb_delta_array_update_where_path` code path? Verify the guard is in
      place there too (`src/lib.rs`).
- [ ] **Key length bypass**: Is `MAX_KEY_LENGTH` enforced in bytes or chars?
      Non-ASCII paths with multi-byte characters could produce a key shorter
      than 256 *chars* but longer than 256 *bytes*. Clarify and document.
      (`String::len()` in Rust returns bytes; `String::chars().count()` returns
      chars — decide which is correct and be consistent.)
- [ ] **Empty match_key**: Confirm all 7 affected functions listed in Phase 3
      have the guard. Run `grep -n "validate_match_key\|match_key.is_empty"
      src/array_ops.rs src/merge.rs src/lib.rs` and count call sites.
- [ ] **Depth error message**: Does reporting the actual depth (Phase 5 Cycle 4)
      leak any sensitive internal state? (No — it only reports the integer
      depth, which is harmless.)

---

### 3. Archaeology Removal

Run each command and fix any hits:

```bash
# Phase markers
git grep -in "phase" -- src/ test/sql/

# Development TODOs/FIXMEs
git grep -in "todo\|fixme\|hack" -- src/ test/sql/

# Leftover debug artifacts
git grep -in "dbg!\|eprintln!\|println!" -- src/

# Module-level "will be moved" comments
git grep -n "will be moved" -- src/

# Duplicate helper functions
# value_type_name appears in lib.rs, merge.rs, array_ops.rs — consolidate
grep -rn "fn value_type_name" src/

# find_element_by_match appears in multiple files — consolidate
grep -rn "fn find_element_by_match" src/
```

#### Deduplication: `value_type_name`

This helper is defined in `src/lib.rs`, `src/merge.rs`, and `src/array_ops.rs`.
Consolidate into one location:

1. Move it to `src/lib.rs` (already there, make it `pub(crate)`)
2. Delete the copies from `src/merge.rs` and `src/array_ops.rs`
3. Add `use crate::value_type_name;` to each module that needs it
4. Run `cargo clippy` — it will catch any missed references

#### Deduplication: `find_element_by_match`

Same process. Find all copies with `grep -rn "fn find_element_by_match" src/`,
pick one canonical location, re-export as `pub(crate)`, delete the rest.

#### Remove `.phases/`

```bash
rm -rf .phases/
git add -A
```

Do this as the very last commit of this phase.

---

### 4. Documentation Polish

#### `docs/API.md`

Add a **Security & Limits** section (or update an existing one) documenting:

```markdown
## Security & Limits

### Array Size Cap
Array indices used in path operations (`jsonb_delta_set_path`,
`jsonb_delta_array_update_where_path`) are capped at **100,000**. Requests
for larger indices return an error: `Array index N exceeds maximum allowed
size 100000`.

### Path Key Length
Individual key segments in dot-notation paths are capped at **256 bytes**.
Longer segments return an error: `Invalid path: key segment exceeds 256 characters`.

### Nesting Depth
JSONB documents passed to all functions are validated against a maximum
nesting depth of **1,000 levels**. Deeper documents return an error:
`JSONB nesting too deep (max 1000, found depth N)`.

### Non-empty `match_key`
All array-matching functions (`jsonb_array_update_where`, `jsonb_array_delete_where`,
etc.) require a non-empty `match_key`. Passing an empty string returns an error:
`match_key must not be empty`.
```

#### `CHANGELOG.md`

Add an entry at the top:

```markdown
## [Unreleased]

### Security
- Added array index cap (`MAX_JSONB_ARRAY_SIZE = 100,000`) to prevent OOM
  attacks via large index padding in `jsonb_delta_set_path` and
  `jsonb_delta_array_update_where_path`.
- Added `match_key` non-empty validation to all 7 array-matching functions.
- Added path key-segment length cap (`MAX_KEY_LENGTH = 256`) in `parse_path()`.

### Performance
- `find_insertion_point()` now uses binary search (`partition_point`) for
  O(log n) complexity, down from O(n).

### Fixed
- Depth validation error now reports the actual depth found, not just `>max`.
```

---

### 5. Final Verification Checklist

Run these in order and fix any failures before proceeding to the next:

```bash
# 1. Formatting
cargo fmt --check

# 2. Lints (zero warnings)
cargo clippy --all-targets --all-features -- -D warnings

# 3. Rust tests
cargo nextest run

# 4. pgrx integration tests
cargo pgrx test pg18

# 5. SQL integration tests
just test-sql

# 6. Schema consistency
cargo pgrx schema pg18 | diff - sql/jsonb_delta--0.1.0.sql
# If the schema changed (new functions added, signatures changed), update the file:
cargo pgrx schema pg18 > sql/jsonb_delta--0.1.0.sql

# 7. No archaeology
git grep -iE "phase|todo|fixme|hack" -- src/ test/sql/
# Expected: no output
```

---

### 6. Final Commits

Use these commit messages (in order):

```
refactor(utils): consolidate value_type_name and find_element_by_match helpers

refactor(docs): update API.md with security limits section

chore(changelog): add security hardening entry

chore: remove .phases/ development directory
```

---

## Dependencies

- Requires: All prior phases complete

## Status
[ ] Not Started
