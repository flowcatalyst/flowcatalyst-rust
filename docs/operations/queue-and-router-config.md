# Queue and Router Configuration

The router pulls its pool and queue configuration from the platform at runtime — there's no static config file. This document covers what to provision in your queue backend, how the dynamic config flows from platform to router, and how to operate it.

For architecture details see [../architecture/message-router.md](../architecture/message-router.md).

---

## Queue backend

Production: **AWS SQS FIFO**. Other backends (ActiveMQ, NATS JetStream, PostgreSQL) are supported in code but less commonly run in production.

### SQS provisioning

One **FIFO** queue per logical queue defined in your platform configuration. (Most deployments have one — `fc-default.fifo` — and route everything through it. Some operators split queues by tenant or by priority.)

Required SQS settings:

| Setting | Value |
|---|---|
| Queue type | **FIFO** (required — standard queues don't preserve `MessageGroupId`) |
| Content-based deduplication | **enabled** (the router uses `Message.id` as the dedup token, but SQS also dedupes on the content hash; both being on is belt-and-braces) |
| Visibility timeout | match `FC_SCHEDULER_STALE_THRESHOLD_MINUTES` (default 15 min). The router extends visibility every 55 s while processing. |
| Message retention period | 4 days (default) |
| Receive message wait time | 20 s (long polling — reduces SQS API calls) |
| Maximum message size | 256 KB (default; we send pointers, not payloads — kilobytes is plenty) |
| DLQ | optional — we re-queue stuck jobs via `stale_recovery`, so a SQS-level DLQ is a secondary safety net |

Terraform example:

```hcl
resource "aws_sqs_queue" "fc_default" {
  name                        = "fc-default.fifo"
  fifo_queue                  = true
  content_based_deduplication = true
  visibility_timeout_seconds  = 900       # 15 min, matches stale_threshold
  message_retention_seconds   = 345600    # 4 days
  receive_wait_time_seconds   = 20

  redrive_policy = jsonencode({
    deadLetterTargetArn = aws_sqs_queue.fc_default_dlq.arn
    maxReceiveCount     = 10
  })
}

resource "aws_sqs_queue" "fc_default_dlq" {
  name       = "fc-default-dlq.fifo"
  fifo_queue = true
}
```

The DLQ is optional. We'd rather catch a stuck message via `stale_recovery` and unblock it explicitly. The DLQ catches the case where stale-recovery itself fails (Postgres outage during recovery, etc.).

### IAM for the router

The router needs `sqs:ReceiveMessage`, `sqs:DeleteMessage`, `sqs:ChangeMessageVisibility`, `sqs:GetQueueAttributes` on each queue. The scheduler additionally needs `sqs:SendMessage`.

If you run router and scheduler in the same `fc-server` process: one IAM role per node, both sets of permissions.

If you split (Topology 3 in [topologies.md](topologies.md)): the router gets the receiver permissions only; the scheduler runs as part of `fc-server` on the platform node and needs send permissions. This is a real benefit of the split topology — the router's IAM gets no write access to SQS.

---

## Router configuration model

The router asks the platform "what pools and queues exist?" every 5 minutes. The endpoint:

```
GET /api/config/router    →    { "processing_pools": [...], "queues": [...] }
```

(In practice, the platform's `bff_dashboard_api` and `platform_config_api` expose this; the actual route name varies between releases. Inspect `crates/fc-platform/src/router.rs` for current routing.)

The response shape:

```json
{
  "processing_pools": [
    {
      "code": "DEFAULT",
      "concurrency": 10,
      "rate_limit_per_minute": null
    },
    {
      "code": "high-volume-emails",
      "concurrency": 50,
      "rate_limit_per_minute": 6000
    }
  ],
  "queues": [
    {
      "name": "fc-default.fifo",
      "uri": "https://sqs.eu-west-1.amazonaws.com/123456789/fc-default.fifo",
      "connections": 2,
      "visibility_timeout": 900
    }
  ]
}
```

### Pools

Each pool defines a logical traffic class. Subscriptions opt into a pool via `dispatch_pool_id` on the subscription. Jobs created from those subscriptions inherit the pool code.

| Field | Meaning |
|---|---|
| `code` | Identifier referenced by `dispatch_pool_id` |
| `concurrency` | Max parallel in-flight messages in this pool |
| `rate_limit_per_minute` | Optional governor (token-bucket); `null` means unlimited |

Operators tune these through the admin UI (`/api/dispatch-pools`). Changes propagate within 5 minutes — no router restart.

### Queues

| Field | Meaning |
|---|---|
| `name` | Display name |
| `uri` | Full SQS URL |
| `connections` | Number of parallel poll tasks against this queue (rarely > 4) |
| `visibility_timeout` | Initial SQS visibility timeout (the router extends this every 55s while processing) |

Most deployments have one queue. Splitting is appropriate when:

- Tenants have wildly different priority (a "VIP" queue with its own DLQ).
- Different teams own different webhook endpoints (split by IAM separation).
- You want different scaling per queue.

---

## Sizing pools

Pool sizing is the main operator lever for managing webhook delivery. The relevant variables:

| Question | Pool setting |
|---|---|
| How fast does the **endpoint** accept work? | `rate_limit_per_minute` — set just below the endpoint's documented rate limit. |
| How many **concurrent** in-flight requests can the endpoint handle? | `concurrency` — start with 10, raise if endpoint can take it. |
| Should the pool **isolate** failures from other workloads? | Yes — give failure-prone endpoints their own pool, so a flapping endpoint can't starve healthy ones. |
| Should the pool **isolate** high-volume from low-volume? | Yes — high-volume gets a dedicated pool; low-volume shares a pool. |

Two anti-patterns:

1. **One pool for everything.** Works at low scale; fails at scale because a single misbehaving endpoint can saturate the concurrency cap and stall every other delivery.
2. **One pool per subscription.** Hundreds of pools is fine — they're cheap (~few KB each idle). But not necessary; group by endpoint behaviour rather than by subscription identity.

For homogeneous workloads (similar nominal latency, similar failure profile), one pool per endpoint family is the sweet spot.

The forthcoming adaptive concurrency feature (see [../architecture/adaptive-concurrency.md](../architecture/adaptive-concurrency.md)) will let pools adjust `concurrency` automatically using the Vegas algorithm. Until that ships, manual tuning is the only knob.

---

## Connections and pause

A `connection` is the webhook endpoint metadata: URL, auth method, signing secret, status. Subscriptions reference connections. When a connection is **paused** (status `PAUSED`), the scheduler skips dispatch jobs whose subscription points at that connection — they stay `PENDING` and resume automatically when the connection is un-paused.

Pause is the operator's "stop the bleeding" lever. Use cases:

- Receiver is down — pause connection, jobs stack up, un-pause when receiver recovers.
- Receiver is misconfigured — pause, fix, un-pause.
- Receiver is being load-tested — pause production traffic during the test.

Pause is **persistent** (lives in `msg_connections.status`). Compare to circuit breakers in the router which are automatic and transient. Both layers exist for different time horizons.

The `PausedConnectionCache` in the scheduler refreshes every 60 seconds, so pause / un-pause takes up to a minute to take effect on already-queued PENDING jobs.

---

## Hot reload mechanics

Config sync runs every `FLOWCATALYST_CONFIG_INTERVAL` (default 300 s). On each sync:

1. Fetch from all `FLOWCATALYST_CONFIG_URL` endpoints (comma-separated; partial failures tolerated).
2. Compute diff against current router state.
3. For new pools: spawn `ProcessPool`.
4. For pools with changed concurrency: resize the `Semaphore` (live, in-flight tasks unaffected — they retain their old permit until completion, new tasks see the new cap).
5. For pools with changed rate limit: swap the governor in place.
6. For removed pools: move to `draining_pools`, in-flight work finishes, then drop.
7. For new queues: spawn an SQS consumer + poll task.
8. For removed queues: stop polling, finish in-flight, drop.

A misbehaving config endpoint (returns 500s, slow response, malformed JSON) is **non-fatal** — the router retains its current config and retries on the next interval. Initial sync at startup is the one exception: if the router can't get an initial config (12 attempts × 5 s delay = 1 minute), it exits with an error. Don't deploy the router before the platform is reachable.

---

## DEFAULT-POOL invariant

The router uses `DEFAULT-POOL` as the fallback when a message's `pool_code` doesn't match any known pool. If `DEFAULT-POOL` doesn't exist either, the message gets NACKed and bounced through SQS forever.

**Always have a `DEFAULT-POOL`** in your pool configuration. It can have tiny concurrency (1) — the point is to exist so that an unknown-pool message can drain somewhere instead of hot-looping.

---

## Switching backends

Most deployments stay on SQS. If you need to migrate to a different backend (NATS, ActiveMQ, PostgreSQL queue):

1. Provision the new backend.
2. Update the queue config in the platform admin UI (or directly in `msg_dispatch_pools` and queue tables; varies by release).
3. Roll the routers: new pods pick up the new config; old pods finish draining their SQS consumers.
4. Drain SQS: keep one router pointed at SQS until `ApproximateNumberOfMessages = 0`.
5. Delete SQS queues.

The `QueueConsumer` / `QueuePublisher` trait abstraction (`crates/fc-queue/`) means the router code doesn't change. The risk is in the migration choreography, not in code.

---

## LocalStack for development

`FLOWCATALYST_DEV_MODE=true` switches the router to LocalStack SQS. LocalStack provides an SQS-compatible API endpoint at `localhost:4566`; the router reads `LOCALSTACK_ENDPOINT` and creates SQS queues against it.

For `fc-dev` development this is automatic. For ad-hoc testing on a workstation:

```sh
docker-compose -f docker-compose.dev.yml up -d  # starts LocalStack
FLOWCATALYST_DEV_MODE=true \
LOCALSTACK_ENDPOINT=http://localhost:4566 \
LOCALSTACK_SQS_HOST=http://sqs.eu-west-1.localhost.localstack.cloud:4566 \
FC_PLATFORM_ENABLED=false FC_ROUTER_ENABLED=true \
  fc-server
```

The default queue (`fc-default.fifo`) and pool (`DEFAULT`) are baked into the dev-mode config; you don't need to provision anything.

---

## Code references

- Router config sync: `crates/fc-router/src/config_sync.rs`.
- SQS consumer/publisher: `crates/fc-queue/src/sqs.rs`.
- Pool definition: `crates/fc-common/src/lib.rs::PoolConfig`.
- Dispatch pool aggregate: `crates/fc-platform/src/dispatch_pool/`.
- Connection aggregate: `crates/fc-platform/src/connection/`.
- Paused-connection filter: `crates/fc-platform-messaging/src/scheduler/poller.rs::PausedConnectionCache`.
