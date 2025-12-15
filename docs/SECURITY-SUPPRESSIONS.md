# Security Scan Suppressions

This document provides detailed justification for vulnerabilities suppressed in `.trivyignore`.

## Process

1. **All suppressions require**:
   - Clear justification of why vulnerability doesn't apply
   - Risk assessment if it's an accepted risk
   - Expiry date for re-evaluation
   - Link to tracking issue for accepted risks

2. **Review schedule**:
   - False positives: Quarterly (every 3 months)
   - Accepted risks: Monthly for CRITICAL, Quarterly for HIGH
   - After any base image update

3. **Approval required**:
   - CRITICAL suppressions: Security team + Maintainer approval
   - HIGH suppressions: Maintainer approval

## Suppressions

### CVE-2025-7458 - SQLite Integer Overflow

**Package**: libsqlite3-0@3.40.1-2+deb12u2
**Severity**: CRITICAL
**Type**: False Positive

**Vulnerability Summary**:
Integer overflow in sqlite3KeyInfoFromExprList function allows DoS or info disclosure via crafted SELECT with large ORDER BY clause.

**Why It Doesn't Apply**:
1. **No SQLite Usage**: jsonb_delta is a PostgreSQL extension using PostgreSQL's native storage (heap tables, TOAST). We don't execute any SQLite code.

2. **Package Origin**: libsqlite3-0 is a dependency of PostgreSQL server itself (for some metadata operations), not our extension.

3. **No Attack Vector**: Our extension only provides:
   - SQL functions (jsonb_delta_*)
   - JSONB data type handling
   - Trigger-based view maintenance

   None of these code paths invoke SQLite.

4. **Exploitation Requirements**: Requires attacker to:
   - Execute arbitrary SQL (already implies PostgreSQL server compromise)
   - Use SQLite-specific SELECT syntax (not exposed in PostgreSQL)

**Risk Assessment**: None - code path not reachable from extension

**Added**: 2025-12-13
**Expires**: 2026-03-13
**Last Reviewed**: 2025-12-13

---

### CVE-2023-45853 - zlib MiniZip Integer Overflow

**Package**: zlib1g@1:1.2.13.dfsg-1
**Severity**: CRITICAL
**Type**: Accepted Risk

**Vulnerability Summary**:
Integer overflow in MiniZip zipOpenNewFileInZip4_64 via long filename/comment/extra field.

**Why Accepted**:
1. **MiniZip vs Core zlib**: Affects MiniZip functionality (ZIP archive handling), not core zlib compression used by PostgreSQL.

2. **No ZIP Processing**: Extension doesn't process ZIP archives or use MiniZip functions.

3. **No Fix Available**: Debian 12 (Bookworm) has no security update for this issue.

4. **Low Impact**: Would require malicious ZIP file processing, which doesn't occur in our container.

**Mitigation**:
- Monitor for Debian security updates
- Consider Alpine base image if needed
- Extension doesn't use affected functionality

**Risk Assessment**: Low - requires specific attack vector not present

**Added**: 2025-12-13
**Expires**: 2026-01-13
**Last Reviewed**: 2025-12-13

---

### CVE-2023-2953 - OpenLDAP Null Pointer Dereference

**Package**: libldap-2.5-0@2.5.13+dfsg-5
**Severity**: HIGH
**Type**: False Positive

**Vulnerability Summary**:
Null pointer dereference in ber_memalloc_x() function.

**Why It Doesn't Apply**:
1. **No LDAP Service**: Container doesn't expose LDAP network service.

2. **No LDAP Client Usage**: Extension doesn't make LDAP client connections.

3. **Package Dependency**: libldap is included as a dependency of other packages but not used by our extension.

**Risk Assessment**: None - no LDAP protocol exposure

**Added**: 2025-12-13
**Expires**: 2026-03-13
**Last Reviewed**: 2025-12-13

---

### CVE-2025-58183 - Go archive/tar Unbounded Allocation

**Package**: stdlib@v1.24.6
**Severity**: HIGH
**Type**: False Positive

**Vulnerability Summary**:
Unbounded allocation when parsing GNU sparse map in tar archives.

**Why It Doesn't Apply**:
1. **No Tar Processing**: Extension doesn't process tar archives.

2. **No File Extraction**: No archive handling or file extraction code.

3. **Go stdlib Usage**: While Go stdlib is present, our extension is pure Rust.

**Risk Assessment**: None - no tar archive processing

**Added**: 2025-12-13
**Expires**: 2026-03-13
**Last Reviewed**: 2025-12-13

---

### CVE-2025-58186 - Go HTTP Headers Issue

**Package**: stdlib@v1.24.6
**Severity**: HIGH
**Type**: False Positive

