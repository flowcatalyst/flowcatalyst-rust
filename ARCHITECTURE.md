# FlowCatalyst — Architecture

Top-level system view. For per-component depth see [`docs/architecture/`](docs/architecture/).

---

## System context (C4 L1)

```
┌───────────────────┐                          ┌──────────────────────┐
│ Consumer apps     │  ──── events ──────▶     │   FlowCatalyst       │
│ (SDK / outbox)    │                          │                      │
│                   │  ◀──── webhooks ────     │   Platform · Router  │
└───────────────────┘                          │   Scheduler · Stream │
                                                │   Outbox             │
                                                └──┬───────────┬───────┘
                                                   │           │
                                  ┌────────────────┴──┐   ┌────┴─────────────┐
                                  │   PostgreSQL      │   │   SQS FIFO       │
                                  │   (all state)     │   │   (dispatch)     │
                                  └───────────────────┘   └──────────────────┘
                                                   │           │
                                  ┌────────────────┴──┐   ┌────┴─────────────┐
                                  │   Redis           │   │   Customer IDPs  │
                                  │   (HA leader)     │   │   (OIDC bridge)  │
                                  └───────────────────┘   └──────────────────┘
```

| External system | Purpose |
|---|---|
| PostgreSQL | All FC state — tenants, IAM, events, dispatch jobs, audit, OAuth tokens |
| SQS FIFO | Dispatch-job delivery (scheduler → router) |
| Redis | Leader election for active/standby HA |
| OIDC IDPs (Entra, Keycloak, …) | External authentication via OIDC bridge |
| Webhook endpoints | Delivery to subscriber connections |
| AWS Secrets Manager | Database credential rotation |
| AWS ALB (optional) | Target-group automation on standby transitions |

---

## Container view (C4 L2)

```
┌────────────────────────────────────────────────────────────────────┐
│                  fc-server (unified production binary)              │
│                                                                     │
│   Platform · Router · Scheduler · Stream · Outbox                   │
│                                                                     │
│   Each subsystem toggleable via env. Background subsystems          │
│   gated by Redis leader lock (FC_STANDBY_ENABLED).                  │
└────────────────────────────────────────────────────────────────────┘

Standalone alternatives (for separation of scaling concerns):

   fc-router            fc-platform-server     fc-stream-processor
   fc-outbox-processor  fc-dev (local dev)     fc-mcp-server
```

Binary inventory and deployment topologies: [`docs/operations/topologies.md`](docs/operations/topologies.md).

---

## Event lifecycle

```
1. App publishes event             POST /api/events/batch
2. Platform stores                  INSERT INTO msg_events
3. Stream's event_fan_out           matches subscriptions, INSERTs msg_dispatch_jobs (PENDING)
4. Scheduler polls                  applies paused-connection + blocked-group filters
5. Scheduler publishes              SQS message + UPDATE msg_dispatch_jobs SET status='QUEUED'
6. Router consumes                  per-pool FIFO with rate limit + circuit breaker
7. Router POSTs callback            /api/dispatch/process { messageId }
8. Platform loads + delivers        HTTP POST to msg_dispatch_jobs.target_url
9. Receiver responds                2xx ACK / 4xx terminal-fail / 5xx transient-retry
```

See [`docs/architecture/system-overview.md`](docs/architecture/system-overview.md) for the full end-to-end walkthrough including parallel pipelines (projections, partition manager, recovery loops).

---

## Domain model

```
CLIENT (tenant)
 ├── PRINCIPAL · ROLE · APPLICATION · SERVICE_ACCOUNT
 ├── EVENT_TYPE
 ├── CONNECTION  (webhook endpoint)
 ├── SUBSCRIPTION  (event type → connection)
 ├── DISPATCH_POOL  (rate-limit + concurrency)
 ├── DISPATCH_JOB  (one per event × subscription)
 │    └── DISPATCH_ATTEMPT
 └── SCHEDULED_JOB  (cron-driven)
      └── SCHEDULED_JOB_INSTANCE
           └── SCHEDULED_JOB_INSTANCE_LOG

IDENTITY_PROVIDER  · EMAIL_DOMAIN_MAPPING  · ANCHOR_DOMAIN
```

