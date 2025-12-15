# Phase 2: Fix Load Tests CI Failure

## Objective

Fix the load tests CI job failure caused by PostgreSQL not running. The tests are failing with "PostgreSQL is not running" error because the workflow installs PostgreSQL but doesn't ensure the cluster is started and accepting connections.

## Context

**Current State:**
- Load tests job in `.github/workflows/test.yml` installs PostgreSQL 17
- The workflow attempts to start the cluster with `pg_createcluster` and `pg_ctlcluster`
- However, the script `run_load_tests.sh` fails at `pg_isready` check
- This indicates PostgreSQL service is not actually running or not accepting connections

**Root Cause:**
The load test script checks if PostgreSQL is accepting connections using `pg_isready -q`, but the workflow doesn't properly start the PostgreSQL service or configure it to accept connections.

**Error Log:**
```
❌ PostgreSQL is not running. Please start PostgreSQL first.
   On Ubuntu/Debian: sudo systemctl start postgresql
   On macOS: brew services start postgresql
Process completed with exit code 1.
```

## Files to Modify

1. `.github/workflows/test.yml` - Fix PostgreSQL startup in load-tests job
2. `scripts/run_load_tests.sh` - Update to work in CI environment (optional)

## Implementation Steps

### Step 1: Ensure PostgreSQL Cluster is Created and Started

Update the "Install PostgreSQL 17" step in load-tests job to properly create and start the cluster:

```yaml
- name: Install PostgreSQL 17
  run: |
    sudo apt-get install -y wget gnupg
    sudo sh -c 'echo "deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list'
    wget --quiet -O - https://www.postgresql.org/media/keys/ACCC4CF8.asc | sudo apt-key add -
    sudo apt-get update
    sudo apt-get install -y postgresql-17 postgresql-server-dev-17

    # Stop default cluster if it exists
    sudo pg_ctlcluster 17 main stop || true

    # Drop and recreate cluster to ensure clean state
    sudo pg_dropcluster --stop 17 main || true
    sudo pg_createcluster 17 main --start

    # Verify cluster is running
    sudo pg_lsclusters
```

**Why:**
- Ubuntu's PostgreSQL packages create a cluster automatically but may not start it
- We explicitly drop/recreate to ensure clean state
- Verification step helps debug if cluster creation fails

### Step 2: Configure PostgreSQL for Local Connections

Add a step to configure PostgreSQL to accept local connections without password (for CI):

```yaml
- name: Configure PostgreSQL
  run: |
    # Configure trust authentication for local connections (CI only)
    sudo sed -i 's/^local.*all.*postgres.*peer/local all postgres trust/' /etc/postgresql/17/main/pg_hba.conf
    sudo sed -i 's/^host.*all.*all.*127.0.0.1.*scram-sha-256/host all all 127.0.0.1\/32 trust/' /etc/postgresql/17/main/pg_hba.conf

    # Reload PostgreSQL to apply changes
    sudo pg_ctlcluster 17 main reload

    # Wait for PostgreSQL to be ready
    timeout 30 bash -c 'until pg_isready -h localhost -p 5432 -U postgres -q; do sleep 1; done'

    echo "✅ PostgreSQL is ready and accepting connections"
```

**Why:**
- Default `pg_hba.conf` requires password authentication
- CI tests need passwordless access for `psql -U postgres` commands
- `trust` authentication is safe in CI (isolated environment)
- Timeout ensures we don't hang forever waiting for PostgreSQL

### Step 3: Update Load Test Script Connection Parameters

Modify `scripts/run_load_tests.sh` to use explicit connection parameters for CI:

```bash
# At the top of run_load_tests.sh, after set -e, add:

# CI environment detection and configuration
if [ -n "$CI" ] || [ -n "$GITHUB_ACTIONS" ]; then
    export PGHOST=localhost
    export PGPORT=5432
    export PGUSER=postgres
fi

# Check if PostgreSQL is running
if ! pg_isready -q; then
```

**Why:**
- Makes the script CI-aware
- Explicitly sets connection parameters for GitHub Actions
- Still works locally without modifications

### Step 4: Add Debug Output to Load Tests

Update the PostgreSQL readiness check in `run_load_tests.sh` to provide better error messages:

