# ── Multi-stage Dockerfile for rust-e2e-chat-api ─────────────────────────────
#
# Stage 1 (builder) — compile the release binary on Rust 1.88
# Stage 2 (runtime) — minimal Debian image with just the binary

# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1.88-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN cargo build --release --bin api

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/api /app/api
COPY --from=builder /app/migrations         /app/migrations

RUN useradd -m -u 1000 appuser && chown -R appuser:appuser /app
USER appuser

# HTTP/1.1 + WebSocket listener
EXPOSE 8080/tcp

ENTRYPOINT ["/app/api"]
