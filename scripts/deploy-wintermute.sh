#!/usr/bin/env bash
#
# deploy-wintermute.sh - Build, deploy, and health-check wintermute with auto-rollback.
#
# Usage: sudo /opt/appview/deploy-wintermute.sh [branch]
#   branch: git branch to pull (defaults to current branch)
#
# Phases:
#   1. Pre-flight  - verify paths, current binary
#   2. Build       - git pull + cargo build --release
#   3. Backup      - copy current binary to .prev, record cursor
#   4. Swap        - stop service, install new binary, start service
#   5. Health      - 30s health check loop with metric verification
#   6. Rollback    - automatic on any failure
#
set -euo pipefail

# Add cargo to PATH (installed under ubuntu user, but sudo sets HOME=/root)
export PATH="/home/ubuntu/.cargo/bin:${PATH}"

readonly REPO_DIR="/opt/appview/rsky"
readonly BINARY_SRC="${REPO_DIR}/target/release/wintermute"
readonly BINARY_DST="/usr/local/bin/rsky-wintermute"
readonly BINARY_PREV="/usr/local/bin/rsky-wintermute.prev"
readonly SERVICE="rsky-wintermute"
readonly METRICS_URL="http://localhost:9090"
readonly HEALTH_TIMEOUT=30
readonly HEALTH_INTERVAL=2
readonly METRIC_CHECK_AFTER=10

BRANCH="${1:-}"

log() { echo "[deploy] $(date '+%H:%M:%S') $*"; }
die() { log "FATAL: $*"; exit 1; }

rollback() {
    log "--- ROLLBACK ---"
    if [[ ! -f "$BINARY_PREV" ]]; then
        die "no previous binary at ${BINARY_PREV}, cannot rollback"
    fi
    systemctl stop "$SERVICE" 2>/dev/null || true
    cp "$BINARY_PREV" "$BINARY_DST"
    systemctl start "$SERVICE"
    log "rolled back to previous binary, service restarted"

    sleep 5
    if systemctl is-active --quiet "$SERVICE"; then
        log "rollback health check: service is active"
    else
        die "rollback failed: service not active after restore"
    fi
}

# --- 1. Pre-flight ---
log "=== Phase 1: Pre-flight ==="

[[ -d "$REPO_DIR" ]] || die "repo not found at ${REPO_DIR}"
[[ -f "$BINARY_DST" ]] || die "current binary not found at ${BINARY_DST}"

if [[ -z "$BRANCH" ]]; then
    BRANCH=$(cd "$REPO_DIR" && git rev-parse --abbrev-ref HEAD)
fi
log "branch: ${BRANCH}"
log "current binary: $(ls -la "$BINARY_DST")"

# --- 2. Build ---
log "=== Phase 2: Build ==="

cd "$REPO_DIR"
git fetch origin
git checkout "$BRANCH"
git pull origin "$BRANCH"

COMMIT=$(git rev-parse --short HEAD)
log "building commit ${COMMIT}"

cargo build --release --package rsky-wintermute
[[ -f "$BINARY_SRC" ]] || die "build succeeded but binary not found at ${BINARY_SRC}"
log "build complete: $(ls -la "$BINARY_SRC")"

# --- 3. Backup ---
log "=== Phase 3: Backup ==="

cp "$BINARY_DST" "$BINARY_PREV"
log "backed up current binary to ${BINARY_PREV}"

# Record pre-deploy metrics snapshot if service is running
if systemctl is-active --quiet "$SERVICE"; then
    PRE_EVENTS=$(curl -sf "${METRICS_URL}/metrics" 2>/dev/null | grep '^ingester_firehose_events_total' | awk '{print $2}' || echo "0")
    log "pre-deploy firehose events: ${PRE_EVENTS}"
fi

# --- 4. Swap ---
log "=== Phase 4: Swap ==="

systemctl stop "$SERVICE"
log "service stopped"

cp "$BINARY_SRC" "$BINARY_DST"
log "new binary installed"

systemctl start "$SERVICE"
log "service started"

# --- 5. Health Check ---
log "=== Phase 5: Health Check (${HEALTH_TIMEOUT}s) ==="

elapsed=0
healthy=false
metrics_checked=false

while (( elapsed < HEALTH_TIMEOUT )); do
    sleep "$HEALTH_INTERVAL"
    elapsed=$(( elapsed + HEALTH_INTERVAL ))

    # Check service is still running
    if ! systemctl is-active --quiet "$SERVICE"; then
        log "FAIL: service crashed after ${elapsed}s"
        rollback
        exit 1
    fi

    # Check health endpoint
    health_status=$(curl -sf -o /dev/null -w '%{http_code}' "${METRICS_URL}/_health" 2>/dev/null || echo "000")
    if [[ "$health_status" != "200" ]]; then
        log "health check returned ${health_status} at ${elapsed}s, waiting..."
        continue
    fi

    log "health endpoint: 200 ok (${elapsed}s)"
    healthy=true

    # After METRIC_CHECK_AFTER seconds, verify firehose is processing
    if (( elapsed >= METRIC_CHECK_AFTER )) && [[ "$metrics_checked" == "false" ]]; then
        POST_EVENTS=$(curl -sf "${METRICS_URL}/metrics" 2>/dev/null | grep '^ingester_firehose_events_total' | awk '{print $2}' || echo "0")
        if [[ -n "$POST_EVENTS" ]] && (( $(echo "$POST_EVENTS > 0" | bc -l 2>/dev/null || echo 0) )); then
            log "firehose events: ${POST_EVENTS} (processing confirmed)"
            metrics_checked=true
        else
            log "firehose events: ${POST_EVENTS} (not yet processing, waiting...)"
        fi
    fi
done

if [[ "$healthy" != "true" ]]; then
    log "FAIL: health check never passed within ${HEALTH_TIMEOUT}s"
    rollback
    exit 1
fi

if [[ "$metrics_checked" != "true" ]]; then
    log "WARNING: firehose metric not confirmed, but health endpoint is 200"
    log "check manually: curl ${METRICS_URL}/metrics | grep ingester_firehose"
fi

# --- Done ---
log "=== Deploy Complete ==="
log "commit: ${COMMIT}"
log "binary: ${BINARY_DST}"
log "backup: ${BINARY_PREV}"
