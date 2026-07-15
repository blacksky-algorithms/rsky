# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust AS builder

WORKDIR /usr/src/rsky

# Copy workspace and all crate manifests for dependency resolution
COPY Cargo.toml Cargo.lock rust-toolchain ./
COPY palomar-sync/Cargo.toml palomar-sync/Cargo.toml
COPY rsky-common/Cargo.toml rsky-common/Cargo.toml
COPY rsky-crypto/Cargo.toml rsky-crypto/Cargo.toml
COPY rsky-daemon/Cargo.toml rsky-daemon/Cargo.toml
COPY rsky-feedgen/Cargo.toml rsky-feedgen/Cargo.toml
COPY rsky-firehose/Cargo.toml rsky-firehose/Cargo.toml
COPY rsky-identity/Cargo.toml rsky-identity/Cargo.toml
COPY rsky-jetstream-subscriber/Cargo.toml rsky-jetstream-subscriber/Cargo.toml
COPY rsky-labeler/Cargo.toml rsky-labeler/Cargo.toml
COPY rsky-lexicon/Cargo.toml rsky-lexicon/Cargo.toml
COPY rsky-oauth/Cargo.toml rsky-oauth/Cargo.toml
COPY rsky-pds/Cargo.toml rsky-pds/Cargo.toml
COPY rsky-relay/Cargo.toml rsky-relay/Cargo.toml
COPY rsky-repo/Cargo.toml rsky-repo/Cargo.toml
COPY rsky-satnav/Cargo.toml rsky-satnav/Cargo.toml
COPY rsky-space/Cargo.toml rsky-space/Cargo.toml
COPY rsky-space-host/Cargo.toml rsky-space-host/Cargo.toml
COPY rsky-syntax/Cargo.toml rsky-syntax/Cargo.toml
COPY rsky-video/Cargo.toml rsky-video/Cargo.toml
COPY rsky-wintermute/Cargo.toml rsky-wintermute/Cargo.toml

# Copy real source for library crates that rsky-pds depends on
COPY rsky-common/src rsky-common/src
COPY rsky-crypto/src rsky-crypto/src
COPY rsky-identity/src rsky-identity/src
COPY rsky-lexicon/src rsky-lexicon/src
COPY rsky-oauth/src rsky-oauth/src
COPY rsky-repo/src rsky-repo/src
COPY rsky-syntax/src rsky-syntax/src

# Stub out binary workspace members so cargo can resolve the workspace
RUN mkdir -p \
    palomar-sync/src rsky-daemon/src rsky-feedgen/src rsky-firehose/src \
    rsky-jetstream-subscriber/src rsky-labeler/src rsky-relay/src \
    rsky-satnav/src rsky-space/src rsky-space-host/src rsky-video/src \
    rsky-wintermute/src && \
    for crate in palomar-sync rsky-daemon rsky-feedgen rsky-firehose \
        rsky-jetstream-subscriber rsky-labeler rsky-relay rsky-satnav \
        rsky-space-host rsky-video rsky-wintermute; do \
        echo 'fn main() {}' > $crate/src/main.rs; \
    done && \
    mkdir -p rsky-wintermute/src/bin && \
    for bin in queue_backfill fix_blob_refs plc_import label_sync car_loader \
        cleanup_stale_deactivated; do \
        echo 'fn main() {}' > rsky-wintermute/src/bin/$bin.rs; \
    done && \
    touch rsky-space/src/lib.rs

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir -p rsky-pds/src && echo "fn main() {}" > rsky-pds/src/main.rs

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-pds

# Now copy the real source code and build the final binary
COPY rsky-pds/src rsky-pds/src
COPY rsky-pds/templates rsky-pds/templates

RUN cargo build --release --package rsky-pds

FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*
WORKDIR /usr/src/rsky
COPY --from=builder /usr/src/rsky/target/release/rsky-pds rsky-pds
LABEL org.opencontainers.image.source=https://github.com/blacksky-algorithms/rsky
# Storage is sqlite + local disk (or S3): mount a volume at PDS_DATA_DIRECTORY.
# Required env: PDS_DATA_DIRECTORY, PDS_HOSTNAME, PDS_SERVICE_DID,
# PDS_JWT_KEY_K256_PRIVATE_KEY_HEX, PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX,
# PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX, PDS_DPOP_SECRET, PDS_ADMIN_PASS.
# See rsky-pds/README.md for the full reference.
CMD ["sh", "-c", "ROCKET_ADDRESS=0.0.0.0 ROCKET_PORT=${PORT:-2583} ./rsky-pds"]
