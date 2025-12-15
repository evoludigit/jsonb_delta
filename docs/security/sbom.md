# SBOM Generation Process

**Document Version:** 1.0
**Last Updated:** 2025-12-12
**Classification:** Public
**Applicable Standards:** CycloneDX, SLSA, US EO 14028, EU NIS2/CRA, PCI-DSS 4.0

## Executive Summary

jsonb_delta implements automated Software Bill of Materials (SBOM) generation to comply with global supply chain security standards. SBOMs are generated in CycloneDX format and cryptographically signed for integrity verification.

## What is an SBOM?

A Software Bill of Materials (SBOM) is a formal, machine-readable inventory of software components and dependencies. It serves as a "nutrition label" for software, enabling:

- **Transparency**: Know exactly what's in your software
- **Vulnerability Management**: Quickly identify affected systems when CVEs are disclosed
- **License Compliance**: Ensure all dependencies meet legal requirements
- **Supply Chain Security**: Verify component integrity and provenance

## Regulatory Compliance

### Global Supply Chain Security Standards

jsonb_delta's SBOM generation complies with:

1. 🇺🇸 **US Executive Order 14028** (May 2021) - Software supply chain security for federal procurement
2. 🇪🇺 **EU NIS2 Directive** (2022/2555) - Supply chain security requirements
3. 🇪🇺 **EU Cyber Resilience Act (CRA)** - Explicit SBOM requirement for software products (2025-2027)
4. 💳 **PCI-DSS 4.0** Requirement 6.3.2 - Software component inventory (effective March 31, 2025)
5. 🌐 **ISO 27001:2022** Control 5.21 - ICT supply chain security management
6. **NIST SP 800-161** - Cyber Supply Chain Risk Management

## SBOM Format

jsonb_delta generates SBOMs in **CycloneDX** format (JSON).

### Why CycloneDX?

- ✅ OWASP standard designed for security use cases
- ✅ Comprehensive metadata (licenses, hashes, vulnerabilities)
- ✅ Wide tool support (vulnerability scanners, compliance tools)
- ✅ Rust ecosystem support via `cargo-sbom`

## Automated Generation

### GitHub Actions Workflow

SBOM generation is fully automated via `.github/workflows/sbom-generation.yml`:

**Trigger Events:**
- On every release (tagged version)
- Manual workflow dispatch

**Workflow Steps:**
1. Check out repository code
2. Install Rust toolchain and cargo-sbom
3. Generate CycloneDX JSON SBOM
4. Add project metadata (version, licenses, authors)
5. Sign SBOM with Cosign (keyless Sigstore)
6. Generate SHA256 checksums
7. Attach SBOM artifacts to GitHub release

### Manual Generation

For local development or testing:

```bash
# Install cargo-sbom
cargo install cargo-sbom

# Generate SBOM
cargo sbom --output-format cyclonedx_json > jsonb_delta-sbom.json

# Validate JSON
jq empty jsonb_delta-sbom.json
```

## SBOM Contents

### Component Inventory

The SBOM includes:

- **Direct dependencies** (from Cargo.toml)
- **Transitive dependencies** (full dependency tree)
- **License information** for each component
- **Package URLs (PURL)** for vulnerability tracking
- **Version information**
- **Component hashes** (for integrity verification)

### Metadata

Each SBOM includes:
- **Timestamp** of generation
- **Tool information** (cargo-sbom version)
- **Project metadata** (name, version, description, license)
- **Authors and suppliers**
- **Serial number** (unique SBOM identifier)

## Cryptographic Signing

### Cosign (Sigstore)

All SBOMs are signed using **Cosign** with **keyless signing**:

```bash
# Signing (automated in CI)
cosign sign-blob --yes \
  jsonb_delta-0.3.1-sbom.json \
  --output-signature jsonb_delta-0.3.1-sbom.json.sig \
  --output-certificate jsonb_delta-0.3.1-sbom.json.pem
```

