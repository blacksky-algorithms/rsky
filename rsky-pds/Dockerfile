# Use the official Rust image.
# https://hub.docker.com/_/rust
# FROM --platform=linux/amd64 rust AS builder
FROM rust as builder

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY Cargo.toml ./
COPY cypher cypher
COPY rsky-common rsky-common
COPY rsky-crypto rsky-crypto
COPY rsky-feedgen rsky-feedgen
COPY rsky-firehose rsky-firehose
COPY rsky-identity rsky-identity
COPY rsky-jetstream-subscriber rsky-jetstream-subscriber
COPY rsky-labeler rsky-labeler
COPY rsky-lexicon rsky-lexicon
COPY rsky-relay rsky-relay
COPY rsky-repo rsky-repo
COPY rsky-satnav rsky-satnav
COPY rsky-syntax rsky-syntax

COPY rsky-pds/Cargo.toml rsky-pds/Cargo.toml

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir rsky-pds/src && echo "fn main() {}" > rsky-pds/src/main.rs

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-pds

# Now copy the real source code and build the final binary
COPY rsky-pds/src rsky-pds/src
COPY rsky-pds/migrations rsky-pds/migrations
COPY rsky-pds/diesel.toml rsky-pds/diesel.toml

RUN cargo build --release --package rsky-pds

# FROM --platform=linux/amd64 rust
FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y extra-runtime-dependencies && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/rsky

COPY --from=builder /usr/src/rsky/target/release/rsky-pds rsky-pds

# Run the web service on container startup with the same environment variables
CMD ["sh", "-c", "ROCKET_PORT=$PORT ROCKET_ADDRESS=0.0.0.0" "./rsky-pds"]