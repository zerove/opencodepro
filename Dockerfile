# syntax=docker/dockerfile:1.7
# Multi-stage build for sisyphus-bridge (includes all crates)

# ── Stage 1: Planner (cargo-chef) ─────────────────────────────────────
FROM rust:1.81-slim-bookworm AS planner
WORKDIR /app
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 2: Builder ──────────────────────────────────────────────────
FROM rust:1.81-slim-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev clang lld \
    && rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release -p sisyphus-bridge

# ── Stage 3: Runtime ──────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/sisyphus-bridge /app/sisyphus-bridge
COPY --from=builder /app/wasm_tools /app/wasm_tools

EXPOSE 50051
EXPOSE 9100

ENTRYPOINT ["/app/sisyphus-bridge"]
