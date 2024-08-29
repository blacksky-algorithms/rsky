# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM --platform=linux/amd64 rust:1.80

# Install cmake and other build tools
RUN apt-get update && apt-get install -y cmake build-essential libclang-dev

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY . .

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-pds

# Run the web service on container startup.
CMD ROCKET_PORT=$PORT ROCKET_ADDRESS=0.0.0.0 cargo run --release --package rsky-pds