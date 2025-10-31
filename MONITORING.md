# Prometheus Monitoring for rsky Services

## Quick Reference

- **Setup Guide**: See [PROMETHEUS_SETUP.md](./PROMETHEUS_SETUP.md) for complete implementation details
- **Config File**: [prometheus.yml](./prometheus.yml)
- **Grafana Dashboard**: Import the JSON from your existing dashboard and update queries

## Current Status

✅ **Backfiller**: Metrics implemented and exposed on port 9090
✅ **Ingester**: Metrics implemented and exposed on port 4100
✅ **Indexer**: Metrics implemented and exposed on port 9090
✅ **Prometheus**: Config file deployed to production
⏳ **Grafana**: Dashboard needs update for ingester and indexer metrics

## Quick Deployment Checklist

On production server (`blacksky@api`):

### 1. Deploy Prometheus Config
```bash
cd /mnt/nvme/bsky/atproto
# Copy prometheus.yml from rsky repo to here
```

### 2. Update docker-compose.prod-rust.yml
Add volume mount to prometheus service:
```yaml
volumes:
  - /mnt/nvme/bsky/atproto/prometheus.yml:/etc/prometheus/prometheus.yml:ro
```

### 3. Restart Prometheus
```bash
docker compose -f docker-compose.prod-rust.yml restart prometheus
```

### 4. Verify Metrics Collection (Backfiller Only)
```bash
# Check Prometheus targets
curl http://localhost:9090/api/v1/targets | jq '.data.activeTargets[] | {job: .labels.job, health: .health}'

# Check backfiller metrics directly
curl http://localhost:9091/metrics | grep backfiller
curl http://localhost:9092/metrics | grep backfiller
```

Expected output:
```
backfiller_repos_processed_total 1234
backfiller_repos_failed_total 5
backfiller_records_extracted_total 567890
backfiller_output_stream_length 150000
```

## Available Metrics (Current)

### Backfiller Metrics (Port 9090)

| Metric | Type | Description |
|--------|------|-------------|
| `backfiller_repos_processed_total` | Counter | Total repos successfully processed |
| `backfiller_repos_failed_total` | Counter | Total repos that failed processing |
| `backfiller_repos_dead_lettered_total` | Counter | Repos sent to dead letter queue |
| `backfiller_records_extracted_total` | Counter | Total records extracted from repos |
| `backfiller_retries_attempted_total` | Counter | Total retry attempts |
| `backfiller_repos_waiting` | Gauge | Repos waiting in input stream |
| `backfiller_repos_running` | Gauge | Repos actively being processed |
| `backfiller_output_stream_length` | Gauge | Length of output stream (backpressure indicator) |
| `backfiller_car_fetch_errors_total` | Counter | CAR fetch errors |
| `backfiller_car_parse_errors_total` | Counter | CAR parse errors |
| `backfiller_verification_errors_total` | Counter | Repo verification errors |

### Planned Metrics (Ingester - Port 4100)

| Metric | Type | Description |
|--------|------|-------------|
| `ingester_firehose_events_total` | Counter | Events from firehose |
| `ingester_labeler_events_total` | Counter | Events from labeler |
| `ingester_stream_events_total` | Counter | Events written to Redis |
| `ingester_errors_total` | Counter | Total errors |
| `ingester_firehose_live_length` | Gauge | firehose_live stream length |
| `ingester_label_live_length` | Gauge | label_live stream length |
| `ingester_backpressure_active` | Gauge | Backpressure status (1=active, 0=inactive) |
| `ingester_websocket_connections` | Gauge | Active WebSocket connections |

### Planned Metrics (Indexer - Port 9090)

| Metric | Type | Description |
|--------|------|-------------|
| `indexer_events_processed_total` | Counter | Total events processed |
| `indexer_create_events_total` | Counter | Create events |
| `indexer_update_events_total` | Counter | Update events |
| `indexer_delete_events_total` | Counter | Delete events |
| `indexer_db_writes_total` | Counter | PostgreSQL writes |
| `indexer_errors_total` | Counter | Total errors |
| `indexer_post_events_total` | Counter | Posts indexed |
| `indexer_like_events_total` | Counter | Likes indexed |
| `indexer_repost_events_total` | Counter | Reposts indexed |
| `indexer_follow_events_total` | Counter | Follows indexed |
| `indexer_pending_messages` | Gauge | Pending messages in consumer group |
| `indexer_active_tasks` | Gauge | Active concurrent tasks |

