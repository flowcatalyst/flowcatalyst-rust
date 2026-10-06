# FlowCatalyst Rust — Production Image
# Multi-stage build: frontend (Vite) + backend (Cargo) → distroless runtime
#
# Build from repo root:
#   docker build --platform linux/amd64 -t flowcatalyst-rust .
# The inhance ECS tasks (platform, worker and router) run ARM64: build with
# --platform linux/arm64 for them. One image serves every task, the router
# included (MESSAGE_ROUTER_ENABLED=true, PLATFORM_ENABLED=false selects the
# router role, with no database) — see docs/parity/router-env-vs-go.md.
# fc-server is the only deployed binary: every role (platform, router,
# schedulers, stream, outbox, MCP, function host) is a flag on this image —
# see docs/operations/topologies.md.

# ── Stage 1: Build frontend ─────────────────────────────────────────
FROM node:24-alpine AS frontend

RUN corepack enable && corepack prepare pnpm@latest --activate

WORKDIR /app/frontend

# Copy package manifests + local workspace packages for dependency install cache
COPY frontend/package.json frontend/pnpm-lock.yaml ./
COPY frontend/packages/ ./packages/
RUN pnpm install --frozen-lockfile

COPY frontend/ ./
RUN pnpm build

# ── Stage 2: Plan Rust dependencies ─────────────────────────────────
# Rust 1.98: the toolchain CI (pinned in rust-toolchain.toml) and local
# builds use. The workspace needs at least 1.96 (wasmtime 49's cranelift
# crates declare it); 1.92 no longer builds main.
FROM lukemathwalker/cargo-chef:latest-rust-1.98-bookworm AS chef
WORKDIR /app
# cargo-auditable embeds the dependency list in the binary, readable later
# with `cargo audit bin` or `rust-audit-info` (docs/operations/
# supply-chain.md). Pinned; `--locked` builds it from its own lockfile.
ARG CARGO_AUDITABLE_VERSION=0.7.6
RUN cargo install --locked "cargo-auditable@${CARGO_AUDITABLE_VERSION}"
# lld links the release binary, as `.cargo/config.toml` has it for a Linux
# checkout (GNU ld, the aarch64 default, is 4-6x slower:
# docs/plans/build-speed-2026-09-28.md, section 4.2). The builds below pass
# `-fuse-ld=lld` in RUSTFLAGS (which would replace config rustflags).
RUN apt-get update && apt-get install -y --no-install-recommends lld && rm -rf /var/lib/apt/lists/*

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY bin ./bin
# Workspace members too (`cargo metadata` reads every member's manifest).
COPY harness ./harness
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: Build Rust dependencies (cached layer) ─────────────────
FROM chef AS builder

# Tokio task dumps (GET /diagnostics/task-dump; see
# docs/operations/diagnosing-stuck-processes.md): tokio's `taskdump` feature
# needs `--cfg tokio_unstable`, so the image is built with both. It costs
# nothing measurable until a dump is asked for (tokio's own docs, and
# crates/fc-router/tests/throughput_bench.rs on Linux). Build with
# --build-arg FC_TASKDUMP=0 for an image on stable tokio only; its
# /diagnostics/task-dump then answers 501. The flags must be the same for
# the cook and the build, or the cooked dependencies are thrown away.
ARG FC_TASKDUMP=1
ENV FC_TASKDUMP=${FC_TASKDUMP}

COPY --from=planner /app/recipe.json recipe.json
# fc-dev's optional `web` dependency lives outside the workspace, so the
# recipe does not carry it; cargo still reads its manifest to resolve the
# lockfile (it is never built here).
COPY crates/fc-web ./crates/fc-web
# `--locked`: the versions (and checksums) in Cargo.lock, or fail.
RUN export RUSTFLAGS="-C link-arg=-fuse-ld=lld"; \
    if [ "$FC_TASKDUMP" = "1" ]; then \
      export RUSTFLAGS="$RUSTFLAGS --cfg tokio_unstable" FEATURES="--features fc-server/taskdump"; \
    fi; \
    cargo chef cook --release --locked --recipe-path recipe.json $FEATURES

# Copy source and build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY bin ./bin
COPY harness ./harness
COPY migrations ./migrations
# Read at compile time: the function host's WIT package (wasmtime's
# bindgen!) and the platform's published docs (include_str!).
COPY wit ./wit
COPY docs/published ./docs/published
# The version the binary reports (owner decision #33), e.g.
# --build-arg FC_BUILD_VERSION=$(git describe --tags --always); unset or empty
# means the workspace package version.
ARG FC_BUILD_VERSION=
ENV FC_BUILD_VERSION=${FC_BUILD_VERSION}
RUN export RUSTFLAGS="-C link-arg=-fuse-ld=lld"; \
    if [ "$FC_TASKDUMP" = "1" ]; then \
      export RUSTFLAGS="$RUSTFLAGS --cfg tokio_unstable" FEATURES="--features taskdump"; \
    fi; \
    cargo auditable build --release --locked -p fc-server --bin fc-server $FEATURES

# ── Stage 4: Runtime — distroless (no shell, no package manager) ────
# All TLS is via rustls (no OpenSSL needed). CA certs are bundled.
FROM gcr.io/distroless/cc-debian12:nonroot
LABEL org.opencontainers.image.source=https://github.com/flowcatalyst/flowcatalyst-rust

COPY --from=builder /app/target/release/fc-server /app/fc-server
COPY --from=builder /app/migrations /app/migrations
COPY --from=frontend /app/frontend/dist /app/frontend/dist

EXPOSE 8080

ENTRYPOINT ["/app/fc-server"]
