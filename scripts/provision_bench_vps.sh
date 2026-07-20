#!/usr/bin/env bash
#
# provision_bench_vps.sh — provision, use, and destroy the benchmark host.
#
# Phase 4 takes jsonb_delta's published performance numbers on a machine a third
# party can rent, because "keyed to version + machine profile" is the actual
# deliverable of issue #15 and a developer laptop is not a machine profile.
#
# Provisioning and teardown live in this one file on purpose: a teardown step
# that lives in someone's memory is a standing charge waiting to happen.
#
#   ./scripts/provision_bench_vps.sh status     # read-only, free
#   ./scripts/provision_bench_vps.sh up         # BILLS — needs explicit consent
#   ./scripts/provision_bench_vps.sh run        # measure on the live host
#   ./scripts/provision_bench_vps.sh down       # destroy everything, idempotent
#   ./scripts/provision_bench_vps.sh selftest   # exercise the free paths
#
# Every subcommand accepts --dry-run, which prints the hcloud calls instead of
# making them, so the whole path can be reviewed without spending anything.
#
# Cost discipline:
#   * `up` refuses to run unless BENCH_CONFIRM_SPEND=yes is set explicitly. There
#     is no interactive prompt to fat-finger and no default that bills.
#   * every resource is labelled, and `down` deletes by label, so a resource
#     created alongside the server cannot be orphaned by being forgotten.
#   * `up` and `down` both print what `status` would, so the billing state is
#     visible at the end of every run.

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
#
# Dedicated vCPU (CCX), not shared (CX/CPX). Shared instances have noisy
# neighbours, and steal time at the ~1 ms scale being measured is not a rounding
# error — it is the entire signal. The premium is a further argument for keeping
# the instance short-lived, not for downgrading it.
# ---------------------------------------------------------------------------
BENCH_NAME="${BENCH_NAME:-jsonb-delta-bench}"
BENCH_TYPE="${BENCH_TYPE:-ccx13}"          # dedicated vCPU, 2 vCPU / 8 GB
BENCH_LOCATION="${BENCH_LOCATION:-nbg1}"
BENCH_IMAGE="${BENCH_IMAGE:-ubuntu-24.04}"
BENCH_PG_MAJOR="${BENCH_PG_MAJOR:-17}"
BENCH_PGRX_VERSION="${BENCH_PGRX_VERSION:-0.17.0}"
BENCH_LABEL="purpose=jsonb-delta-bench"
BENCH_SSH_KEY="${BENCH_SSH_KEY:-}"          # name of the key registered in hcloud
BENCH_SSH_IDENTITY="${BENCH_SSH_IDENTITY:-}" # local private key matching it

# Without an explicit identity, ssh offers its default keys, which will not be the
# one registered with the provider — and the failure surfaces only after the
# instance exists and is billing.
SSH_OPTS=(-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10)
[[ -n "$BENCH_SSH_IDENTITY" ]] && SSH_OPTS+=(-i "$BENCH_SSH_IDENTITY" -o IdentitiesOnly=yes)

DRY_RUN=0
RESULTS_DIR="${RESULTS_DIR:-benchmarks}"

