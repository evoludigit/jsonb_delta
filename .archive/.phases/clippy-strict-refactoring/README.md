# Clippy Strict Refactoring

This directory contains the phase plan for fixing 85 clippy strict warnings in the jsonb_ivm codebase.

## Quick Start for Agent

If you're an agent tasked with fixing clippy strict warnings:

1. **Check you're on the right branch**:
```bash
git branch
# Should show: feat/clippy-strict-refactoring
```

2. **Read the phase plan**:
```bash
cat .phases/clippy-strict-refactoring/phase-plan.md
```

3. **Start with Phase 1** (Setup and Analysis):
   - Run strict clippy to capture warnings
   - Categorize by risk level
   - Plan your approach

4. **Work incrementally**:
   - Fix low-risk warnings first
   - Test after each category
   - Only proceed to higher-risk changes if tests pass

5. **Safety first**:
   - If something breaks, revert immediately
   - Document any issues in `lessons-learned.md`
   - Don't skip PostgreSQL integration tests

## Branch Information

- **Branch**: `feat/clippy-strict-refactoring`
- **Base**: `main`
- **Purpose**: Fix 85 clippy strict warnings without breaking functionality
- **Risk Level**: Medium (refactoring working FFI code)
- **Status**: Ready for work

## Important Notes

⚠️ **This is PostgreSQL FFI code** - function signature changes could break the extension

✅ **Tests are critical** - Must pass both Rust tests and PostgreSQL integration tests

📝 **Document decisions** - Especially for high-risk changes that are allowed instead of fixed

## Files to Work On

Primary file: `src/lib.rs` (contains all 13 functions with warnings)

## Expected Outcome

- 85 clippy strict warnings resolved (either fixed or explicitly allowed with `#[allow]`)
- 100% backward compatible
- All tests passing
- Ready to merge into main via PR

## Getting Help

If you get stuck or unsure about FFI behavior:
- Check pgrx documentation: https://docs.rs/pgrx/latest/pgrx/
- Review existing function patterns in the codebase
- Ask for clarification before making high-risk changes

## Phase Plan

See `phase-plan.md` for the complete step-by-step guide.
