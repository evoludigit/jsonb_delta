# Phase 3: `jsonb_smart_patch_array` Compatibility

Resolves: #12 (cross-ref fraiseql/pg_tviews#50)

## Objective

Decide and implement how jsonb_delta responds to pg_tviews calling a
`jsonb_smart_patch_array` signature this extension does not export.

## Confirmed facts

Exported (`sql/jsonb_delta--0.1.0.sql:179-188`, `src/merge.rs:304`):

```sql
jsonb_smart_patch_array(target jsonb, source jsonb, array_path TEXT,
                        match_key TEXT, match_value jsonb)
```

Called by pg_tviews (`src/refresh/main.rs:470-515`):

```sql
jsonb_smart_patch_array(<expr>, $1::jsonb, ARRAY['path'], 'match_key')
```

Two differences, not one: **arity** (5 vs 4) and **path type** (`TEXT` vs `TEXT[]`).
The reporter is correct that this fails with `function … does not exist`.

## OPEN QUESTION — resolve before writing code

The 4-arg form supplies no `match_value`. Its only coherent semantics is
*"patch the element whose `match_key` equals `source->match_key`"* — i.e. the
incoming row identifies itself. That is a reasonable and arguably nicer API, but
it is an **inference**, not something the issue states.

**Do not implement until confirmed with the pg_tviews maintainers on #50.** If
pg_tviews instead intends something else (e.g. match on a value carried elsewhere
in the payload), an overload built on the wrong guess is worse than no overload —
it would silently patch the wrong element instead of erroring loudly.

Also note `TEXT[]` implies **nested path** support. The current implementation is
explicitly single-level (`src/merge.rs:318`: "Navigate to array location (single
level for now)"). Supporting `ARRAY['a','b','items']` is real work, not an alias.

## Decision: who fixes it?

| Option | Pros | Cons |
|---|---|---|
| A. pg_tviews fixes its emitter | Correct layering — caller adapts to the published contract. No new permanent API surface here. | jsonb_delta users on older pg_tviews stay broken; we don't control their release. |
| B. jsonb_delta adds a 4-arg `TEXT[]` overload | Fixes both directions; downstream unblocked without coordinated release | Permanent API surface added to satisfy one consumer; two ways to do one thing; requires nested-path work |
| C. Both | Fastest unblock, defence in depth | Most work; overload may become vestigial |

**Recommendation: A, with B only if pg_tviews cannot land the fix promptly.**
The published SQL is the contract; pg_tviews' own test stub defined a
nonexistent signature, which is the actual root cause of it going unnoticed. The
durable fix is that pg_tviews tests against the real extension. Adding an
overload here rewards testing against a stub.

**Regardless of A/B, do this now:** a contract test that pins all 15 exported
signatures against `pg_proc`, so a signature drift can never again be discovered
by a downstream consumer. degustation already froze this inventory in
`c4_tviews_jsonb_delta.toml` — mirror it in-repo.

## TDD Cycles

### Cycle 1: Signature contract test (do this unconditionally)
- **RED**: `tests/exported_signatures.rs` asserts the exact 15 name+argtype
  signatures against `pg_proc` after `CREATE EXTENSION`. Should pass today;
  write it so it fails if any signature is altered.
- **GREEN**: Pin the current inventory.
- **REFACTOR**: Generate the expected list from the `.sql` script rather than
  hardcoding, so it tracks the shipped contract.
- **CLEANUP**: Add to `just ci`.

### Cycle 2: Overload (ONLY if option B is chosen after #50 confirms semantics)
- **RED**: Test calling `jsonb_smart_patch_array(target, source, ARRAY['posts'], 'id')`
  patches the element whose `id` equals `source->'id'`. Fails — no such function.
- **GREEN**: Add `#[pg_extern]` 4-arg variant delegating to the 5-arg impl with
  `match_value = source[match_key]`, erroring clearly if `source` lacks `match_key`.
- **REFACTOR**: Extract shared patch logic so both arities share one code path.
- **CLEANUP**: Document both arities; note the nested-path limitation explicitly
  if `TEXT[]` remains single-level (error, don't silently ignore, on len > 1).

### Cycle 3: Nested path support (only if Cycle 2 lands and depth > 1 is required)
- **RED**: Test patching `ARRAY['company','departments','staff']`.
- **GREEN**: Recursive navigation.
- **REFACTOR**: Reuse the existing path-walk helper from `jsonb_merge_at_path`.
- **CLEANUP**: Bound recursion depth — SECURITY.md documents depth limits;
  respect them here.

## Dependencies

- Requires: confirmation on fraiseql/pg_tviews#50 (blocking for Cycles 2–3)
- Blocks: cutting the 0.2.0 release, if the overload is to ship in it

## Status
[ ] Not Started — Cycle 1 ready; Cycles 2–3 blocked on #50
