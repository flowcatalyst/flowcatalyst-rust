# FlowCatalyst Rust - Development Tasks
#
# Quick start:
#   just setup     — first time: up + migrate + dev
#   just dev       — start fc-dev with hot reload (backend only)
#   just dev-full  — start both backend + frontend with hot reload
#
# Prerequisites:
#   cargo install cargo-watch
#   docker / docker compose
#   pnpm (for frontend)

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
    FC_API_PORT={{ FC_API_PORT }} FC_DATABASE_URL={{ FC_DATABASE_URL }} cargo watch -w crates -w bin -x 'run --bin fc-dev'

# Run fc-dev with debug logging
dev-debug:
    RUST_LOG=debug FC_DATABASE_URL={{ FC_DATABASE_URL }} cargo watch -w crates -w bin -x 'run --bin fc-dev'

# Run fc-dev once (no watch)
run:
    FC_DATABASE_URL={{ FC_DATABASE_URL }} cargo run --bin fc-dev

# ─── fcdev: build and run the local dev server ────────────────────────────
#
# One step each: install the SPA's locked dependencies (the SPA is Go's; its
# lockfile differs from the old Vue app's), build fc-dev (its build script
# builds the SPA), and run it with the embedded Postgres unless
# FC_EMBEDDED_DB=false says to use FC_DATABASE_URL. Extra arguments go to
# fc-dev, e.g. `just fcdev --port 8081`.
#
# The embedded Postgres is the PostgreSQL 18 cluster Go's and Java's fcdev
# use (~/Library/Application Support/flowcatalyst/embedded-pg on macOS, port
# 15432): switching binaries keeps the data. Only one may run at a time;
# fc-dev refuses to start beside another and names it. `just fcdev-stop`
# stops whichever is running (they share one PID file).
#
#   just fcdev              Go SPA at http://localhost:{{ FC_API_PORT }}/
#   just fcdev-web          plus the Topcoat UI trial at /ui (--features web)
#   just fcdev-init …       first run: create the admin (fc-dev init)
#   just fcdev-stop         stop the running fcdev (Rust, Go or Java)

# Build fc-dev (SPA dependencies installed from the lockfile first)
fcdev-build:
    cd frontend && pnpm install --frozen-lockfile
    cargo build -p fc-dev

# Build fc-dev with the Topcoat UI trial (fc-web, --features web)
fcdev-build-web:
    cd frontend && pnpm install --frozen-lockfile
    cargo build -p fc-dev --features web

# Build and run fc-dev (Go SPA)
fcdev *ARGS: fcdev-build (_fcdev-port-free ARGS)
    target/debug/fc-dev {{ ARGS }}

# Build and run fc-dev with the Topcoat UI at /ui
fcdev-web *ARGS: fcdev-build-web (_fcdev-port-free ARGS)
    target/debug/fc-dev {{ ARGS }}

# First run: create the admin and a first application (prompts if no flags)
fcdev-init *ARGS:
    target/debug/fc-dev init {{ ARGS }}

# Stop the running fcdev — Rust, Go or Java — via the shared PID file
fcdev-stop:
    target/debug/fc-dev stop

# Refuse to start when the API port is taken, naming the process holding it
[private]
_fcdev-port-free *ARGS:
    #!/usr/bin/env bash
    port="{{ FC_API_PORT }}"
    args=({{ ARGS }})
    for i in "${!args[@]}"; do
      [ "${args[$i]}" = "--port" ] && port="${args[$((i+1))]}"
      case "${args[$i]}" in --port=*) port="${args[$i]#--port=}";; esac
    done
    if holder=$(lsof -nP -iTCP:"$port" -sTCP:LISTEN 2>/dev/null | awk 'NR==2 {print $1" (pid "$2")"}') && [ -n "$holder" ]; then
      echo "✗ Port $port is in use by $holder. Stop it, or pass --port <other>."
      exit 1
    fi

# ─── SDKs ─────────────────────────────────────────────────────────────────

