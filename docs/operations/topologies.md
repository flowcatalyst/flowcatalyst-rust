# Deployment Topologies

FlowCatalyst can be deployed in five shapes. Each is appropriate for a different operating scale and team setup. All five are first-class — no topology is a "demo" topology, all are run in production by someone.

This document supersedes the older `docs/builds.md`.

---

## Binary inventory

| Binary | Purpose | Recommended for |
|---|---|---|
| `fc-server` | Unified production binary, all subsystems toggleable (the platform API alone is its default role) | Most deployments, and every tier of a split topology |
| `fc-router` | Standalone SQS consumer + webhook delivery | Split topologies, separate IAM |
| `fc-stream-processor` | Projections, fan-out, partition manager | Split topologies |
| `fc-outbox-processor` | Application outbox dispatcher (sidecar for apps) | Always — runs alongside each app |
| `fc-dev` | Development monolith with embedded PG + SQLite queue | Local dev only |
| `fc-mcp-server` | Read-only MCP server for LLM clients | Optional |

---

## Topology 1 — Single instance, all subsystems

The simplest production topology. One node runs everything.

```
┌──────────────────────────────────────────┐
│            fc-server                     │
│                                          │
│   Platform · Router · Scheduler ·        │
│   Stream · Outbox                        │
└──────────────────────────────────────────┘
            │           │
            ▼           ▼
       PostgreSQL    SQS FIFO
```

Configuration:

```sh
FC_PLATFORM_ENABLED=true \
FC_ROUTER_ENABLED=true \
FC_SCHEDULER_ENABLED=true \
FC_STREAM_PROCESSOR_ENABLED=true \
FC_OUTBOX_ENABLED=false \
FC_DATABASE_URL=postgresql://... \
FLOWCATALYST_CONFIG_URL=http://localhost:3000/api/config/router \
  fc-server
```

Note `FLOWCATALYST_CONFIG_URL` points at the same process (the platform API serves the router config endpoint). Self-referential is fine — the router only fetches config on a 5-minute interval, not on the critical path.

When to use:
- Single-region deployment, single tenant, modest throughput.
- Development staging environments.

When not:
- HA matters — a node restart pauses dispatch for the restart duration.
- Sharing scaling: API traffic is independent of dispatch throughput; one large node is wasteful if you have lots of webhook traffic but few API users.

---

## Topology 2 — Active/Standby HA

Two `fc-server` nodes, one Redis lock. The platform API runs on both; background subsystems only on the leader.

```
        ┌──────────────┐         ┌──────────────┐
        │  fc-server-1 │◀───lock──▶│  fc-server-2 │
        │  LEADER      │         │  STANDBY     │
        │              │         │              │
        │  API:up      │         │  API:up      │
        │  Router:up   │         │  Router:off  │
        │  Scheduler:up│         │  Scheduler:  │
        │              │         │           off│
        │  Stream:up   │         │  Stream:off  │
        └──────────────┘         └──────────────┘
                │                       │
                └──────┬────────────────┘
                       ▼
              LB / ALB → API
                       │
                       ▼
                  PostgreSQL
                       │
                       ▼
                     SQS
                       │
                       ▼
                    Redis
```

Configuration (both nodes, identical):

```sh
FC_PLATFORM_ENABLED=true \
FC_ROUTER_ENABLED=true \
FC_SCHEDULER_ENABLED=true \
FC_STREAM_PROCESSOR_ENABLED=true \
FC_STANDBY_ENABLED=true \
FC_STANDBY_REDIS_URL=redis://redis.internal:6379 \
FC_STANDBY_LOCK_KEY=fc:server:leader \
FC_DATABASE_URL=postgresql://... \
FLOWCATALYST_CONFIG_URL=http://localhost:3000/api/config/router \
  fc-server
```

Failover characteristics:

- **Leader crash:** the lock expires after `FC_STANDBY_LOCK_TTL_SECONDS` (default 30 s). The standby acquires it on its next refresh tick. Worst-case dispatch pause: 30 s.
- **In-flight work:** the scheduler's stale-recovery (15 min default) catches anything stuck in QUEUED; the outbox processor's recovery catches stuck IN_PROGRESS. No work is lost.
- **Platform API:** unaffected. Both nodes serve API traffic; the LB sees both as healthy.

