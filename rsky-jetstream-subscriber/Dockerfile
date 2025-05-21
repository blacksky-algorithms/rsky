# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust AS builder

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY Cargo.toml rust-toolchain ./
COPY cypher cypher
COPY rsky-common rsky-common
COPY rsky-crypto rsky-crypto
COPY rsky-feedgen rsky-feedgen
COPY rsky-firehose rsky-firehose
COPY rsky-identity rsky-identity
COPY rsky-jetstream-subscriber/Cargo.toml rsky-jetstream-subscriber/Cargo.toml
COPY rsky-labeler rsky-labeler
COPY rsky-lexicon rsky-lexicon
COPY rsky-pds rsky-pds
COPY rsky-relay rsky-relay
COPY rsky-repo rsky-repo
COPY rsky-satnav rsky-satnav
COPY rsky-syntax rsky-syntax

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir rsky-jetstream-subscriber/src && echo "fn main() {}" > rsky-jetstream-subscriber/src/main.rs

## Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-jetstream-subscriber

COPY rsky-jetstream-subscriber/src rsky-jetstream-subscriber/src

RUN cargo build --release --package rsky-jetstream-subscriber


FROM debian:bullseye-slim
WORKDIR /usr/src/rsky
COPY --from=builder /usr/src/rsky/target/release/rsky-jetstream-subscriber rsky-jetstream-subscriber
LABEL org.opencontainers.image.source=https://github.com/blacksky-algorithms/rsky
CMD ["./rsky-jetstream-subscriber"]