# Requires fc-dev (or fc-platform-server) to be serving on FC_API_PORT.
#
# The SDKs' generated clients (and their vendored openapi.json) are the
# published ones, generated from the Go platform's spec. This platform's
# spec still uses path-derived operationIds, so regenerating from it would
# rename every generated class and break the published SDK API. Until its
# operationIds match Go's (see docs/sdks.md), the SDK steps refuse to run
# unless FC_SDK_REGEN_FROM_RUST_SPEC=1.
#
# The frontend is not regenerated here: the SPA is Go's and is typed against
# Go's OpenAPI lockfile (frontend/openapi/openapi.json, a copy of
# flowcatalyst-go's api/openapi.lock.json — see frontend/PROVENANCE.md).
# `cd frontend && pnpm api:generate` regenerates its types from that copy.
# Regenerate every SDK from the live platform's OpenAPI spec
regen-sdks:
    @curl -fsS http://localhost:{{ FC_API_PORT }}/q/openapi >/dev/null \
        || (echo "✗ Platform not reachable at http://localhost:{{ FC_API_PORT }}/q/openapi — run 'just run' (or 'just dev') first."; exit 1)
    @[ "${FC_SDK_REGEN_FROM_RUST_SPEC:-}" = "1" ] \
        || (echo "✗ Not regenerating the SDKs from this platform's spec: its operationIds differ from the published SDKs' (docs/sdks.md). Set FC_SDK_REGEN_FROM_RUST_SPEC=1 to override."; exit 1)
    @echo "▸ Refreshing SDK OpenAPI snapshots from /q/openapi"
    @curl -fsS http://localhost:{{ FC_API_PORT }}/q/openapi -o clients/typescript-sdk/openapi/openapi.json
    @curl -fsS http://localhost:{{ FC_API_PORT }}/q/openapi -o clients/laravel-sdk/openapi/openapi.json
    @echo "▸ TypeScript SDK"
    cd clients/typescript-sdk && pnpm build
    @echo "▸ Laravel SDK"
    # XDEBUG_MODE=off — Homebrew's Xdebug defaults to step-debug mode and
    # silently blocks every CLI invocation waiting for a debugger on :9003.
    cd clients/laravel-sdk && XDEBUG_MODE=off php scripts/prepare-openapi.php && XDEBUG_MODE=off vendor/bin/jane-openapi generate --config-file=jane-openapi.php
    @echo "✓ SDKs regenerated"

# ─── Frontend ─────────────────────────────────────────────────────────────

# Install frontend dependencies
frontend-install:
    cd frontend && pnpm install

# Run Vite dev server (hot-reload, proxies API to Rust backend)
frontend-dev:
    cd frontend && VITE_BACKEND_PORT={{ FC_API_PORT }} pnpm dev

# Build frontend for production
frontend-build:
    cd frontend && pnpm build

# Run both Rust backend + Vite frontend concurrently (full dev experience)
dev-full:
    cd frontend && FC_API_PORT={{ FC_API_PORT }} FC_DATABASE_URL={{ FC_DATABASE_URL }} pnpm dev:full

# Run fc-dev serving the built frontend (production-like)
dev-static: frontend-build
    FC_STATIC_DIR=frontend/dist FC_DATABASE_URL={{ FC_DATABASE_URL }} cargo run --bin fc-dev

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

# Build all binaries with --release profile
build-release:
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

build-server:
    cargo build --bin fc-server

# Run unified server (all subsystems via env vars)
run-server:
    FC_DATABASE_URL={{ FC_DATABASE_URL }} cargo run --bin fc-server

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

# Format all code (default-features only — fast)
fmt:
    cargo fmt --all

# `cargo fmt --all` only walks files reachable from each crate's default
# features, so cfg-gated code (e.g. the `oidc-flow`-gated module in
# fc-router) can drift unnoticed until CI's `cargo fmt --check` catches it.
# This recipe formats every .rs file under crates/ and bin/ instead.

# Format every .rs file including feature-gated modules (slower; matches CI)
fmt-all:
    find crates bin -name '*.rs' -not -path '*/target/*' -exec rustfmt --edition 2021 {} +

# Check formatting (matches CI)
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

# ─── Release ───────────────────────────────────────────────────────────────

# Show the current fc-dev version
release-version:
    @grep -E '^version = ' bin/fc-dev/Cargo.toml | head -1 | sed -E 's/^version = "([^"]+)"/\1/'

