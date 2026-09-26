# FlowCatalyst Rust — Production Image
# Multi-stage build: frontend (Vite) + backend (Cargo) → distroless runtime
#
# Build from repo root:
#   docker build --platform linux/amd64 -t flowcatalyst-rust .
# The inhance ECS tasks (platform, worker and router) run ARM64: build with
# --platform linux/arm64 for them. One image serves every task, the router
# included (MESSAGE_ROUTER_ENABLED=true, PLATFORM_ENABLED=false selects the
# router role, with no database) — see docs/parity/router-env-vs-go.md.

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
FROM lukemathwalker/cargo-chef:latest-rust-1.92-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY bin ./bin
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: Build Rust dependencies (cached layer) ─────────────────
FROM chef AS builder

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source and build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY bin ./bin
COPY migrations ./migrations
# The version the binary reports (owner decision #33), e.g.
# --build-arg FC_BUILD_VERSION=$(git describe --tags --always); unset or empty
# means the workspace package version.
ARG FC_BUILD_VERSION=
ENV FC_BUILD_VERSION=${FC_BUILD_VERSION}
RUN cargo build --release --bin fc-server

# ── Stage 4: Runtime — distroless (no shell, no package manager) ────
# All TLS is via rustls (no OpenSSL needed). CA certs are bundled.
FROM gcr.io/distroless/cc-debian12:nonroot
LABEL org.opencontainers.image.source=https://github.com/flowcatalyst/flowcatalyst-rust

COPY --from=builder /app/target/release/fc-server /app/fc-server
COPY --from=builder /app/migrations /app/migrations
COPY --from=frontend /app/frontend/dist /app/frontend/dist

EXPOSE 8080

ENTRYPOINT ["/app/fc-server"]
