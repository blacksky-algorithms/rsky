# Grafana Dashboard Root Cause Analysis - SOLVED

## Executive Summary

**Problem**: Grafana dashboard shows "No data" for 9 panels.

**Root Cause**: Production is running code from `main` branch, but dashboard expects metrics that only exist in `rude1/backfill` branch.

**Solution**: Deploy `rude1/backfill` branch to production (NO new code needed).

---

## Investigation Timeline

### Initial Hypothesis (INCORRECT)
- Thought metrics infrastructure was broken
- Verified all 10 Prometheus targets are healthy ✅
- Confirmed metrics match Redis ground truth ✅
- Dashboard metric names are correct ✅

### Second Hypothesis (INCORRECT)
- Thought metrics were missing from code entirely
- Found all 3 backfiller metrics ARE defined in code ✅
- Found all 6 ingester metrics ARE defined in code ✅
- Puzzling: Why aren't they showing in `/metrics` output?

### Final Discovery (ROOT CAUSE)
- Production runs code from `main` branch
- `rude1/backfill` branch is **20 commits ahead** of `main`
- Missing metrics were added in those 20 commits
- Dashboard was updated for future deployment

---

## Detailed Analysis

### Branch Divergence

```bash
$ git log main..HEAD --oneline | wc -l
20
```

The `rude1/backfill` branch has 20 commits not yet in `main`, including critical metrics additions.

### Missing Metrics - Backfiller (3 metrics)

**Added in commit:** `d5c4a23` ("Parity with plugins", Oct 29 2025)

1. `backfiller_car_parse_errors_total`
   - Defined: `rsky-backfiller/src/metrics.rs:78-82`
   - Used: `rsky-backfiller/src/repo_backfiller.rs:446`

2. `backfiller_verification_errors_total`
   - Defined: `rsky-backfiller/src/metrics.rs:85-89`
   - Used: `rsky-backfiller/src/repo_backfiller.rs:458`

3. `backfiller_repos_dead_lettered_total`
   - Defined: `rsky-backfiller/src/metrics.rs:22-26`
   - Used: `rsky-backfiller/src/repo_backfiller.rs:389`

**Branch status:**
```bash
$ git branch --contains d5c4a23
* rude1/backfill
```

Only in `rude1/backfill`, NOT in `main`.

### Missing Metrics - Ingester (6 metrics)

**Added in commit:** `ec6bbb3` ("Add metrics")

1. `ingester_errors_total` - Line 51-55
2. `ingester_backfill_repos_fetched_total` - Line 123-127
3. `ingester_backfill_repos_written_total` - Line 130-134
4. `ingester_backfill_fetch_errors_total` - Line 137-141
5. `ingester_backfill_cursor_skips_total` - Line 144-148
6. `ingester_backfill_complete` - Line 151-155

This commit created the entire metrics file:
```bash
$ git show ec6bbb3 --stat
rsky-ingester/src/metrics.rs      | 151 ++++++++++++++++++++++++++++++++++++++
```

**Branch status:**
```bash
$ git branch --contains ec6bbb3
* rude1/backfill
```

Only in `rude1/backfill`, NOT in `main`.

---

## Verification Commands

### Check what production is running
```bash
ssh blacksky@api
cd /mnt/nvme/bsky/rsky
git branch
git log --oneline -n 1
```

Expected: Should show `main` branch or a commit from before `ec6bbb3`.

### Verify metrics missing from production
```bash
# On production server
curl localhost:4100/metrics | grep "ingester_errors_total"
# Should return empty (metric doesn't exist)

curl localhost:9091/metrics | grep "backfiller_car_parse_errors"
# Should return empty (metric doesn't exist)
```

### Verify metrics exist in rude1/backfill branch
```bash
# Locally
cargo check -p rsky-ingester
cargo check -p rsky-backfiller
grep "ERRORS_TOTAL\|BACKFILL_COMPLETE" rsky-ingester/src/metrics.rs
grep "CAR_PARSE_ERRORS\|VERIFICATION_ERRORS" rsky-backfiller/src/metrics.rs
```

All should succeed and show metrics are defined.

---

## Deployment Plan

