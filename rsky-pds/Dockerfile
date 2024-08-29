# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust:latest

# Install cmake and other build tools
RUN apt-get update && apt-get install -y cmake build-essential

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY . .

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-pds

# Run the web service on container startup.
CMD ROCKET_PORT=$PORT ROCKET_ADDRESS=0.0.0.0 ROCKET_ENV=prod cargo run --release --package rsky-pds