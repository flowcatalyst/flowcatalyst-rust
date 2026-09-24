# Configuration Reference

Every environment variable, per binary. Two equivalent names are shown where a legacy TypeScript alias exists (for compatibility with existing ECS task definitions).

For deployment shape (which binary, which subsystem toggles), see [topologies.md](topologies.md).

---

## Core (applies to every binary)

| Variable | Alias | Default | Description |
|---|---|---|---|
| `FC_API_PORT` | `PORT` | `3000` (fc-server) / `8080` (fc-dev) | HTTP API port |
| `FC_METRICS_PORT` | — | `9090` | Metrics + health port |
| `RUST_LOG` | — | `info` | Log level filter (`debug`, `info`, `warn`, `error`, or per-module `fc_router=debug,info`) |
| `FC_LOG_FORMAT` | — | `text` (dev) / `json` (prod) | Log encoding |
| `FC_EXTERNAL_BASE_URL` | `EXTERNAL_BASE_URL` | `http://localhost:{port}` | The OIDC issuer / external URL used in token claims and OIDC redirects |
| `FC_DEV_MODE` | — | `false` | Enable dev data seeding |

---

## Database

Three modes, tried in order. The first one whose required vars are set wins.

### Mode 1 — full connection URL (preferred)

| Variable | Alias | Default | Description |
|---|---|---|---|
| `FC_DATABASE_URL` | `DATABASE_URL` | — | Full PostgreSQL URL: `postgresql://user:pass@host:5432/db` |

### Mode 2 — AWS Secrets Manager

Resolves credentials from Secrets Manager. On RDS-managed rotation, the secret provider polls every `DB_SECRET_REFRESH_INTERVAL_MS` and updates pool connect-options when the password rotates.

| Variable | Alias | Default | Description |
|---|---|---|---|
| `DB_HOST` | — | — | Postgres host |
| `DB_NAME` | — | `flowcatalyst` | Database name |
| `DB_PORT` | — | `5432` | Postgres port |
| `DB_SECRET_ARN` | — | — | Secrets Manager ARN of the credentials JSON (must contain `username` and `password`) |
| `DB_SECRET_PROVIDER` | — | `aws` | Provider type (only `aws` supported today) |
| `DB_SECRET_REFRESH_INTERVAL_MS` | — | `300000` (5 min) | How often to re-read the secret |

### Mode 3 — explicit credentials

| Variable | Alias | Default | Description |
|---|---|---|---|
| `DB_HOST` | — | — | Postgres host |
| `DB_NAME` | — | `flowcatalyst` | Database name |
| `DB_PORT` | — | `5432` | Postgres port |
| `DB_USERNAME` | — | `postgres` | Username |
| `DB_PASSWORD` | — | — | Password (URL-encoded automatically) |

See [postgres.md](postgres.md) for sizing, partitioning, migration discipline.

---

## Subsystem toggles (fc-server only)

| Variable | Alias | Default | Description |
|---|---|---|---|
| `FC_PLATFORM_ENABLED` | `PLATFORM_ENABLED` | `true` | Run the platform REST API |
| `FC_ROUTER_ENABLED` | `MESSAGE_ROUTER_ENABLED` | `false` | Run the SQS message router |
| `FC_SCHEDULER_ENABLED` | `DISPATCH_SCHEDULER_ENABLED` | `false` | Run the dispatch scheduler |
| `FC_STREAM_PROCESSOR_ENABLED` | `STREAM_PROCESSOR_ENABLED` | `false` | Run the CQRS stream processor + fan-out + partition manager |
| `FC_OUTBOX_ENABLED` | `OUTBOX_PROCESSOR_ENABLED` | `false` | Run the embedded outbox processor (uncommon — outbox usually runs as application sidecar) |

---

## High availability (fc-server + standalone background binaries)

| Variable | Alias | Default | Description |
|---|---|---|---|
| `FC_STANDBY_ENABLED` | `STANDBY_ENABLED` | `false` | Enable Redis leader election |
| `FC_STANDBY_REDIS_URL` | `REDIS_URL` | `redis://127.0.0.1:6379` | Redis URL |
| `FC_STANDBY_LOCK_KEY` | — | `fc:server:leader` | Redis lock key (unique per cluster role) |
| `FC_STANDBY_LOCK_TTL_SECONDS` | — | `30` | Lock TTL (worst-case failover lag) |
| `FC_STANDBY_REFRESH_INTERVAL_SECONDS` | — | `10` | Lock renewal cadence |
| `FC_STANDBY_INSTANCE_ID` | — | hostname | This instance's identifier (for diagnostics) |