# ---------------------------------------------------------------------------
# Plumbing
# ---------------------------------------------------------------------------
log()  { printf '\033[1m==>\033[0m %s\n' "$*" >&2; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# All mutating hcloud calls funnel through here so --dry-run is total rather
# than best-effort. Read-only queries call hcloud directly.
hc() {
    if [[ $DRY_RUN -eq 1 ]]; then
        printf '  [dry-run] hcloud %s\n' "$*" >&2
        return 0
    fi
    hcloud "$@"
}

require_hcloud() {
    command -v hcloud >/dev/null 2>&1 \
        || die "hcloud CLI not found. See https://github.com/hetznercloud/cli"
    hcloud context active >/dev/null 2>&1 \
        || die "no active hcloud context. Run: hcloud context create <name>"
}

server_exists() { hcloud server describe "$BENCH_NAME" >/dev/null 2>&1; }

server_ip() { hcloud server ip "$BENCH_NAME" 2>/dev/null || true; }

# ---------------------------------------------------------------------------
# status — read-only, always free, safe to run at any time
# ---------------------------------------------------------------------------
cmd_status() {
    require_hcloud
    log "hcloud context: $(hcloud context active)"
    if server_exists; then
        warn "BILLING: server '$BENCH_NAME' exists and is being charged for."
        hcloud server describe "$BENCH_NAME" \
            -o 'format={{.Name}}  {{.ServerType.Name}}  {{.Datacenter.Location.Name}}  {{.Status}}  {{.PublicNet.IPv4.IP}}'
    else
        log "no server named '$BENCH_NAME' — nothing is being billed under that name"
    fi

    local extras
    extras="$(hcloud volume list -l "$BENCH_LABEL" -o noheader 2>/dev/null || true)"
    [[ -n "$extras" ]] && warn "labelled volumes still exist:"$'\n'"$extras"
    extras="$(hcloud floating-ip list -l "$BENCH_LABEL" -o noheader 2>/dev/null || true)"
    [[ -n "$extras" ]] && warn "labelled floating IPs still exist:"$'\n'"$extras"
    return 0
}

# ---------------------------------------------------------------------------
# up — create the measurement host
#
# Re-entrant: if the server already exists it is reused rather than duplicated,
# so a run that failed halfway through can be retried without hand-editing cloud
# state or accidentally paying for two boxes.
# ---------------------------------------------------------------------------
cmd_up() {
    require_hcloud

    [[ "${BENCH_CONFIRM_SPEND:-}" == "yes" ]] || die \
"refusing to provision: this creates a billable Hetzner server.
  Re-run with BENCH_CONFIRM_SPEND=yes once the repository owner has authorized it:
      BENCH_CONFIRM_SPEND=yes $0 up
  To review the exact calls without spending anything:
      $0 up --dry-run"

    if server_exists; then
        log "server '$BENCH_NAME' already exists — reusing it (up is re-entrant)"
    else
        [[ -n "$BENCH_SSH_KEY" ]] || die \
"BENCH_SSH_KEY is required so the host is reachable after creation.
  Available keys: $(hcloud ssh-key list -o noheader -o columns=name 2>/dev/null | tr '\n' ' ')"

        log "creating $BENCH_TYPE in $BENCH_LOCATION from $BENCH_IMAGE"
        hc server create \
            --name "$BENCH_NAME" \
            --type "$BENCH_TYPE" \
            --image "$BENCH_IMAGE" \
            --location "$BENCH_LOCATION" \
            --ssh-key "$BENCH_SSH_KEY" \
            --label "$BENCH_LABEL"
    fi

    if [[ $DRY_RUN -eq 1 ]]; then
        log "dry run: skipping provisioning and profile capture"
        return 0
    fi

    local ip
    ip="$(server_ip)"
    [[ -n "$ip" ]] || die "server created but has no IPv4 address"
    log "waiting for sshd on $ip"
    local i
    for i in $(seq 1 60); do
        ssh "${SSH_OPTS[@]}" "root@$ip" true 2>/dev/null && break
        [[ $i -eq 60 ]] && die "sshd did not come up within 5 minutes"
        sleep 5
    done

    log "installing PostgreSQL $BENCH_PG_MAJOR and build toolchain"
    # Reason: SC2087 is the intended behaviour here. $BENCH_PG_MAJOR must expand
    # client-side (the remote has no such variable); anything that must survive to
    # the server is escaped as \$… below.
    # shellcheck disable=SC2087
    ssh "${SSH_OPTS[@]}" "root@$ip" bash -s <<EOF
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq curl ca-certificates gnupg build-essential git pkg-config \
    libssl-dev libclang-dev flex bison libreadline-dev zlib1g-dev
install -d /usr/share/postgresql-common/pgdg
curl -fsSL -o /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc \
    https://www.postgresql.org/media/keys/ACCC4CF8.asc
echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc] \
http://apt.postgresql.org/pub/repos/apt \$(. /etc/os-release && echo \$VERSION_CODENAME)-pgdg main" \
    > /etc/apt/sources.list.d/pgdg.list
apt-get update -qq
apt-get install -y -qq postgresql-$BENCH_PG_MAJOR postgresql-server-dev-$BENCH_PG_MAJOR

# Rust and cargo-pgrx, pinned to the version this repository builds with. A
# version skew between cargo-pgrx and the pgrx dependency fails the build after
# the instance is already running and billing, so it is pinned rather than latest.
if ! command -v cargo >/dev/null 2>&1; then
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
fi
export PATH="/root/.cargo/bin:\$PATH"
cargo install --locked cargo-pgrx --version $BENCH_PGRX_VERSION 2>&1 | tail -1

# Point pgrx at the distro PostgreSQL rather than letting it build one from
# source: the packaged server is the thing a third party would rent and reproduce,
# and a source build would add ~20 minutes of billed time for no benefit.
cargo pgrx init --pg$BENCH_PG_MAJOR /usr/lib/postgresql/$BENCH_PG_MAJOR/bin/pg_config

# pg_tviews must never be preloaded here: its ProcessUtility hook fires on the
# tv_-prefixed fixtures, and an extension hooking utility statements has no
# business being in the loop while utility statements are timed.
grep -q "^shared_preload_libraries" /etc/postgresql/$BENCH_PG_MAJOR/main/postgresql.conf \
    && sed -i "s/^shared_preload_libraries.*/shared_preload_libraries = ''/" \
        /etc/postgresql/$BENCH_PG_MAJOR/main/postgresql.conf
systemctl restart postgresql
EOF

    cmd_profile
    cmd_status
}

