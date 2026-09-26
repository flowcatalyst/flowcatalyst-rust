# System Overview

This is the C4-Level-1 / Level-2 view of FlowCatalyst. For per-component deep-dives, follow the links to the individual architecture docs.

---

## System context (C4 L1)

FlowCatalyst is a multi-tenant event-driven integration platform. Consumer applications publish domain events; the platform routes them through subscriptions to webhook endpoints with rate-limiting, ordering, and retry guarantees.

```
┌───────────────────┐                          ┌──────────────────────┐
│ Consumer apps     │   ──── events  ───────▶  │   FlowCatalyst       │
│ (SDK / outbox)    │                          │                      │
│                   │   ◀─── webhooks  ──────  │   Platform · Router  │
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
                                  │   Redis           │   │   Entra ID,      │
                                  │   (HA leader)     │   │   Keycloak       │
                                  └───────────────────┘   │   (customer IDP) │
                                                          └──────────────────┘
```

| External system | Protocol | Purpose |
|---|---|---|
| PostgreSQL | TCP/TLS | All FC state: tenants, IAM, events, dispatch jobs, audit, OAuth tokens |
| SQS FIFO | AWS SDK | Dispatch-job delivery (scheduler → router) |
| Redis | TCP/TLS | Leader election for active/standby HA |
| Customer IDPs | HTTPS | External authentication (OIDC bridge) |
| Webhook endpoints | HTTPS | Delivery to subscriber connections |
| AWS Secrets Manager | AWS SDK | Database credential rotation in production |
| AWS ALB (optional) | AWS SDK | Target-group automation on standby transitions |
| Teams webhooks (optional) | HTTPS | Operational alerts |

---

## Container view (C4 L2)

```
┌────────────────────────────────────────────────────────────────────┐
│                  fc-server (unified production binary)              │
│                                                                     │
│  ┌─────────────┐ ┌────────────┐ ┌───────────┐ ┌──────────────┐    │
│  │  Platform   │ │ Scheduler  │ │  Router   │ │ Stream       │    │
│  │  API        │ │            │ │  (SQS)    │ │ Processor    │    │
│  │  (Axum)     │ │ (poller +  │ │           │ │ (CQRS, fan-  │    │
│  │             │ │  group     │ │           │ │  out, parts) │    │
│  │             │ │  dispatch) │ │           │ │              │    │
│  └─────────────┘ └────────────┘ └───────────┘ └──────────────┘    │
│  ┌─────────────────────────────────────────┐                       │
│  │  Outbox processor (optional, embedded)  │                       │
│  └─────────────────────────────────────────┘                       │
│                                                                     │
│  Each subsystem toggled via env:                                    │
│      FC_PLATFORM_ENABLED, FC_ROUTER_ENABLED,                        │
│      FC_SCHEDULER_ENABLED, FC_STREAM_PROCESSOR_ENABLED,             │
│      FC_OUTBOX_ENABLED                                              │
│                                                                     │
│  Background subsystems are gated by FC_STANDBY_ENABLED (Redis lock).│
└────────────────────────────────────────────────────────────────────┘

Standalone alternatives (for separation of scaling concerns):

┌──────────────┐  ┌───────────────┐  ┌──────────────────┐
│ fc-router    │  │ fc-platform-  │  │ fc-stream-       │
│ (no PG dep)  │  │ server        │  │ processor        │
└──────────────┘  └───────────────┘  └──────────────────┘
┌──────────────┐  ┌───────────────┐
│ fc-outbox-   │  │ fc-dev        │
│ processor    │  │ (dev monolith,│
│ (sidecar     │  │  embedded SQL)│
│  for apps)   │  │               │
└──────────────┘  └───────────────┘
```

Binary inventory:

| Binary | Subsystems | DB |
|---|---|---|
| `fc-server` | Platform + Scheduler + Router + Stream + Outbox (toggleable) | PostgreSQL |
| `fc-router` | Standalone SQS consumer + HTTP delivery | none (config via HTTP) |
| `fc-stream-processor` | Projections + fan-out + partition mgr | PostgreSQL (small pool) |
| `fc-outbox-processor` | Application outbox dispatcher | Application's own DB |
| `fc-dev` | All subsystems + embedded PG + SQLite queue | embedded |
| `fc-mcp-server` | MCP server for LLMs (read-only) | none (uses platform API) |

Deployment topologies in [operations/topologies.md](../operations/topologies.md).

---

## Event lifecycle, end to end

