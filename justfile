# FlowCatalyst Rust - Development Tasks
#
# Quick start:
#   just setup     — first time: up + migrate + dev
#   just dev       — start fc-dev with hot reload
#
# Prerequisites:
#   cargo install cargo-watch
#   docker / docker compose

set dotenv-filename := ".env.development"

# Defaults
FC_DATABASE_URL := env("FC_DATABASE_URL", "postgresql://flowcatalyst:flowcatalyst@localhost:5432/flowcatalyst")
FC_API_PORT := env("FC_API_PORT", "8080")
RUST_LOG := env("RUST_LOG", "info,fc_platform=debug,fc_dev=debug")

# List available recipes
default:
    @just --list

# ─── Full Setup ─────────────────────────────────────────────────────────────

# First-time setup: start infra, migrate, seed, then show instructions
setup: up wait-for-db migrate seed
    @echo ""
    @echo "Setup complete! Run 'just dev' to start the server."
    @echo ""
    @echo "  API:     http://localhost:{{ FC_API_PORT }}"
    @echo "  Health:  http://localhost:{{ FC_API_PORT }}/health"
    @echo "  Metrics: http://localhost:9090/metrics"
    @echo ""
    @echo "  Dev credentials:"
    @echo "    admin@flowcatalyst.local / DevPassword123!"
    @echo "    alice@acme.com / DevPassword123!"
    @echo "    bob@acme.com / DevPassword123!"

# ─── Infrastructure ────────────────────────────────────────────────────────

# Start PostgreSQL, LocalStack, Redis
up:
    docker compose up -d
    @echo ""
    @echo "Services:"
    @echo "  PostgreSQL: localhost:5432"
    @echo "  LocalStack: localhost:4566"
    @echo "  Redis:      localhost:6379"

# Stop all Docker services
down:
    docker compose down

# Tail Docker service logs
logs:
    docker compose logs -f

# Show Docker service status
ps:
    docker compose ps

# Wait for PostgreSQL to accept connections
[private]
wait-for-db:
    @echo "Waiting for PostgreSQL..."
    @until docker exec fc-postgres pg_isready -U flowcatalyst -q 2>/dev/null; do sleep 1; done
    @echo "PostgreSQL is ready."

# ─── Database ──────────────────────────────────────────────────────────────

# Run all SQL migrations
migrate: wait-for-db
    @echo "Running migrations..."
    @for f in migrations/*.sql; do \
        echo "  $f"; \
        docker exec -i fc-postgres psql -U flowcatalyst -d flowcatalyst -q < "$f" 2>&1 | grep -v "^$" || true; \
    done
    @echo "Migrations complete."

# Drop and recreate database + re-migrate
db-reset: wait-for-db
    docker exec fc-postgres psql -U flowcatalyst -d postgres -c "DROP DATABASE IF EXISTS flowcatalyst;"
    docker exec fc-postgres psql -U flowcatalyst -d postgres -c "CREATE DATABASE flowcatalyst;"
    just migrate

# Open a psql shell
db-shell:
    docker exec -it fc-postgres psql -U flowcatalyst -d flowcatalyst

# Seed development data
seed:
    @echo "Seeding development data..."
    FC_DATABASE_URL={{ FC_DATABASE_URL }} cargo run --bin fc-dev -- --seed 2>/dev/null || \
        echo "  (seed flag not yet implemented — will seed on first startup)"

# ─── Development ───────────────────────────────────────────────────────────

# Run fc-dev with auto-restart on source changes
dev:
    FC_DATABASE_URL={{ FC_DATABASE_URL }} cargo watch -w crates -w bin -x 'run --bin fc-dev'

# Run fc-dev with debug logging
dev-debug:
    RUST_LOG=debug FC_DATABASE_URL={{ FC_DATABASE_URL }} cargo watch -w crates -w bin -x 'run --bin fc-dev'

# Run fc-dev once (no watch)
run:
    FC_DATABASE_URL={{ FC_DATABASE_URL }} cargo run --bin fc-dev

# Watch and run tests on file changes
watch-test:
    cargo watch -x 'test --lib'

# Watch platform crate tests
watch-test-platform:
    cargo watch -w crates/fc-platform -x 'test --package fc-platform --lib'

# ─── Build ─────────────────────────────────────────────────────────────────

# Build all binaries (debug)
build:
    cargo build --all-targets

# Build all binaries (release)
release:
    cargo build --release --all-targets

# Fast compile check
check:
    cargo check --all-targets

# Build individual binaries
build-dev:
    cargo build --bin fc-dev

build-router:
    cargo build --bin fc-router-bin

build-platform:
    cargo build --bin fc-platform-server

build-outbox:
    cargo build --bin fc-outbox-processor

build-stream:
    cargo build --bin fc-stream-processor

# ─── Testing ───────────────────────────────────────────────────────────────

# Run all tests
test:
    cargo test --all-targets

# Run library tests only (faster)
test-lib:
    cargo test --lib

# Run platform tests
test-platform:
    cargo test --package fc-platform

# Run SDK tests
test-sdk:
    cargo test --package fc-sdk --all-features

# Run tests with output
test-verbose:
    cargo test --all-targets -- --nocapture

# ─── Code Quality ──────────────────────────────────────────────────────────

# Format all code
fmt:
    cargo fmt --all

# Check formatting (CI)
fmt-check:
    cargo fmt --all -- --check

# Run clippy linter
lint:
    cargo clippy --all-targets -- -D warnings

# Run clippy with auto-fix
lint-fix:
    cargo clippy --all-targets --fix --allow-dirty

# ─── Cleanup ───────────────────────────────────────────────────────────────

# Clean build artifacts
clean:
    cargo clean

# Remove everything including Docker volumes
nuke: down
    docker volume rm flowcatalyst-rust_fc-pgdata 2>/dev/null || true
    @echo "All data removed. Run 'just setup' to start fresh."

# ─── Tools ─────────────────────────────────────────────────────────────────

# Install development tools
install-tools:
    cargo install cargo-watch just
    @echo ""
    @echo "For faster builds, install lld linker:"
    @echo "  macOS:  brew install llvm"
    @echo "  Linux:  sudo apt install lld clang"
