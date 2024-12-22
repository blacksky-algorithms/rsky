# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM rust

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY . .

# Install production dependencies and build a release artifact.
RUN cargo build --package rsky-labeler

# Run the web service on container startup.
CMD ["sh", "-c", "cargo run --package rsky-labeler"]