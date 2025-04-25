# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM --platform=linux/amd64 rust AS builder

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY Cargo.toml Cargo.lock rust-toolchain ./
COPY rsky-lexicon rsky-lexicon
COPY rsky-identity rsky-identity
COPY cypher cypher
COPY rsky-syntax rsky-syntax
COPY rsky-repo rsky-repo
COPY rsky-jetstream-subscriber rsky-jetstream-subscriber
COPY rsky-crypto rsky-crypto
COPY rsky-feedgen rsky-feedgen
COPY rsky-firehose rsky-firehose
COPY rsky-oauth rsky-oauth
COPY rsky-satnav rsky-satnav
COPY rsky-common rsky-common
COPY rsky-labeler rsky-labeler
COPY rsky-pds/Cargo.toml rsky-pds/Cargo.toml

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir rsky-pds/src && echo "fn main() {}" > rsky-pds/src/main.rs

## Install production dependencies and build a release artifact.
RUN cargo build --package rsky-pds

COPY rsky-pds/src rsky-pds/src

RUN cargo build --package rsky-pds


FROM --platform=linux/amd64 rust

WORKDIR /usr/src/rsky

COPY --from=builder /usr/src/rsky/target/debug/rsky-pds rsky-pds

CMD ["./rsky-pds"]