```
1. Application publishes event (via SDK or outbox)
        │  POST /api/events/batch (with bearer token)
        ▼
2. Platform stores event
        │  INSERT INTO msg_events (CloudEvents 1.0 shape, partitioned)
        ▼
3. Stream processor's event_fan_out claims unfanned events
        │  for each event, find matching subscriptions,
        │  insert msg_dispatch_jobs rows (status = PENDING),
        │  stamp event.fanned_out_at, commit
        ▼
4. Scheduler polls PENDING jobs
        │  filters: paused connections, blocked groups
        │  groups by message_group, in-process semaphore caps concurrent groups
        ▼
5. Scheduler publishes MessagePointer to SQS
        │  pointer payload: { id, pool_code, mediation_target, message_group_id, dispatch_mode }
        │  update msg_dispatch_jobs.status = QUEUED
        ▼
6. Router consumes from SQS
        │  dedup (broker_id and app_message_id checks)
        │  route to ProcessPool by pool_code
        │  per-group FIFO drain (or fully concurrent in IMMEDIATE mode)
        │  rate-limit, circuit-breaker, mediator
        ▼
7. Router POSTs to platform's /api/dispatch/process
        │  with HMAC-signed body { "messageId": "<id>" }
        ▼
8. Platform loads the dispatch job, performs the webhook HTTP POST to the
   subscriber's endpoint, records attempt, updates status
        │  status: PROCESSING → COMPLETED | FAILED | (Retry-After re-pending)
        │  attempt row in msg_dispatch_job_attempts
        │  response back to router: ACK or NACK with delay
        ▼
9. Router ACKs (DeleteMessage) or NACKs (visibility timeout) on SQS
```

Parallel paths during this flow:

- **Stream processor** also projects `msg_events` → `msg_events_read` and `msg_dispatch_jobs` → `msg_dispatch_jobs_read` for API list queries.
- **Partition manager** maintains monthly partitions for the seven high-volume tables.
- **Stale recovery** (in scheduler) catches dispatch jobs stuck in QUEUED past 15 minutes.
- **Outbox recovery** (in outbox processor) catches outbox items stuck in IN_PROGRESS.
- **Lifecycle reaper** (in router) catches in-pipeline entries leaked by panicked callbacks.

Three independent leak-stoppers because the failure modes are independent — losing one (or all three) silently is worse than the cost of running them.

---

## Domain model

```
CLIENT (tenant)
 ├── PRINCIPAL (user or service account)
 │    ├── ROLE assignments (junction iam_principal_roles)
 │    ├── CLIENT access grants (junction iam_client_access_grants)
 │    └── APPLICATION access (junction iam_principal_application_access)
 │
 ├── EVENT_TYPE definitions
 │    └── SPEC_VERSION (schema versions)
 │
 ├── CONNECTION (webhook endpoint)
 │    └── SERVICE_ACCOUNT (auth credentials)
 │
 ├── SUBSCRIPTION (event type → connection binding)
 │    ├── EVENT_TYPE_BINDING (pattern, wildcards)
 │    ├── DISPATCH_POOL (rate-limit / concurrency)
 │    └── CONFIG entries (key-value)
 │
 ├── DISPATCH_JOB (async delivery unit)
 │    └── DISPATCH_ATTEMPT (delivery attempts)
 │
 └── SCHEDULED_JOB (cron definition)
      └── SCHEDULED_JOB_INSTANCE (each firing)
           └── SCHEDULED_JOB_INSTANCE_LOG (per firing log lines)

APPLICATION
 ├── ROLE definitions (application-scoped)
 │    └── PERMISSION grants (iam_role_permissions)
 └── SERVICE_ACCOUNT (machine credentials)

IDENTITY_PROVIDER (external OIDC)
 └── EMAIL_DOMAIN_MAPPING (domain → IDP + scope + roles)

ANCHOR_DOMAIN (platform-admin email domains)
```

Each aggregate lives in its own directory under `crates/fc-platform/src/<name>/`. Convention covered in [platform-control-plane.md](platform-control-plane.md).

### ID format (TSID)

Every aggregate ID is a typed TSID: 3-letter prefix + underscore + 13-char Crockford-Base32 body.

```
clt_0HZXEQ5Y8JY5Z   Client
usr_0HZXEQ6A2B3C4   Principal/User
evt_0HZXEQ7D5E6F7   Event Type
sub_0HZXEQ8G8H9I0   Subscription
djb_0HZXEQ9J1K2L3   Dispatch Job
mev_…               Event
con_…               Connection
dpl_…               Dispatch Pool
rol_…               Role
svc_…               Service Account
app_…               Application
idp_…               Identity Provider
edm_…               Email Domain Mapping
cor_…               CORS Origin
aud_…               Audit Log
```

