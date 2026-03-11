#!/bin/bash
# FlowCatalyst Rust Development Script
# Usage: ./dev.sh [command]
#
# Commands:
#   start     - Start all infrastructure + fc-dev (default)
#   platform  - Start platform server only
#   stream    - Start stream processor only
#   db        - Start PostgreSQL only
#   db:stop   - Stop PostgreSQL
#   db:shell  - Open psql shell
#   migrate   - Run database migrations
#   build     - Build release binaries
#   clean     - Clean build artifacts

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# Load environment
export FC_DATABASE_URL="${FC_DATABASE_URL:-postgresql://flowcatalyst:flowcatalyst@localhost:5432/flowcatalyst}"
export FC_API_PORT="${FC_API_PORT:-8080}"
export FC_METRICS_PORT="${FC_METRICS_PORT:-9090}"
export RUST_LOG="${RUST_LOG:-info,fc_platform=debug,fc_stream=debug,fc_dev=debug}"

# Check for cargo-watch
check_cargo_watch() {
    if ! command -v cargo-watch &> /dev/null; then
        echo -e "${YELLOW}cargo-watch not found. Installing...${NC}"
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

# Wait for PostgreSQL
wait_for_pg() {
    echo -e "${BLUE}Waiting for PostgreSQL...${NC}"
    local max_attempts=30
    local attempt=0
    while [ $attempt -lt $max_attempts ]; do
        if docker exec fc-postgres pg_isready -U flowcatalyst -q 2>/dev/null; then
            echo -e "${GREEN}PostgreSQL is ready!${NC}"
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    echo -e "${RED}PostgreSQL failed to start${NC}"
    return 1
}

# Start infrastructure
start_infra() {
    echo -e "${BLUE}Starting infrastructure...${NC}"
    docker compose up -d
    wait_for_pg
    echo -e "${GREEN}Infrastructure ready:${NC}"
    echo -e "  PostgreSQL: localhost:5432"
    echo -e "  LocalStack (SQS):    localhost:4566"
    echo -e "  Redis:                localhost:6379"
}

# Stop infrastructure
stop_infra() {
    echo -e "${BLUE}Stopping infrastructure...${NC}"
    docker compose down
}

# Run migrations
run_migrations() {
    echo -e "${BLUE}Running database migrations...${NC}"
    for f in migrations/*.sql; do
        echo -e "  ${MAGENTA}$f${NC}"
        docker exec -i fc-postgres psql -U flowcatalyst -d flowcatalyst -q < "$f" 2>&1 | grep -v "^$" || true
    done
    echo -e "${GREEN}Migrations complete.${NC}"
}

# Start platform server (with watch for auto-rebuild)
start_platform() {
    check_cargo_watch
    echo -e "${BLUE}Starting Platform Server on port $FC_API_PORT...${NC}"
    cargo watch -w crates -w bin -x 'run -p fc-platform-server'
}

# Start stream processor (with watch for auto-rebuild)
start_stream() {
    check_cargo_watch
    export FC_METRICS_PORT="9091"  # Different port to avoid conflict
    echo -e "${MAGENTA}Starting Stream Processor...${NC}"
    wait_for_health "http://localhost:$FC_API_PORT/health" "Platform Server"
    cargo watch -x 'run -p fc-stream-processor'
}

# Start all services
start_all() {
    start_infra
    run_migrations

    echo ""
    echo -e "${GREEN}Starting fc-dev monolith...${NC}"
    echo -e "  API:     http://localhost:$FC_API_PORT"
    echo -e "  Health:  http://localhost:$FC_API_PORT/health"
    echo -e "  Metrics: http://localhost:$FC_METRICS_PORT/metrics"
    echo ""

    check_cargo_watch
    cargo watch -w crates -w bin -x 'run --bin fc-dev'
}

# Build release binaries
build_release() {
    echo -e "${BLUE}Building release binaries...${NC}"
    cargo build --release -p fc-dev -p fc-platform-server -p fc-stream-processor
    echo -e "${GREEN}Build complete!${NC}"
    echo "Binaries in target/release/"
}

# Main command handler
case "${1:-start}" in
    start|dev)
        start_all
        ;;
    platform)
        start_platform
        ;;
    stream)
        start_stream
        ;;
    db|up)
        start_infra
        ;;
    db:stop|down)
        stop_infra
        ;;
    db:shell|psql)
        docker exec -it fc-postgres psql -U flowcatalyst -d flowcatalyst
        ;;
    migrate)
        run_migrations
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
        echo "  start, dev    Start infrastructure + fc-dev with auto-reload (default)"
        echo "  platform      Start platform server with auto-reload"
        echo "  stream        Start stream processor with auto-reload"
        echo "  db, up        Start infrastructure (PostgreSQL, LocalStack, Redis)"
        echo "  db:stop, down Stop infrastructure"
        echo "  db:shell      Open psql shell"
        echo "  migrate       Run database migrations"
        echo "  build         Build release binaries"
        echo "  clean         Clean build artifacts"
        echo ""
        echo "Environment:"
        echo "  FC_DATABASE_URL  PostgreSQL connection (default: postgresql://flowcatalyst:flowcatalyst@localhost:5432/flowcatalyst)"
        echo "  FC_API_PORT      API port (default: 8080)"
        echo "  RUST_LOG         Log level"
        ;;
    *)
        echo -e "${RED}Unknown command: $1${NC}"
        echo "Run './dev.sh help' for usage"
        exit 1
        ;;
esac
