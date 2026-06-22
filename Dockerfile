# Stage 1: Builder
FROM rust:1.85-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the entire workspace to ensure all crate dependencies are available
COPY . .

# Build the bridge service in release mode
RUN cargo build --release -p sisyphus-bridge

# Stage 2: Runtime
FROM gcr.io/distroless/cc-debian12

WORKDIR /app

# Copy the compiled binary from the builder stage
COPY --from=builder /app/target/release/sisyphus-bridge /app/sisyphus-bridge

# Copy the WASM tools directory
COPY --from=builder /app/tools /app/tools

# Copy the example config as the default config
COPY --from=builder /app/config.example.toml /app/config.toml

# Set environment variables
ENV SISYPHUS_CONFIG=/app/config.toml

# Expose gRPC and Prometheus ports
EXPOSE 50051 9100

# Run the bridge service
ENTRYPOINT ["/app/sisyphus-bridge"]