**Vulnerability Summary**:
HTTP headers default limit bypass.

**Why It Doesn't Apply**:
1. **No HTTP Server**: Extension doesn't run HTTP server.

2. **No HTTP Processing**: No HTTP request/response handling.

3. **Pure Database Extension**: Only provides PostgreSQL functions and triggers.

**Risk Assessment**: None - no HTTP service exposure

**Added**: 2025-12-13
**Expires**: 2026-03-13
**Last Reviewed**: 2025-12-13

---

### CVE-2025-58187 - Go Name Constraint Checking

**Package**: stdlib@v1.24.6
**Severity**: HIGH
**Type**: False Positive

**Vulnerability Summary**:
Certificate validation name constraint checking issue.

**Why It Doesn't Apply**:
1. **No Certificate Validation**: Extension doesn't perform certificate validation.

2. **No TLS Certificate Processing**: No certificate chain validation.

3. **Database-Only**: Pure PostgreSQL extension with no network/crypto operations.

**Risk Assessment**: None - no certificate processing

**Added**: 2025-12-13
**Expires**: 2026-03-13
**Last Reviewed**: 2025-12-13

---

### CVE-2025-61729 - Go crypto/x509 Resource Consumption

**Package**: stdlib@v1.24.6
**Severity**: HIGH
**Type**: False Positive

**Vulnerability Summary**:
Excessive resource consumption in certificate error printing.

**Why It Doesn't Apply**:
1. **No X.509 Processing**: Extension doesn't process X.509 certificates.

2. **No Certificate Validation**: No certificate parsing or validation.

3. **No Crypto Operations**: Pure database operations only.

**Risk Assessment**: None - no certificate handling

**Added**: 2025-12-13
**Expires**: 2026-03-13
**Last Reviewed**: 2025-12-13

---

### CVE-2025-6020 - linux-pam Directory Traversal

**Package**: libpam-modules-bin@1.5.2-6+deb12u1, libpam-modules@1.5.2-6+deb12u1, libpam-runtime@1.5.2-6+deb12u1, libpam0g@1.5.2-6+deb12u1
**Severity**: HIGH
**Type**: False Positive

**Vulnerability Summary**:
Directory traversal in pam_namespace allowing privilege escalation via symlink attacks.

**Why It Doesn't Apply**:
1. **No PAM Namespace Configuration**: pam_namespace module not configured in any PAM service.

2. **No PAM Authentication**: PostgreSQL doesn't use PAM for authentication.

3. **Single User Context**: Container runs as single postgres user, no user namespace isolation needed.

4. **No Namespace Requirements**: Extension operates within PostgreSQL's security context.

**Investigation Results**:
- PAM is installed but pam_namespace is not configured
- PostgreSQL uses peer/local authentication, not PAM
- Container runs single postgres user (uid 999)
- No user namespace isolation required

**Risk Assessment**: None - vulnerability requires pam_namespace configuration which is absent

**Added**: 2025-12-13
**Expires**: 2026-03-13
**Last Reviewed**: 2025-12-13

---

## Legacy Suppressions

### CVE-2005-2541 - tar setuid/setgid Issue

**Type**: Legacy (20+ years old)
**Status**: Not Applicable

**Justification**: Ancient tar utility issue requiring physical access. Extension doesn't invoke tar.

### CVE-2011-4116 - Perl File::Temp Race Condition

**Type**: Legacy (14 years old)
**Status**: Not Applicable

**Justification**: Perl temp file handling issue. Extension is pure Rust, no Perl usage.

### TEMP-0290435-0B57B5 - tar rmt Command

**Type**: Disputed/Invalid CVE
**Status**: Not Applicable

**Justification**: Disputed vulnerability in remote tape functionality not used by extension.

### TEMP-0517018-A83CE6 - sysvinit Installer

**Type**: Installation-time only
**Status**: Not Applicable

**Justification**: Affects OS installation, not runtime. Extension installed via cargo/pgrx.

---

## Compliance Alignment

All suppressions align with international security standards:

- **NIST 800-53 SI-2**: Flaw remediation with documented risk assessments
- **FedRAMP Moderate**: Continuous monitoring and vulnerability tracking
- **NIS2 Article 21**: Risk management with documented analysis
- **ISO 27001**: Supply chain security controls
- **SOC 2**: Security, availability, and integrity controls

---

## Review Schedule

- **Monthly**: Review expiry dates, check for new patches
- **Quarterly**: Re-assess false positive justifications
- **After Base Image Updates**: Full vulnerability re-scan
- **Annual**: Compliance certification renewal

---

## Contact

For questions about these suppressions, contact the security team or create an issue in the project repository.