Each aggregate lives in its own directory under `crates/fc-platform/src/`. Convention covered in [`docs/architecture/platform-control-plane.md`](docs/architecture/platform-control-plane.md).

All entity IDs are **typed TSIDs** — `clt_`, `usr_`, `evt_`, `sub_`, `djb_`, … see [`docs/architecture/system-overview.md#id-format-tsid`](docs/architecture/system-overview.md#id-format-tsid).

---

## CQRS, fan-out, partitioning

```
msg_events             event_projection ──▶ msg_events_read
                  fan-out (subscriptions) ──▶ msg_dispatch_jobs
msg_dispatch_jobs       dispatch_job_projection ──▶ msg_dispatch_jobs_read
```

Write tables are lean (only partial indexes for the projection claim queries). Read tables have all the indexes the API needs. Splitting keeps transactional paths fast.

Seven tables are RANGE-partitioned monthly on `created_at` — managed by an in-Rust `PartitionManagerService`, **no `pg_partman` required**. See [`docs/architecture/partitioning.md`](docs/architecture/partitioning.md).

---

## Authentication

```
   Customer IDP                          Local password                    Service account
   (Entra / Keycloak / Google)           (Argon2id)                        (OAuth client_credentials)
        │                                     │                                  │
        ▼                                     ▼                                  ▼
   OIDC bridge (jwks_cache,             auth_service                      oauth/token endpoint
    validates ID token)                  (verify hash)                     (verify client secret)
        │                                     │                                  │
        └─────────────────┬───────────────────┴──────────────────────────────────┘
                          ▼
              AuthService::generate_access_token
                          │
                          ▼
               RS256-signed FC JWT
                  · scope (Anchor/Partner/Client)
                  · clients[]    (tenant access)
                  · roles[], applications[]
                  · email
```

Detail: [`docs/architecture/auth-and-oidc.md`](docs/architecture/auth-and-oidc.md). Operator setup: [`docs/operations/identity-and-auth.md`](docs/operations/identity-and-auth.md).

---

## High availability

When `FC_STANDBY_ENABLED=true`:

| Subsystem | Behaviour |
|---|---|
| Platform API | runs on every node |
| Router, Scheduler, Stream, Outbox | leader-only (Redis lock) |
| Migrations | per-node at startup; `_schema_migrations` serialises |

Failover: ≤ `FC_STANDBY_LOCK_TTL_SECONDS` (default 30 s). Detail: [`docs/operations/high-availability.md`](docs/operations/high-availability.md).

---

## UseCase + UnitOfWork seal

Every platform write goes through a `UseCase` impl that ends with a `UnitOfWork::commit(...)` call. `UseCaseResult::success` is `pub(in crate::usecase)` — only UoW can construct it. **Compile-time guaranteed** that no use case can return success without going through UoW.

UoW writes the aggregate, a domain event into `msg_events`, and an audit log into `aud_logs` — all in one Postgres transaction. The stream processor then fan-outs the event to dispatch jobs.

Detailed: [`docs/architecture/platform-control-plane.md#usecase--unitofwork`](docs/architecture/platform-control-plane.md#usecase--unitofwork).

Infrastructure paths that bypass UoW (ingest, status transitions, OAuth state, scheduled-job firings, role seeding) are an explicit exception list documented in `CLAUDE.md`. Wrapping them would emit recursive domain events.

---

## Crate layout

```
fc-server / fc-dev / fc-router / fc-platform-server / fc-stream-processor / fc-outbox-processor
        │
        ▼
fc-platform   fc-router   fc-stream   fc-outbox   fc-mcp   fc-sdk
        │           │          │         │           │       │
        └───────┬───┴──────────┴─────────┴───────────┴───────┘
                ▼
fc-common · fc-queue · fc-standby
```

Detail: [`docs/architecture/shared-crates.md`](docs/architecture/shared-crates.md).

---

## Read this next

- For **building applications on FlowCatalyst**: [`docs/developers/`](docs/developers/).
- For **deploying and running**: [`docs/operations/`](docs/operations/).
- For **working on FlowCatalyst itself**: [`docs/architecture/`](docs/architecture/) (component depth) and [`CLAUDE.md`](CLAUDE.md) (conventions).
