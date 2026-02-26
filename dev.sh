#!/bin/bash
# FlowCatalyst Rust Development Script
# Usage: ./dev.sh [command]
#
# Commands:
#   start     - Start MongoDB and all services (default)
#   platform  - Start platform server only
#   stream    - Start stream processor only
#   db        - Start MongoDB only
#   db:stop   - Stop MongoDB
#   build     - Build release binaries
#   clean     - Clean build artifacts

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
NC='\033[0m' # No Color

# Load environment
export FC_MONGO_URL="mongodb://localhost:27017/?replicaSet=rs0&directConnection=true"
export FC_MONGO_DB="flowcatalyst"
export FC_API_PORT="8080"
export RUST_LOG="info,fc_platform=debug,fc_stream=debug"

# Check for cargo-watch
check_cargo_watch() {
    if ! command -v cargo-watch &> /dev/null; then
        echo -e "${RED}cargo-watch not found. Installing...${NC}"
        cargo install cargo-watch
    fi
}

# Wait for service to be healthy
wait_for_health() {
    local url=$1
    local name=$2
    local max_attempts=30
    local attempt=0

    echo -e "${BLUE}Waiting for $name to be healthy...${NC}"
    while [ $attempt -lt $max_attempts ]; do
        if curl -s "$url" > /dev/null 2>&1; then
            echo -e "${GREEN}$name is healthy!${NC}"
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    echo -e "${RED}$name failed to become healthy${NC}"
    return 1
}

# Start MongoDB
start_db() {
    echo -e "${BLUE}Starting MongoDB...${NC}"
    docker compose -f ../docker-compose.dev.yml up -d

    # Wait for MongoDB to be ready
    echo -e "${BLUE}Waiting for MongoDB replica set...${NC}"
    sleep 5

    # Check if replica set is initialized
    local max_attempts=30
    local attempt=0
    while [ $attempt -lt $max_attempts ]; do
        if docker exec flowcatalyst-mongo mongosh --quiet --eval "rs.status().ok" 2>/dev/null | grep -q "1"; then
            echo -e "${GREEN}MongoDB replica set is ready!${NC}"
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 2
    done
    echo -e "${RED}MongoDB replica set failed to initialize${NC}"
    return 1
}

# Stop MongoDB
stop_db() {
    echo -e "${BLUE}Stopping MongoDB...${NC}"
    docker compose -f ../docker-compose.dev.yml down
}

# Start platform server (with watch for auto-rebuild)
start_platform() {
    check_cargo_watch
    echo -e "${BLUE}Starting Platform Server on port $FC_API_PORT...${NC}"
    cargo watch -x 'run -p fc-platform-server'
}

# Start platform server (release build, no watch)
start_platform_release() {
    echo -e "${BLUE}Starting Platform Server (release) on port $FC_API_PORT...${NC}"
    cargo run --release -p fc-platform-server
}

# Start stream processor (with watch for auto-rebuild)
start_stream() {
    check_cargo_watch
    export FC_METRICS_PORT="9091"  # Different port to avoid conflict
    echo -e "${MAGENTA}Starting Stream Processor...${NC}"
    wait_for_health "http://localhost:$FC_API_PORT/health" "Platform Server"
    cargo watch -x 'run -p fc-stream-processor'
}

# Start stream processor (release build, no watch)
start_stream_release() {
    export FC_METRICS_PORT="9091"
    echo -e "${MAGENTA}Starting Stream Processor (release)...${NC}"
    wait_for_health "http://localhost:$FC_API_PORT/health" "Platform Server"
    cargo run --release -p fc-stream-processor
}

# Start all services
start_all() {
    start_db

    echo -e "${GREEN}Starting all services...${NC}"
    echo -e "${BLUE}Platform Server: http://localhost:$FC_API_PORT${NC}"
    echo -e "${MAGENTA}Stream Processor: watching for events${NC}"
    echo ""

    # Run both in parallel
    trap 'kill $(jobs -p) 2>/dev/null' EXIT

    FC_METRICS_PORT=9090 cargo run -p fc-platform-server &
    PLATFORM_PID=$!

    # Wait for platform to be ready
    wait_for_health "http://localhost:$FC_API_PORT/health" "Platform Server"

    FC_METRICS_PORT=9091 cargo run -p fc-stream-processor &
    STREAM_PID=$!

    echo -e "${GREEN}All services started!${NC}"
    echo "Press Ctrl+C to stop all services"

    wait
}

# Build release binaries
build_release() {
    echo -e "${BLUE}Building release binaries...${NC}"
    cargo build --release -p fc-platform-server -p fc-stream-processor
    echo -e "${GREEN}Build complete!${NC}"
    echo "Binaries: target/release/fc-platform-server, target/release/fc-stream-processor"
}

# Main command handler
case "${1:-start}" in
    start|dev)
        start_all
        ;;
    platform)
        start_platform
        ;;
    platform:release)
        start_platform_release
        ;;
    stream)
        start_stream
        ;;
    stream:release)
        start_stream_release
        ;;
    db)
        start_db
        ;;
    db:stop)
        stop_db
        ;;
    build)
        build_release
        ;;
    clean)
        cargo clean
        ;;
    help|--help|-h)
        echo "FlowCatalyst Rust Development Script"
        echo ""
        echo "Usage: ./dev.sh [command]"
        echo ""
        echo "Commands:"
        echo "  start, dev      Start MongoDB and all services (default)"
        echo "  platform        Start platform server with auto-reload"
        echo "  platform:release Start platform server (release build)"
        echo "  stream          Start stream processor with auto-reload"
        echo "  stream:release  Start stream processor (release build)"
        echo "  db              Start MongoDB only"
        echo "  db:stop         Stop MongoDB"
        echo "  build           Build release binaries"
        echo "  clean           Clean build artifacts"
        echo ""
        echo "Environment:"
        echo "  FC_MONGO_URL    MongoDB connection URL"
        echo "  FC_MONGO_DB     MongoDB database name"
        echo "  FC_API_PORT     Platform API port (default: 8080)"
        echo "  RUST_LOG        Log level"
        ;;
    *)
        echo -e "${RED}Unknown command: $1${NC}"
        echo "Run './dev.sh help' for usage"
        exit 1
        ;;
esac
