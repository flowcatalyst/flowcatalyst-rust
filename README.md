# FlowCatalyst

A multi-tenant event router and webhook delivery platform written in Rust.

Applications publish domain events. Other applications (or yours, or external services) consume them via webhook subscriptions. FlowCatalyst handles routing, FIFO ordering, retry, rate-limiting, circuit breaking, and audit — across multiple tenants — so you don't have to reimplement that machinery in every consumer.

---

## Architecture at a glance

```
┌───────────────────┐                          ┌──────────────────────┐
│ Consumer apps     │  ──── events ──────▶     │   FlowCatalyst       │
│ (SDK / outbox)    │                          │                      │
│                   │  ◀──── webhooks ────     │   Platform · Router  │
└───────────────────┘                          │   Scheduler · Stream │
                                                │   Outbox             │
                                                └──┬───────────┬───────┘
                                                   │           │
                                                   ▼           ▼
                                              PostgreSQL    SQS FIFO
                                                   │           │
                                                   ▼           ▼
                                                Redis       Customer IDPs
                                              (HA leader)  (OIDC bridge)
```

Detailed C4-style view in [`ARCHITECTURE.md`](ARCHITECTURE.md) and [`docs/architecture/system-overview.md`](docs/architecture/system-overview.md).

---

## Quick start

### As a developer publishing events

```sh
# 1. Install fc-dev (the local development binary)
curl -fsSL https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.sh | sh

# 2. Run it
fc-dev

# 3. Open http://localhost:8080
```

Next steps:

- [`docs/developers/quickstart.md`](docs/developers/quickstart.md) — five-minute end-to-end walkthrough.
- [`docs/developers/fc-dev.md`](docs/developers/fc-dev.md) — complete CLI reference: every subcommand (`start`, `init`, `fresh`, `outbox poll`, `mcp`, `upgrade`), every flag, every env var, recipes for external Postgres / PostGIS workflows, plus the rationale for the `fc-dev` name.

### As an operator deploying production

Pick a topology (single instance / active-standby HA / split services), configure env vars, deploy `fc-server` (or the standalone binaries). See [`docs/operations/topologies.md`](docs/operations/topologies.md).

### As an engineer working on FlowCatalyst itself

Read [`docs/architecture/system-overview.md`](docs/architecture/system-overview.md), then drill into the component you're touching. Conventions are in [`CLAUDE.md`](CLAUDE.md).

---

## Documentation

Organised by audience:

### Developers (building on the platform)

- [Quickstart](docs/developers/quickstart.md) — fc-dev in 5 minutes
- [fc-dev CLI reference](docs/developers/fc-dev.md) — every subcommand, every flag, the PostGIS / external-Postgres recipe, naming rationale
- [Concepts](docs/developers/concepts.md) — events, subscriptions, pools, dispatch modes
- [Publishing events](docs/developers/publishing-events.md) — outbox pattern, batch API, SDK sync
- [Receiving webhooks](docs/developers/receiving-webhooks.md) — HMAC, ack/nack contract, retries
- [Subscriptions and pools](docs/developers/subscriptions-and-pools.md) — pattern matching, dispatch modes, pool sizing
- [Scheduled jobs](docs/developers/scheduled-jobs.md) — cron-triggered events
- [Debugging](docs/developers/debugging.md) — by symptom

### Operations (deploying and running in production)

- [Topologies](docs/operations/topologies.md) — single instance, HA, split services
- [Configuration reference](docs/operations/configuration.md) — every env var, per binary
- [PostgreSQL](docs/operations/postgres.md) — provisioning, migrations, credentials
- [Queue and router config](docs/operations/queue-and-router-config.md) — SQS, pools, hot reload
- [High availability](docs/operations/high-availability.md) — Redis leader election, failover
- [Secrets and rotation](docs/operations/secrets-and-rotation.md) — JWT keys, app key, DB credentials
- [Identity and auth](docs/operations/identity-and-auth.md) — OIDC IDPs, anchor domains, MFA
- [Observability](docs/operations/observability.md) — Prometheus metrics, logs, health endpoints
- [Runbooks](docs/operations/runbooks.md) — incident playbooks, routine procedures

### Architecture (internal)

