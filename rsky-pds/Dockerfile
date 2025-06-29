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


# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir -p rsky-pds/src && echo "fn main() {}" > rsky-pds/src/main.rs

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-pds

# Now copy the real source code and build the final binary
COPY rsky-pds/src rsky-pds/src
COPY rsky-pds/migrations rsky-pds/migrations
COPY rsky-pds/diesel.toml rsky-pds/diesel.toml

RUN cargo build --release --package rsky-pds

FROM debian:bullseye-slim
WORKDIR /usr/src/rsky
COPY --from=builder /usr/src/rsky/target/release/rsky-pds rsky-pds
LABEL org.opencontainers.image.source=https://github.com/blacksky-algorithms/rsky
# Run the web service on container startup with the same environment variables
CMD ["sh", "-c", "ROCKET_PORT=$PORT ROCKET_ADDRESS=0.0.0.0", "./rsky-pds"]