There are 30 entity types. TSIDs are time-ordered (Crockford encoding of a 64-bit ms timestamp + entropy), URL-safe, case-insensitive, and safe from JavaScript number precision (because they're strings, not bigints).

### Schema layout

| Table prefix | Domain |
|---|---|
| `tnt_*` | Tenancy (clients, anchor domains, CORS, email-domain mappings) |
| `iam_*` | Identity & access (principals, roles, permissions, junctions, login attempts) |
| `app_*` | Applications (registry, openapi specs, client configs) |
| `msg_*` | Messaging (events, event types, subscriptions, connections, pools, jobs, attempts, scheduled jobs, read models) |
| `oauth_*` | OAuth/OIDC state (clients, payloads, login states) |
| `aud_*` | Audit logs |
| `outbox_*` | Platform-side outbox (used by the platform's own UoW) |

Migration history in `migrations/`. See [operations/postgres.md](../operations/postgres.md) for production setup.

---

## CQRS projections

Two tables have write+read separation:

```
msg_events ──── event_projection ──▶ msg_events_read
msg_dispatch_jobs ── dispatch_job_projection ──▶ msg_dispatch_jobs_read
```

Why: the write tables have minimal indexes (a few partial ones for the projection/fan-out claim queries, that's it). The read tables have all the indexes the API needs for filter dropdowns, status counts, etc. Splitting keeps transactional paths fast while still allowing rich list queries.

API endpoints always read from `*_read`. Lag is bounded by the projection batch size and cadence (default 100 rows every 1 s when idle, immediately when busy).

---

## Partitioning

Seven tables are RANGE-partitioned monthly on `created_at`:

```
msg_events
msg_events_read
msg_dispatch_jobs
msg_dispatch_jobs_read
msg_dispatch_job_attempts
msg_scheduled_job_instances
msg_scheduled_job_instance_logs
```

Partition lifecycle is managed by `fc-stream::PartitionManagerService` — pure Rust, no `pg_partman` extension. Same code in dev (embedded PG in fc-dev) and prod (RDS / self-hosted). Defaults: 3 forward partitions, 90-day retention. Detailed in [partitioning.md](partitioning.md).

---

## High availability

Active/standby via Redis leader election (`fc-standby`). When `FC_STANDBY_ENABLED=true`:

| Subsystem | Behaviour |
|---|---|
| Platform API | Runs on every node. Stateless, behind LB. |
| Scheduler | Only on leader. |
| Stream processor | Only on leader. |
| Router | Only on leader, optionally registers with ALB. |
| Outbox | Only on leader. |
| Partition manager | Only on leader (subsystem of stream processor). |

Failover takes one `lock_ttl_seconds` (default 30 s). In-flight work for non-platform subsystems either commits or is reclaimed by the new leader's recovery loops (stale-recovery in the scheduler, recovery task in the outbox).

See [operations/high-availability.md](../operations/high-availability.md).

---

## Configuration model

Two distinct config flows:

1. **Process configuration** — env vars, optional `config.toml`. What each binary reads at startup. See [operations/configuration.md](../operations/configuration.md).
2. **Router configuration** — pools and queues. The router fetches this from the platform's config endpoint every 5 minutes and hot-reloads (no restart needed for pool changes). Pool/queue config is sourced from `msg_dispatch_pools` + Postgres-managed queue definitions, not from a config file.

Anything the operator can change without restart goes through path 2. Anything that requires a redeploy — toggles, ports, DB URLs — goes through path 1.

---

## Cross-component dependencies

```
Application                          Platform                       Router

[outbox row]                         msg_events
   │                                  │
   │ HTTP POST                        │ fan-out
   ▼                                  ▼
fc-outbox-processor ──── POST ───▶ /api/events/batch
                                       │
                                       │ INSERT msg_dispatch_jobs
                                       ▼
                                  msg_dispatch_jobs (PENDING)
                                       │
                                       │ scheduler poll
                                       ▼
                                  SQS publish, status=QUEUED
                                       │
                                       │ SQS poll                    ◀── fc-router
                                       │                                     │ HTTP POST
                                       │                                     ▼
                                  /api/dispatch/process ◀── HMAC body ──── router
                                       │
                                       │ load job, POST target_url
                                       ▼
                                  Webhook receiver
                                       │
                                       │ 2xx / 4xx / 5xx
                                       ▼
                                  attempt record, status update
                                       │
                                       │ ACK/NACK back to router
                                       ▼
                                  SQS DeleteMessage (ack) or NACK
```

Five processes (or however many you've consolidated into fc-server), but logically one pipeline. The hop count is intentional: each hop is a backpressure point, an at-least-once boundary, and a separate failure surface that can be observed independently.

---

## Read this next

- Drilling into a specific component: [message-router](message-router.md), [scheduler](scheduler.md), [stream-processor](stream-processor.md), [outbox-processor](outbox-processor.md), [platform-control-plane](platform-control-plane.md).
- Auth and tenancy: [auth-and-oidc](auth-and-oidc.md).
- Cross-cutting: [shared-crates](shared-crates.md), [partitioning](partitioning.md), [adaptive-concurrency](adaptive-concurrency.md), [architecture-direction](architecture-direction.md).
- Deployment & ops: [operations/](../operations/).
- Building against the platform: [developers/](../developers/).
