# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM --platform=linux/amd64 rust AS builder

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY Cargo.toml Cargo.lock rust-toolchain ./
COPY rsky-lexicon rsky-lexicon
COPY rsky-identity rsky-identity
COPY rsky-syntax rsky-syntax
COPY rsky-jetstream/Cargo.toml rsky-jetstream/Cargo.toml
COPY rsky-crypto rsky-crypto
COPY rsky-feedgen rsky-feedgen
COPY rsky-firehose rsky-firehose
COPY rsky-common rsky-common
COPY rsky-labeler rsky-labeler
COPY rsky-pds rsky-pds

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir rsky-jetstream/src && echo "fn main() {}" > rsky-jetstream/src/main.rs

## Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-jetstream

COPY rsky-jetstream/src rsky-jetstream/src

RUN cargo build --release --package rsky-jetstream


FROM --platform=linux/amd64 rust

WORKDIR /usr/src/rsky

COPY --from=builder /usr/src/rsky/target/release/rsky-jetstream rsky-jetstream

CMD ["./rsky-jetstream"]