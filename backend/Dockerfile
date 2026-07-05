# syntax=docker/dockerfile:1
#
# Gamma backend — ONE image for all three binaries (MASTERPLAN M4.2); the
# command picks which one runs:
#   core-api (default)  · transcode_worker  · settlement_scheduler
# e.g.  docker run …            → core-api
#       docker run … transcode_worker
#
# Multi-stage: a pinned Rust builder compiles --release with the committed
# .sqlx cache (SQLX_OFFLINE — no database at build time), then a slim non-root
# runtime. ffmpeg is included because transcode_worker shells out to it; the
# few extra MB in the api/scheduler containers buy the one-image simplicity.

# Matches rust-toolchain.toml (bump both together — the toolchain file wins if
# they diverge; matching tags just avoids a second toolchain download).
FROM rust:1.96.0-slim-bookworm AS builder
WORKDIR /build
ENV SQLX_OFFLINE=true
COPY . .
RUN cargo build --release -p core-api \
 && strip target/release/core-api target/release/transcode_worker target/release/settlement_scheduler

FROM debian:bookworm-slim AS runtime
# ca-certificates: TLS to S3/object storage. ffmpeg: the transcode worker.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
 && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --uid 10001 gamma
COPY --from=builder /build/target/release/core-api /usr/local/bin/
COPY --from=builder /build/target/release/transcode_worker /usr/local/bin/
COPY --from=builder /build/target/release/settlement_scheduler /usr/local/bin/
USER gamma
# All config comes from the environment (see .env.example). Migrations run at
# core-api startup (embedded, forward-only); the workers assume a migrated DB.
EXPOSE 8080
# Probes core-api's /health via bash's /dev/tcp (no curl in the slim image).
# NOTE: only meaningful for the DEFAULT command — worker/scheduler containers
# expose no port, so compose (M4.3) overrides/disables this for them.
HEALTHCHECK --interval=30s --timeout=3s --start-period=15s --retries=3 \
  CMD ["/bin/bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/8080 && printf 'GET /health HTTP/1.0\\r\\n\\r\\n' >&3 && head -1 <&3 | grep -q 200"]
CMD ["core-api"]
