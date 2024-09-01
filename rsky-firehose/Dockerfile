# Use the official Rust image.
# https://hub.docker.com/_/rust
FROM --platform=linux/amd64 rust

# Copy local code to the container image.
WORKDIR /usr/src/rsky
COPY . .

# Install production dependencies and build a release artifact.
RUN cargo build --release --package rsky-firehose

# Run the web service on container startup.
CMD cargo run --package rsky-firehose