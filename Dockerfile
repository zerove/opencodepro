# ── Sisyphus Bridge Dockerfile ─────────────────────────────────────────
# Multi-stage build: compile with nightly, run on minimal debian.
# gRPC (50051) + HTTP (8080).

# Stage 1: Builder
FROM rust:nightly-slim-bookworm AS builder

# Install build dependencies (pkg-config for openssl-sys, libssl-dev for tonic)
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests first for dependency layer caching
COPY Cargo.toml Cargo.lock ./
COPY crates/sisyphus-bridge-api/Cargo.toml crates/sisyphus-bridge-api/
COPY crates/sisyphus-bridge/Cargo.toml crates/sisyphus-bridge/
COPY crates/sisyphus-types/Cargo.toml crates/sisyphus-types/
COPY crates/sisyphus-core/Cargo.toml crates/sisyphus-core/
COPY crates/sisyphus-tools-core/Cargo.toml crates/sisyphus-tools-core/
COPY crates/sisyphus-tools-llm/Cargo.toml crates/sisyphus-tools-llm/
COPY crates/sisyphus-tools-system/Cargo.toml crates/sisyphus-tools-system/
COPY crates/sisyphus-tools-mcp/Cargo.toml crates/sisyphus-tools-mcp/
COPY crates/sisyphus-tools-session/Cargo.toml crates/sisyphus-tools-session/
COPY crates/sisyphus-tools-agent/Cargo.toml crates/sisyphus-tools-agent/
COPY crates/sisyphus-tools-executor/Cargo.toml crates/sisyphus-tools-executor/
COPY crates/sisyphus-tools-llm-tools/Cargo.toml crates/sisyphus-tools-llm-tools/
COPY crates/sisyphus-mem/Cargo.toml crates/sisyphus-mem/
COPY crates/sisyphus-intel/Cargo.toml crates/sisyphus-intel/
COPY crates/sisyphus-metrics/Cargo.toml crates/sisyphus-metrics/
COPY crates/sisyphus-roadmap/Cargo.toml crates/sisyphus-roadmap/
COPY crates/sisyphus-tools-build/Cargo.toml crates/sisyphus-tools-build/
COPY crates/sisyphus-tools-scm/Cargo.toml crates/sisyphus-tools-scm/

# Create dummy main.rs/lib.rs so `cargo build` can resolve dep tree without full source
RUN mkdir -p crates/sisyphus-bridge/src && \
    echo "fn main() {}" > crates/sisyphus-bridge/src/main.rs && \
    mkdir -p crates/sisyphus-bridge-api/src && \
    echo "" > crates/sisyphus-bridge-api/src/lib.rs && \
    mkdir -p crates/sisyphus-types/src && \
    echo "" > crates/sisyphus-types/src/lib.rs && \
    mkdir -p crates/sisyphus-core/src && \
    echo "" > crates/sisyphus-core/src/lib.rs && \
    mkdir -p crates/sisyphus-tools-core/src && \
    echo "" > crates/sisyphus-tools-core/src/lib.rs && \
    mkdir -p crates/sisyphus-tools-llm/src && \
    echo "" > crates/sisyphus-tools-llm/src/lib.rs && \
    mkdir -p crates/sisyphus-tools-system/src && \
    echo "" > crates/sisyphus-tools-system/src/lib.rs && \
    mkdir -p crates/sisyphus-tools-mcp/src && \
    echo "" > crates/sisyphus-tools-mcp/src/lib.rs && \
    mkdir -p crates/sisyphus-tools-session/src && \
    echo "" > crates/sisyphus-tools-session/src/lib.rs && \
    mkdir -p crates/sisyphus-tools-agent/src && \
    echo "" > crates/sisyphus-tools-agent/src/lib.rs && \
    mkdir -p crates/sisyphus-tools-executor/src && \
    echo "" > crates/sisyphus-tools-executor/src/lib.rs && \
    mkdir -p crates/sisyphus-tools-llm-tools/src && \
    echo "" > crates/sisyphus-tools-llm-tools/src/lib.rs && \
    mkdir -p crates/sisyphus-mem/src && \
    echo "" > crates/sisyphus-mem/src/lib.rs && \
    mkdir -p crates/sisyphus-intel/src && \
    echo "" > crates/sisyphus-intel/src/lib.rs && \
    mkdir -p crates/sisyphus-metrics/src && \
    echo "" > crates/sisyphus-metrics/src/lib.rs && \
    mkdir -p crates/sisyphus-roadmap/src && \
    echo "" > crates/sisyphus-roadmap/src/lib.rs && \
    mkdir -p crates/sisyphus-tools-build/src && \
    echo "" > crates/sisyphus-tools-build/src/lib.rs && \
    mkdir -p crates/sisyphus-tools-scm/src && \
    echo "" > crates/sisyphus-tools-scm/src/lib.rs

# Build dependencies only (cached layer)
RUN cargo build --release -p sisyphus-bridge 2>/dev/null || true

# Now copy the full source and rebuild (only changed crates recompile)
COPY . .

# Build the bridge service in release mode
RUN cargo build --release -p sisyphus-bridge

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies (ca-certificates for HTTPS, libssl3 for tonic)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Create data directory for persistent state
RUN mkdir -p /app/data

# Copy the compiled binary from the builder stage
COPY --from=builder /app/target/release/sisyphus-bridge /app/sisyphus-bridge

# Copy default config
COPY --from=builder /app/config.example.toml /app/config.toml

# Set environment variables
ENV SISYPHUS_CONFIG=/app/config.toml
ENV SISYPHUS_DATA_DIR=/app/data

# Expose gRPC and HTTP ports
EXPOSE 50051 8080

# Health check (gRPC health probe — requires grpc_health_probe binary in production)
# HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
#   CMD grpc_health_probe -addr=localhost:50051 || exit 1

ENTRYPOINT ["/app/sisyphus-bridge"]
