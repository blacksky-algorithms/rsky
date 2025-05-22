# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust AS builder

# Copy local code to the container image.
WORKDIR /usr/src/rsky

# Start by copying the workspace configuration files
COPY Cargo.toml rust-toolchain ./

# Copy only the Cargo.toml from our package
COPY rsky-firehose/Cargo.toml rsky-firehose/Cargo.toml

# Copy all workspace members except our target package
COPY cypher cypher
COPY rsky-common rsky-common
COPY rsky-crypto rsky-crypto
COPY rsky-feedgen rsky-feedgen
COPY rsky-identity rsky-identity
COPY rsky-jetstream-subscriber rsky-jetstream-subscriber
COPY rsky-labeler rsky-labeler
COPY rsky-lexicon rsky-lexicon
COPY rsky-pds rsky-pds
COPY rsky-relay rsky-relay
COPY rsky-repo rsky-repo
COPY rsky-satnav rsky-satnav
COPY rsky-syntax rsky-syntax

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir -p rsky-firehose/src && echo "fn main() {}" > rsky-firehose/src/main.rs

## Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-firehose

# Now copy the real source code and build the final binary
COPY rsky-firehose/src rsky-firehose/src

RUN cargo build --release --package rsky-firehose

FROM debian:bullseye-slim
WORKDIR /usr/src/rsky
COPY --from=builder /usr/src/rsky/target/release/rsky-firehose rsky-firehose
LABEL org.opencontainers.image.source=https://github.com/blacksky-algorithms/rsky
CMD ["./rsky-firehose"]