# ---------------------------------------------------------------------------
# profile — print the machine profile the results must be keyed to
#
# An unnamed machine profile makes numbers exactly as unverifiable as the ones
# Phase 4 exists to replace, so this is a deliverable and not diagnostics.
# ---------------------------------------------------------------------------
cmd_profile() {
    require_hcloud
    server_exists || die "no server '$BENCH_NAME' — cannot report a machine profile for a host that does not exist"

    local ip
    ip="$(server_ip)"
    {
        echo "# jsonb_delta benchmark machine profile"
        echo "provider:      Hetzner Cloud"
        echo "instance:      $BENCH_TYPE (dedicated vCPU)"
        echo "location:      $BENCH_LOCATION"
        echo "image:         $BENCH_IMAGE"
        echo "commit:        $(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
        [[ $DRY_RUN -eq 0 ]] && {
            echo "cpu:           $(ssh "${SSH_OPTS[@]}" "root@$ip" "lscpu | sed -n 's/^Model name: *//p'" 2>/dev/null || echo unknown)"
            echo "vcpu/ram:      $(ssh "${SSH_OPTS[@]}" "root@$ip" "nproc" 2>/dev/null || echo '?') vCPU / $(ssh "${SSH_OPTS[@]}" "root@$ip" "free -g | awk '/^Mem:/{print \$2}'" 2>/dev/null || echo '?') GB"
            echo "postgresql:    $(ssh "${SSH_OPTS[@]}" "root@$ip" "psql --version" 2>/dev/null || echo unknown)"
        }
    } | tee "$RESULTS_DIR/machine-profile.txt"
}

# ---------------------------------------------------------------------------
# run — measure on the live host
#
# Fails loudly on a nonexistent instance rather than silently continuing: a
# benchmark that quietly measures the wrong machine is worse than one that
# refuses to run.
# ---------------------------------------------------------------------------
cmd_run() {
    require_hcloud
    server_exists || die \
"no server named '$BENCH_NAME' — nothing to measure.
  Provision one first (this bills):
      BENCH_CONFIRM_SPEND=yes $0 up"

    local ip
    ip="$(server_ip)"
    [[ -n "$ip" ]] || die "server '$BENCH_NAME' exists but has no reachable IPv4 address"

    mkdir -p "$RESULTS_DIR"
    log "running harness on $ip"

    if [[ $DRY_RUN -eq 1 ]]; then
        log "dry run: would rsync the working tree, build --release, and run bench.run_all()"
        return 0
    fi

    rsync -az -e "ssh ${SSH_OPTS[*]}" --exclude target --exclude .git ./ "root@$ip:/opt/jsonb_delta/"
    # Reason: SC2087 as above — $BENCH_PG_MAJOR selects the pgrx feature flag and
    # must be resolved before the heredoc is sent.
    # shellcheck disable=SC2087
    ssh "${SSH_OPTS[@]}" "root@$ip" bash -s <<EOF
set -euo pipefail
cd /opt/jsonb_delta
export PATH="/root/.cargo/bin:\$PATH"
cargo pgrx install --release --no-default-features --features pg$BENCH_PG_MAJOR \
    --pg-config /usr/lib/postgresql/$BENCH_PG_MAJOR/bin/pg_config
sudo -u postgres dropdb --if-exists bench
sudo -u postgres createdb bench

# harness_test.sql is the calibration gate, and ON_ERROR_STOP makes it a real
# one: if the instrument cannot detect a known-slower arm, no scenario runs and
# the host is torn down without producing numbers.
sudo -u postgres psql -d bench -v ON_ERROR_STOP=1 \
    -c 'CREATE EXTENSION IF NOT EXISTS jsonb_delta;' \
    -f test/bench/harness.sql \
    -f test/bench/harness_test.sql

for f in test/bench/scenarios_*.sql; do
    echo "loading \$f"
    sudo -u postgres psql -d bench -v ON_ERROR_STOP=1 -q -f "\$f"
done

# The calibration controls have served their purpose by this point, and two of
# them sleep on purpose -- keeping them in the matrix would bill minutes of wall
# clock to re-prove something already proven.
sudo -u postgres psql -d bench -qc "DELETE FROM bench.scenario WHERE name LIKE 'control_%';"
sudo -u postgres psql -d bench -c 'SELECT count(*) FROM bench.run_all();'
EOF

    local stamp base
    stamp="$(date -u +%Y-%m-%d)"
    base="$RESULTS_DIR/results-$stamp-$BENCH_TYPE"

    { cat "$RESULTS_DIR/machine-profile.txt" 2>/dev/null || true; echo; } > "$base.txt"
    ssh "${SSH_OPTS[@]}" "root@$ip" "sudo -u postgres psql -d bench -c 'SELECT * FROM bench.report;'" >> "$base.txt"

    # Raw per-trial timings as well as the summary. Retaining the evidence rather
    # than only the conclusion is the substance of #15; a reader has to be able to
    # recompute the statistics instead of trusting them.
    ssh "${SSH_OPTS[@]}" "root@$ip" "sudo -u postgres psql -d bench -c \"COPY (SELECT scenario, description, n_trials, native_median_ms, native_p95_ms, delta_median_ms, delta_p95_ms, speedup, outputs_match, native_trials_ms, delta_trials_ms FROM bench.result r JOIN (SELECT scenario s, max(id) i FROM bench.result GROUP BY 1) l ON l.i = r.id ORDER BY scenario) TO STDOUT WITH CSV HEADER\"" \
        > "$base.csv"
    log "results written to $base.{txt,csv}"
    warn "the host is still running and still billing — run '$0 down' when finished"
}