# Cut an fc-dev release. Pass `patch`, `minor`, `major`, or `X.Y.Z[-suffix]`
release bump:
    #!/usr/bin/env bash
    set -euo pipefail

    if [ -n "$(git status --porcelain)" ]; then
        echo "✗ Working tree is dirty. Commit or stash first." >&2
        git status --short
        exit 1
    fi

    cargo_version=$(grep -E '^version = ' bin/fc-dev/Cargo.toml | head -1 | sed -E 's/^version = "([^"]+)"/\1/')
    clean_re='^[0-9]+\.[0-9]+\.[0-9]+$'

    # The bump base is max(Cargo.toml version, latest published GitHub
    # release). This catches the case where someone hand-tagged a release
    # without going through this recipe — without it, the next bump
    # collides with an already-published tag.
    gh_latest=""
    if command -v curl >/dev/null 2>&1; then
        gh_latest=$(curl -fsSL "https://api.github.com/repos/flowcatalyst/flowcatalyst/releases?per_page=100" 2>/dev/null \
            | tr ',' '\n' \
            | grep '"tag_name"' \
            | sed -e 's/.*"tag_name"[[:space:]]*:[[:space:]]*"//' -e 's/".*//' \
            | grep '^fc-dev/v' \
            | sed 's|^fc-dev/v||' \
            | awk -F. '/^[0-9]+\.[0-9]+\.[0-9]+$/ { printf "%010d%010d%010d %s\n", $1, $2, $3, $0 }' \
            | sort -r \
            | awk 'NR==1 {print $2}')
    fi

    # Pick whichever is higher. Awk's printf-padded sort key works for any
    # plain X.Y.Z; pre-release suffixes are handled below by erroring out.
    if [ -n "$gh_latest" ] && [[ "$gh_latest" =~ $clean_re ]] && [[ "$cargo_version" =~ $clean_re ]]; then
        higher=$(printf '%s\n%s\n' "$cargo_version" "$gh_latest" \
            | awk -F. '{ printf "%010d%010d%010d %s\n", $1, $2, $3, $0 }' \
            | sort -r | awk 'NR==1 {print $2}')
        current=$higher
        if [ "$higher" = "$gh_latest" ] && [ "$gh_latest" != "$cargo_version" ]; then
            echo "  Cargo.toml: $cargo_version (behind)"
            echo "  GitHub:     $gh_latest (latest published)"
            echo "  ↑ bumping from GitHub's latest, not Cargo.toml."
        fi
    else
        current=$cargo_version
        if [ -z "$gh_latest" ]; then
            echo "  (couldn't reach GitHub — falling back to Cargo.toml version: $current)"
        fi
    fi

    case "{{ bump }}" in
        patch|minor|major)
            if [[ ! "$current" =~ $clean_re ]]; then
                echo "✗ Cannot auto-bump '$current' (has prerelease suffix). Pass an explicit version." >&2
                exit 1
            fi
            ;;
    esac

    case "{{ bump }}" in
        patch) new=$(echo "$current" | awk -F. -v OFS=. '{$3++; print}') ;;
        minor) new=$(echo "$current" | awk -F. -v OFS=. '{$2++; $3=0; print}') ;;
        major) new=$(echo "$current" | awk -F. -v OFS=. '{$1++; $2=0; $3=0; print}') ;;
        *)
            if [[ "{{ bump }}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
                new="{{ bump }}"
            else
                echo "✗ '{{ bump }}' is not patch|minor|major|X.Y.Z[-suffix]" >&2
                exit 1
            fi
            ;;
    esac

    if [ "$new" = "$current" ]; then
        echo "✗ Computed version $new is the same as current. Did you forget to bump?" >&2
        exit 1
    fi

    echo ""
    echo "  fc-dev: $current → $new"
    echo ""

    # The bare `version = "..."` line is unique in this file (workspace
    # bump uses `version.workspace = true` elsewhere in the workspace,
    # but fc-dev pins its own version literal).
    awk -v new="$new" '/^version = "[^"]+"$/ && !done {print "version = \"" new "\""; done=1; next} {print}' \
        bin/fc-dev/Cargo.toml > bin/fc-dev/Cargo.toml.tmp
    mv bin/fc-dev/Cargo.toml.tmp bin/fc-dev/Cargo.toml

    echo "Refreshing Cargo.lock…"
    if ! FC_SKIP_FRONTEND_BUILD=1 cargo check -p fc-dev --quiet; then
        echo "✗ cargo check failed. Reverting." >&2
        git checkout -- bin/fc-dev/Cargo.toml Cargo.lock 2>/dev/null || true
        exit 1
    fi

    echo ""
    echo "Changes:"
    git --no-pager diff --stat bin/fc-dev/Cargo.toml Cargo.lock
    echo ""
    read -r -p "Commit 'fc-dev v$new', tag fc-dev/v$new, and push? [y/N] " confirm || confirm="n"
    case "$confirm" in
        y|Y|yes|YES) ;;
        *)
            echo "Aborted. Reverting."
            git checkout -- bin/fc-dev/Cargo.toml Cargo.lock
            exit 1
            ;;
    esac

    git add bin/fc-dev/Cargo.toml Cargo.lock
    git commit -m "fc-dev v$new"
    git tag "fc-dev/v$new"
    git push origin HEAD "fc-dev/v$new"

    echo ""
    echo "✓ Released fc-dev v$new"
    echo ""
    echo "  Workflow:  https://github.com/flowcatalyst/flowcatalyst/actions/workflows/release-fc-dev.yml"
    echo "  Release:   https://github.com/flowcatalyst/flowcatalyst/releases/tag/fc-dev/v$new"

