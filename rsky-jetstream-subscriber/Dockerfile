# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust AS builder

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY Cargo.toml rust-toolchain ./

# Copy only the Cargo.toml from our package
COPY rsky-jetstream-subscriber/Cargo.toml rsky-jetstream-subscriber/Cargo.toml

# Copy all workspace members except our target package
COPY cypher cypher
COPY rsky-common rsky-common
COPY rsky-crypto rsky-crypto
COPY rsky-feedgen rsky-feedgen
COPY rsky-firehose rsky-firehose
COPY rsky-identity rsky-identity
COPY rsky-labeler rsky-labeler
COPY rsky-lexicon rsky-lexicon
COPY rsky-pds rsky-pds
COPY rsky-relay rsky-relay
COPY rsky-repo rsky-repo
COPY rsky-satnav rsky-satnav
COPY rsky-syntax rsky-syntax

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir -p rsky-jetstream-subscriber/src && echo "fn main() {}" > rsky-jetstream-subscriber/src/main.rs

## Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-jetstream-subscriber

# Now copy the real source code and build the final binary
COPY rsky-jetstream-subscriber/src rsky-jetstream-subscriber/src

RUN cargo build --release --package rsky-jetstream-subscriber

FROM debian:bullseye-slim
WORKDIR /usr/src/rsky
COPY --from=builder /usr/src/rsky/target/release/rsky-jetstream-subscriber rsky-jetstream-subscriber
LABEL org.opencontainers.image.source=https://github.com/blacksky-algorithms/rsky
CMD ["./rsky-jetstream-subscriber"]