- [System overview](docs/architecture/system-overview.md) — C4 L1/L2, domain model, event lifecycle
- [Message router](docs/architecture/message-router.md) — pool semantics, dispatch modes, mediator, circuit breakers
- [Dispatch scheduler](docs/architecture/scheduler.md) — poller, message-group dispatcher, stale recovery
- [Stream processor](docs/architecture/stream-processor.md) — event fan-out, projections, partition manager
- [Outbox processor](docs/architecture/outbox-processor.md) — application-side outbox pattern
- [Platform control plane](docs/architecture/platform-control-plane.md) — DDD layout, UoW seal, BFF vs API
- [Auth and OIDC](docs/architecture/auth-and-oidc.md) — token shape, OIDC bridge, tenancy
- [Shared crates](docs/architecture/shared-crates.md) — fc-common, fc-queue, fc-secrets, fc-standby, fc-sdk, fc-mcp
- [Partitioning](docs/architecture/partitioning.md) — monthly RANGE partitioning, in-Rust manager
- [Adaptive concurrency](docs/architecture/adaptive-concurrency.md) — design notes (not yet shipped)
- [Architecture direction](docs/architecture/architecture-direction.md) — long-term shape
- [Release signing](docs/architecture/release-signing.md) — cosign / Authenticode

---

## Binaries

| Binary | Purpose |
|---|---|
| `fc-server` | Unified production server — all subsystems toggleable via env vars |
| `fc-platform-server` | Platform REST API only (split topologies) |
| `fc-router` | Standalone SQS consumer + webhook delivery |
| `fc-stream-processor` | Projections, fan-out, partition manager |
| `fc-outbox-processor` | Application-side outbox dispatcher (sidecar) |
| `fc-dev` | Local development monolith with embedded PG + SQLite queue; also bundles `init`, `fresh`, `mcp`, and `outbox poll` subcommands — see [`docs/developers/fc-dev.md`](docs/developers/fc-dev.md) |
| `fc-mcp-server` | Read-only MCP server for LLM clients |

See [`docs/operations/topologies.md`](docs/operations/topologies.md) for which to deploy in which topology.

---

## Building from source

```sh
# Build everything (debug)
just build

# Release build
just release

# Build a specific binary
just build-server      # fc-server
just build-dev         # fc-dev
just build-router      # fc-router
just build-platform    # fc-platform-server
just build-stream      # fc-stream-processor
just build-outbox      # fc-outbox-processor
```

Local dev with hot-reload:

```sh
just dev               # fc-dev with cargo-watch
just dev-full          # plus the frontend dev server
```

---

## SDK clients

| Language | Location |
|---|---|
| TypeScript / JavaScript | [`clients/typescript-sdk/`](clients/typescript-sdk/) |
| Laravel / PHP | [`clients/laravel-sdk/`](clients/laravel-sdk/) |
| Rust (in-workspace crate) | [`crates/fc-sdk/`](crates/fc-sdk/) |

All SDKs cover the outbox pattern for atomic event publishing, definition syncing for declaring event types and roles, and webhook signature verification.

---

## Technology stack

| Layer | Technology |
|---|---|
| Async runtime | Tokio |
| Web framework | Axum 0.8 |
| Database | PostgreSQL (handwritten SQLx for new code; older SeaORM being phased out) |
| Queue | AWS SQS FIFO (prod), SQLite (dev), PostgreSQL / ActiveMQ / NATS supported |
| Auth | RS256 JWT, OIDC bridge, Argon2id passwords, HMAC webhook signing |
| Caching | DashMap (in-process), Redis (HA leader election) |
| Metrics | Prometheus, HdrHistogram for percentiles |
| Rate limiting | governor (RFC-compliant token bucket) |
| Secrets | AWS Secrets Manager, AWS SSM, Vault, encrypted files |
| Logging | tracing + tracing-subscriber (JSON or text) |
| API docs | utoipa (OpenAPI / Swagger) |

PostgreSQL extensions: **none required**. Partitioning is managed by an in-Rust service (`fc-stream::PartitionManagerService`) — same code in dev and prod, no `pg_partman` or `pg_cron`.

---

## License

Copyright (c) 2026 Belac (FlowCatalyst).

This repository is dual-licensed by component:

| Component | License | Text |
|---|---|---|
| The platform — every crate and binary under `crates/` and `bin/` (except `fc-sdk`), the `frontend/`, migrations, docs | [GNU AGPL-3.0-or-later](https://www.gnu.org/licenses/agpl-3.0.html) | [`LICENSE`](LICENSE) |
| The SDKs — [`crates/fc-sdk`](crates/fc-sdk) (Rust), [`clients/typescript-sdk`](clients/typescript-sdk), [`clients/laravel-sdk`](clients/laravel-sdk), [`clients/go-sdk`](clients/go-sdk) | [MPL-2.0](https://www.mozilla.org/en-US/MPL/2.0/) | `LICENSE` in each SDK directory |

**Why the split.** The AGPL keeps the platform copyleft even when it is only
run as a hosted service: anyone who modifies it and lets users interact with
it over a network must offer them the modified source. The SDKs are embedded
in customer applications; MPL-2.0 is a file-level copyleft, so changes to the
SDK's own files must be shared, but your application that links against it
stays yours under whatever license you choose. Using an SDK does not make your
application AGPL.
