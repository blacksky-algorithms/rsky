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
COPY rsky-spaces-parity/Cargo.toml rsky-spaces-parity/Cargo.toml
COPY rsky-syntax/Cargo.toml rsky-syntax/Cargo.toml
COPY rsky-video/Cargo.toml rsky-video/Cargo.toml
COPY rsky-wintermute/Cargo.toml rsky-wintermute/Cargo.toml

# Copy real source for library crates that rsky-space-host depends on
COPY rsky-common/src rsky-common/src
COPY rsky-crypto/src rsky-crypto/src
COPY rsky-identity/src rsky-identity/src
COPY rsky-lexicon/src rsky-lexicon/src
COPY rsky-oauth/src rsky-oauth/src
COPY rsky-space/src rsky-space/src
COPY rsky-syntax/src rsky-syntax/src

# Stub out the remaining workspace members so cargo can resolve the workspace
RUN mkdir -p \
    palomar-sync/src rsky-daemon/src rsky-feedgen/src rsky-firehose/src \
    rsky-jetstream-subscriber/src rsky-labeler/src rsky-pds/src \
    rsky-relay/src rsky-repo/src rsky-satnav/src rsky-video/src \
    rsky-wintermute/src && \
    for crate in palomar-sync rsky-daemon rsky-feedgen rsky-firehose \
        rsky-jetstream-subscriber rsky-labeler rsky-pds rsky-relay \
        rsky-satnav rsky-video rsky-wintermute; do \
        echo 'fn main() {}' > $crate/src/main.rs; \
    done && \
    touch rsky-pds/src/lib.rs rsky-repo/src/lib.rs && \
    mkdir -p rsky-spaces-parity/src && touch rsky-spaces-parity/src/lib.rs && \
    mkdir -p rsky-wintermute/src/bin && \
    for bin in queue_backfill fix_blob_refs plc_import label_sync car_loader \
        cleanup_stale_deactivated; do \
        echo 'fn main() {}' > rsky-wintermute/src/bin/$bin.rs; \
    done

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir -p rsky-space-host/src && echo "fn main() {}" > rsky-space-host/src/main.rs

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-space-host

# Now copy the real source code and build the final binary
COPY rsky-space-host/src rsky-space-host/src

RUN cargo build --release --package rsky-space-host

FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*
WORKDIR /usr/src/rsky
COPY --from=builder /usr/src/rsky/target/release/rsky-space-host rsky-space-host
LABEL org.opencontainers.image.source=https://github.com/blacksky-algorithms/rsky
# State is sqlite: mount a volume covering SPACEHOST_DB_PATH, and mount the PDS
# actor store read-only at SPACEHOST_ACTOR_STORE_DIR. Required env:
# SPACEHOST_PUBLIC_URL, SPACEHOST_MINT_TOKEN, and either the
# SPACEHOST_AUTHORITY_DID/SPACEHOST_SIGNING_KEY_HEX bootstrap pin or an actor
# store to acquire authorities from. See rsky-space-host/src/config.rs for the
# full reference.
CMD ["sh", "-c", "SPACEHOST_BIND=${SPACEHOST_BIND:-0.0.0.0:3600} ./rsky-space-host"]
