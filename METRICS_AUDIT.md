# Metrics Audit Report

## Executive Summary

Comprehensive audit of all Prometheus metrics across rsky-ingester, rsky-indexer, and rsky-backfiller. This document identifies:
- Metrics defined in code
- Whether metrics are being updated
- Whether metrics are exposed via HTTP endpoints
- Whether metrics appear in Grafana dashboard

## Status: ✅ All metrics have working logic and HTTP endpoints

**Issue Found**: Some defined metrics are MISSING from the Grafana dashboard.

---

## rsky-ingester Metrics

### File: rsky-ingester/src/metrics.rs

| Metric Name | Type | Updated in Code | In Dashboard | Notes |
|------------|------|-----------------|--------------|-------|
| `ingester_firehose_events_total` | Counter | ✅ firehose.rs | ✅ | Event count from firehose |
| `ingester_firehose_create_events_total` | Counter | ✅ firehose.rs | ✅ | |
| `ingester_firehose_update_events_total` | Counter | ✅ firehose.rs | ✅ | |
| `ingester_firehose_delete_events_total` | Counter | ✅ firehose.rs | ✅ | |
| `ingester_firehose_filtered_operations_total` | Counter | ✅ firehose.rs | ❌ | **MISSING** |
| `ingester_stream_events_total` | Counter | ✅ firehose.rs | ✅ | |
| `ingester_errors_total` | Counter | ✅ firehose.rs | ❌ | **MISSING** |
| `ingester_firehose_live_length` | Gauge | ✅ firehose.rs | ✅ | |
| `ingester_firehose_backfill_length` | Gauge | ✅ firehose.rs | ❌ | **MISSING** (just added) |
| `ingester_label_live_length` | Gauge | ✅ firehose.rs | ✅ | |
| `ingester_repo_backfill_length` | Gauge | ✅ firehose.rs | ✅ | |
| `ingester_backpressure_active` | Gauge | ✅ firehose.rs | ✅ | |
| `ingester_websocket_connections` | Gauge | ✅ firehose.rs | ✅ | |
| `ingester_events_in_memory` | Gauge | ✅ firehose.rs | ✅ | |
| `ingester_labeler_events_total` | Counter | ✅ labeler.rs | ❌ | **MISSING** |
| `ingester_labels_written_total` | Counter | ✅ labeler.rs | ❌ | **MISSING** |
| `ingester_backfill_repos_fetched_total` | Counter | ✅ backfill.rs | ❌ | **MISSING** |
| `ingester_backfill_repos_written_total` | Counter | ✅ backfill.rs | ❌ | **MISSING** |
| `ingester_backfill_fetch_errors_total` | Counter | ✅ backfill.rs | ❌ | **MISSING** |
| `ingester_backfill_cursor_skips_total` | Counter | ✅ backfill.rs | ❌ | **MISSING** |
| `ingester_backfill_complete` | Gauge | ✅ backfill.rs | ❌ | **MISSING** |

**HTTP Endpoint**: ✅ Exposed on port 4100 (configurable)
**Update Location**: firehose.rs, labeler.rs, backfill.rs

---

## rsky-backfiller Metrics

### File: rsky-backfiller/src/metrics.rs

| Metric Name | Type | Updated in Code | In Dashboard | Notes |
|------------|------|-----------------|--------------|-------|
| `backfiller_repos_processed_total` | Counter | ✅ repo_backfiller.rs:331,340 | ✅ | |
| `backfiller_repos_failed_total` | Counter | ✅ repo_backfiller.rs:346 | ✅ | |
| `backfiller_repos_dead_lettered_total` | Counter | ✅ repo_backfiller.rs:354 | ❌ | **MISSING** |
| `backfiller_records_extracted_total` | Counter | ✅ repo_backfiller.rs:696 | ✅ | |
| `backfiller_records_filtered_total` | Counter | ✅ repo_backfiller.rs:673 | ❌ | **MISSING** |
| `backfiller_retries_attempted_total` | Counter | ✅ repo_backfiller.rs:374 | ❌ | **MISSING** |
| `backfiller_repos_waiting` | Gauge | ✅ repo_backfiller.rs:264 | ❌ | **MISSING** |
| `backfiller_repos_running` | Gauge | ✅ repo_backfiller.rs:323,327 | ❌ | **MISSING** |
| `backfiller_output_stream_length` | Gauge | ✅ repo_backfiller.rs:300 | ✅ | |
| `backfiller_car_fetch_errors_total` | Counter | ✅ repo_backfiller.rs:393,402 | ❌ | **MISSING** |
| `backfiller_car_parse_errors_total` | Counter | ✅ repo_backfiller.rs:411 | ❌ | **MISSING** |
| `backfiller_verification_errors_total` | Counter | ✅ repo_backfiller.rs:423 | ❌ | **MISSING** |

**HTTP Endpoint**: ✅ Exposed on port 9090 (configurable via BACKFILLER_METRICS_PORT)
**Update Location**: repo_backfiller.rs

---

## rsky-indexer Metrics

### File: rsky-indexer/src/metrics.rs

