# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM --platform=linux/amd64 rust

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY . .

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-feedgen

# Run the web service on container startup.
CMD ROCKET_PORT=$PORT ROCKET_ADDRESS=0.0.0.0 ROCKET_ENV=prod cargo run --release --package rsky-feedgen