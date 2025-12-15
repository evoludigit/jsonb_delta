# Security Documentation

This directory contains all security-related documentation for jsonb_delta.

## Quick Links

- **[Vulnerability Disclosure](../../SECURITY.md)** - How to report security issues
- **[Threat Model](threat-model.md)** - STRIDE analysis of attack vectors
- **[SBOM Process](sbom.md)** - Software Bill of Materials generation

## Security Posture

jsonb_delta achieves **A+ security grade** through:

- ✅ **Memory Safety**: Rust's compile-time guarantees + MIRI testing
- ✅ **Fuzzing**: Continuous fuzzing with cargo-fuzz + OSS-Fuzz ready
- ✅ **Supply Chain**: SBOM + SLSA Level 3 provenance + Cosign signing
- ✅ **Dependency Scanning**: cargo audit + Dependabot + Trivy
- ✅ **Secrets Detection**: TruffleHog on every PR
- ✅ **Memory Sanitizers**: Address, leak, and thread sanitizers in CI
- ✅ **Container Security**: Trivy scanning of Docker images
- ✅ **Bug Bounty**: Informal program ($50-$500 rewards)

## For Security Researchers

- **Bug Bounty**: See [SECURITY.md](../../SECURITY.md#bug-bounty-program-informal)
- **Scope**: Memory safety, DoS, SQL injection, privilege escalation
- **Rewards**: $50-$500 + Hall of Fame + swag

## For Users

### Best Practices

1. **Use least privilege**:

   ```sql
   GRANT EXECUTE ON FUNCTION jsonb_merge_shallow TO app_user;
   REVOKE ALL ON TABLE source_table FROM app_user;
   ```

2. **Set resource limits**:

   ```sql
   ALTER ROLE app_user SET statement_timeout = '5s';
   ALTER ROLE app_user SET work_mem = '64MB';
   ```

3. **Enable audit logging** (for sensitive data):

   ```sql
   CREATE TABLE audit_log (
       ts TIMESTAMPTZ DEFAULT NOW(),
       operation TEXT,
       old_value JSONB,
       new_value JSONB
   );
   ```

4. **Keep dependencies updated**:

   ```bash
   cargo update
   cargo audit
   cargo pgrx install
   ```

## For Developers

### Security Testing

```bash
# Memory safety (MIRI)
cargo +nightly miri test --lib

# Fuzzing (local)
cargo +nightly fuzz run fuzz_merge_shallow

# Security audit
cargo audit

# Container scanning
docker build -t jsonb_delta:test .
trivy image jsonb_delta:test
```

### CI/CD Security Checks

All security checks run automatically:
- **Every PR**: MIRI, sanitizers, secrets scan, dependency audit
- **Weekly**: Full security scan, container scan, fuzzing
- **On Release**: SBOM generation, SLSA provenance, signing

See: [.github/workflows/security-compliance.yml](../../.github/workflows/security-compliance.yml)

## Compliance

jsonb_delta complies with:

- 🇺🇸 **US Executive Order 14028** (SBOM requirement)
- 🇪🇺 **EU NIS2 Directive** & **Cyber Resilience Act**
- 💳 **PCI-DSS 4.0** Requirement 6.3.2 (component inventory)
- 🌐 **ISO 27001:2022** Control 5.21 (supply chain security)
- **NIST SP 800-161** (cyber supply chain risk management)

## Contact

- **Security issues**: [GitHub Security Advisories](https://github.com/evoludigit/jsonb_delta/security/advisories/new)
- **Questions**: <security@evolution-digitale.fr>
- **Discussions**: [GitHub Discussions](https://github.com/evoludigit/jsonb_delta/discussions)
