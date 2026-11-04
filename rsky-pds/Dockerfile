# Builds rsky-pds from the workspace. Everything is copied so a new
# workspace member or dependency never needs a Dockerfile edit; BuildKit
# cache mounts keep the registry and build artifacts between builds.
FROM rust:1-bookworm AS builder

WORKDIR /usr/src/rsky
COPY . .

ARG CARGO_BUILD_JOBS=default
RUN --mount=type=cache,id=rsky-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=rsky-pds-target,target=/usr/src/rsky/target \
    cargo build --release --package rsky-pds -j "$CARGO_BUILD_JOBS" && \
    cp target/release/rsky-pds /usr/local/bin/rsky-pds

FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*
WORKDIR /usr/src/rsky
COPY --from=builder /usr/local/bin/rsky-pds rsky-pds
LABEL org.opencontainers.image.source=https://github.com/blacksky-algorithms/rsky
# Storage is sqlite + local disk (or S3): mount a volume at PDS_DATA_DIRECTORY.
# Required env: PDS_DATA_DIRECTORY, PDS_HOSTNAME, PDS_SERVICE_DID,
# PDS_JWT_KEY_K256_PRIVATE_KEY_HEX, PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX,
# PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX, PDS_DPOP_SECRET, PDS_ADMIN_PASS.
# See rsky-pds/README.md for the full reference.
CMD ["sh", "-c", "ROCKET_ADDRESS=0.0.0.0 ROCKET_PORT=${PORT:-2583} ./rsky-pds"]