# ---------------------------------------------------------------------------
# down — destroy everything, idempotent
#
# Safe to run twice, and safe to run when nothing was ever created: teardown that
# errors on an already-clean account trains people to ignore its output, which is
# how instances get left running.
# ---------------------------------------------------------------------------
cmd_down() {
    require_hcloud

    if server_exists; then
        log "deleting server '$BENCH_NAME'"
        hc server delete "$BENCH_NAME"
    else
        log "no server named '$BENCH_NAME' — nothing to delete"
    fi

    # Volumes and floating IPs survive server deletion and keep billing, so they
    # are swept by label rather than assumed absent.
    local name
    while read -r name; do
        [[ -z "$name" ]] && continue
        log "deleting volume '$name'"
        hc volume delete "$name"
    done < <(hcloud volume list -l "$BENCH_LABEL" -o noheader -o columns=name 2>/dev/null || true)

    while read -r name; do
        [[ -z "$name" ]] && continue
        log "deleting floating IP '$name'"
        hc floating-ip delete "$name"
    done < <(hcloud floating-ip list -l "$BENCH_LABEL" -o noheader -o columns=name 2>/dev/null || true)

    if [[ $DRY_RUN -eq 0 ]] && server_exists; then
        die "server '$BENCH_NAME' still present after delete — CHECK THE CONSOLE, it is still billing"
    fi
    log "teardown confirmed"
    cmd_status
}

# ---------------------------------------------------------------------------
# selftest — exercise every path that costs nothing
#
# Covers the two behaviours Phase 4 Cycle 2 specifies as its RED conditions:
# `run` must fail loudly with no instance, and `down` must be idempotent.
# ---------------------------------------------------------------------------
cmd_selftest() {
    require_hcloud
    local failures=0
    check() {
        if eval "$2"; then printf '  ok    %s\n' "$1"
        else printf '  FAIL  %s\n' "$1"; failures=$((failures + 1)); fi
    }

    log "self-test (no billable operation is performed)"
    server_exists && die "self-test needs a clean slate, but '$BENCH_NAME' exists. Run '$0 down' first."

    check "up refuses without BENCH_CONFIRM_SPEND"      "! BENCH_CONFIRM_SPEND= $0 up >/dev/null 2>&1"
    check "run fails loudly with no instance"           "! $0 run >/dev/null 2>&1"
    check "profile fails loudly with no instance"       "! $0 profile >/dev/null 2>&1"
    check "down is safe when nothing exists"            "$0 down >/dev/null 2>&1"
    check "down is idempotent (second run)"             "$0 down >/dev/null 2>&1"
    check "status is safe and read-only"                "$0 status >/dev/null 2>&1"
    check "up --dry-run makes no call"                  "BENCH_CONFIRM_SPEND=yes BENCH_SSH_KEY=x $0 up --dry-run >/dev/null 2>&1"
    check "no server was created by the self-test"      "! hcloud server describe $BENCH_NAME >/dev/null 2>&1"

    [[ $failures -eq 0 ]] || die "$failures self-test check(s) failed"
    log "self-test passed — billable paths remain gated behind BENCH_CONFIRM_SPEND"
}

# ---------------------------------------------------------------------------
main() {
    local cmd="${1:-}"; shift || true
    for arg in "$@"; do
        [[ "$arg" == "--dry-run" ]] && DRY_RUN=1
    done
    mkdir -p "$RESULTS_DIR"

    case "$cmd" in
        up)       cmd_up ;;
        run)      cmd_run ;;
        down)     cmd_down ;;
        status)   cmd_status ;;
        profile)  cmd_profile ;;
        selftest) cmd_selftest ;;
        *)        die "usage: $0 {up|run|down|status|profile|selftest} [--dry-run]" ;;
    esac
}

main "$@"
