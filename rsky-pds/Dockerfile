FROM rust:1.86-bookworm AS builder

WORKDIR /usr/src/rsky
COPY . .
ENV CARGO_BUILD_JOBS=1
RUN cargo build --release --package rsky-pds --jobs "${CARGO_BUILD_JOBS}"

FROM debian:bullseye-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/rsky/target/release/rsky-pds /usr/local/bin/rsky-pds
LABEL org.opencontainers.image.source=https://github.com/rabble/rsky
CMD ["sh", "-c", "ROCKET_PORT=${PDS_PORT:-2583} ROCKET_ADDRESS=0.0.0.0 exec /usr/local/bin/rsky-pds"]
