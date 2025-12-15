# Phase 2: Fix Load Tests PostgreSQL Configuration

## Objective

Fix load tests CI failure in the "Configure PostgreSQL" step. The pg_isready check times out even with our improved loop, suggesting PostgreSQL cluster is not starting on the expected port or there's a configuration issue.

## Context

**Current State:**
- Load tests job successfully installs PostgreSQL 17
- PostgreSQL cluster creation appears to succeed (no errors)
- "Configure PostgreSQL" step times out waiting for pg_isready
- pg_isready never succeeds within 30 seconds

**Root Cause (Hypothesis):**
Several possible issues:
1. PostgreSQL cluster is created but not listening on port 5432
2. Cluster is listening on Unix socket but not TCP
3. pg_hba.conf edits are malformed or not applied
4. Cluster creation succeeded but startup failed silently
5. Port 5432 is already in use by another service

**Error Pattern:**
```
X Configure PostgreSQL (exit code 1 or timeout)
  ⏳ Waiting for PostgreSQL... (1/30)
  ⏳ Waiting for PostgreSQL... (2/30)
  ...
  ⏳ Waiting for PostgreSQL... (30/30)
  ❌ PostgreSQL failed to start within 30 seconds
```

## Files to Modify

1. `.github/workflows/test.yml` - Enhance PostgreSQL setup and diagnostics in load-tests job

## Investigation Steps

### Step 1: Add Comprehensive Diagnostics

Before the "Configure PostgreSQL" step, add diagnostics to understand the actual state:

```yaml
      - name: Debug PostgreSQL Setup
        run: |
          echo "=== PostgreSQL Cluster Status ==="
          sudo pg_lsclusters || echo "No clusters found"

          echo ""
          echo "=== PostgreSQL Processes ==="
          ps aux | grep postgres | grep -v grep || echo "No postgres processes"

          echo ""
          echo "=== Network Listeners on 5432 ==="
          sudo lsof -i :5432 || echo "Port 5432 not in use"

          echo ""
          echo "=== PostgreSQL Logs (last 20 lines) ==="
          sudo tail -20 /var/log/postgresql/postgresql-17-main.log 2>/dev/null || echo "No log file found"
```

This will show us:
- Whether cluster exists and its status
- If postgres processes are running
- What (if anything) is on port 5432
- Any errors in PostgreSQL logs

### Step 2: Fix Cluster Configuration

The issue might be that Ubuntu's pg_createcluster doesn't configure TCP listening by default. Update the "Install PostgreSQL 17" step:

```yaml
      - name: Install PostgreSQL 17
        run: |
          sudo apt-get install -y wget gnupg
          sudo sh -c 'echo "deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list'
          wget --quiet -O - https://www.postgresql.org/media/keys/ACCC4CF8.asc | sudo apt-key add -
          sudo apt-get update
          sudo apt-get install -y postgresql-17 postgresql-server-dev-17

          # Stop and remove any existing clusters
          sudo pg_ctlcluster 17 main stop 2>/dev/null || true
          sudo pg_dropcluster 17 main --stop 2>/dev/null || true

          # Create cluster with explicit port
          sudo pg_createcluster -p 5432 17 main

          # Configure PostgreSQL to listen on TCP
          sudo sed -i "s/#listen_addresses = 'localhost'/listen_addresses = 'localhost'/" /etc/postgresql/17/main/postgresql.conf
          sudo sed -i "s/listen_addresses = 'localhost'/listen_addresses = '*'/" /etc/postgresql/17/main/postgresql.conf

          # Start the cluster
          sudo pg_ctlcluster 17 main start

          # Verify it started
          sleep 2
          sudo pg_lsclusters
```

**Key changes:**
- Explicitly specify port 5432 with `-p 5432`
- Configure `listen_addresses` in postgresql.conf
- Add verification step after start

### Step 3: Improve Configure PostgreSQL Step

Update the "Configure PostgreSQL" step with better error handling:

```yaml
      - name: Configure PostgreSQL
        run: |
          echo "=== Current Cluster Status ==="
          sudo pg_lsclusters

          # Wait for cluster to be fully ready
          echo ""
          echo "=== Waiting for cluster to accept connections ==="
          sleep 3

          # Configure trust authentication for local connections (CI only)
          echo "=== Configuring pg_hba.conf ==="
          sudo cp /etc/postgresql/17/main/pg_hba.conf /etc/postgresql/17/main/pg_hba.conf.backup
          sudo sed -i 's/^local.*all.*postgres.*peer/local all postgres trust/' /etc/postgresql/17/main/pg_hba.conf
          sudo sed -i 's/^local.*all.*all.*peer/local all all trust/' /etc/postgresql/17/main/pg_hba.conf
          sudo sed -i 's/^host.*all.*all.*127.0.0.1.*scram-sha-256/host all all 127.0.0.1\/32 trust/' /etc/postgresql/17/main/pg_hba.conf
          sudo sed -i 's/^host.*all.*all.*::1.*scram-sha-256/host all all ::1\/128 trust/' /etc/postgresql/17/main/pg_hba.conf

          echo "=== pg_hba.conf changes ==="
          diff /etc/postgresql/17/main/pg_hba.conf.backup /etc/postgresql/17/main/pg_hba.conf || true

          # Reload PostgreSQL to apply changes
          echo ""
          echo "=== Reloading PostgreSQL ==="
          sudo pg_ctlcluster 17 main reload

          # Wait for reload to complete
          sleep 2

          # Check if PostgreSQL is ready with multiple connection attempts
          echo ""
          echo "=== Testing PostgreSQL Connectivity ==="

          # Try Unix socket first
          if psql -U postgres -c "SELECT version();" 2>/dev/null; then
            echo "✅ Unix socket connection successful"
          else
            echo "⚠️  Unix socket connection failed"
          fi

          # Try TCP connection
          for i in {1..30}; do
            if pg_isready -h localhost -p 5432 -U postgres 2>/dev/null; then
              echo "✅ TCP connection successful on attempt $i"
              psql -h localhost -p 5432 -U postgres -c "SELECT version();"
              echo ""
              echo "✅ PostgreSQL is ready and accepting connections"
              exit 0
            fi

            if [ $i -eq 1 ] || [ $((i % 5)) -eq 0 ]; then
              echo "⏳ Waiting for PostgreSQL TCP... ($i/30)"
            fi
            sleep 1
          done

          # If we get here, PostgreSQL didn't start properly
          echo ""
          echo "❌ PostgreSQL failed to accept TCP connections within 30 seconds"
          echo ""
          echo "=== Final Cluster Status ==="
          sudo pg_lsclusters

          echo ""
          echo "=== PostgreSQL Processes ==="
          ps aux | grep postgres | grep -v grep || echo "No postgres processes"

          echo ""
          echo "=== Port 5432 Status ==="
          sudo lsof -i :5432 || echo "Port 5432 not in use"

          echo ""
          echo "=== PostgreSQL Configuration ==="
          grep "^listen_addresses" /etc/postgresql/17/main/postgresql.conf || echo "listen_addresses not set"
          grep "^port" /etc/postgresql/17/main/postgresql.conf || echo "port not explicitly set"

          echo ""
          echo "=== PostgreSQL Logs (last 50 lines) ==="
          sudo tail -50 /var/log/postgresql/postgresql-17-main.log 2>/dev/null || echo "No log file found"

          echo ""
          echo "=== Systemctl Status ==="
          sudo systemctl status postgresql@17-main --no-pager || true

          exit 1
```

**Improvements:**
- Shows cluster status before attempting fixes
- Backs up pg_hba.conf before modifying
- Shows diff of pg_hba.conf changes
- Tests both Unix socket and TCP connections
- Comprehensive diagnostics on failure
- Only shows progress every 5 attempts (less noise)

## Implementation Steps

### Step 1: Apply Diagnostic Step

Add the "Debug PostgreSQL Setup" step between "Install PostgreSQL 17" and "Configure PostgreSQL":

```yaml
      - name: Install PostgreSQL 17
        run: |
          # ... existing install code ...

      - name: Debug PostgreSQL Setup
        run: |
          # ... diagnostic commands ...

      - name: Configure PostgreSQL
        run: |
          # ... enhanced configuration ...
```

### Step 2: Update Install Step with Port and Listen Address

Replace the current PostgreSQL installation with the enhanced version that explicitly configures TCP listening.

### Step 3: Test Locally with Docker

Since local testing might not reproduce CI issues, test with Docker to simulate Ubuntu environment:

```bash
# Create a test dockerfile
cat > Dockerfile.pg-test <<'EOF'
FROM ubuntu:22.04

RUN apt-get update && apt-get install -y wget gnupg sudo lsof

# Install PostgreSQL (same as CI)
RUN sh -c 'echo "deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list'
RUN wget --quiet -O - https://www.postgresql.org/media/keys/ACCC4CF8.asc | apt-key add -
RUN apt-get update && apt-get install -y postgresql-17 postgresql-client-17

# Run setup script
COPY setup-pg.sh /setup-pg.sh
RUN chmod +x /setup-pg.sh
RUN /setup-pg.sh

CMD ["tail", "-f", "/dev/null"]
EOF

# Create setup script (from our workflow)
cat > setup-pg.sh <<'EOF'
#!/bin/bash
set -x

# Stop/remove existing
pg_ctlcluster 17 main stop 2>/dev/null || true
pg_dropcluster 17 main --stop 2>/dev/null || true

# Create with explicit port
pg_createcluster -p 5432 17 main

# Configure TCP listening
sed -i "s/#listen_addresses = 'localhost'/listen_addresses = '*'/" /etc/postgresql/17/main/postgresql.conf

# Start
pg_ctlcluster 17 main start
sleep 2

# Verify
pg_lsclusters
pg_isready -h localhost -p 5432 -U postgres
EOF

# Build and test
docker build -f Dockerfile.pg-test -t pg-test .
docker run --rm pg-test pg_isready -h localhost -p 5432 -U postgres
```

