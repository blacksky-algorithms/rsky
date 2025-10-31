# Prometheus Monitoring Setup for rsky Services

## Overview

This guide sets up complete Prometheus metrics collection for all Rust services (ingester, backfiller, indexer) with Grafana dashboards.

## Current Status

- ✅ **Backfiller**: Has Prometheus metrics implemented
- ❌ **Ingester**: Needs metrics implementation
- ❌ **Indexer**: Needs metrics implementation
- ❌ **Prometheus**: Needs configuration file
- ❌ **Grafana Dashboard**: Needs update for Rust metrics

## Architecture

```
┌────────────┐   ┌────────────┐   ┌────────────┐
│  Ingester  │   │ Backfiller │   │  Indexer   │
│  :4100     │   │  :9090     │   │  :9090     │
└─────┬──────┘   └─────┬──────┘   └─────┬──────┘
      │                │                │
      │ /metrics       │ /metrics       │ /metrics
      └────────┬───────┴────────┬───────┘
               │                │
               ▼                ▼
         ┌─────────────────────────┐
         │     Prometheus          │
         │       :9090             │
         └───────────┬─────────────┘
                     │
                     ▼
         ┌─────────────────────────┐
         │       Grafana           │
         │       :3001             │
         └─────────────────────────┘
```

## Implementation Steps

### Step 1: Add Metrics Dependencies

#### rsky-ingester/Cargo.toml
Add these dependencies:
```toml
# Metrics (add to end of [dependencies])
prometheus = "0.13"
lazy_static = "1.5"
warp = "0.3"
```

#### rsky-indexer/Cargo.toml
Add these dependencies:
```toml
# Metrics (add to end of [dependencies])
prometheus = "0.13"
lazy_static = "1.5"
warp = "0.3"
```

### Step 2: Create Metrics Modules

#### rsky-ingester/src/metrics.rs
Create this new file with Prometheus metrics for the ingester:

```rust
use lazy_static::lazy_static;
use prometheus::{
    register_int_counter, register_int_gauge, Encoder, IntCounter, IntGauge, TextEncoder,
};

lazy_static! {
    /// Total incoming events from firehose
    pub static ref FIREHOSE_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "ingester_firehose_events_total",
        "Total events received from firehose"
    )
    .unwrap();

    /// Total incoming events from labeler
    pub static ref LABELER_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "ingester_labeler_events_total",
        "Total events received from labeler"
    )
    .unwrap();

    /// Total events written to Redis streams
    pub static ref STREAM_EVENTS_TOTAL: IntCounter = register_int_counter!(
        "ingester_stream_events_total",
        "Total events written to Redis streams"
    )
    .unwrap();

    /// Total errors encountered
    pub static ref ERRORS_TOTAL: IntCounter = register_int_counter!(
        "ingester_errors_total",
        "Total errors encountered"
    )
    .unwrap();

    /// Current firehose_live stream length
    pub static ref FIREHOSE_LIVE_LENGTH: IntGauge = register_int_gauge!(
        "ingester_firehose_live_length",
        "Current length of firehose_live stream"
    )
    .unwrap();

    /// Current label_live stream length
    pub static ref LABEL_LIVE_LENGTH: IntGauge = register_int_gauge!(
        "ingester_label_live_length",
        "Current length of label_live stream"
    )
    .unwrap();

    /// Backpressure active (1 = yes, 0 = no)
    pub static ref BACKPRESSURE_ACTIVE: IntGauge = register_int_gauge!(
        "ingester_backpressure_active",
        "Whether backpressure is currently active (1=yes, 0=no)"
    )
    .unwrap();

    /// WebSocket connections active
    pub static ref WEBSOCKET_CONNECTIONS: IntGauge = register_int_gauge!(
        "ingester_websocket_connections",
        "Number of active WebSocket connections"
    )
    .unwrap();
}

/// Encode metrics for Prometheus scraping
pub fn encode_metrics() -> Result<String, Box<dyn std::error::Error>> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
}
```

#### rsky-indexer/src/metrics.rs
Create this new file with Prometheus metrics for the indexer:

```rust
use lazy_static::lazy_static;
use prometheus::{
    register_int_counter, register_int_gauge, Encoder, IntCounter, IntGauge, TextEncoder,
};

lazy_static! {
    /// Total events processed
    pub static ref EVENTS_PROCESSED: IntCounter = register_int_counter!(
        "indexer_events_processed_total",
        "Total events processed"
    )
    .unwrap();

    /// Total events by operation type
    pub static ref CREATE_EVENTS: IntCounter = register_int_counter!(
        "indexer_create_events_total",
        "Total create events processed"
    )
    .unwrap();

    pub static ref UPDATE_EVENTS: IntCounter = register_int_counter!(
        "indexer_update_events_total",
        "Total update events processed"
    )
    .unwrap();

    pub static ref DELETE_EVENTS: IntCounter = register_int_counter!(
        "indexer_delete_events_total",
        "Total delete events processed"
    )
    .unwrap();

    /// Total database writes
    pub static ref DB_WRITES_TOTAL: IntCounter = register_int_counter!(
        "indexer_db_writes_total",
        "Total writes to PostgreSQL"
    )
    .unwrap();

    /// Total errors
    pub static ref ERRORS_TOTAL: IntCounter = register_int_counter!(
        "indexer_errors_total",
        "Total errors encountered"
    )
    .unwrap();

    /// Events by collection type
    pub static ref POST_EVENTS: IntCounter = register_int_counter!(
        "indexer_post_events_total",
        "Total post events processed"
    )
    .unwrap();

    pub static ref LIKE_EVENTS: IntCounter = register_int_counter!(
        "indexer_like_events_total",
        "Total like events processed"
    )
    .unwrap();

    pub static ref REPOST_EVENTS: IntCounter = register_int_counter!(
        "indexer_repost_events_total",
        "Total repost events processed"
    )
    .unwrap();

    pub static ref FOLLOW_EVENTS: IntCounter = register_int_counter!(
        "indexer_follow_events_total",
        "Total follow events processed"
    )
    .unwrap();

    /// Current pending message count
    pub static ref PENDING_MESSAGES: IntGauge = register_int_gauge!(
        "indexer_pending_messages",
        "Number of pending messages in consumer group"
    )
    .unwrap();

    /// Active concurrent tasks
    pub static ref ACTIVE_TASKS: IntGauge = register_int_gauge!(
        "indexer_active_tasks",
        "Number of actively processing tasks"
    )
    .unwrap();
}

/// Encode metrics for Prometheus scraping
pub fn encode_metrics() -> Result<String, Box<dyn std::error::Error>> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
}
```

### Step 3: Add Metrics HTTP Endpoints

For each service (ingester, indexer), you need to add a metrics HTTP server. The backfiller already has this implemented. Here's the pattern:

#### In main.rs for ingester and indexer:

```rust
mod metrics;  // Add this at top

// In main() function, spawn metrics server:
let metrics_port = std::env::var("METRICS_PORT")
    .unwrap_or_else(|_| "4100".to_string())  // 4100 for ingester, 9090 for indexer
    .parse::<u16>()
    .expect("METRICS_PORT must be a valid port number");

// Spawn metrics server
tokio::spawn(async move {
    let metrics_route = warp::path!("metrics").map(|| {
        match metrics::encode_metrics() {
            Ok(metrics) => warp::reply::with_status(metrics, warp::http::StatusCode::OK),
            Err(e) => {
                error!("Failed to encode metrics: {:?}", e);
                warp::reply::with_status(
                    format!("Error: {}", e),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
            }
        }
    });

    info!("Metrics server starting on port {}", metrics_port);
    warp::serve(metrics_route)
        .run(([0, 0, 0, 0], metrics_port))
        .await;
});
```

### Step 4: Instrument Code with Metrics

Throughout the code, increment metrics at key points:

#### Ingester example:
```rust
use crate::metrics;

// When receiving firehose event:
metrics::FIREHOSE_EVENTS_TOTAL.inc();

// When writing to stream:
metrics::STREAM_EVENTS_TOTAL.inc_by(batch.len() as u64);

// When backpressure triggers:
metrics::BACKPRESSURE_ACTIVE.set(1);

// When backpressure clears:
metrics::BACKPRESSURE_ACTIVE.set(0);
```

#### Indexer example:
```rust
use crate::metrics;

// When processing event:
metrics::EVENTS_PROCESSED.inc();

// For specific operations:
match event_type {
    "create" => metrics::CREATE_EVENTS.inc(),
    "update" => metrics::UPDATE_EVENTS.inc(),
    "delete" => metrics::DELETE_EVENTS.inc(),
    _ => {}
}

// For collections:
if collection == "app.bsky.feed.post" {
    metrics::POST_EVENTS.inc();
}

// When writing to DB:
metrics::DB_WRITES_TOTAL.inc();
```

### Step 5: Deploy Prometheus Configuration

Copy prometheus.yml to production server:

```bash
# On your local machine:
scp ~/Projects/rsky/prometheus.yml blacksky@api:/mnt/nvme/bsky/atproto/

# On production server:
sudo chown 65534:65534 /mnt/nvme/bsky/atproto/prometheus.yml
```

### Step 6: Update docker-compose.prod-rust.yml

Update the Prometheus service to mount the config:

```yaml
  prometheus:
    image: prom/prometheus:latest
    container_name: prometheus
    restart: unless-stopped
    networks:
      - backfill-net
    ports:
      - "9090:9090"
    volumes:
      - /data/prometheus:/prometheus
      - /mnt/nvme/bsky/atproto/prometheus.yml:/etc/prometheus/prometheus.yml:ro  # ADD THIS LINE
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
    mem_limit: 2g
```

Also add metrics ports to ingester and indexers:

```yaml
  ingester:
    ports:
      - "4100:4100"  # Metrics port (already exists)
    environment:
      METRICS_PORT: "4100"  # ADD THIS

  indexer1:
    ports:
      - "9093:9090"  # ADD THIS (metrics port)
    environment:
      METRICS_PORT: "9090"  # ADD THIS

  indexer2:
    ports:
      - "9094:9090"  # ADD THIS
    environment:
      METRICS_PORT: "9090"  # ADD THIS

  # ... repeat for indexer3-6 with ports 9095-9098
```

### Step 7: Build and Deploy

```bash
# On local machine (from ~/Projects/rsky):
cargo build --release --bin ingester
cargo build --release --bin indexer
cargo build --release --bin backfiller

# Build Docker images:
docker build -t rsky-ingester:latest -f rsky-ingester/Dockerfile .
docker build -t rsky-indexer:latest -f rsky-indexer/Dockerfile .
docker build -t rsky-backfiller:latest -f rsky-backfiller/Dockerfile .

# Save and transfer to production:
docker save rsky-ingester:latest | gzip > rsky-ingester.tar.gz
docker save rsky-indexer:latest | gzip > rsky-indexer.tar.gz
docker save rsky-backfiller:latest | gzip > rsky-backfiller.tar.gz

scp rsky-*.tar.gz blacksky@api:/tmp/

# On production server:
cd /tmp
docker load < rsky-ingester.tar.gz
docker load < rsky-indexer.tar.gz
docker load < rsky-backfiller.tar.gz

# Restart services:
cd /mnt/nvme/bsky/atproto
docker compose -f docker-compose.prod-rust.yml restart prometheus
docker compose -f docker-compose.prod-rust.yml restart ingester
docker compose -f docker-compose.prod-rust.yml restart indexer1 indexer2 indexer3 indexer4 indexer5 indexer6
docker compose -f docker-compose.prod-rust.yml restart backfiller1 backfiller2
```

### Step 8: Verify Metrics Collection

```bash
# Check if Prometheus is scraping targets:
curl http://localhost:9090/api/v1/targets | jq

# Check ingester metrics:
curl http://localhost:4100/metrics

# Check backfiller metrics:
curl http://localhost:9091/metrics
curl http://localhost:9092/metrics

# Check indexer metrics:
curl http://localhost:9093/metrics
curl http://localhost:9094/metrics
# ... etc for all indexers
```

### Step 9: Update Grafana Dashboard

The provided dashboard expects these key metrics. Update queries to use Rust metrics:

**Old (Node.js)**:
- `nodejs_eventloop_lag_mean_seconds`
- `nodejs_heap_size_used_bytes`
- `firehose_ingester_incoming_events_total`

**New (Rust)**:
- `ingester_firehose_events_total`
- `ingester_stream_events_total`
- `backfiller_repos_processed_total`
- `indexer_events_processed_total`
- `indexer_db_writes_total`

See GRAFANA_DASHBOARD.json for the complete updated dashboard.

## Key Metrics to Monitor

### Ingester
- `ingester_firehose_events_total`: Events from firehose
- `ingester_stream_events_total`: Events written to Redis
- `ingester_backpressure_active`: Backpressure status
- `ingester_firehose_live_length`: Redis stream length

### Backfiller
- `backfiller_repos_processed_total`: Total repos processed
- `backfiller_repos_failed_total`: Failed repos
- `backfiller_records_extracted_total`: Records from repos
- `backfiller_output_stream_length`: firehose_backfill length

### Indexer
- `indexer_events_processed_total`: Total events indexed
- `indexer_db_writes_total`: PostgreSQL writes
- `indexer_post_events_total`: Posts indexed
- `indexer_pending_messages`: Queue backlog

## Troubleshooting

### No data in Prometheus
1. Check targets: http://localhost:9090/targets
2. Verify metrics endpoints respond: `curl http://container:port/metrics`
3. Check Prometheus logs: `docker logs prometheus`

### Metrics not updating
1. Verify metrics are being incremented in code
2. Check service logs for errors
3. Restart the service

### Grafana shows "No Data"
1. Verify Prometheus data source is configured
2. Check metric names match dashboard queries
3. Verify time range is appropriate
