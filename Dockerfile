# FlowCatalyst Rust — Production Image
# Multi-stage build: frontend (Vite) + backend (Cargo) → distroless runtime
#
# Build from repo root:
#   docker build --platform linux/amd64 -t flowcatalyst-rust .

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