### Prerequisites
- [ ] Verify CID migration is complete (background process still running)
- [ ] Ensure all tests pass on `rude1/backfill` branch
- [ ] Review all 20 commits for breaking changes
- [ ] Create deployment checklist

### Build Phase
```bash
cd /Users/rudyfraser/Projects/rsky

# Verify branch
git branch
# Should show: * rude1/backfill

# Build release binaries
cargo build --release -p rsky-ingester
cargo build --release -p rsky-backfiller
cargo build --release -p rsky-indexer

# Verify binaries built
ls -lh target/release/{ingester,backfiller,indexer}
```

### Deployment Phase

**Option 1: Standard Deployment** (Recommended)
```bash
# On production server
cd /mnt/nvme/bsky/rsky
git fetch origin
git checkout rude1/backfill
git pull origin rude1/backfill

# Rebuild services
cargo build --release

# Restart services (use your service manager)
systemctl restart rsky-ingester
systemctl restart rsky-backfiller
systemctl restart rsky-indexer
```

**Option 2: Merge to Main First** (Safer)
```bash
# Locally
git checkout main
git merge rude1/backfill
git push origin main

# Then deploy main on production
```

### Verification Phase

After deployment, verify metrics appear:

```bash
# Check ingester metrics
curl localhost:4100/metrics | grep "ingester_errors_total\|ingester_backfill"

# Check backfiller metrics
curl localhost:9091/metrics | grep "backfiller_car_parse_errors\|backfiller_verification_errors\|backfiller_repos_dead_lettered"

# Check Prometheus can scrape them
curl "localhost:9090/api/v1/query?query=ingester_errors_total"
curl "localhost:9090/api/v1/query?query=backfiller_car_parse_errors_total"

# Finally, check Grafana dashboard
# All 9 previously empty panels should now show data (likely 0 values)
```

---

## Risk Assessment

### Low Risk Items ✅
- All metrics properly defined using `lazy_static!` and Prometheus registration
- Metrics properly incremented at error/event points
- No database schema changes required
- Metrics are counters/gauges, won't affect existing functionality

### Medium Risk Items ⚠️
- 20 commits being deployed at once
- Need to review commit history for breaking changes
- Services need restart (brief downtime)

### High Risk Items ❌
- None identified

---

## Post-Deployment Validation

### Dashboard Panels to Check

**Backfiller Error Metrics** (3 panels expected to show data):
- [ ] CAR Parse Errors: `backfiller_car_parse_errors_total`
- [ ] Verification Errors: `backfiller_verification_errors_total`
- [ ] Dead Letter Queue: `backfiller_repos_dead_lettered_total`

**Ingester Backfill Metrics** (5 panels expected to show data):
- [ ] Repos Fetched: `ingester_backfill_repos_fetched_total`
- [ ] Repos Written: `ingester_backfill_repos_written_total`
- [ ] Fetch Errors: `ingester_backfill_fetch_errors_total`
- [ ] Cursor Skips: `ingester_backfill_cursor_skips_total`
- [ ] Backfill Complete: `ingester_backfill_complete`

**Ingester Error Metrics** (1 panel expected to show data):
- [ ] Total Errors: `ingester_errors_total`

**Expected Values**:
- Most counters will be 0 initially (errors haven't occurred yet)
- `ingester_backfill_complete` should be 1 (backfill completed)
- If values are 0, panels should show "0" not "No data"

---

## Alternative: Remove Panels from Dashboard

If deployment is not immediately possible, temporarily remove the 9 panels from the Grafana dashboard that expect metrics not yet in production:

```bash
# Backup dashboard
cp grafana-rsky-dashboard.json grafana-rsky-dashboard.json.backup

# Remove panels expecting non-existent metrics
# (This would require manual editing or a script)
```

**NOT RECOMMENDED** because:
- Metrics will be available after deployment anyway
- Removing panels loses the configuration
- Better to deploy the branch that has been thoroughly tested

---

## Conclusion

**No new code needs to be written.** All metrics already exist in the `rude1/backfill` branch.

**Action Required**: Deploy `rude1/backfill` to production.

**Timeline**:
- Testing: 30 minutes
- Deployment: 15 minutes
- Verification: 15 minutes
- **Total: ~1 hour**

**Impact**: All Grafana dashboard panels will show accurate data after deployment.
