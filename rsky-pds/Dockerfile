# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust AS builder

WORKDIR /usr/src/rsky

# Copy workspace and all crate manifests for dependency resolution
COPY Cargo.toml Cargo.lock ./
COPY palomar-sync/Cargo.toml palomar-sync/Cargo.toml
COPY rsky-common/Cargo.toml rsky-common/Cargo.toml
COPY rsky-crypto/Cargo.toml rsky-crypto/Cargo.toml
COPY rsky-feedgen/Cargo.toml rsky-feedgen/Cargo.toml
COPY rsky-firehose/Cargo.toml rsky-firehose/Cargo.toml
COPY rsky-identity/Cargo.toml rsky-identity/Cargo.toml
COPY rsky-jetstream-subscriber/Cargo.toml rsky-jetstream-subscriber/Cargo.toml
COPY rsky-labeler/Cargo.toml rsky-labeler/Cargo.toml
COPY rsky-lexicon/Cargo.toml rsky-lexicon/Cargo.toml
COPY rsky-pds/Cargo.toml rsky-pds/Cargo.toml
COPY rsky-relay/Cargo.toml rsky-relay/Cargo.toml
COPY rsky-repo/Cargo.toml rsky-repo/Cargo.toml
COPY rsky-satnav/Cargo.toml rsky-satnav/Cargo.toml
COPY rsky-syntax/Cargo.toml rsky-syntax/Cargo.toml
COPY rsky-video/Cargo.toml rsky-video/Cargo.toml
COPY rsky-wintermute/Cargo.toml rsky-wintermute/Cargo.toml

# Copy real source for library crates that rsky-pds depends on
COPY rsky-common/src rsky-common/src
COPY rsky-crypto/src rsky-crypto/src
COPY rsky-identity/src rsky-identity/src
COPY rsky-lexicon/src rsky-lexicon/src
COPY rsky-repo/src rsky-repo/src
COPY rsky-syntax/src rsky-syntax/src

# Stub out binary workspace members so cargo can resolve the workspace
RUN mkdir -p \
    palomar-sync/src rsky-feedgen/src rsky-firehose/src \
    rsky-jetstream-subscriber/src rsky-labeler/src rsky-relay/src \
    rsky-satnav/src rsky-video/src rsky-wintermute/src && \
    for crate in palomar-sync rsky-feedgen rsky-firehose rsky-jetstream-subscriber \
        rsky-labeler rsky-relay rsky-satnav rsky-video rsky-wintermute; do \
        echo 'fn main() {}' > $crate/src/main.rs; \
    done

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir -p rsky-pds/src && echo "fn main() {}" > rsky-pds/src/main.rs

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-pds

# Now copy the real source code and build the final binary
COPY rsky-pds/src rsky-pds/src
COPY rsky-pds/migrations rsky-pds/migrations
COPY rsky-pds/diesel.toml rsky-pds/diesel.toml

RUN cargo build --release --package rsky-pds

FROM debian:bullseye-slim
WORKDIR /usr/src/rsky
COPY --from=builder /usr/src/rsky/target/release/rsky-pds rsky-pds
LABEL org.opencontainers.image.source=https://github.com/blacksky-algorithms/rsky
# Run the web service on container startup with the same environment variables
CMD ["sh", "-c", "ROCKET_PORT=$PORT ROCKET_ADDRESS=0.0.0.0", "./rsky-pds"]
