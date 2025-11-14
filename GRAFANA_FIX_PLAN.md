# Grafana Dashboard Fix Plan

## Root Cause Analysis - CONFIRMED ✅

**Problem**: Dashboard queries use OLD metric names that don't exist anymore.

**Evidence**:
1. ✅ All services exposing metrics properly
2. ✅ Prometheus scraping all 10 targets successfully (all "up")
3. ✅ Metrics match Redis ground truth perfectly
4. ✅ Backpressure resolved (was 40M, now 0)

**The Issue**: Metric name mismatch between dashboard and actual metrics

---

## Metric Name Mapping

### OLD Names (Dashboard Expects) ❌
```
ingester_redis_stream_length{stream="firehose_backfill"}
ingester_redis_stream_length{stream="firehose_live"}
ingester_redis_stream_length{stream="repo_backfill"}
```

### ACTUAL Names (Services Expose) ✅
```
ingester_firehose_backfill_length
ingester_firehose_live_length
ingester_repo_backfill_length
ingester_label_live_length
```

**Key Difference**:
- Old: Single metric with `stream` label
- New: Separate metrics, no labels

---

## Fix Steps

### Step 1: Download Current Dashboard

On production server:
```bash
cd /mnt/nvme/bsky/atproto
cp grafana-rsky-dashboard.json grafana-rsky-dashboard.json.backup
scp grafana-rsky-dashboard.json rudyfraser@localhost:~/Projects/rsky/
```

### Step 2: Find and Replace Metric Names

Replace all occurrences in dashboard JSON:

**Stream Length Metrics:**
```bash
# OLD → NEW replacements needed:
ingester_redis_stream_length{stream="firehose_backfill"}
  → ingester_firehose_backfill_length

ingester_redis_stream_length{stream="firehose_live"}
  → ingester_firehose_live_length

ingester_redis_stream_length{stream="repo_backfill"}
  → ingester_repo_backfill_length

ingester_redis_stream_length{stream="label_live"}
  → ingester_label_live_length
```

**Other Potential Mismatches:**
- Check for any other metrics that might have been renamed
- Verify all panel queries against actual metric names

### Step 3: Test Queries in Prometheus

On production server, test each metric:
```bash
# Test new metric names work
curl -s "localhost:9090/api/v1/query?query=ingester_firehose_backfill_length" | jq '.data.result[0].value'
curl -s "localhost:9090/api/v1/query?query=ingester_firehose_live_length" | jq '.data.result[0].value'
curl -s "localhost:9090/api/v1/query?query=ingester_repo_backfill_length" | jq '.data.result[0].value'
curl -s "localhost:9090/api/v1/query?query=ingester_label_live_length" | jq '.data.result[0].value'

# These should return actual values, not empty results
```

### Step 4: Update Dashboard

Upload fixed dashboard back to production:
```bash
scp ~/Projects/rsky/grafana-rsky-dashboard.json rudyfraser@api:/mnt/nvme/bsky/atproto/
```

Then re-import to Grafana UI or restart Grafana if it auto-loads from file.

### Step 5: Verify All Panels Show Data

Check each panel in dashboard:
- [ ] Ingester Overview - Stream Lengths
- [ ] Backfiller Overview - Output Stream Length
- [ ] Indexer Overview - All metrics
- [ ] Error rate panels
- [ ] Throughput panels

---

## Quick Sed Commands for Fixes

```bash
cd ~/Projects/rsky

# Backup
cp grafana-rsky-dashboard.json grafana-rsky-dashboard.json.orig

# Replace stream length metrics
sed -i.bak 's/ingester_redis_stream_length{stream=\\"firehose_backfill\\"}/ingester_firehose_backfill_length/g' grafana-rsky-dashboard.json
sed -i.bak 's/ingester_redis_stream_length{stream=\\"firehose_live\\"}/ingester_firehose_live_length/g' grafana-rsky-dashboard.json
sed -i.bak 's/ingester_redis_stream_length{stream=\\"repo_backfill\\"}/ingester_repo_backfill_length/g' grafana-rsky-dashboard.json
sed -i.bak 's/ingester_redis_stream_length{stream=\\"label_live\\"}/ingester_label_live_length/g' grafana-rsky-dashboard.json

# Check what changed
diff grafana-rsky-dashboard.json.orig grafana-rsky-dashboard.json | head -50
```

---

## Validation Checklist

After applying fixes:

1. **Ingester Metrics** ✅
   - [ ] `ingester_firehose_backfill_length` shows 0
   - [ ] `ingester_firehose_live_length` shows 0
   - [ ] `ingester_repo_backfill_length` shows 177
   - [ ] `ingester_label_live_length` shows ~4700

2. **Backfiller Metrics** ✅
   - [ ] `backfiller_output_stream_length` shows 0
   - [ ] `backfiller_repos_waiting` shows 0
   - [ ] `backfiller_repos_running` shows 2
   - [ ] `backfiller_repos_processed_total` shows 27K+

3. **Indexer Metrics** ✅
   - [ ] All 6 indexers showing activity
   - [ ] Event throughput visible
   - [ ] No "No data" panels

---

## Bonus: All Available Metrics

**Ingester Metrics:**
```
ingester_backpressure_active
ingester_events_in_memory
ingester_firehose_backfill_length
ingester_firehose_create_events_total
ingester_firehose_delete_events_total
ingester_firehose_events_total
ingester_firehose_filtered_operations_total
ingester_firehose_live_length
ingester_firehose_update_events_total
ingester_label_live_length
ingester_repo_backfill_length
ingester_stream_events_total
ingester_websocket_connections
```

**Backfiller Metrics:**
```
backfiller_car_fetch_errors_total
backfiller_output_stream_length
backfiller_records_extracted_total
backfiller_records_filtered_total
backfiller_repos_failed_total
backfiller_repos_processed_total
backfiller_repos_running
backfiller_repos_waiting
backfiller_retries_attempted_total
```

Use these for reference when fixing dashboard queries!

---

## Summary

**What's Working:**
- ✅ All metrics infrastructure (Prometheus, scraping, services)
- ✅ All targets healthy ("up" status)
- ✅ Metrics accurate (match Redis ground truth)
- ✅ Backfill queue processed (was 40M, now 0)

**What Needs Fixing:**
- ❌ Dashboard JSON has outdated metric names
- ❌ Need to replace `ingester_redis_stream_length{stream="X"}` with `ingester_X_length`

**Estimated Fix Time:** 15 minutes (sed + upload + verify)
