# Use the official Rust image.
# https://hub.docker.com/_/rust
# FROM rust

# # Copy local code to the container image.
# WORKDIR /usr/src/rsky
# COPY . .

# # Install production dependencies and build a release artifact.
# RUN cargo build --release --package rsky-firehose

# # Run the web service on container startup.
# CMD["cargo", "run", "--package", "rsky-firehose"]

# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust AS builder

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY Cargo.toml rust-toolchain ./
COPY rsky-lexicon rsky-lexicon
COPY rsky-identity rsky-identity
COPY rsky-syntax rsky-syntax
COPY rsky-jetstream-subscriber rsky-jetstream-subscriber
COPY rsky-crypto rsky-crypto
COPY rsky-feedgen rsky-feedgen
COPY rsky-firehose/Cargo.toml rsky-firehose/Cargo.toml
COPY rsky-common rsky-common
COPY rsky-labeler rsky-labeler
COPY rsky-pds rsky-pds

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir rsky-firehose/src && echo "fn main() {}" > rsky-firehose/src/main.rs

## Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-jetstream-subscriber

COPY rsky-jetstream-subscriber/src rsky-jetstream-subscriber/src

RUN cargo build --release --package rsky-jetstream-subscriber


FROM rust

WORKDIR /usr/src/rsky

COPY --from=builder /usr/src/rsky/target/release/rsky-firehose rsky-firehose

CMD ["./rsky-firehose"]