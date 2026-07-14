#!/bin/bash

# ==============================================================================
# Sisyphus & jcode Symbiosis Launcher (Final Optimized Version)
# ==============================================================================

# Configuration
ROOT_DIR="$(pwd)"
BRIDGE_DIR="$ROOT_DIR/crates/sisyphus-bridge"
JCODE_DIR="/home/zeron/.jcode/source/jcode"
HEALTH_URL="http://[::1]:8080/health"
LOG_FILE="$ROOT_DIR/symbiosis_bridge.log"
CONFIG_FILE="$ROOT_DIR/config.toml"

echo "🚀 Starting Sisyphus-jcode Symbiosis Loop..."

# 1. Pre-build both projects to avoid lock contention during runtime
echo "🔨 Pre-building components to avoid lock contention..."
echo "Note: This may take several minutes on the first run. Please wait for 'Pre-build complete'."

cd "$BRIDGE_DIR" || { echo "❌ Error: Bridge directory not found"; exit 1; }
echo "📦 Building Sisyphus Bridge..."
cargo build --release

cd "$JCODE_DIR" || { echo "❌ Error: jcode directory not found"; exit 1; }
echo "📦 Building jcode..."
cargo build --release --bin jcode

echo "✅ Pre-build complete."

# 2. Start Sisyphus Bridge in background
echo "🧠 Launching Sisyphus Bridge (Slow Thinking Layer)..."
export SISYPHUS_CONFIG="$CONFIG_FILE"
"$ROOT_DIR/target/release/sisyphus-bridge" > "$LOG_FILE" 2>&1 &
BRIDGE_PID=$!

# 3. Poll for Health
echo -n "⏳ Waiting for Sisyphus to wake up"
MAX_RETRIES=30
COUNT=0
while true; do
    if curl -s "$HEALTH_URL" > /dev/null; then
        echo -e "\n✅ Sisyphus is healthy and ready!"
        break
    fi
    
    if [ $COUNT -ge $MAX_RETRIES ]; then
        echo -e "\n❌ Error: Sisyphus failed to start within timeout."
        kill $BRIDGE_PID
        exit 1
    fi
    
    echo -n "."
    sleep 1
    ((COUNT++))
done

# 4. Start jcode in foreground
echo "💻 Activating jcode (Fast Execution Layer)..."
cd "$JCODE_DIR" || { echo "❌ Error: jcode directory not found"; exit 1; }
./target/release/jcode

# 5. Cleanup on exit
trap "echo 'Stopping Sisyphus Bridge...'; kill $BRIDGE_PID; exit" SIGINT SIGTERM