`fc-router` (standalone) uses the `FLOWCATALYST_*` variants of these. Listed under [router](#message-router) below.

See [high-availability.md](high-availability.md).

---

## Authentication / JWT

| Variable | Alias | Default | Description |
|---|---|---|---|
| `FC_JWT_PRIVATE_KEY_PATH` | — | — | Path to RSA private key (PEM) |
| `FC_JWT_PUBLIC_KEY_PATH` | — | — | Path to RSA public key (PEM) |
| `FLOWCATALYST_JWT_PRIVATE_KEY` | — | — | RSA private key (inline PEM) — for env-injected secrets |
| `FLOWCATALYST_JWT_PUBLIC_KEY` | — | — | RSA public key (inline PEM) |
| `FC_JWT_PUBLIC_KEY_PATH_PREVIOUS` | — | — | Previous public key during key rotation |
| `FC_JWT_ISSUER` | — | derived from `FC_EXTERNAL_BASE_URL` | JWT `iss` claim |
| `FC_ACCESS_TOKEN_EXPIRY_SECS` | — | `3600` (1 h) | Access token TTL |
| `FC_SESSION_TOKEN_EXPIRY_SECS` | — | `28800` (8 h) | Session cookie TTL |
| `FC_REFRESH_TOKEN_EXPIRY_SECS` | — | `2592000` (30 d) | Refresh token TTL |
| `FLOWCATALYST_APP_KEY` | — | — | AES-256 key for encrypting OIDC client secrets at rest. **Required in prod.** |
| `FC_SESSION_COOKIE_SAME_SITE` | — | `Lax` | `Lax` or `Strict` |

If neither file nor inline-env key is set, fc-server auto-generates a pair on first boot and persists to `.jwt-keys/`. **Acceptable in dev only.** Production must provide keys explicitly because auto-gen means every restart rotates the key, invalidating every issued token.

Generate keys:

```sh
openssl genrsa -out jwt-private.pem 2048
openssl rsa  -in jwt-private.pem -pubout -out jwt-public.pem
```

See [identity-and-auth.md](identity-and-auth.md) for IDP setup, rotation procedure.

---

## Router (`fc-router` standalone or `fc-server` with `FC_ROUTER_ENABLED=true`)

| Variable | Default | Description |
|---|---|---|
| `FLOWCATALYST_CONFIG_URL` | — | Pool/queue config URL(s), comma-separated. **Required** unless dev mode. |
| `FLOWCATALYST_CONFIG_INTERVAL` | `300` | Config sync interval (seconds) |
| `FLOWCATALYST_DEV_MODE` | `false` | Use LocalStack SQS + built-in dev config |
| `LOCALSTACK_ENDPOINT` | `http://localhost:4566` | LocalStack endpoint (dev only) |
| `LOCALSTACK_SQS_HOST` | `http://sqs.eu-west-1.localhost.localstack.cloud:4566` | LocalStack SQS host (dev only) |
| `AWS_REGION` | (AWS default chain) | SQS region |
| `AUTH_MODE` | `NONE` | `NONE`, `API_KEY`, or `OIDC` — auth for the router's monitoring API |
| `OIDC_ISSUER_URL`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET` | — | When `AUTH_MODE=OIDC` |

For the standalone binary, standby uses `FLOWCATALYST_*` prefix:

| Variable | Default | Description |
|---|---|---|
| `FLOWCATALYST_STANDBY_ENABLED` | `false` | Enable Redis leader election |
| `FLOWCATALYST_REDIS_URL` | `redis://127.0.0.1:6379` | Redis URL |
| `FLOWCATALYST_LOCK_KEY` | `fc:router:leader` | Lock key |
| `FLOWCATALYST_LOCK_TTL_SECONDS` | `30` | Lock TTL |
| `FLOWCATALYST_HEARTBEAT_INTERVAL_SECONDS` | `10` | Renewal interval |
| `FLOWCATALYST_INSTANCE_ID` | hostname | Instance ID |

Notifications (optional, dispatched to Teams):

| Variable | Default | Description |
|---|---|---|
| `NOTIFICATION_TEAMS_ENABLED` | `false` | Enable Teams webhook notifications |
| `NOTIFICATION_TEAMS_WEBHOOK_URL` | — | Teams webhook URL |
| `NOTIFICATION_MIN_SEVERITY` | `WARN` | `INFO`, `WARN`, `ERROR`, `CRITICAL` |
| `NOTIFICATION_BATCH_INTERVAL` | `300` | Batch window (seconds) |

ALB integration (requires `alb` build feature):

| Variable | Default | Description |
|---|---|---|
| `FC_ALB_ENABLED` | `false` | Register with ALB target group when leader |
| `FC_ALB_TARGET_GROUP_ARN` | — | Target group ARN (required if enabled) |
| `FC_ALB_TARGET_ID` | — | Instance ID or IP (required if enabled) |
| `FC_ALB_TARGET_PORT` | `8080` | Health check port |

Router architecture: [../architecture/message-router.md](../architecture/message-router.md).

---

## Scheduler (`fc-server` with `FC_SCHEDULER_ENABLED=true`)

| Variable | Alias | Default | Description |
|---|---|---|---|
| `FLOWCATALYST_SCHEDULER_ENABLED` | — | `true` | Scheduler-internal toggle (only `true`/`false` are understood; `FC_SCHEDULER_ENABLED` is the subsystem switch) |
| `FLOWCATALYST_SCHEDULER_POLL_INTERVAL_MS` | — | `100` | Pending-job poll cadence |
| `FLOWCATALYST_SCHEDULER_DISPATCH_MODE` | — | `immediate` | Default dispatch mode (case-insensitive; unknown values fall back to `NEXT_ON_ERROR`) |
| `FC_SCHEDULER_MAX_CONCURRENT_GROUPS` | — | `10` | Cap on parallel group dispatch |
| `FC_SCHEDULER_DEFAULT_POOL_CODE` | — | `DISPATCH-POOL` | Pool used when `dispatch_pool_id` is null |
| `FC_SCHEDULER_PROCESSING_ENDPOINT` | `DISPATCH_SCHEDULER_PROCESSING_ENDPOINT` | `http://localhost:8080/api/dispatch/process` | Where the router calls back |

Batch size (100) and the stale-job threshold (15 minutes) are fixed.

Scheduler architecture: [../architecture/scheduler.md](../architecture/scheduler.md).

---

## Stream processor (`fc-server` with `FC_STREAM_PROCESSOR_ENABLED=true`, or `fc-stream-processor` standalone)

| Variable | Default | Description |
|---|---|---|
| `FC_STREAM_EVENTS_ENABLED` | `true` | Toggle event projection |
| `FC_STREAM_EVENTS_BATCH_SIZE` | `100` | Events per projection cycle |
| `FC_STREAM_DISPATCH_JOBS_ENABLED` | `true` | Toggle dispatch-job projection |
| `FC_STREAM_DISPATCH_JOBS_BATCH_SIZE` | `100` | Jobs per projection cycle |
| `FC_STREAM_FAN_OUT_ENABLED` | `true` | Toggle event-to-job fan-out |
| `FC_STREAM_FAN_OUT_BATCH_SIZE` | `200` | Events per fan-out cycle |
| `FC_STREAM_FAN_OUT_SUBS_REFRESH_SECS` | `5` | Subscription cache TTL |
| `FC_STREAM_PARTITION_MANAGER_ENABLED` | `true` | Toggle monthly partition maintenance |

Stream processor architecture: [../architecture/stream-processor.md](../architecture/stream-processor.md).

---

## Outbox processor (`fc-outbox-processor` standalone, or `fc-server` with `FC_OUTBOX_ENABLED=true`)

| Variable | Default | Description |
|---|---|---|
| `FC_OUTBOX_DB_TYPE` | `postgres` | `sqlite`, `postgres`, `mysql`, `mongo` |
| `FC_OUTBOX_DB_URL` | — (required) | Application database URL |
| `FC_OUTBOX_MONGO_DB` | `flowcatalyst` | MongoDB database name (mongo only) |
| `FC_OUTBOX_EVENTS_TABLE` | `outbox_messages` | Per-type table override |
| `FC_OUTBOX_DISPATCH_JOBS_TABLE` | `outbox_messages` | Per-type table override |
| `FC_OUTBOX_AUDIT_LOGS_TABLE` | `outbox_messages` | Per-type table override |
| `FC_OUTBOX_POLL_INTERVAL_MS` | `1000` | Poll cadence when idle |
| `FC_OUTBOX_BATCH_SIZE` | `500` | Max items per poll (across all types) |
| `FC_API_BASE_URL` | `http://localhost:8080` | Platform API base URL |
| `FC_API_TOKEN` | — | Bearer token (required in prod) |
| `FC_API_BATCH_SIZE` | `100` | Items per HTTP POST to platform |
| `FC_MAX_IN_FLIGHT` | `5000` | Cap on claimed-but-undispatched items |
| `FC_GLOBAL_BUFFER_SIZE` | `1000` | Buffer between repo and distributor |
| `FC_MAX_CONCURRENT_GROUPS` | `10` | Active groups dispatching simultaneously |

Outbox architecture: [../architecture/outbox-processor.md](../architecture/outbox-processor.md).

---

## Frontend / static assets

| Variable | Default | Description |
|---|---|---|
| `FC_STATIC_DIR` | — | Path to built frontend assets. If set, serves from disk; otherwise uses embedded assets compiled into the binary. |

The platform binary embeds `frontend/dist/` via `rust-embed`. Override with `FC_STATIC_DIR` during dev to pick up hot-reloaded changes without rebuilding.

---

## fc-dev (development monolith)

Most fc-server vars work in fc-dev. Additional dev-specific:

| Variable / flag | Default | Description |
|---|---|---|
| `--embedded-db` / `FC_EMBEDDED_DB` | `true` | Use bundled embedded Postgres (requires `embedded-db` feature) |
| `--reset-db` / `FC_RESET_DB` | `false` | Wipe the embedded PG data directory at startup |
| `--scheduler-enabled` | `true` | Run the scheduler in-process |
| `--outbox-enabled` | `false` | Run the outbox processor in-process |
| `--pool-concurrency` / `FC_POOL_CONCURRENCY` | `10` | Default pool concurrency |
| `--outbox-db-type` | `sqlite` | Outbox backend |
| `--outbox-db-url` | — | Outbox connection URL |
| `FC_DEV_UPDATE_CHECK` | `true` | Best-effort GitHub release check on startup |

---

## Putting it together

A typical production fc-server invocation:

```sh
FC_DATABASE_URL=postgresql://...                                  \
FC_API_PORT=3000                                                  \
FC_EXTERNAL_BASE_URL=https://platform.example.com                 \
FC_JWT_PRIVATE_KEY_PATH=/secrets/jwt/private.pem                  \
FC_JWT_PUBLIC_KEY_PATH=/secrets/jwt/public.pem                    \
FLOWCATALYST_APP_KEY=$(cat /secrets/app-key)                      \
FC_PLATFORM_ENABLED=true                                          \
FC_ROUTER_ENABLED=true                                            \
FC_SCHEDULER_ENABLED=true                                         \
FC_STREAM_PROCESSOR_ENABLED=true                                  \
FC_STANDBY_ENABLED=true                                           \
FC_STANDBY_REDIS_URL=redis://redis.internal:6379                  \
FLOWCATALYST_CONFIG_URL=http://localhost:3000/api/config/router   \
RUST_LOG=info,fc_router=info,fc_platform=info                     \
FC_LOG_FORMAT=json                                                \
  fc-server
```

And a typical sidecar outbox processor:

```sh
FC_OUTBOX_DB_TYPE=postgres                                       \
FC_OUTBOX_DB_URL=postgresql://app-pg.internal/myapp              \
FC_API_BASE_URL=https://platform.example.com                     \
FC_API_TOKEN=$(cat /secrets/fc-api-token)                        \
FC_STANDBY_ENABLED=true                                          \
FC_STANDBY_REDIS_URL=redis://app-redis.internal:6379             \
FC_STANDBY_LOCK_KEY=app-myapp-outbox-leader                      \
RUST_LOG=info                                                    \
FC_LOG_FORMAT=json                                               \
  fc-outbox-processor
```
