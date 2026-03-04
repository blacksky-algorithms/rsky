#!/usr/bin/env bash
#
# rollback-wintermute.sh - Restore previous wintermute binary and restart.
#
# Usage: sudo /opt/appview/rollback-wintermute.sh
#
set -euo pipefail

readonly BINARY_DST="/usr/local/bin/rsky-wintermute"
readonly BINARY_PREV="/usr/local/bin/rsky-wintermute.prev"
readonly SERVICE="rsky-wintermute"
readonly METRICS_URL="http://localhost:9090"

log() { echo "[rollback] $(date '+%H:%M:%S') $*"; }
die() { log "FATAL: $*"; exit 1; }

[[ -f "$BINARY_PREV" ]] || die "no previous binary at ${BINARY_PREV}"

log "current binary: $(ls -la "$BINARY_DST")"
log "previous binary: $(ls -la "$BINARY_PREV")"

log "stopping ${SERVICE}"
systemctl stop "$SERVICE"

log "restoring previous binary"
cp "$BINARY_PREV" "$BINARY_DST"

log "starting ${SERVICE}"
systemctl start "$SERVICE"

log "waiting 5s for startup..."
sleep 5

if ! systemctl is-active --quiet "$SERVICE"; then
    die "service failed to start after rollback"
fi

health_status=$(curl -sf -o /dev/null -w '%{http_code}' "${METRICS_URL}/_health" 2>/dev/null || echo "000")
if [[ "$health_status" == "200" ]]; then
    log "health check: 200 ok"
else
    log "WARNING: health endpoint returned ${health_status} (service is active, may still be starting)"
fi

log "rollback complete"