If this works in Docker, the CI should work too.

## Verification Commands

**CI verification after fix:**
```bash
# Push changes
git push origin main

# Watch the load-tests job
gh run watch <run-id>

# Should see:
# ✓ Install PostgreSQL 17
# ✓ Debug PostgreSQL Setup
#   === PostgreSQL Cluster Status ===
#   Ver Cluster Port Status Owner
#   17  main    5432 online postgres
#   === Network Listeners on 5432 ===
#   postgres 12345 ... TCP *:5432 (LISTEN)
# ✓ Configure PostgreSQL
#   ✅ Unix socket connection successful
#   ✅ TCP connection successful on attempt 1
#   ✅ PostgreSQL is ready and accepting connections
# ✓ Install cargo-pgrx
# ✓ Initialize pgrx
# ✓ Build extension
# ✓ Install extension
# ✓ Run load tests
#   🚀 Starting PostgreSQL load tests...
#   ✅ All load tests passed!
```

**Check specific failure details:**
```bash
# If still failing, get diagnostic output
gh run view <run-id> --log | grep -A 100 "Configure PostgreSQL"

# Look for:
# - Cluster status (should show "online")
# - Processes (should show postgres)
# - Port listener (should show postgres on 5432)
# - Logs (check for errors)
```

## Acceptance Criteria

- [ ] PostgreSQL cluster creates successfully on port 5432
- [ ] Cluster is configured to listen on TCP (listen_addresses set)
- [ ] pg_isready succeeds within first few attempts
- [ ] Both Unix socket and TCP connections work
- [ ] pg_hba.conf is configured for trust authentication
- [ ] Load tests can connect and run benchmarks
- [ ] Diagnostic output is clear and helpful if failure occurs
- [ ] Job completes in reasonable time (< 6 minutes total)

## DO NOT

- Do NOT remove the diagnostic steps after fixing - they're valuable for future debugging
- Do NOT use passwords in pg_hba.conf - trust authentication is safe in isolated CI
- Do NOT skip the pg_isready check - it validates the setup worked
- Do NOT reduce the timeout below 30 seconds - cluster startup can be slow
- Do NOT remove error logs from the workflow - they help diagnose issues

## Notes

**Why PostgreSQL might not listen on TCP by default:**

Ubuntu's `pg_createcluster` creates clusters for local (Unix socket) use by default. The `listen_addresses` parameter in `postgresql.conf` may be:
- Commented out (defaults to localhost Unix socket only)
- Set to '' (empty, Unix socket only)
- Set to 'localhost' (but might prefer Unix socket)

Our fix explicitly sets it to '*' to ensure TCP listening.

**Common PostgreSQL startup issues in CI:**

1. **Port already in use**: GitHub Actions runners are clean, but worth checking
2. **Insufficient permissions**: Using `sudo` for all operations solves this
3. **Config syntax errors**: Our sed commands are tested patterns
4. **Slow startup**: Cold start can take 5-10 seconds, hence the sleep
5. **Init system confusion**: Using `pg_ctlcluster` directly avoids systemd issues

**Why we test both Unix socket and TCP:**

- **Unix socket**: Faster, used by local `psql` commands
- **TCP**: Required for our load test script with PGHOST=localhost
- Testing both ensures PostgreSQL is fully functional

**pg_hba.conf trust authentication is safe because:**

- CI environment is ephemeral (destroyed after workflow)
- No sensitive data in test database
- PostgreSQL only listens on localhost (not exposed)
- Alternative (passwords) adds complexity without security benefit

**Alternative approaches considered:**

1. **Use PostgreSQL service container**: More complex, not needed for simple tests
2. **Use systemctl instead of pg_ctlcluster**: Less reliable, Ubuntu-specific tool is better
3. **Skip load tests**: Would lose valuable performance testing
4. **Mock PostgreSQL**: Defeats the purpose of load testing

**If this still fails:**

Check if GitHub Actions runners have changed their base image or if there's a known issue with PostgreSQL 17 on Ubuntu 22.04. Fallback: Test with PostgreSQL 16 instead.
