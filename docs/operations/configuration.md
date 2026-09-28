# Configuration Reference

Every environment variable, per binary. FlowCatalyst ships three binaries: `fc-server` (every production role, each behind a flag below), `fc-outbox-processor` (the application-side outbox sidecar) and `fc-dev` (local development). Two equivalent names are shown where a legacy TypeScript alias exists (for compatibility with existing ECS task definitions).

For deployment shape (which binary, which subsystem toggles), see [topologies.md](topologies.md).

---

## Core (applies to every binary)

| Variable | Alias | Default | Description |
|---|---|---|---|
| `FC_API_PORT` | `PORT` | `8080` | HTTP API port |
| `FC_METRICS_PORT` | — | `9090` | Metrics + health port |
| `RUST_LOG` | — | `info` | Log level filter (`debug`, `info`, `warn`, `error`, or per-module `fc_router=debug,info`) |
| `FC_LOG_FORMAT` | — | `text` (dev) / `json` (prod) | Log encoding |
| `FC_EXTERNAL_BASE_URL` | `EXTERNAL_BASE_URL` | `http://localhost:{port}` | The OIDC issuer / external URL used in token claims and OIDC redirects |
| `FC_DEV_MODE` | — | `false` | Enable dev data seeding |

## Diagnostics (every binary)

The full guide is [diagnosing-stuck-processes.md](diagnosing-stuck-processes.md).

| Variable | Default | Description |
|---|---|---|
| `FC_ROUTER_FLIGHT_RECORDER_EVENTS` | `16384` | Router: events the flight recorder keeps (`0` = off) |
| `FC_DIAGNOSTICS_PLATFORM_URL` | `FC_ROUTER_PLATFORM_URL`, else the in-process platform | fc-server: the platform that verifies tokens for `:9090/diagnostics/*` (none: 401) |
| `FC_LOG_SPAN_EVENTS` | unset | `close` logs a line (with its duration) whenever a span closes; debugging only |
| `FC_TOKIO_CONSOLE` / `FC_TOKIO_CONSOLE_BIND` | `false` / `127.0.0.1:6669` | tokio-console server (builds with the `tokio-console` feature) |
| `FC_OTEL_ENABLED`, `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_SERVICE_NAME` | `false`, `http://localhost:4318`, the binary's name | OTLP/HTTP span export (builds with the `otel` feature) |

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

Each role is a flag, with Go's names and truth table (`1/true/yes/on`, `0/false/no/off`). A role that is off costs nothing: no connection, no listener. Only the platform, stream, scheduler, scheduled-job and outbox roles connect to Postgres.

