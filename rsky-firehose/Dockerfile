# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust AS builder

# Copy local code to the container image.
WORKDIR /usr/src/rsky
RUN git clone --depth 1 https://github.com/blacksky-algorithms/rsky.git .
# We can swap the line above for the lines below once we have stronger versioning
# per workspace member
# RUN git clone --depth 1 https://github.com/blacksky-algorithms/rsky.git . && \
#     git checkout <TBD when we have stronger versioning>

# Manifests come from the build context so dependency changes build before they reach the default branch
COPY Cargo.toml Cargo.lock ./
COPY rsky-firehose/Cargo.toml rsky-firehose/Cargo.toml

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