# Phase 3: Fix Docker Packaging in Security & Compliance Workflow

## Objective

Fix the Docker container build failure in the Security & Compliance workflow. The build is failing during `cargo pgrx package` step with "No such file or directory (os error 2)", preventing the Trivy security scan from running.

## Context

**Current State:**
- Security & Compliance workflow builds a Docker image for Trivy scanning
- Dockerfile successfully builds the extension but fails during `cargo pgrx package`
- Error occurs at the final packaging step: "No such file or directory (os error 2)"
- Without a successful build, Trivy cannot scan for vulnerabilities

**Root Cause (Hypothesis):**
The `cargo pgrx package` command expects specific files or paths that don't exist in the Docker build context. Possible causes:
1. Missing SQL schema files (sql/*.sql)
2. Missing control file (jsonb_ivm.control)
3. Incorrect pg_config path in Docker
4. pgrx trying to access files outside build context

**Error Log:**
```
#16 3.862 Error:
#16 3.862    1: No such file or directory (os error 2)
ERROR: failed to build: failed to solve: process "/bin/sh -c cargo pgrx package --pg-config /root/.pgrx/17.2/pgrx-install/bin/pg_config" did not complete successfully: exit code: 1
```

## Files to Investigate & Modify

1. `Dockerfile` - Review and fix packaging step
2. `.dockerignore` - Ensure necessary files are not excluded
3. `jsonb_ivm.control` - Verify it exists and is included
4. `sql/jsonb_ivm--0.1.0.sql` - Verify SQL schema is included

## Investigation Steps

### Step 1: Review Current Dockerfile

Read the current Dockerfile to understand the build process:

```bash
cat Dockerfile
```

**Look for:**
- Which files are COPYed into the image
- How pgrx is initialized in the Docker build
- The exact `cargo pgrx package` command
- Whether all required files are present

### Step 2: Check .dockerignore

Verify that .dockerignore isn't excluding necessary files:

```bash
cat .dockerignore 2>/dev/null || echo "No .dockerignore file"
```

**Files that MUST NOT be ignored:**
- `Cargo.toml`, `Cargo.lock`
- `src/**`
- `sql/*.sql`
- `*.control`
- `README.md`, `LICENSE`

### Step 3: Verify Required Files Exist

Check that all files required by `cargo pgrx package` are present:

```bash
# Control file (required)
ls -la *.control

# SQL files (required)
ls -la sql/

# Cargo files (required)
ls -la Cargo.toml Cargo.lock
```

### Step 4: Test Local Docker Build

Build the Docker image locally to reproduce the error:

```bash
docker build -t jsonb_ivm:test .
```

If it fails, we can see the exact error and investigate.

## Implementation Steps

### Fix Option 1: Ensure All Required Files are Copied

Update Dockerfile to explicitly copy all required files before packaging:

```dockerfile
# After building the extension, before packaging:

# Copy control file and SQL files (required for packaging)
COPY jsonb_ivm.control ./
COPY sql/*.sql ./sql/

# Now package should work
RUN cargo pgrx package --pg-config /root/.pgrx/17.2/pgrx-install/bin/pg_config
```

**Why:** `cargo pgrx package` needs control file and SQL schema to create the package.

### Fix Option 2: Add Verbose Output to Debug

Add verbose/debug flags to see what's failing:

```dockerfile
# Replace the packaging step with verbose version
RUN cargo pgrx package \
    --pg-config /root/.pgrx/17.2/pgrx-install/bin/pg_config \
    --verbose \
    || (echo "Packaging failed. Directory contents:"; ls -laR; exit 1)
```

**Why:** This will show us exactly what file is missing and where pgrx is looking.

### Fix Option 3: Use Multi-Stage Build Correctly

Ensure the multi-stage build preserves all necessary artifacts:

```dockerfile
# In the builder stage - ensure we copy everything needed
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY sql ./sql
COPY *.control ./

# Build extension
RUN cargo pgrx install \
    --pg-config /root/.pgrx/17.2/pgrx-install/bin/pg_config \
    --release

# Package extension (creates .deb or .tar.gz in target/release)
RUN cargo pgrx package \
    --pg-config /root/.pgrx/17.2/pgrx-install/bin/pg_config

# In the final stage - copy the built package
FROM postgres:17-bookworm
COPY --from=builder /usr/src/jsonb_ivm/target/release/jsonb_ivm-pg17 /usr/share/postgresql/17/extension/
```

**Why:** Multi-stage builds can lose artifacts if not carefully managed.

### Fix Option 4: Check pg_config Path

Verify the pg_config path is correct in Docker:

```dockerfile
# Before packaging, verify pg_config exists and works
RUN /root/.pgrx/17.2/pgrx-install/bin/pg_config --version || \
    (echo "pg_config not found or not working"; find /root/.pgrx -name pg_config; exit 1)

RUN cargo pgrx package --pg-config /root/.pgrx/17.2/pgrx-install/bin/pg_config
```

**Why:** If pg_config path is wrong, pgrx package will fail with cryptic errors.

## Likely Solution (Based on Error Pattern)

The most likely issue is missing SQL schema or control file. Here's the recommended fix:

```dockerfile
# In Dockerfile, update the COPY section:

FROM rust:1.85-slim-bookworm AS builder

WORKDIR /usr/src/jsonb_ivm

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    libclang-dev \
    pkg-config \
    postgresql-server-dev-all \
    libreadline-dev \
    zlib1g-dev \
    bison \
    flex \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-pgrx
RUN cargo install --locked cargo-pgrx --version 0.16.1

# Initialize pgrx
RUN cargo pgrx init --pg17=$(pg_config --bindir)/pg_config

# Copy all source files INCLUDING sql/ and *.control
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY sql ./sql
COPY jsonb_ivm.control ./

# Build the extension
RUN cargo build --release --locked --no-default-features --features pg17

# Install the extension
RUN cargo pgrx install \
    --pg-config /root/.pgrx/17.2/pgrx-install/bin/pg_config \
    --release

# Package the extension (now should work with all files present)
RUN cargo pgrx package \
    --pg-config /root/.pgrx/17.2/pgrx-install/bin/pg_config \
    || (echo "📁 Build directory contents:"; ls -laR .; echo "📁 SQL files:"; ls -la sql/ || echo "No sql/"; echo "📁 Control file:"; ls -la *.control || echo "No control file"; exit 1)

# Final stage
FROM postgres:17-bookworm

# Copy the extension from builder
COPY --from=builder /usr/share/postgresql/17/extension/jsonb_ivm* /usr/share/postgresql/17/extension/
COPY --from=builder /usr/lib/postgresql/17/lib/jsonb_ivm.so /usr/lib/postgresql/17/lib/

# Set up extension
RUN echo "shared_preload_libraries = 'jsonb_ivm'" >> /usr/share/postgresql/postgresql.conf.sample
```

## Verification Commands

**Local Docker build test:**
```bash
# Build the Docker image
docker build -t jsonb_ivm:test .

# Should complete without errors

# Run the container to verify extension works
docker run --rm -d --name pg-test jsonb_ivm:test

# Wait for PostgreSQL to start
sleep 5

# Test extension installation
docker exec pg-test psql -U postgres -c "CREATE EXTENSION jsonb_ivm;"
docker exec pg-test psql -U postgres -c "\dx jsonb_ivm"

# Cleanup
docker stop pg-test
```

**CI verification:**
```bash
# After committing fixes
gh run watch <run-id>

# Should see:
# ✓ Build Docker image
# ✓ Scan with Trivy
# ✓ Upload SARIF results
# ✓ Security compliance check
```

**Check Trivy scan results:**
```bash
gh run view <run-id> --log | grep -A 10 "Trivy scan"
```

## Acceptance Criteria

- [ ] Docker image builds successfully without "No such file or directory" error
- [ ] `cargo pgrx package` step completes successfully
- [ ] Extension files (*.so, *.control, *.sql) are present in final image
- [ ] Trivy security scan runs successfully
- [ ] SARIF results are uploaded to GitHub Security tab
- [ ] No HIGH or CRITICAL vulnerabilities in dependencies
- [ ] Container build completes in reasonable time (< 10 minutes)

## DO NOT

- Do NOT remove the Docker build from CI - security scanning is important
- Do NOT ignore Trivy findings - address vulnerabilities or document exceptions
- Do NOT copy entire repository into Docker - only copy necessary files
- Do NOT use `:latest` tags - pin specific versions for reproducibility
- Do NOT skip multi-stage build - it reduces final image size significantly

## Notes

**Why Docker packaging matters:**
- Trivy scans for vulnerabilities in both OS packages and Rust dependencies
- Container security scan catches issues that cargo audit might miss
- Provides SARIF output for GitHub Security tab integration
- Validates that the extension can be deployed as a container

**Common Docker + pgrx issues:**
1. **Missing control file**: pgrx package requires `<extension>.control`
2. **Missing SQL files**: pgrx package needs schema files in `sql/`
3. **Wrong pg_config**: Must match the PostgreSQL version in pgrx init
4. **Build context**: Files not COPYed won't be available to pgrx

**Debugging Docker builds:**
```bash
# Build with --progress=plain to see all output
docker build --progress=plain -t jsonb_ivm:test .

# Build up to a specific stage and inspect
docker build --target builder -t jsonb_ivm:builder .
docker run --rm -it jsonb_ivm:builder /bin/bash
# Inside container: ls -la, check files
```

**Security scan considerations:**
- Trivy checks for CVEs in OS packages (Debian bookworm)
- Trivy checks Rust dependencies via Cargo.lock
- Some Rust crates may have outdated dependencies (not our code)
- Document any accepted risks in security policy

**Alternative approaches:**
1. **Skip Docker**: Lose container security scanning
2. **Use pre-built PostgreSQL image with extension**: Complex to maintain
3. **Build extension outside Docker**: Doesn't validate containerization
