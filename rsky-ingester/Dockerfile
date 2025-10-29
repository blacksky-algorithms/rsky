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
COPY rsky-ingester ./rsky-ingester
COPY rsky-lexicon ./rsky-lexicon
COPY rsky-syntax ./rsky-syntax
COPY rsky-identity ./rsky-identity
COPY rsky-crypto ./rsky-crypto
COPY rsky-common ./rsky-common

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
