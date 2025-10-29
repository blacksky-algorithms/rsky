# Project Context
Building Rust implementations of Bluesky AppView services based on:
- Source: https://github.com/bluesky-social/atproto/tree/divy/backfill
- Reference: rsky-relay and rsky-firehose patterns

## Architecture
- Redis streams for event processing
- PostgreSQL for data storage
- Consumer groups for distributed processing
- Prometheus metrics

## References
- rsky-relay: Event ingestion patterns
- rsky-firehose: Firehose subscription handling
- Use tokio for async runtime
- Use redis crate with streams support