# ─── SDK releases ──────────────────────────────────────────────────────────
#
# The SDKs are released from this repo (see docs/sdks.md). Each SDK has a
# VERSION file (the last released version) that continues the numbering of
# the releases cut from flowcatalyst-go. A release bumps it, commits,
# tags `<sdk>/vX.Y.Z`, and pushes; the split-*-sdk workflow picks the tag
# up and mirrors the SDK to its standalone repo as plain `vX.Y.Z`.

# Bumps VERSION + package.json, tags `typescript-sdk/vX.Y.Z`, pushes.
# Cut a TypeScript SDK release (`patch`, `minor`, `major`, or `X.Y.Z`)
release-ts-sdk bump:
    #!/usr/bin/env bash
    set -euo pipefail
    just _release-sdk ts "{{ bump }}"

# composer.json has no version field (Composer reads the tag), so this
# bumps VERSION, tags `laravel-sdk/vX.Y.Z`, and pushes.
# Cut a Laravel SDK release (`patch`, `minor`, `major`, or `X.Y.Z`)
release-laravel-sdk bump:
    #!/usr/bin/env bash
    set -euo pipefail
    just _release-sdk laravel "{{ bump }}"

# Bumps VERSION + pom.xml, tags `java-sdk/vX.Y.Z`, pushes. No workflow
# publishes it yet (see docs/sdks.md).
# Cut a Java SDK release (`patch`, `minor`, `major`, or `X.Y.Z`)
release-java-sdk bump:
    #!/usr/bin/env bash
    set -euo pipefail
    just _release-sdk java "{{ bump }}"