**Benefits:**
- No key management required (uses OIDC tokens)
- Tamper-evident (any modification breaks signature)
- Transparency log (Rekor) for auditability
- GitHub Actions identity verification

### Verification

Users can verify SBOM authenticity:

```bash
# Verify signature
cosign verify-blob \
  --signature jsonb_delta-0.3.1-sbom.json.sig \
  --certificate jsonb_delta-0.3.1-sbom.json.pem \
  --certificate-identity-regexp "https://github.com/evoludigit/jsonb_delta" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  jsonb_delta-0.3.1-sbom.json

# Verify checksum
sha256sum -c jsonb_delta-0.3.1-sbom.json.sha256
```

## Distribution

### GitHub Releases

SBOMs are attached to every GitHub release:

1. Navigate to [Releases](https://github.com/evoludigit/jsonb_delta/releases)
2. Download SBOM artifacts:
   - `jsonb_delta-x.y.z-sbom.json` (SBOM)
   - `jsonb_delta-x.y.z-sbom.json.sig` (Signature)
   - `jsonb_delta-x.y.z-sbom.json.pem` (Certificate)
   - `jsonb_delta-x.y.z-sbom.json.sha256` (Checksum)

### Retention

- **GitHub Actions artifacts**: 90 days
- **GitHub Release assets**: Permanent

## Vulnerability Tracking

### Package URLs (PURL)

Each component in the SBOM includes a Package URL for cross-referencing with vulnerability databases:

```json
{
  "purl": "pkg:cargo/serde@1.0.210",
  "name": "serde",
  "version": "1.0.210"
}
```

### Integration with Security Tools

The SBOM can be imported into:

- **GitHub Advanced Security** (automated)
- **Dependabot** (vulnerability alerts)
- **Grype / Syft** (vulnerability scanning)
- **OWASP Dependency-Track** (SBOM management platform)
- **JFrog Xray** (commercial scanning)

## License Compliance

### License Detection

All dependencies are scanned for license information:

```bash
# Generate license report
cargo license --json > licenses.json
cargo license --tsv > licenses.tsv
```

### License Policy

jsonb_delta uses the **PostgreSQL License** (permissive).

**Dependency License Policy:**
- ✅ Permissive licenses allowed (MIT, Apache-2.0, BSD)
- ✅ LGPL allowed (PostgreSQL ecosystem compatibility)
- ⚠️ GPL licenses reviewed case-by-case
- ❌ Proprietary licenses not allowed

## Compliance Checklist

For procurement and security teams:

- [x] **SBOM Generated**: Automated on every release
- [x] **Format**: CycloneDX 1.5 (OWASP standard)
- [x] **Signed**: Cosign keyless (Sigstore)
- [x] **Checksums**: SHA256 for integrity
- [x] **Components**: Complete dependency tree
- [x] **Licenses**: All dependencies documented
- [x] **Vulnerability Tracking**: Package URLs included
- [x] **Public Availability**: GitHub releases
- [x] **Machine-Readable**: JSON format

## For Procurement Teams

### Request an SBOM

1. **Latest release**: Download from [GitHub Releases](https://github.com/evoludigit/jsonb_delta/releases/latest)
2. **Specific version**: Navigate to the version tag
3. **Custom request**: Email <security@evolution-digitale.fr>

### Verify SBOM Authenticity

1. Download all 4 files (.json, .sig, .pem, .sha256)
2. Verify checksum: `sha256sum -c jsonb_delta-x.y.z-sbom.json.sha256`
3. Verify signature: `cosign verify-blob ...` (see Verification section)

### Questions?

- **Email**: <security@evolution-digitale.fr>
- **GitHub Discussions**: [jsonb_delta Discussions](https://github.com/evoludigit/jsonb_delta/discussions)
- **Documentation**: See `docs/` directory

---

**Last Updated**: 2025-12-12
**Version**: 1.0
**Maintainer**: FraiseQL Security Team