## Grafana Dashboard Queries

### Backfill Throughput (works now)
```promql
rate(backfiller_repos_processed_total[5m])
```

### Records Extraction Rate (works now)
```promql
rate(backfiller_records_extracted_total[5m])
```

### Backfill Queue Length (works now)
```promql
sum(backfiller_output_stream_length)
```

### Backfiller Error Rate (works now)
```promql
rate(backfiller_repos_failed_total[5m])
```

### Ingester Throughput (after implementation)
```promql
rate(ingester_firehose_events_total[5m])
```

### Indexer Throughput (after implementation)
```promql
sum(rate(indexer_events_processed_total[5m]))
```

### Database Write Rate (after implementation)
```promql
sum(rate(indexer_db_writes_total[5m]))
```

## Monitoring Dashboards

### System Health Dashboard

1. **Service Uptime**
   - Show `up{job="rust-ingester"}`, `up{job="rust-backfiller"}`, `up{job="rust-indexer"}`

2. **Event Throughput**
   - Ingester: Events/sec received
   - Backfiller: Repos/sec processed
   - Indexer: Events/sec indexed

3. **Queue Lengths**
   - repo_backfill stream length
   - firehose_backfill stream length
   - firehose_live stream length
   - Consumer group pending counts

4. **Error Rates**
   - Errors per service
   - Failed repos
   - Database errors

### Performance Dashboard

1. **Latency Metrics**
   - Processing time per repo (backfiller)
   - Batch processing time (indexer)

2. **Resource Usage**
   - Memory usage per service
   - CPU usage
   - Database connection pool usage

3. **Backpressure Indicators**
   - Stream lengths approaching high water marks
   - Backpressure activation frequency

## Alerting Rules (Future)

Example alert definitions for Prometheus alertmanager:

```yaml
groups:
  - name: rsky_alerts
    rules:
      - alert: BackfillerHighErrorRate
        expr: rate(backfiller_repos_failed_total[5m]) > 10
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Backfiller error rate is high"

      - alert: IndexerConsumerGroupLag
        expr: indexer_pending_messages > 100000
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Indexer falling behind"

      - alert: IngesterBackpressure
        expr: ingester_backpressure_active == 1
        for: 5m
        labels:
          severity: info
        annotations:
          summary: "Ingester experiencing backpressure"
```

## Troubleshooting

### Prometheus shows "DOWN" for targets

1. Check service is running:
   ```bash
   docker ps | grep rust-ingester
   ```

2. Check metrics endpoint responds:
   ```bash
   docker exec rust-ingester curl http://localhost:4100/metrics
   ```

3. Check network connectivity:
   ```bash
   docker exec prometheus wget -O- http://ingester:4100/metrics
   ```

### Metrics not updating

1. Verify metrics are being incremented in code
2. Check service logs for errors
3. Restart the service

### Grafana shows "No Data"

1. Verify Prometheus data source is configured correctly
2. Check metric names match dashboard queries
3. Verify time range includes data
4. Check Prometheus has scraped the target recently

## Next Steps

1. ✅ Deploy prometheus.yml to production
2. ⏳ Implement ingester metrics (see PROMETHEUS_SETUP.md)
3. ⏳ Implement indexer metrics (see PROMETHEUS_SETUP.md)
4. ⏳ Update Grafana dashboard with Rust metrics
5. ⏳ Set up alerting rules
6. ⏳ Create runbooks for common alerts

## Related Documentation

- [PROMETHEUS_SETUP.md](./PROMETHEUS_SETUP.md) - Complete implementation guide
- [prometheus.yml](./prometheus.yml) - Prometheus configuration
- [docker-compose.prod-rust.yml](./docker-compose.prod-rust.yml) - Service configuration
- [CLAUDE.md](./CLAUDE.md) - Production deployment guide