# Shared SDK-release driver. `kind` is `ts`, `laravel` or `java`.
[private]
_release-sdk kind bump:
    #!/usr/bin/env bash
    set -euo pipefail

    manifest=""
    manifest_pom=""
    case "{{ kind }}" in
        ts)      prefix="typescript-sdk"; manifest="clients/typescript-sdk/package.json" ;;
        laravel) prefix="laravel-sdk" ;;
        java)    prefix="java-sdk";       manifest_pom="clients/java-sdk/pom.xml" ;;
        *) echo "✗ unknown SDK kind: {{ kind }}" >&2; exit 1 ;;
    esac
    version_file="clients/$prefix/VERSION"

    if [ -n "$(git status --porcelain)" ]; then
        echo "✗ Working tree is dirty. Commit or stash first." >&2
        git status --short
        exit 1
    fi
    [ -f "$version_file" ] || { echo "✗ missing $version_file" >&2; exit 1; }

    clean_re='^[0-9]+\.[0-9]+\.[0-9]+$'
    file_version="$(tr -d '[:space:]' < "$version_file")"

    # Highest existing $prefix/vX.Y.Z tag (plain semver only).
    tag_version="$(git tag --list "$prefix/v*" \
        | sed "s|^$prefix/v||" \
        | awk -F. '/^[0-9]+\.[0-9]+\.[0-9]+$/ { printf "%010d%010d%010d %s\n", $1, $2, $3, $0 }' \
        | sort -r | awk 'NR==1 {print $2}')"

    # Bump base = max(VERSION file, highest tag), so a hand-cut tag can never
    # collide with the next computed bump.
    current="$file_version"
    if [ -n "$tag_version" ] && [[ "$file_version" =~ $clean_re ]] && [[ "$tag_version" =~ $clean_re ]]; then
        current="$(printf '%s\n%s\n' "$file_version" "$tag_version" \
            | awk -F. '{ printf "%010d%010d%010d %s\n", $1, $2, $3, $0 }' \
            | sort -r | awk 'NR==1 {print $2}')"
    fi

    case "{{ bump }}" in
        patch|minor|major)
            if [[ ! "$current" =~ $clean_re ]]; then
                echo "✗ Cannot auto-bump '$current' (has a prerelease suffix). Pass an explicit X.Y.Z." >&2
                exit 1
            fi
            ;;
    esac
    case "{{ bump }}" in
        patch) new=$(echo "$current" | awk -F. -v OFS=. '{$3++; print}') ;;
        minor) new=$(echo "$current" | awk -F. -v OFS=. '{$2++; $3=0; print}') ;;
        major) new=$(echo "$current" | awk -F. -v OFS=. '{$1++; $2=0; $3=0; print}') ;;
        *)
            if [[ "{{ bump }}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
                new="{{ bump }}"
            else
                echo "✗ '{{ bump }}' is not patch|minor|major|X.Y.Z[-suffix]" >&2
                exit 1
            fi
            ;;
    esac

    if [ "$new" = "$current" ]; then
        echo "✗ Computed version $new equals current. Nothing to bump." >&2
        exit 1
    fi
    if git rev-parse -q --verify "refs/tags/$prefix/v$new" >/dev/null; then
        echo "✗ Tag $prefix/v$new already exists." >&2
        exit 1
    fi

    echo ""
    echo "  $prefix: $current → $new"
    echo ""

    printf '%s\n' "$new" > "$version_file"

    # Keep package.json's version in lockstep (TS). POSIX [[:space:]] so this
    # works on macOS BSD awk as well as gawk.
    if [ -n "$manifest" ]; then
        awk -v new="$new" '
            /^[[:space:]]*"version":[[:space:]]*"[^"]+",?[[:space:]]*$/ && !done {
                sub(/"version":[[:space:]]*"[^"]+"/, "\"version\": \"" new "\"")
                done=1
            }
            {print}
        ' "$manifest" > "$manifest.tmp" && mv "$manifest.tmp" "$manifest"
        if ! grep -q "\"version\": \"$new\"" "$manifest"; then
            echo "✗ Failed to update version in $manifest. Reverting." >&2
            git checkout -- "$version_file" "$manifest"
            exit 1
        fi
    fi

    # Keep the pom's own <version> (the first one) in lockstep (Java).
    if [ -n "$manifest_pom" ]; then
        awk -v new="$new" '
            /<version>[^<]+<\/version>/ && !done {
                sub(/<version>[^<]+<\/version>/, "<version>" new "</version>")
                done=1
            }
            {print}
        ' "$manifest_pom" > "$manifest_pom.tmp" && mv "$manifest_pom.tmp" "$manifest_pom"
        if ! grep -q "<version>$new</version>" "$manifest_pom"; then
            echo "✗ Failed to update version in $manifest_pom. Reverting." >&2
            git checkout -- "$version_file" "$manifest_pom"
            exit 1
        fi
    fi

    git --no-pager diff --stat -- "$version_file" $manifest $manifest_pom
    echo ""
    echo "  Did you move the CHANGELOG's 'Unreleased' section under v$new?"
    read -r -p "Commit '$prefix v$new', tag $prefix/v$new, and push? [y/N] " confirm || confirm="n"
    case "$confirm" in
        y|Y|yes|YES) ;;
        *)
            echo "Aborted. Reverting."
            git checkout -- "$version_file" $manifest $manifest_pom
            exit 1
            ;;
    esac

    git add -- "$version_file" $manifest $manifest_pom
    git commit -m "$prefix v$new"
    git tag "$prefix/v$new"
    git push origin HEAD "$prefix/v$new"

    echo ""
    echo "✓ Released $prefix v$new"
    echo ""
    echo "  Workflow:  https://github.com/flowcatalyst/flowcatalyst-rust/actions/workflows/split-$prefix.yml"

# ─── Tools ─────────────────────────────────────────────────────────────────

# Install development tools
install-tools:
    cargo install cargo-watch just
    @echo ""
    @echo "For faster builds, install lld linker:"
    @echo "  macOS:  brew install llvm"
    @echo "  Linux:  sudo apt install lld clang"