| Variable | Alias | Default | Description |
|---|---|---|---|
| `FC_PLATFORM_ENABLED` | `PLATFORM_ENABLED` | `true` | Run the platform REST API (and serve the SPA) |
| `FC_ROUTER_ENABLED` | `MESSAGE_ROUTER_ENABLED` | `false` | Run the SQS message router (surface under `FC_ROUTER_HTTP_PREFIX`, default `/router`) |
| `FC_SCHEDULER_ENABLED` | `DISPATCH_SCHEDULER_ENABLED` | `false` | Run the dispatch scheduler |
| `FC_SCHEDULED_JOB_ENABLED` | `SCHEDULED_JOB_SCHEDULER_ENABLED` | `false` | Run the scheduled-job cron engine |
| `FC_STREAM_PROCESSOR_ENABLED` | `STREAM_PROCESSOR_ENABLED` | `false` | Run the CQRS stream processor + fan-out + partition manager |
| `FC_OUTBOX_ENABLED` | `OUTBOX_PROCESSOR_ENABLED` | `false` | Run the embedded outbox processor (uncommon — outbox usually runs as application sidecar) |
| `FC_MCP_ENABLED` | — | `false` | Run the read-only MCP server on its own listener ([MCP](#mcp-server-fc-server-with-fc_mcp_enabledtrue)) |
| `FC_FUNCTION_HOST_ENABLED` | — | `false` | Run the function host: WASI components and JS bundles ([function host](#function-host-fc-server-with-fc_function_host_enabledtrue)) |

---

## High availability (fc-server and fc-outbox-processor)

| Variable | Alias | Default | Description |
|---|---|---|---|
| `FC_STANDBY_ENABLED` | `STANDBY_ENABLED` | `false` | Enable Redis leader election |
| `FC_STANDBY_REDIS_URL` | `REDIS_URL` | `redis://127.0.0.1:6379` | Redis URL |
| `FC_STANDBY_LOCK_KEY` | — | `fc:server:leader` | Redis lock key (unique per cluster role) |
| `FC_STANDBY_LOCK_TTL_SECONDS` | — | `30` | Lock TTL (worst-case failover lag) |
| `FC_STANDBY_REFRESH_INTERVAL_SECONDS` | — | `10` | Lock renewal cadence |
| `FC_STANDBY_INSTANCE_ID` | — | hostname | This instance's identifier (for diagnostics) |

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

## Router (`fc-server` with `FC_ROUTER_ENABLED=true`)

The complete contract, Go vs Rust, is [../parity/router-env-vs-go.md](../parity/router-env-vs-go.md); the main ones:

| Variable | Default | Description |
|---|---|---|
| `FLOWCATALYST_CONFIG_URL` | — | Pool/queue config URL(s), comma-separated. **Required** unless dev mode. |
| `FLOWCATALYST_CONFIG_INTERVAL` | `300` | Config sync interval (seconds) |
| `FLOWCATALYST_DEV_MODE` | `false` | Use LocalStack SQS + built-in dev config |
| `LOCALSTACK_ENDPOINT` | `http://localhost:4566` | LocalStack endpoint (dev only) |
| `LOCALSTACK_SQS_HOST` | `http://sqs.eu-west-1.localhost.localstack.cloud:4566` | LocalStack SQS host (dev only) |
| `AWS_REGION` | (AWS default chain) | SQS region |
| `AUTH_MODE` | unset | Auth for the router's API (owner ruling 2). Outside dev mode: unset, `BEARER` or `OIDC` require a platform bearer token holding `platform:messaging:router:view` (reads) or `:operate` (everything else); `NONE` leaves the API open **for now**, with a WARN (decision #43); anything else is ignored with a WARN. In dev mode: `NONE`/unset open, `BASIC` (or a user set) Basic auth, `OIDC`/`OIDC_FLOW` the external-IdP modes, `BEARER` platform tokens. |
| `FC_ROUTER_PLATFORM_URL` | — | The platform whose JWKS verifies router API tokens (and the router's own config credential's origin). Without it, `fc-server` with the platform role verifies against itself; with neither, every protected route answers 401. |
| `FC_ROUTER_DASHBOARD_CLIENT_ID` | — | The public OAuth client the router dashboard signs in through (authorization code + PKCE). Unset: dashboard sign-in off. |
| `FC_ROUTER_AUTH_USER` / `FC_ROUTER_AUTH_PASS` | — | Basic auth for the router's API, dev mode only (ignored with a WARN elsewhere) |
| `OIDC_ISSUER`, `OIDC_AUDIENCE`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET` | — | Dev mode's `AUTH_MODE=OIDC`/`OIDC_FLOW` (an external IdP) |

Standby is fc-server's own election (`FC_STANDBY_*` above), shared by every role in the process. (The removed standalone router also read `FLOWCATALYST_STANDBY_*`; fc-server does not, as Go does not.)

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
| `FC_ALB_DEREGISTRATION_DELAY_SECONDS` | `300` | Longest wait for deregistration to drain |

Build features for the router role: `alb`, `email` (e-mail notifications) and `oidc-flow` (the OIDC-flow router UI auth), e.g. `cargo build --release -p fc-server --features alb`.

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

## Stream processor (`fc-server` with `FC_STREAM_PROCESSOR_ENABLED=true`)

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
| `FC_OUTBOX_BACKEND` / `FC_OUTBOX_DB_TYPE` | `postgres` | `sqlite`, `postgres`, `mysql`, `mongo` (`mongo`: `fc-outbox-processor` only) |
| `FC_OUTBOX_DB_URL` (mongo also `FC_OUTBOX_MONGO_URI`) | — | Application database URL. Required by `fc-outbox-processor`; `fc-server` reads a `postgres` outbox from the platform database when unset (Go) |
| `FC_OUTBOX_MONGO_DB` | `flowcatalyst` | MongoDB database name (mongo only) |
| `FC_OUTBOX_EVENTS_TABLE` | `outbox_messages` | Per-type table override |
| `FC_OUTBOX_DISPATCH_JOBS_TABLE` | `outbox_messages` | Per-type table override |
| `FC_OUTBOX_AUDIT_LOGS_TABLE` | `outbox_messages` | Per-type table override |
| `FC_OUTBOX_POLL_INTERVAL_MS` | `1000` | Poll interval |
| `FC_OUTBOX_BATCH_SIZE` | `100` | Rows claimed per poll (across all types) |
| `FC_OUTBOX_PLATFORM_URL` / `FC_OUTBOX_API_URL` / `FC_API_BASE_URL` | `http://localhost:8080` | Platform API base URL |
| `FC_OUTBOX_PLATFORM_AUTH_TOKEN` / `FC_OUTBOX_TOKEN` / `FC_API_TOKEN` | — | Bearer token (required in prod) |
| `FC_API_BATCH_SIZE` | `100` | Most items per HTTP POST (ungrouped) |
| `FC_OUTBOX_MAX_IN_FLIGHT` / `FC_MAX_IN_FLIGHT` | `1000` | No poll at or above this many in flight |
| `FC_OUTBOX_MAX_CONCURRENT_GROUPS` / `FC_MAX_CONCURRENT_GROUPS` | `10` | Groups sending at once |
| `FC_OUTBOX_MAX_RETRIES` | `3` | Attempts before a retryable failure is final |
| `FC_OUTBOX_BLOCK_ON_ERROR` | `true` | A failed item stops its message group |
| `FC_OUTBOX_ADMIN_PORT` | — | Group admin API on 127.0.0.1 (both binaries) |

Outbox architecture: [../architecture/outbox-processor.md](../architecture/outbox-processor.md).

---

## MCP server (`fc-server` with `FC_MCP_ENABLED=true`)

The read-only MCP server (Go's `StartMCP`), a streamable-HTTP service at `/mcp` (and `GET /health`) on its own listener. It calls the platform over HTTP, so it needs no database; it is not leader-gated. Refuses to boot without credentials.

| Variable | Default | Description |
|---|---|---|
| `FC_MCP_BIND` | `127.0.0.1` | Bind host (localhost only unless set); a `host:port` is also accepted |
| `FC_MCP_PORT` | `8090` | Listener port |
| `FLOWCATALYST_URL` / `FC_MCP_PLATFORM_URL` | `http://localhost:{FC_API_PORT}` | The platform it calls |
| `FLOWCATALYST_CLIENT_ID` / `FLOWCATALYST_CLIENT_SECRET` | — | Its `client_credentials` client (required) |

Locally: `fc-dev mcp` (stdio or `--http`) or `fc-dev --mcp`.

---

## Function host (`fc-server` with `FC_FUNCTION_HOST_ENABLED=true`)

The function host (`crates/fc-fnhost-core`, a drop-in for Java's `fc-fnhost`). It loads WASI 0.2 components (`runtime: component` / `wasm`) and, with fc-server's default `js` cargo feature, JS bundles in V8 isolates (`runtime: js`, `crates/fc-fnhost-js`); its heartbeat reports the runtimes it loads (`["component","js","wasm"]`). It reads its own `FC_FN_*` environment:

| Variable | Default | Description |
|---|---|---|
| `FC_FN_PLATFORM_URL` | — (required) | The platform whose `/control/functions/*` it polls |
| `FC_FN_CLIENT_ID` / `FC_FN_CLIENT_SECRET` | — (required) | Its `client_credentials` client (role `platform:function-host`) |
| `FC_FN_POOL` | `default` | The pool it serves (a DNS label) |
| `FC_FN_HOST_ID` | `<hostname>-<random>` | The id its heartbeats carry |
| `FC_FN_SIGNATURES` / `FC_FN_TRUST_ROOT` | `required` | Artifact signature policy (`off` only with `FLOWCATALYST_DEV_MODE=true`) |
| `FC_FN_CACHE_DIR` | `<tmp>/fc-fn-cache` | Artifact and compiled-module cache |
| `FC_FN_MAX_LOADED` / `FC_FN_MAX_CONCURRENCY` / `FC_FN_MAX_EXECUTING` | `200` / `512` / cores − 1 | Capacity limits. `FC_FN_MAX_EXECUTING` is one host-wide budget: at most that many guests execute on a CPU at once, WASM and JS together. A guest holds a permit only while it runs, never while it awaits I/O (outbound HTTP, a database query, an emit, a timer), and queues for one (FIFO, across both runtimes) before it resumes; one still queued at its deadline answers 504 as any timeout. Each runtime keeps that many threads of its own, so either alone can use the whole budget |
| `FC_FN_TRUSTED_PROXIES` | RFC 1918 + loopback + ULA | Who may set `X-Forwarded-For` on the public listener |
| `FC_DRAIN_TIMEOUT_SECONDS` | `60` | In-flight wait at shutdown |
| `FC_FN_PORT` | `8080` host only / `8095` beside other roles | Private function listener (`/functions/<address>/…`) |
| `FC_FN_PUBLIC_PORT` | `8081` host only / `8096` beside other roles | Public listener for claimed hostnames (`off` disables) |
| `FC_METRICS_PORT` (host only) / `FC_FN_METRICS_PORT` (beside other roles) | `9090` / `9091` | The host's `/health`, `/ready`, `/metrics` |
| `FC_EXIT_AFTER_START` | `false` | Host only: exit 0 right after start-up |
| `FC_FN_MAX_DB_POOLS` | `16` | Distinct function database pools (one per connection a manifest `db[]` names); one more fails that load with `DB_POOL_LIMIT` |
| `FC_FN_DB_MAX_CONNECTIONS_PER_INVOCATION` | `2` | Open transactions one invocation may hold per database (never more than its `db[].poolSize`) |
| `FC_FN_DB_SECRET_REFRESH_SECONDS` | `300` | How often an `aws-sm://` function database secret is re-read (a rotated password reaches its pool); `0` never |
| `FC_FN_JS_SNAPSHOT` | `true` on Linux, `false` elsewhere | JS runtime: make each request's isolate from V8's base snapshot (≈0.7 ms) rather than from scratch (≈2.5 ms). Off by default outside Linux: on macOS, disposing thousands of snapshot-made isolates aborted the process (`docs/function-runner-density.md` §10.1) |

**Function databases** (`docs/developers/functions.md#database-access`): a manifest's `db[]` connections are opened by the host, not the platform — so the host needs network reach to them, and, for `aws-sm://` secrets, `secretsmanager:GetSecretValue` on those secrets for its own IAM role. Each pool is sqlx, lazily connected (nothing at load), sized to the largest `poolSize` of the functions sharing it, idle connections closed after 60 s and every connection replaced after 30 min; a connection is reset (`ROLLBACK` if needed, `DISCARD ALL`) each time it goes back. An unreachable database is logged at WARN (throttled, 10 s) with the `db` name and SQLSTATE, never SQL or a secret.

**Metering** (owner decision #13): `/metrics` also exports `fc_fn_fuel_total`, `fc_fn_invocation_fuel` and `fc_fn_invocation_peak_memory_bytes`, labelled `address` and `client`; `fc_fn_invocations_total{outcome="fuel_exhausted"}` counts calls stopped by `limits.maxFuel`.

**Executing budget** (`FC_FN_MAX_EXECUTING`): `/metrics` exports `fc_fn_executing` (guests executing now, every runtime together), `fc_fn_executing_waiting` (guests ready to run, queued for a permit) and `fc_fn_executing_limit`. A waiting count that stays above zero means the host's CPUs, not the listener's permits, are the bottleneck. Loads are outside the budget: compiling a component and running a bundle's top-level code at load happen on the reconciler's blocking threads.

**Host only** (this flag on, every other role off — `FC_PLATFORM_ENABLED=false` too): `fc-server` is exactly the former `fc-fnhost` daemon — no database, none of `fc-server`'s own listeners, exit 2 naming every bad variable. **Beside other roles** the host runs in the process on its own ports (a port another listener of the process holds refuses the boot), starts once the API listener is bound, and drains first at shutdown.


## Functions control plane (`fc-server` with the platform on)

The platform side of functions: `/api/functions*`, `/control/functions/*` and the wiring done at promote. Every
function host, Rust or Java, talks to it.

| Variable | Default | Description |
|---|---|---|
| `FC_FN_ARTIFACT_STORE` | unset (uploads refused) | Where uploaded artifacts live: `s3://bucket[/prefix]` or `file:///abs/dir`. Hosts download through the platform (`platform://` refs, `GET /control/functions/artifacts/{versionId}`), so they need no access to it |
| `FC_FN_POOL_URL` | `http://fn-{pool}:8080` | The URL a pool's hosts are reached at (subscription and scheduled-job targets are `<url>/functions/<address><path>`). `{pool}` is optional; one per pool is the norm, so the service for pool `jvm` is `fn-jvm` |
| `FC_FN_SIGNATURES` / `FC_FN_TRUST_ROOT` | `required` / Sigstore public-good | Publish-time signature check (`off` only with `FLOWCATALYST_DEV_MODE=true`). A private Sigstore needs the same `trusted_root.json` on the platform and every host |
| `FC_FN_DEFAULT_MAX_DURATION_MS` / `FC_FN_DEFAULT_MAX_CONCURRENCY` / `FC_FN_DEFAULT_WASM_MEMORY_MB` / `FC_FN_DEFAULT_DB_POOL_SIZE` | `30000` / `32` / `64` / `4` | Limits a manifest leaves out |
| `FC_FN_MAX_WARM_PER_HOST` | `200` | Live warm (`"warm": true`) functions one pool may hold; one more is `WARM_CAPACITY_EXCEEDED` at publish |

## JVM function host (Java's `fc-fnhost`)

`runtime: jvm` functions (jars) run on Java's function host, never on a Rust one (owner, 2026-09-28). The host is
`function-host` in `flowcatalyst-javalin`, packaged as the image `flowcatalyst-fnhost`. Build it from the javalin root
with `docker build -f function-host/Dockerfile -t flowcatalyst-fnhost .`, or take the one javalin's `fnhost-image`
workflow rebuilds weekly and pushes to ECR (`FNHOST_ECR_REPOSITORY`). The image is a jlink JRE on Alpine and runs
as user `10001`. Its entrypoint passes `--enable-preview` and fences the heap, direct memory and metaspace from the
container's memory limit (metaspace defaults to half: `FC_JVM_METASPACE_PERCENT=50`, between 10 and 70). A bare
`java -jar flowcatalyst-function-host-*-exec.jar` needs `--enable-preview --enable-native-access=ALL-UNNAMED`.

**Topology.** Run one service per JVM pool, separate from the Rust hosts:

- Name the pool `jvm` (or `jvm-<purpose>`) and the service `fn-<pool>`, the name `FC_FN_POOL_URL`'s default
  resolves.
- Every JVM function's manifest says `"pool": "jvm"`.
- Rust hosts keep their own pools. A pool is one URL, so a request for a jar that lands on a Rust host answers `503`
  (the table in [functions](../developers/functions.md#pools-keep-runtimes-apart)).
- The platform refuses a `jvm` publish to a pool whose live hosts are all Rust hosts. It cannot tell a Java-only pool
  apart, because Java hosts report no runtimes, so the manifest check there always warns `POOL_RUNTIME_UNKNOWN`.

**Credentials.** Create a service account with the role `platform:function-host` and no application access, and
give its OAuth client to the host. Publishing is a separate identity: a pipeline's service account with
`platform:function-publisher` and access to the functions' applications. The owner's signer policy
(`PUT /api/function-policies/{owner}`) must list `jvm` for the pipeline's signer.

| Variable | Default | Description |
|---|---|---|
| `FC_FN_PLATFORM_URL` | — (required) | The platform, as the host reaches it (a Service Connect alias is fine). The host also reads the platform's discovery document and JWKS here to verify `auth: platform` callers; the token issuer comes from that document |
| `FC_FN_CLIENT_ID` / `FC_FN_CLIENT_SECRET` | — (required) | The `platform:function-host` service account's client |
| `FC_FN_POOL` | `default` | The pool it serves: set it (`jvm`) |
| `FC_FN_HOST_ID` | `<hostname>-<random>` | The id its heartbeats carry |
| `FC_FN_SIGNATURES` / `FC_FN_TRUST_ROOT` | `required` | As on the platform; `off` needs `FLOWCATALYST_DEV_MODE=true` |
| `FC_FN_CACHE_DIR` | `/var/lib/fc-fnhost/cache` (image) | Verified artifact cache (a volume) |
| `FC_FN_PORT` | `8080` | The function listener, the pool URL's target |
| `FC_FN_PUBLIC_PORT` | `8081` | Public routes (claimed hostnames); `off` disables. The image exposes only 8080 and 9090 |
| `FC_METRICS_PORT` | `9090` | `/health` (the image's `HEALTHCHECK`), `/ready`, `/metrics` |
| `FC_FN_MAX_LOADED` / `FC_FN_MAX_CONCURRENCY` / `FC_FN_MAX_DB_POOLS` | `200` / `512` / `16` | Capacity limits |
| `FC_FN_TRUSTED_PROXIES` | RFC 1918 + loopback + ULA | Who may set `X-Forwarded-For` on the public listener |
| `FC_DRAIN_TIMEOUT_SECONDS` | `60` | In-flight wait at shutdown. The JVM exits `143` after its shutdown hook. Java's drain only flags the reconciler, so the host's row usually keeps `ACTIVE` until it goes stale |
| `FC_LOG_FORMAT` / `FC_LOG_LEVEL` (or `RUST_LOG`) | JSON off a terminal / `info` | Logging |

The check that this works end to end is `bin/fc-server/tests/jvm_function_host_e2e.rs`; see
[functions](../developers/functions.md#what-is-proven).
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
| `--embedded-db` / `FC_EMBEDDED_DB` | `true` | Use the embedded PostgreSQL 18 cluster shared with Go/Java `fcdev` (requires `embedded-db` feature) |
| `--embedded-db-path` / `FC_EMBEDDED_DB_PATH` | `<userDataDir>/flowcatalyst/embedded-pg` | Cluster directory (cluster in `<path>/data`) |
| `--embedded-db-port` / `FC_EMBEDDED_DB_PORT` | `15432` | Embedded PG port |
| `--embedded-db-extensions-from` / `FC_EMBEDDED_DB_EXTENSIONS_FROM` | — | PG 18 tree to copy PostGIS from |
| `--embedded-db-reset` / `FC_EMBEDDED_DB_RESET` (`--reset-db` / `FC_RESET_DB`) | `false` | Delete the embedded PG directory at startup; the shared default also needs `--confirm-shared-db-reset` |
| `--pid-file` / `FC_DEV_PID_FILE` | `<userDataDir>/flowcatalyst/fcdev.pid` | PID file shared with Go/Java `fcdev` (`fc-dev stop`) |
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
FC_API_PORT=8080                                                  \
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
FLOWCATALYST_CONFIG_URL=http://localhost:8080/api/dispatch/router-config \
FC_ROUTER_PLATFORM_URL=http://localhost:8080                      \
FC_ROUTER_CLIENT_ID=... FC_ROUTER_CLIENT_SECRET=...               \
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