ALB integration (optional, `alb` feature) registers the leader in an AWS target group on leadership acquisition, deregisters on loss. Useful when standby pairs need to appear as a single endpoint to external callers (rare — most callers go through an LB anyway).

When to use:
- Any production deployment with availability SLOs better than "best effort".
- Multi-AZ deployments where you want one node per AZ.

When not:
- Brief outages are acceptable (dev/staging — Topology 1 is cheaper).

---

## Topology 3 — Split services

Run each subsystem in its own binary, scale independently.

```
   ┌───────────────────────────┐
   │  fc-server, platform (n)  │  ← scales horizontally behind LB
   └──────────────┬────────────┘
                  │
   ┌──────────────┼─────────────┬─────────────┬─────────────┐
   │              │             │             │             │
   ▼              ▼             ▼             ▼             ▼
fc-router      fc-stream-    fc-outbox-   PostgreSQL    Redis
(active/       processor     processor
 standby pair) (1 leader)   (per app)
   │              │             │
   ▼              ▼             │
  SQS            (reads PG)     ▼
                              Platform /api/events/batch
```

Per-binary configuration:

```sh
# Platform API tier — N instances, no background work (fc-server's
# default role: platform on, every background subsystem off)
fc-server  \
  FC_API_PORT=3000  \
  FC_DATABASE_URL=postgresql://...

# Router tier — active/standby pair
fc-router  \
  API_PORT=8080  \
  FLOWCATALYST_CONFIG_URL=http://platform:3000/api/config/router  \
  FLOWCATALYST_STANDBY_ENABLED=true  \
  FLOWCATALYST_REDIS_URL=redis://...  \
  FLOWCATALYST_LOCK_KEY=fc:router:leader

# Stream / scheduler tier — single active instance
fc-server  \
  FC_PLATFORM_ENABLED=false  \
  FC_SCHEDULER_ENABLED=true  \
  FC_STREAM_PROCESSOR_ENABLED=true  \
  FC_STANDBY_ENABLED=true  \
  FC_STANDBY_REDIS_URL=redis://...  \
  FC_STANDBY_LOCK_KEY=fc:processors:leader

# Optional: dedicated stream processor (if scheduler and stream want
# different leader keys or scaling)
fc-stream-processor  \
  FC_DATABASE_URL=postgresql://...
```

When to use:
- The API tier sees much more traffic than dispatch (separate scaling).
- IAM separation: the router needs SQS permissions; the platform shouldn't.
- Different node sizes per role (small API nodes, one big dispatch node).
- You want the router to deploy on a different cadence than the platform.

When not:
- Operational overhead exceeds the benefit (typically below ~1k events/sec).
- The team isn't comfortable managing five rolling deploys.

---

## Topology 4 — Hybrid: platform standalone, background unified

Common compromise — platform API scales horizontally; one node handles all background work.

```
   ┌────────────────────────────┐
   │  fc-server, platform (n)   │
   └──────────────┬─────────────┘
                  │
   ┌──────────────┼─────────────┐
   │              │             │
   ▼              ▼             ▼
   │           PostgreSQL    Redis
   │
   ▼
┌────────────────────────────────┐
│  fc-server (active/standby)    │
│  no platform, all background   │
│                                │
│  Router · Scheduler · Stream   │
│  Outbox (optional)             │
└────────────────────────────────┘
```

Configuration:

```sh
# Platform tier (fc-server's default role)
fc-server  FC_DATABASE_URL=...

# Background tier
fc-server  \
  FC_PLATFORM_ENABLED=false  \
  FC_ROUTER_ENABLED=true  \
  FC_SCHEDULER_ENABLED=true  \
  FC_STREAM_PROCESSOR_ENABLED=true  \
  FC_STANDBY_ENABLED=true  \
  FC_STANDBY_REDIS_URL=redis://...
```

When to use:
- API and dispatch have different scaling profiles.
- You want one leader lock for all background work (lower coordination overhead than Topology 3).

---

## Topology 5 — Application sidecar (outbox processor)

Independent of the above. Every application that publishes events runs its own `fc-outbox-processor` alongside its app process.

```
┌────────────────────────────────────────┐
│  Customer Application                  │
│                                        │
│  Business logic                        │
│  ┌────────────────┐                    │
│  │  PG / SQLite   │  outbox_messages   │
│  └────────┬───────┘                    │
│           │                            │
│  fc-outbox-processor                   │
│  (sidecar process)                     │
└──────────┼─────────────────────────────┘
           │
           │ HTTPS  POST /api/events/batch
           │        POST /api/dispatch-jobs/batch
           │        POST /api/audit-logs/batch
           ▼
   ┌──────────────────────┐
   │ FlowCatalyst Platform│
   └──────────────────────┘
```

