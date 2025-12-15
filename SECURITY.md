# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1   | :x:                |

**Note**: jsonb_delta is currently in **initial release** (v0.1.x). While we maintain backward compatibility and security patches for supported versions, the project is not yet recommended for production use in security-critical environments.

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue in jsonb_delta, please follow these steps:

### 1. Do NOT Open a Public Issue

Please do not report security vulnerabilities through public GitHub issues.

### 2. Report via GitHub Security Advisories (Recommended)

**[Create a Security Advisory](https://github.com/evoludigit/jsonb_delta/security/advisories/new)**

If you prefer email, send details to: **<security@evolution-digitale.fr>**

### 3. What to Include

Please include:
- Description of the vulnerability
- PostgreSQL version(s) affected
- Steps to reproduce the issue
- SQL queries or Rust code demonstrating the vulnerability
- Potential impact (data corruption, DoS, privilege escalation, etc.)
- Suggested fix (if any)

### 4. Response Timeline

- **Initial Response**: Within 48 hours
- **Vulnerability Assessment**: Within 7 days
- **Fix Timeline**:
  - **CRITICAL**: 7 days
  - **HIGH**: 14 days
  - **MEDIUM**: 30 days
  - **LOW**: 90 days

## Security Features

jsonb_delta is designed with security in mind:

### 1. Memory Safety

Built in Rust with strict safety guarantees:

```rust
// All JSONB manipulation is memory-safe
// No buffer overflows, no use-after-free
// Type-safe PostgreSQL bindings via pgrx
```

**Benefits**:
- No C-style memory vulnerabilities
- No buffer overflows
- No null pointer dereferences
- Compile-time safety checks

### 2. SQL Injection Prevention

All functions use PostgreSQL's type system:

```sql
-- Safe - JSONB type enforcement
SELECT jsonb_merge_shallow(document, '{"key": "value"}'::jsonb);

-- Safe - parameterized path handling
SELECT jsonb_merge_at_path(document, 'users[0]', '{"name": "updated"}'::jsonb);
```

**No string concatenation** - all operations use pgrx's type-safe bindings.

### 3. Input Validation

All functions validate inputs before processing:

```sql
-- Type validation: expects JSONB object
SELECT jsonb_merge_shallow('{"a": 1}'::jsonb, '[]'::jsonb);
-- ERROR: Expected object, found: array

-- Path validation: checks path exists
SELECT jsonb_merge_at_path('{"a": 1}'::jsonb, 'nonexistent', '{"b": 2}'::jsonb);
-- ERROR: Path 'nonexistent' does not exist in document

-- NULL handling: explicit error messages
SELECT jsonb_merge_shallow(NULL, '{"a": 1}'::jsonb);
-- ERROR: Document cannot be NULL
```

### 4. DoS Protection

Protected against resource exhaustion:

- **Bounded complexity**: No recursive algorithms without depth limits
- **Efficient algorithms**: O(n) time complexity for most operations
- **SIMD optimization**: Fast processing reduces CPU time
- **No unbounded memory allocation**: All allocations are proportional to input size

### 5. Minimal Attack Surface

Small dependency footprint:

```toml
[dependencies]
pgrx = "0.16.1"        # PostgreSQL bindings
serde = "1.0"          # JSON serialization
serde_json = "1.0"     # JSON value type
```

**Only 3 runtime dependencies** - reduces supply chain attack surface.

## Security Best Practices

### 1. PostgreSQL Permissions

Use least-privilege principle:

```sql
-- Read-only access to views
GRANT SELECT ON users_view TO app_user;

-- Execute-only access to JSONB functions
GRANT EXECUTE ON FUNCTION jsonb_merge_shallow(jsonb, jsonb) TO app_user;
REVOKE ALL ON TABLE source_table FROM app_user;
```

### 2. Input Sanitization

Validate user inputs before passing to jsonb_delta:

```sql
-- Bad: Unchecked user input
SELECT jsonb_merge_shallow(document, user_input::jsonb);

-- Good: Validated input
SELECT jsonb_merge_shallow(
    document,
    CASE
        WHEN jsonb_typeof(user_input::jsonb) = 'object'
        THEN user_input::jsonb
        ELSE '{}'::jsonb
    END
);
```

### 3. Audit Logging

Log sensitive JSONB updates:

```sql
-- Create audit log
CREATE TABLE jsonb_audit_log (
    id SERIAL PRIMARY KEY,
    table_name TEXT,
    operation TEXT,
    old_value JSONB,
    new_value JSONB,
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    updated_by TEXT DEFAULT CURRENT_USER
);

-- Audit trigger
CREATE OR REPLACE FUNCTION audit_jsonb_update()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO jsonb_audit_log (table_name, operation, old_value, new_value)
    VALUES (TG_TABLE_NAME, TG_OP, OLD.data, NEW.data);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
```

### 4. Keep Dependencies Updated

Monitor security advisories:

```bash
# Check for Rust dependency vulnerabilities
cargo audit

# Update dependencies
cargo update
cargo pgrx install
```

### 5. Secure PostgreSQL Configuration

Harden PostgreSQL for production:

```conf
# postgresql.conf
ssl = on
ssl_ciphers = 'HIGH:MEDIUM:+3DES:!aNULL'
password_encryption = scram-sha-256

# Restrict connections
listen_addresses = 'localhost'  # Or specific IPs
```

### 6. Monitor Query Performance

Prevent DoS via expensive queries:

```sql
-- Set statement timeout
SET statement_timeout = '5s';

-- Monitor slow queries
SELECT * FROM pg_stat_statements
WHERE query LIKE '%jsonb_%'
ORDER BY mean_exec_time DESC;
```

## Known Vulnerabilities

### Active Monitoring

We actively monitor for vulnerabilities in:
- Rust toolchain (rustc, cargo)
- pgrx framework
- serde/serde_json
- PostgreSQL itself

Check [Security Advisories](https://github.com/evoludigit/jsonb_delta/security/advisories) for disclosed vulnerabilities.

### CVE Database

No CVEs have been reported for jsonb_delta as of 2025-12-13.

### Container Security Posture (2025-12-13)

**Trivy Security Scan Results**: ✅ **PASS** (0 CRITICAL/HIGH vulnerabilities)

**Vulnerabilities Addressed**:
- **12 total vulnerabilities** identified in base Docker image
- **11 suppressed** as false positives (not applicable to PostgreSQL extension use case)
- **1 addressed** via package updates (libxml2 security patches)

**Suppressed Vulnerabilities** (False Positives):
- SQLite integer overflow (not used by extension)
- zlib MiniZip overflow (not used by extension)
- OpenLDAP null pointer dereference (no LDAP service)
- Go stdlib issues (HTTP/tar/crypto not used)
- linux-pam directory traversal (PAM not configured)
- libxslt heap use-after-free (no XSLT processing)

**Security Controls**:
- ✅ **Automated scanning** in CI/CD pipeline
- ✅ **Suppression documentation** in `.trivyignore` and `docs/SECURITY-SUPPRESSIONS.md`
- ✅ **Regular reviews** (quarterly for false positives, monthly for accepted risks)
- ✅ **Compliance alignment** (NIST, FedRAMP, NIS2, GDPR, ISO 27001, SOC 2)

See `docs/SECURITY-SUPPRESSIONS.md` for detailed justifications.

## Disclosure Policy

When we receive a security report:

1. **Acknowledge** (within 48 hours)
2. **Investigate** and confirm the issue (within 7 days)
3. **Develop a fix** (timeline depends on severity)
4. **Prepare security advisory** (CVE request if applicable)
5. **Release patched version**
6. **Publicly disclose** (coordinated with reporter)

We aim to coordinate disclosure with the reporter and follow responsible disclosure practices.

## Security Updates

Stay informed about security updates:

- **Watch the GitHub repository** for security advisories
- **Subscribe to release notifications** (GitHub releases)
- **Check CHANGELOG.md** for security-related fixes
- **Follow [@fraiseql](https://github.com/evoludigit)** for announcements

## Third-Party Dependencies

jsonb_delta monitors security advisories for:

- **pgrx** (PostgreSQL bindings framework)
- **serde/serde_json** (JSON serialization)
- **Rust toolchain** (rustc, cargo, LLVM)

We use:
- **Dependabot** for automated dependency updates
- **cargo audit** in CI/CD for vulnerability scanning
- **Locked dependencies** (Cargo.lock) for reproducible builds

## Compliance

jsonb_delta supports compliance requirements:

### GDPR

- **Right to be forgotten**: Use `jsonb_merge_shallow` to redact fields
- **Data minimization**: Update only necessary fields
- **Audit trails**: Track all JSONB updates

### SOC 2

- **Access controls**: PostgreSQL GRANT/REVOKE
- **Audit logging**: Trigger-based audit logs
- **Integrity**: Cryptographic checksums for JSONB data

### HIPAA

- **Access controls**: Row-level security + JSONB field filtering
- **Audit logging**: Complete change history
- **Encryption**: PostgreSQL SSL/TLS + disk encryption

## Threat Model

### In Scope

- Memory safety violations (buffer overflows, use-after-free)
- SQL injection via JSONB manipulation
- Denial of service (resource exhaustion, infinite loops)
- Data corruption (invalid JSONB, race conditions)
- Authentication/authorization bypass (PostgreSQL permissions)

### Out of Scope

- PostgreSQL vulnerabilities (report to PostgreSQL Security Team)
- Host OS vulnerabilities (report to OS vendor)
- Network-level attacks (firewall, TLS configuration)
- Physical access attacks

## Bug Bounty

We do not currently have a formal bug bounty program. However, we deeply appreciate security research and will:

- Publicly acknowledge your contribution (if desired)
- Credit you in the security advisory
- Provide a reference letter for your security research portfolio

## Security Hardening

### Recommended Production Configuration

```sql
-- Create dedicated schema
CREATE SCHEMA jsonb_delta;

-- Install extension in dedicated schema
CREATE EXTENSION jsonb_delta SCHEMA jsonb_delta;

-- Grant minimal permissions
GRANT USAGE ON SCHEMA jsonb_delta TO app_user;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA jsonb_delta TO app_user;

-- Revoke public access
REVOKE ALL ON SCHEMA jsonb_delta FROM PUBLIC;

-- Enable row-level security
ALTER TABLE your_table ENABLE ROW LEVEL SECURITY;

-- Set resource limits
ALTER ROLE app_user SET statement_timeout = '5s';
ALTER ROLE app_user SET idle_in_transaction_session_timeout = '10s';
```

## Bug Bounty Program (Informal)

We appreciate security research! While we don't have a formal bug bounty platform yet, we offer:

### Rewards

- **Hall of Fame**: Public recognition in this file and release notes
- **Swag**: jsonb_delta t-shirt and stickers (for significant findings)
- **Reference Letter**: Professional reference for your security research portfolio
- **Negotiable Bounty**: $50-$500 for severe vulnerabilities (budget permitting)

### Scope

**In Scope** ✅:
- Memory safety issues (buffer overflow, use-after-free, undefined behavior)
- SQL injection via JSONB manipulation
- Denial of service (infinite loops, memory exhaustion, algorithmic complexity)
- Privilege escalation
- Data corruption vulnerabilities
- Supply chain attacks (dependency vulnerabilities)

**Out of Scope** ❌:
- PostgreSQL core vulnerabilities (report to PostgreSQL Security)
- Social engineering attacks
- Physical attacks
- Issues in example/test code (non-production)
- Vulnerabilities requiring physical access to the server

### Severity Guidelines

| Severity | Impact | Example | Bounty |
|----------|--------|---------|--------|
| **CRITICAL** | Remote code execution, data corruption | Memory safety issue allowing arbitrary SQL | $300-$500 |
| **HIGH** | DoS, information disclosure | Crash with malicious JSONB input | $100-$300 |
| **MEDIUM** | Limited DoS, minor info leak | Performance degradation with specific input | $50-$100 |
| **LOW** | Informational, best practice | Improvement suggestion | Recognition |

### Rules

1. **Report privately** via [GitHub Security Advisories](https://github.com/evoludigit/jsonb_delta/security/advisories/new)
2. **Allow time to fix** (90 days coordinated disclosure)
3. **No testing on production systems** (use your own PostgreSQL instance)
4. **One issue per report** (separate reports for separate vulnerabilities)
5. **Provide proof-of-concept** (SQL queries, reproduction steps)

### Hall of Fame

Currently no vulnerabilities reported. Be the first!

---

## Questions?

For security questions that aren't vulnerabilities:

- **GitHub Discussions**: [jsonb_delta Discussions](https://github.com/evoludigit/jsonb_delta/discussions)
- **Email**: <security@evolution-digitale.fr>
- **Documentation**: See `docs/troubleshooting.md`

Thank you for helping keep jsonb_delta secure!

---

**Last Updated**: 2025-12-13
**Version**: 1.2
**Maintainer**: FraiseQL Security Team
