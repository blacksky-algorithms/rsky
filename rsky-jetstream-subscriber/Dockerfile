# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust AS builder

# Clone the exact commit under test so the workspace always matches the PR
ARG COMMIT_SHA=main
WORKDIR /usr/src/rsky
RUN git init . && \
    git remote add origin https://github.com/blacksky-algorithms/rsky.git && \
    git fetch --depth 1 origin ${COMMIT_SHA} && \
    git checkout FETCH_HEAD

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