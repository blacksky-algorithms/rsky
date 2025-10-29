# Multi-stage build for rsky-ingester
FROM rust:1.83-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY rsky-common ./rsky-common
COPY rsky-crypto ./rsky-crypto
COPY rsky-feedgen ./rsky-feedgen
COPY rsky-firehose ./rsky-firehose
COPY rsky-identity ./rsky-identity
COPY rsky-jetstream-subscriber ./rsky-jetstream-subscriber
COPY rsky-labeler ./rsky-labeler
COPY rsky-lexicon ./rsky-lexicon
COPY rsky-pds ./rsky-pds
COPY rsky-relay ./rsky-relay
COPY rsky-repo ./rsky-repo
COPY rsky-satnav ./rsky-satnav
COPY rsky-syntax ./rsky-syntax
COPY rsky-ingester ./rsky-ingester
COPY rsky-indexer ./rsky-indexer
COPY rsky-backfiller ./rsky-backfiller
COPY rsky-integration-tests ./rsky-integration-tests

# Build release binary
RUN cargo build --release -p rsky-ingester

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/ingester /usr/local/bin/rsky-ingester

# Create non-root user
RUN useradd -m -u 1000 rsky && chown -R rsky:rsky /usr/local/bin/rsky-ingester

USER rsky

ENTRYPOINT ["/usr/local/bin/rsky-ingester"]
