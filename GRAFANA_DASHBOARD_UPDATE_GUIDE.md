# Grafana Dashboard Update Guide

This guide documents all missing metrics that need to be added to `grafana-rsky-dashboard.json`.

## Overview

**Total Missing**: 17 actively-used metrics across 3 crates
**Current Dashboard**: 2717 lines, organized into 4 sections:
1. 📥 Ingester Overview (line 34)
2. 🔥 Backfiller Overview (line 782)
3. ⚡ Indexer Overview (Stream) (line 1415)
4. 🏷️ Label Indexer (line 2298)

## Metrics to Add

### Section 1: 📥 Ingester Overview

#### 1.1 Add to "Redis Stream Lengths" Panel
**Location**: After line 769
**Current Metrics**: firehose_live, repo_backfill, label_live
**Add**: `ingester_firehose_backfill_length`

**Prometheus Query**:
```
ingester_firehose_backfill_length
```

**Legend**: `firehose_backfill`
**Priority**: ⚠️ **CRITICAL** - Just added to code, needed immediately

---

#### 1.2 New Panel: Ingester Errors
**Location**: After "Events In Memory" panel (line 527)
**Panel Type**: Stat (single value)
**Metrics to include**:
- `ingester_errors_total` - Total ingester errors

**Prometheus Query**:
```
sum(ingester_errors_total)
```

**Threshold Colors**:
- Green: 0
- Yellow: > 10
- Red: > 100

---

#### 1.3 New Panel Group: BackfillIngester Progress
**Location**: After Ingester Event Throughput (line 769)
**Create new row**: "📦 BackfillIngester (listRepos)"

**Panel 1: Repos Fetched (Counter)**
```
sum(ingester_backfill_repos_fetched_total)
```

**Panel 2: Repos Written (Counter)**
```
sum(ingester_backfill_repos_written_total)
```

**Panel 3: Backfill Complete (Gauge)**
```
sum(ingester_backfill_complete)
```
- Value 1 = Complete (green)
- Value 0 = In Progress (yellow)

**Panel 4: Fetch Error Rate**
```
rate(ingester_backfill_fetch_errors_total[5m])
```

**Panel 5: Cursor Skips**
```
sum(ingester_backfill_cursor_skips_total)
```
- Should be 0 in healthy operation

---

#### 1.4 New Panel Group: LabelIngester Activity
**Location**: Add to Label Indexer section (line 2298)

**Panel 1: Labeler Events Received**
```
sum(ingester_labeler_events_total)
```

**Panel 2: Labels Written to Stream**
```
sum(ingester_labels_written_total)
```

---

#### 1.5 New Panel: Filtered Operations
**Location**: After "Total Events Written" (line 527)

**Prometheus Query**:
```
sum(ingester_firehose_filtered_operations_total)
```

**Description**: Operations filtered out (non-app.bsky/chat.bsky collections)

---

### Section 2: 🔥 Backfiller Overview

#### 2.1 New Panel Group: Backpressure Monitoring
**Location**: After "Backfill Queue Length" (line 1196)
**Create new row**: "⚠️ Backpressure Indicators"

**Panel 1: Repos Waiting (Gauge)**
```
sum(backfiller_repos_waiting)
```
**Description**: Number of repos in input stream (repo_backfill)
**Thresholds**:
- Green: < 100K
- Yellow: 100K - 500K
- Red: > 500K

**Panel 2: Repos Running (Gauge)**
```
sum(backfiller_repos_running)
```
**Description**: Repos currently being processed
**Expected**: Should equal concurrency setting (10-50)

---

#### 2.2 New Panel Group: Error Tracking
**Location**: After "Failed Repos" (line 1062)
**Create new row**: "❌ Backfiller Errors"

**Panel 1: CAR Fetch Errors**
```
rate(backfiller_car_fetch_errors_total[5m])
```

**Panel 2: CAR Parse Errors**
```
rate(backfiller_car_parse_errors_total[5m])
```

**Panel 3: Verification Errors**
```
rate(backfiller_verification_errors_total[5m])
```

**Panel 4: Retry Attempts**
```
sum(backfiller_retries_attempted_total)
```

---

#### 2.3 New Panel Group: Quality Metrics
**Location**: After error panels

**Panel 1: Records Filtered**
```
sum(backfiller_records_filtered_total)
```
**Description**: Records filtered out (non-app.bsky/chat.bsky)

**Panel 2: Dead Letter Queue**
```
sum(backfiller_repos_dead_lettered_total)
```
**Description**: Repos that failed after max retries
**Threshold**: Should be 0 or very low (< 10)

---

## Quick Add Priority List

If you only have time to add a few metrics, prioritize these:

### Top Priority (Add Immediately)
1. ✅ `ingester_firehose_backfill_length` - **CRITICAL** for dashboard visibility
2. ✅ `backfiller_repos_waiting` - Backpressure monitoring
3. ✅ `backfiller_repos_running` - Backpressure monitoring

### High Priority (Add Within 24h)
4. `backfiller_car_fetch_errors_total` - Error visibility
5. `backfiller_car_parse_errors_total` - Error visibility
6. `ingester_backfill_repos_fetched_total` - Progress tracking
7. `ingester_backfill_repos_written_total` - Progress tracking
8. `ingester_backfill_complete` - Completion status

### Medium Priority (Add Within Week)
9-17. Remaining metrics (errors, filters, retries, dead letters)

---

## How to Add Panels in Grafana UI

### Method 1: Edit Dashboard JSON (Advanced)
1. Open Grafana dashboard
2. Click gear icon → "JSON Model"
3. Locate appropriate section by line number
4. Add panel JSON following existing patterns
5. Save and refresh

### Method 2: Add via UI (Recommended)
1. Open dashboard in edit mode
2. Click "Add" → "Visualization"
3. Select panel type (Stat, Time series, etc.)
4. Configure query using Prometheus expressions above
5. Set title, thresholds, and styling
6. Save dashboard

---

## Verification

After adding metrics, verify they appear with data:

```bash
# Check ingester metrics
curl -s http://localhost:4100/metrics | grep -E "(firehose_backfill_length|errors_total|backfill_repos)"

# Check backfiller metrics
curl -s http://localhost:9090/metrics | grep -E "(repos_waiting|repos_running|car_.*_errors)"

# Check indexer metrics
curl -s http://localhost:9090/metrics | grep -E "(labels_processed|expected_errors)"
```

All metrics should return non-zero values during active operation.

---

## Reference

- **Full Audit**: `METRICS_AUDIT.md`
- **Metrics Definitions**:
  - rsky-ingester: `rsky-ingester/src/metrics.rs`
  - rsky-indexer: `rsky-indexer/src/metrics.rs`
  - rsky-backfiller: `rsky-backfiller/src/metrics.rs`
- **Dashboard JSON**: `grafana-rsky-dashboard.json` (2717 lines)

---

## Automated Update Script (Future Enhancement)

Consider creating a Python script to programmatically add panels:

```python
import json

def add_panel_to_dashboard(dashboard_path, section, panel_config):
    with open(dashboard_path, 'r') as f:
        dashboard = json.load(f)

    # Find section by ID
    # Insert new panel
    # Update IDs
    # Save dashboard

    with open(dashboard_path, 'w') as f:
        json.dump(dashboard, f, indent=2)
```

This would allow bulk updates without manual JSON editing.