```bash
# Check if PostgreSQL is running
if ! pg_isready -q; then
    echo "❌ PostgreSQL is not running or not accepting connections."
    echo ""
    echo "🔍 Debugging information:"
    echo "  PGHOST: ${PGHOST:-<not set>}"
    echo "  PGPORT: ${PGPORT:-<not set>}"
    echo "  PGUSER: ${PGUSER:-<not set>}"
    echo ""

    # Try to get more info
    if command -v pg_lsclusters &> /dev/null; then
        echo "📊 PostgreSQL clusters:"
        pg_lsclusters 2>&1 || echo "  (cannot list clusters)"
    fi

    echo ""
    echo "💡 To start PostgreSQL:"
    echo "   On Ubuntu/Debian: sudo systemctl start postgresql"
    echo "   On macOS: brew services start postgresql"
    exit 1
fi
```

**Why:**
- Better debugging in CI when things fail
- Shows what connection parameters are being used
- Lists available clusters for troubleshooting

## Verification Commands

**Local verification:**
```bash
# Stop local PostgreSQL to simulate CI failure
sudo systemctl stop postgresql
./scripts/run_load_tests.sh
# Should show clear error message with debug info

# Start PostgreSQL
sudo systemctl start postgresql
./scripts/run_load_tests.sh
# Should run successfully
```

**CI verification:**
```bash
# After committing changes, check the workflow
gh run watch <run-id>

# Should see:
# ✓ Install PostgreSQL 17
# ✓ Configure PostgreSQL
# ✓ Install cargo-pgrx
# ✓ Initialize pgrx
# ✓ Build extension
# ✓ Install extension
# ✓ Run load tests
#   🚀 Starting PostgreSQL load tests...
#   ✅ Test data prepared (1000 rows)
#   ...
#   ✅ All load tests passed!
```

**Manual verification on fresh Ubuntu system:**
```bash
# On clean Ubuntu VM/container
sudo apt-get update
sudo apt-get install -y wget gnupg
sudo sh -c 'echo "deb http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" > /etc/apt/sources.list.d/pgdg.list'
wget --quiet -O - https://www.postgresql.org/media/keys/ACCC4CF8.asc | sudo apt-key add -
sudo apt-get update
sudo apt-get install -y postgresql-17 postgresql-server-dev-17

# Apply our fixes
sudo pg_dropcluster --stop 17 main || true
sudo pg_createcluster 17 main --start
sudo sed -i 's/^local.*all.*postgres.*peer/local all postgres trust/' /etc/postgresql/17/main/pg_hba.conf
sudo pg_ctlcluster 17 main reload
timeout 30 bash -c 'until pg_isready -h localhost -p 5432 -U postgres -q; do sleep 1; done'

# Should succeed
pg_isready -h localhost -p 5432 -U postgres
```

## Acceptance Criteria

- [ ] PostgreSQL cluster is created and started in CI
- [ ] PostgreSQL is configured for trust authentication (CI only)
- [ ] `pg_isready` check succeeds before running load tests
- [ ] Load test script can connect to PostgreSQL without password
- [ ] Load tests complete successfully (all benchmarks run)
- [ ] Clear error messages if PostgreSQL fails to start
- [ ] Job completes in reasonable time (< 6 minutes)

## DO NOT

- Do NOT use trust authentication in production documentation - this is CI-only
- Do NOT skip the load tests - they validate performance under concurrency
- Do NOT reduce the test duration or concurrency - we want realistic load testing
- Do NOT commit PostgreSQL passwords or credentials to the repository
- Do NOT remove the `pg_isready` check - it's a valuable safety check

## Notes

**Why trust authentication is safe in CI:**
- GitHub Actions runners are isolated ephemeral environments
- PostgreSQL only accepts connections from localhost
- Environment is destroyed after the workflow completes
- No sensitive data in the test database

**Alternative approaches considered:**
1. **Use PostgreSQL service container**: More complex setup, not needed for simple tests
2. **Skip pg_isready check**: Bad practice, would hide real failures
3. **Use pgbouncer**: Overkill for simple load tests
4. **Mock PostgreSQL**: Defeats the purpose of load testing

**Performance considerations:**
- PostgreSQL installation: ~30 seconds
- Cluster creation and startup: ~5 seconds
- Load tests execution: ~2-3 minutes
- Total: ~3-4 minutes (acceptable for QA tests)

**Common CI PostgreSQL issues:**
1. Cluster not started: Fixed by explicit `pg_createcluster --start`
2. Wrong port: Fixed by setting PGPORT=5432
3. Authentication failures: Fixed by trust auth in pg_hba.conf
4. Socket permissions: Fixed by using TCP (localhost) instead of Unix socket
