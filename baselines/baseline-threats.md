# Threat Model

**Version**: 1.0
**Last Updated**: 2025-12-12
**Next Review**: 2026-03-12

## Overview

This document describes the security threat model for jsonb_delta using the **STRIDE** framework (Microsoft threat modeling methodology).

## Attack Surface

### 1. SQL Injection via JSONB Manipulation

**Threat Categories**: Spoofing, Tampering

**Attack Vector**:
- Attacker provides malicious JSONB path strings
- Attacker provides malicious JSON documents

**Mitigations**:
- ✅ Type-safe pgrx bindings (no string concatenation)
- ✅ All parameters use PostgreSQL's type system
- ✅ serde_json handles all JSON parsing safely

**Residual Risk**: **LOW**

**Testing**: SQL injection test suite in `test/sql/`

---

### 2. Denial of Service (DoS)

#### 2a. Complex JSONB Documents

**Threat Category**: Denial of Service

**Attack Vectors**:
- Deeply nested JSON (10,000+ levels)
- Extremely large JSON documents (100MB+)
- Cyclic references (JSON doesn't support, but validate)

**Mitigations**:
- ✅ PostgreSQL memory limits (work_mem, max_stack_depth)
- ✅ O(n) time complexity algorithms
- ✅ No unbounded recursion
- ⚠️ **TODO**: Add explicit depth limit check (max 1000 levels)

**Residual Risk**: **MEDIUM** → Needs depth limit

**Recommendation**: Add depth validation in v0.4.0

---

#### 2b. Algorithmic Complexity Attacks

**Threat Category**: Denial of Service

**Attack Vector**:
- Trigger worst-case O(n²) behavior
- Exploit unoptimized array operations

**Mitigations**:
- ✅ SIMD-optimized array search (8-way unrolling)
- ✅ HashMap for batch operations (O(1) lookups)
- ✅ Performance benchmarks validate efficiency

**Residual Risk**: **LOW**

**Testing**: Benchmark suite in `test/benchmark_*.sql`

---

### 3. Memory Safety Vulnerabilities

**Threat Categories**: Tampering, Elevation of Privilege

**Attack Vectors**:
- Buffer overflow in unsafe code
- Use-after-free in pgrx bindings
- Data races in concurrent operations

**Mitigations**:
- ✅ Rust memory safety (compile-time guarantees)
- ✅ MIRI testing (undefined behavior detection)
- ✅ Address/leak/thread sanitizers in CI
- ✅ Minimal unsafe code (only pgrx requirements)
- ✅ All unsafe blocks documented with safety invariants

**Residual Risk**: **LOW**

**Testing**:
- MIRI runs on every PR
- Memory sanitizers in CI/CD
- Fuzzing for edge cases

---

### 4. Dependency Vulnerabilities

**Threat Category**: Elevation of Privilege

**Attack Vector**:
- Exploit vulnerable serde_json version
- Exploit vulnerable pgrx version
- Supply chain attack (malicious dependency)

**Mitigations**:
- ✅ Minimal dependencies (only 3 runtime crates)
- ✅ Weekly cargo audit in CI
- ✅ Dependabot automated updates
- ✅ Locked dependencies (Cargo.lock committed)
- ✅ SBOM generation for transparency

**Residual Risk**: **LOW**

**Testing**: cargo audit on every push

---

### 5. Privilege Escalation

**Threat Category**: Elevation of Privilege

**Attack Vector**:
- Exploit PostgreSQL permission bypass
- Exploit unsafe code to gain shell access
- SQL injection to access unauthorized data

**Mitigations**:
- ✅ PostgreSQL GRANT/REVOKE enforced
- ✅ Functions run with caller's privileges (SECURITY DEFINER not used)
- ✅ No system calls (no shell access)
- ✅ No file I/O operations

**Residual Risk**: **LOW**

**Best Practice**: Users should apply least-privilege principle:

```sql
GRANT EXECUTE ON FUNCTION jsonb_merge_shallow(jsonb, jsonb) TO app_user;
REVOKE ALL ON TABLE source_table FROM app_user;
```

---

### 6. Data Corruption

**Threat Categories**: Tampering, Repudiation

**Attack Vectors**:
- Race condition in concurrent JSONB updates
- Incorrect merge logic corrupts data
- Type confusion leads to invalid JSONB

**Mitigations**:
- ✅ PostgreSQL MVCC (Multi-Version Concurrency Control)
- ✅ Atomic operations (no partial updates)
- ✅ Input validation (type checking)
- ✅ Comprehensive test suite (654 LOC SQL tests)
- ✅ Fuzzing for edge cases

**Residual Risk**: **LOW**

**Testing**:
- 30+ unit tests
- 6 SQL integration test files
- Fuzzing (fuzz_merge_shallow, fuzz_deep_merge)

---

### 7. Information Disclosure

**Threat Category**: Information Disclosure

**Attack Vector**:
- Error messages leak sensitive data
- Debug logs expose internal state

**Mitigations**:
- ✅ Generic error messages (no data leakage)
- ✅ No debug logging in production
- ✅ PostgreSQL controls query visibility (pg_stat_statements)

**Residual Risk**: **LOW**

---

## DREAD Risk Scoring

| Threat | Damage | Reproducibility | Exploitability | Affected Users | Discoverability | **Total** | Priority |
|--------|--------|-----------------|----------------|----------------|-----------------|-----------|----------|
| SQL Injection | 9 | 1 | 1 | 10 | 1 | **22/50** | LOW |
| DoS (depth) | 5 | 8 | 7 | 8 | 7 | **35/50** | **MEDIUM** |
| DoS (complexity) | 3 | 5 | 4 | 5 | 4 | **21/50** | LOW |
| Memory safety | 10 | 1 | 1 | 10 | 2 | **24/50** | LOW |
| Dependencies | 8 | 3 | 3 | 10 | 4 | **28/50** | LOW |
| Privilege escalation | 10 | 1 | 1 | 5 | 1 | **18/50** | LOW |
| Data corruption | 8 | 2 | 2 | 9 | 3 | **24/50** | LOW |
| Info disclosure | 4 | 3 | 4 | 3 | 5 | **19/50** | LOW |

**Key Finding**: DoS via deeply nested JSON is the highest risk (35/50, MEDIUM priority).

**Recommendation**: Add depth limit validation in next release.

---

## Out of Scope

The following are **NOT** covered by this threat model:

1. **PostgreSQL vulnerabilities** → Report to PostgreSQL Security Team
2. **Operating system vulnerabilities** → Report to OS vendor
3. **Network-level attacks** (TLS, firewall) → Infrastructure responsibility
4. **Physical attacks** → Data center security
5. **Social engineering** → Organizational policy

---

## Security Controls

### Defense in Depth

1. **Language Layer**: Rust memory safety
2. **Application Layer**: Type-safe pgrx bindings
3. **Database Layer**: PostgreSQL permissions, MVCC
4. **Testing Layer**: Fuzzing, MIRI, sanitizers
5. **Supply Chain**: SBOM, dependency scanning, SLSA provenance

---

## Recommendations

### Immediate (v0.3.2)

1. ✅ Add MIRI testing (DONE - in CI)
2. ✅ Add fuzzing (DONE - nightly CI)
3. ✅ Add container scanning (DONE - Trivy)

### Short-term (v0.4.0)

1. ⚠️ **Add depth limit validation** (max 1000 levels)
2. Add complexity limit for large documents
3. External security audit

### Long-term (v1.0.0)

1. Formal verification (Prusti/Kani)
2. SOC 2 certification
3. Bug bounty program (formal)

---

## Incident Response

See: [SECURITY.md](../../SECURITY.md) for vulnerability disclosure process.

---

**Review Schedule**: Quarterly (or after significant changes)
**Next Review**: 2026-03-12
**Owner**: FraiseQL Security Team