One `fc-outbox-processor` per application database, run as a sidecar container or k8s sidecar. Crash-safe: even if the app process dies mid-business-flow, the outbox row was committed in the business transaction and gets delivered eventually.

For HA: run two processors, enable standby with a per-application lock key.

```sh
FC_OUTBOX_DB_TYPE=postgres
FC_OUTBOX_DB_URL=postgresql://app-db.internal/myapp
FC_API_BASE_URL=https://flowcatalyst.example.com
FC_API_TOKEN=fc_svc_abc123...               # service account creds
FC_STANDBY_ENABLED=true
FC_STANDBY_REDIS_URL=redis://app-redis:6379
FC_STANDBY_LOCK_KEY=app-myapp-outbox-leader   # unique per outbox
fc-outbox-processor
```

This topology is independent of how the platform itself is deployed (it can co-exist with topologies 1-4).

---

## Choosing a topology

```
              Throughput
                  ▲
                  │
   Topology 3     │     ─────────  Topology 4
   (split for    ─┼──── (hybrid)
   scaling)      │
                  │
                  │
   Topology 2    ─┼────  Topology 1
   (HA)           │      (single)
                  │
                  └──────────────▶  Operational complexity
```

Decision tree:

1. **Local dev?** → `fc-dev`. Done.
2. **Throughput < 100 events/sec, downtime tolerable?** → Topology 1.
3. **Throughput < 100 events/sec, downtime not tolerable?** → Topology 2.
4. **API traffic dominant?** → Topology 4.
5. **Need separate IAM/scaling per subsystem?** → Topology 3.
6. **Publishing events from your app?** → Topology 5 in addition to whichever above.

You can change topology by changing env vars — the binaries themselves are the same. No code or DB changes required.

---

## fc-dev (local development)

Not a production topology, but worth mentioning here. `fc-dev` is the all-in-one dev monolith.

```
fc-dev
   ├── Platform API
   ├── Router (with SQLite queue, not SQS)
   ├── Scheduler
   ├── Stream processor
   ├── Outbox processor (optional)
   ├── Embedded Postgres (optional, `embedded-db` feature)
   └── Embedded frontend (rust-embed of frontend/dist/)
```

Runs in one process. No external dependencies if `--embedded-db` is on (PG binary is bundled into the executable). Used for:

- Application developers running the platform locally to integrate against.
- Demos.
- Integration tests.

Not used in production. See [developers/quickstart.md](../developers/quickstart.md) for full setup.

---

## Health endpoints (every binary)

Two ports per binary: the API port (varies) and the metrics port (default 9090).

| Path | Port | Use |
|---|---|---|
| `GET /health` | metrics | Combined health JSON including subsystem status + leader status |
| `GET /metrics` | metrics | Prometheus scrape target |
| `GET /q/live` | API | Kubernetes liveness probe (router/standalone binaries) |
| `GET /q/ready` | API | Kubernetes readiness probe |

The combined health on `fc-server`:

```json
{
  "status": "UP",
  "leader": true,
  "version": "0.4.0",
  "components": {
    "platform":         "UP",
    "router":           "UP" | "STANDBY" | "DISABLED",
    "scheduler":        "UP" | "STANDBY" | "DISABLED",
    "stream_processor": "UP" | "STANDBY" | "DISABLED",
    "outbox":           "UP" | "STANDBY" | "DISABLED"
  }
}
```

`STANDBY` means the subsystem is enabled but the node is not the leader. `DISABLED` means the subsystem is turned off. `UP` means the subsystem is running on this node.

---

## Code references

- Unified binary: `bin/fc-server/src/main.rs`.
- Subsystem spawners: `bin/fc-server/src/main.rs::spawn_router`, `::spawn_scheduler`, `::spawn_stream_processor`, `::spawn_outbox_processor`.
- Per-binary configuration: `Dockerfile`, `Dockerfile.router`, `justfile`.
- Container assembly examples: `docker-compose.yml`, `docker-compose.dev.yml`.
- Health endpoints: `bin/fc-server/src/main.rs::combined_health_handler`.
