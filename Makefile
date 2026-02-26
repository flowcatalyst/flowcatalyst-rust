# FlowCatalyst Rust - Development Makefile
#
# Prerequisites:
#   cargo install cargo-watch
#   MongoDB running on localhost:27017

.PHONY: help dev dev-debug watch-test check build release test clean fmt lint

# Default target
help:
	@echo "FlowCatalyst Rust Development"
	@echo ""
	@echo "Usage: make <target>"
	@echo ""
	@echo "Development:"
	@echo "  dev          Run fc-dev with auto-restart on file changes"
	@echo "  dev-debug    Run fc-dev with debug logging and auto-restart"
	@echo "  watch-test   Run tests on file changes"
	@echo "  check        Fast compile check (no binary output)"
	@echo ""
	@echo "Build:"
	@echo "  build          Build all binaries (debug)"
	@echo "  release        Build all binaries (release, optimized)"
	@echo "  build-dev      Build fc-dev only"
	@echo "  build-router   Build fc-router only"
	@echo "  build-platform Build fc-platform-server only"
	@echo "  build-outbox   Build fc-outbox-processor only"
	@echo "  build-stream   Build fc-stream-processor only"
	@echo ""
	@echo "Quality:"
	@echo "  test         Run all tests"
	@echo "  fmt          Format code"
	@echo "  lint         Run clippy linter"
	@echo "  clean        Clean build artifacts"
	@echo ""
	@echo "Prerequisites:"
	@echo "  cargo install cargo-watch"
	@echo "  MongoDB running on localhost:27017"

# ============================================================================
# Development (hot reload)
# ============================================================================

# Run dev server with auto-restart on source changes
dev:
	cargo watch -w crates -w bin -x 'run --bin fc-dev'

# Run dev server with debug logging
dev-debug:
	RUST_LOG=debug cargo watch -w crates -w bin -x 'run --bin fc-dev'

# Run dev server with trace logging (very verbose)
dev-trace:
	RUST_LOG=trace cargo watch -w crates -w bin -x 'run --bin fc-dev'

# Watch and run tests on changes
watch-test:
	cargo watch -x 'test --lib'

# Watch specific package tests
watch-test-platform:
	cargo watch -w crates/fc-platform -x 'test --package fc-platform --lib'

watch-test-router:
	cargo watch -w crates/fc-router -x 'test --package fc-router --lib'

# Fast compile check (no output binary)
check:
	cargo check --all-targets

# ============================================================================
# Build
# ============================================================================

# Build all binaries (debug)
build:
	cargo build --all-targets

# Build all binaries (release, optimized)
release:
	cargo build --release --all-targets

# Build only fc-dev
build-dev:
	cargo build --bin fc-dev

# Build only fc-router
build-router:
	cargo build --bin fc-router-bin

# Build only fc-platform-server
build-platform:
	cargo build --bin fc-platform-server

# Build only fc-outbox-processor
build-outbox:
	cargo build --bin fc-outbox-processor

# Build only fc-stream-processor
build-stream:
	cargo build --bin fc-stream-processor

# ============================================================================
# Testing
# ============================================================================

# Run all tests
test:
	cargo test --all-targets

# Run only library tests (faster, no integration tests)
test-lib:
	cargo test --lib

# Run tests for specific packages
test-platform:
	cargo test --package fc-platform

test-router:
	cargo test --package fc-router

test-common:
	cargo test --package fc-common

# Run tests with output
test-verbose:
	cargo test --all-targets -- --nocapture

# ============================================================================
# Code Quality
# ============================================================================

# Format all code
fmt:
	cargo fmt --all

# Check formatting (CI)
fmt-check:
	cargo fmt --all -- --check

# Run clippy linter
lint:
	cargo clippy --all-targets -- -D warnings

# Run clippy with fixes
lint-fix:
	cargo clippy --all-targets --fix --allow-dirty

# ============================================================================
# Cleanup
# ============================================================================

# Clean build artifacts
clean:
	cargo clean

# Clean and rebuild
rebuild: clean build

# ============================================================================
# Docker (optional)
# ============================================================================

# Start MongoDB for development
mongo-start:
	docker run -d -p 27017:27017 --name fc-mongo mongo:7 || docker start fc-mongo

# Stop MongoDB
mongo-stop:
	docker stop fc-mongo

# View MongoDB logs
mongo-logs:
	docker logs -f fc-mongo

# ============================================================================
# Installation
# ============================================================================

# Install development tools
install-tools:
	cargo install cargo-watch
	@echo ""
	@echo "For faster builds, install lld linker:"
	@echo "  macOS:  brew install llvm"
	@echo "  Linux:  sudo apt install lld clang"