| Metric Name | Type | Updated in Code | In Dashboard | Notes |
|------------|------|-----------------|--------------|-------|
| `indexer_events_processed_total` | Counter | ✅ stream_indexer.rs:327 | ✅ | |
| `indexer_create_events_total` | Counter | ✅ stream_indexer.rs:328 | ✅ | |
| `indexer_update_events_total` | Counter | ✅ stream_indexer.rs:381 | ✅ | |
| `indexer_delete_events_total` | Counter | ✅ stream_indexer.rs:405 | ✅ | |
| `indexer_repo_events_total` | Counter | ❌ | ❌ | **NOT USED** |
| `indexer_account_events_total` | Counter | ❌ | ❌ | **NOT USED** |
| `indexer_identity_events_total` | Counter | ❌ | ❌ | **NOT USED** |
| `indexer_db_writes_total` | Counter | ✅ stream_indexer.rs:323,329,376,382,400,406,414,435,439 | ✅ | |
| `indexer_errors_total` | Counter | ❌ | ❌ | **NOT USED** |
| `indexer_expected_errors_total` | Counter | ✅ stream_indexer.rs:202 | ✅ | |
| `indexer_unexpected_errors_total` | Counter | ✅ stream_indexer.rs:208 | ✅ | |
| `indexer_post_events_total` | Counter | ✅ stream_indexer.rs:333 | ✅ | |
| `indexer_like_events_total` | Counter | ✅ stream_indexer.rs:335 | ✅ | |
| `indexer_repost_events_total` | Counter | ✅ stream_indexer.rs:337 | ✅ | |
| `indexer_follow_events_total` | Counter | ✅ stream_indexer.rs:339 | ✅ | |
| `indexer_block_events_total` | Counter | ✅ stream_indexer.rs:341 | ✅ | |
| `indexer_profile_events_total` | Counter | ✅ stream_indexer.rs:343 | ✅ | |
| `indexer_labels_processed_total` | Counter | ✅ label_indexer.rs:231 | ✅ | |
| `indexer_labels_added_total` | Counter | ✅ label_indexer.rs:226 | ✅ | |
| `indexer_labels_removed_total` | Counter | ✅ label_indexer.rs:196 | ✅ | |
| `indexer_pending_messages` | Gauge | ❌ | ❌ | **NOT USED** |
| `indexer_active_tasks` | Gauge | ❌ | ❌ | **NOT USED** |
| `indexer_ack_failures_total` | Counter | ❌ | ❌ | **NOT USED** |

**HTTP Endpoint**: ✅ Exposed on port 9090 (configurable via METRICS_PORT)
**Update Location**: stream_indexer.rs, label_indexer.rs

---

## Summary

### ✅ Working Correctly
- All 3 crates have HTTP metrics endpoints exposed
- All used metrics have working update logic in the code
- Core metrics are tracked in Grafana dashboard

### ❌ Issues Found

**1. Missing from Grafana Dashboard** (25 metrics):

**Ingester** (10 missing):
- `ingester_firehose_filtered_operations_total`
- `ingester_errors_total`
- `ingester_firehose_backfill_length` ⚠️ **HIGH PRIORITY** (just fixed in code)
- `ingester_labeler_events_total`
- `ingester_labels_written_total`
- `ingester_backfill_repos_fetched_total`
- `ingester_backfill_repos_written_total`
- `ingester_backfill_fetch_errors_total`
- `ingester_backfill_cursor_skips_total`
- `ingester_backfill_complete`

**Backfiller** (7 missing):
- `backfiller_repos_dead_lettered_total`
- `backfiller_records_filtered_total`
- `backfiller_retries_attempted_total`
- `backfiller_repos_waiting`
- `backfiller_repos_running`
- `backfiller_car_fetch_errors_total`
- `backfiller_car_parse_errors_total`
- `backfiller_verification_errors_total`

**Indexer** (3 unused, 5 missing but unused):
- Unused metrics should be removed or implemented:
  - `indexer_repo_events_total`
  - `indexer_account_events_total`
  - `indexer_identity_events_total`
  - `indexer_errors_total`
  - `indexer_pending_messages`
  - `indexer_active_tasks`
  - `indexer_ack_failures_total`

---

## Recommendations

### Phase 1: Add Missing Metrics to Grafana (HIGHEST PRIORITY)
Update `grafana-rsky-dashboard.json` to include panels for all 25 missing metrics, prioritizing:
1. `ingester_firehose_backfill_length` (just fixed in code, needed for dashboard)
2. `backfiller_repos_waiting` and `backfiller_repos_running` (backpressure monitoring)
3. Error metrics (all _errors_total counters)
4. BackfillIngester progress metrics (repos_fetched, repos_written, backfill_complete)

### Phase 2: Implement or Remove Unused Metrics
- Either implement update logic for unused indexer metrics
- OR remove them from metrics.rs to reduce clutter

### Phase 3: Add Missing Event Type Counters
Stream events for Account, Identity, and Repo types are processed but not counted:
- Implement `indexer_repo_events_total` (StreamEvent::Repo)
- Implement `indexer_account_events_total` (StreamEvent::Account)
- Implement `indexer_identity_events_total` (StreamEvent::Identity)

---

## Testing Verification

To verify metrics are working:

```bash
# Ingester
curl -s http://localhost:4100/metrics | grep ingester_

# Backfiller
curl -s http://localhost:9090/metrics | grep backfiller_

# Indexer
curl -s http://localhost:9090/metrics | grep indexer_
```

All metrics listed as "✅ Updated in Code" should appear in curl output with non-zero values during operation.
