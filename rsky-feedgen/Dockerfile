# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust AS builder

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY Cargo.toml rust-toolchain ./

# Copy only the Cargo.toml from our package
COPY rsky-feedgen/Cargo.toml rsky-feedgen/Cargo.toml

# Copy all workspace members except our target package
COPY cypher cypher
COPY rsky-common rsky-common
COPY rsky-crypto rsky-crypto
COPY rsky-identity rsky-identity
COPY rsky-firehose rsky-firehose
COPY rsky-jetstream-subscriber rsky-jetstream-subscriber
COPY rsky-labeler rsky-labeler
COPY rsky-lexicon rsky-lexicon
COPY rsky-pds rsky-pds
COPY rsky-relay rsky-relay
COPY rsky-repo rsky-repo
COPY rsky-satnav rsky-satnav
COPY rsky-syntax rsky-syntax

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir -p rsky-feedgen/src && echo "fn main() {}" > rsky-feedgen/src/main.rs

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-feedgen

# Now copy the real source code and build the final binary
COPY rsky-feedgen/src rsky-feedgen/src
COPY rsky-feedgen/migrations rsky-feedgen/migrations
COPY rsky-feedgen/diesel.toml rsky-feedgen/diesel.toml

RUN cargo build --release --package rsky-feedgen

FROM debian:bullseye-slim
WORKDIR /usr/src/rsky
COPY --from=builder /usr/src/rsky/target/release/rsky-feedgen rsky-feedgen
LABEL org.opencontainers.image.source=https://github.com/blacksky-algorithms/rsky
# Run the web service on container startup with the same environment variables
CMD ["sh", "-c", "ROCKET_PORT=$PORT ROCKET_ADDRESS=0.0.0.0 ROCKET_ENV=prod ./rsky-feedgen"]