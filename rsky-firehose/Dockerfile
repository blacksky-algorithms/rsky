# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust AS builder

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY . .

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-firehose

FROM debian:bullseye-slim
WORKDIR /usr/src/rsky
COPY --from=builder /usr/src/rsky/target/release/rsky-firehose rsky-firehose
LABEL org.opencontainers.image.source=https://github.com/blacksky-algorithms/rsky
CMD ["./rsky-firehose"]