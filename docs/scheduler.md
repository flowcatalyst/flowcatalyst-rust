# Scheduler

The Scheduler polls pending dispatch jobs from MongoDB and queues them for delivery by the Message Router.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Scheduler                                      │
│                                                                          │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                       Job Poller                                  │   │
│  │                                                                   │   │
│  │   Poll interval: 1s (configurable)                               │   │
│  │   Batch size: 100 (configurable)                                 │   │
│  │                                                                   │   │
│  │   Query: status = PENDING AND scheduled_at <= NOW()              │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                    │                                     │
│                                    ▼                                     │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                    Job Processor                                  │   │
│  │                                                                   │   │
│  │   1. Mark job as PROCESSING                                      │   │
│  │   2. Build message from job + event                              │   │
│  │   3. Publish to queue                                            │   │
│  │   4. Update job status (QUEUED/FAILED)                           │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                    │                                     │
│                                    ▼                                     │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                   Queue Publisher                                 │   │
│  │                                                                   │   │
│  │   SQS / SQLite / HTTP (to FlowCatalyst Router)                   │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                    │                                     │
│                                    ▼                                     │
│                          ┌─────────────────┐                            │
│                          │  Message Queue  │                            │
│                          └─────────────────┘                            │
└─────────────────────────────────────────────────────────────────────────┘
```

## Dispatch Job Lifecycle

```
┌─────────┐     ┌────────────┐     ┌─────────┐     ┌───────────┐
│ PENDING │────▶│ PROCESSING │────▶│ QUEUED  │────▶│ DELIVERED │
└─────────┘     └────────────┘     └─────────┘     └───────────┘
                      │                  │
                      │                  │ (Router reports)
                      ▼                  ▼
               ┌────────────┐     ┌───────────┐
               │   FAILED   │     │   FAILED  │
               │ (queue err)│     │(delivery) │
               └────────────┘     └───────────┘
```

### Job States

| State | Description |
|-------|-------------|
| `PENDING` | Job created, waiting to be scheduled |
| `PROCESSING` | Scheduler picked up, building message |
| `QUEUED` | Message published to queue |
| `DELIVERED` | Router successfully delivered |
| `FAILED` | Terminal failure after retries |

## Components

### Job Poller (`fc-scheduler/src/lib.rs`)

Polls MongoDB for pending jobs:
- Configurable poll interval (default 1 second)
- Batched queries for efficiency
- Respects `scheduled_at` for delayed delivery
- Detects stale jobs (stuck in PROCESSING)

### Job Processor

Transforms jobs into queue messages:

```rust
// Dispatch job to queue message transformation
fn job_to_message(job: &DispatchJob, event: &Event) -> Message {
    Message {
        id: generate_id(),
        pool_code: job.pool_code.clone(),
        mediation_type: MediationType::Http,
        target: job.target.clone(),
        payload: event.payload.clone(),
        headers: Some(build_headers(job, event)),
        message_group_id: Some(job.subscription_id.clone()),
        deduplication_id: Some(job.id.clone()),
    }
}
```

### Queue Publisher Abstraction

Supports multiple backends:

| Backend | Use Case |
|---------|----------|
| SQS | Production with AWS |
| SQLite | Development with fc-dev |
| HTTP | Direct to FlowCatalyst Router |

### Stale Job Detection

Jobs stuck in `PROCESSING` state are recovered:
- Configurable stale threshold (default 5 minutes)
- Automatic reset to `PENDING`
- Logged for investigation

## Binary

### fc-scheduler-server

```bash
cargo build -p fc-scheduler-server --release
./target/release/fc-scheduler-server
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FC_MONGO_URL` | `mongodb://localhost:27017` | MongoDB connection URL |
| `FC_MONGO_DB` | `flowcatalyst` | MongoDB database name |
| `FC_SCHEDULER_POLL_INTERVAL_MS` | `1000` | Poll interval in milliseconds |
| `FC_SCHEDULER_BATCH_SIZE` | `100` | Max jobs per poll |
| `FC_SCHEDULER_STALE_THRESHOLD_SECS` | `300` | Stale job threshold |
| `FC_QUEUE_URL` | - | SQS queue URL |
| `FC_ROUTER_URL` | `http://localhost:8081` | Router URL (HTTP mode) |
| `FC_METRICS_PORT` | `9090` | Metrics/health port |
| `RUST_LOG` | `info` | Log level |

### TOML Configuration

The scheduler also supports TOML configuration:

```toml
# config.toml
[scheduler]
poll_interval_ms = 1000
batch_size = 100
stale_threshold_secs = 300

[mongodb]
url = "mongodb://localhost:27017"
database = "flowcatalyst"

[queue]
type = "sqs"  # or "sqlite", "http"
url = "https://sqs.us-east-1.amazonaws.com/123456789/queue.fifo"

[http]
port = 8080
metrics_port = 9090
```

Load with:
```bash
FC_CONFIG_PATH=./config.toml ./target/release/fc-scheduler-server
```

## High Availability

For HA deployments, use leader election:

```bash
export FC_STANDBY_ENABLED=true
export FC_STANDBY_REDIS_URL=redis://localhost:6379
export FC_STANDBY_LOCK_KEY=scheduler-lock
export FC_STANDBY_INSTANCE_ID=scheduler-1
export FC_STANDBY_LOCK_TTL_SECS=30
export FC_STANDBY_REFRESH_INTERVAL_SECS=10
```

Only the leader instance polls jobs; others remain on standby.

### Failover Behavior

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ scheduler-1 │     │ scheduler-2 │     │ scheduler-3 │
│   LEADER    │     │  STANDBY    │     │  STANDBY    │
└──────┬──────┘     └──────┬──────┘     └──────┬──────┘
       │                   │                   │
       │  ← Holds Redis lock                   │
       │                   │                   │
       ▼                   │                   │
  Processing               │                   │
  jobs...                  │                   │
       │                   │                   │
       X (crash)           │                   │
                           │                   │
                    Lock expires               │
                           │                   │
                    Acquires lock ────────────▶│
                           │                   │
                     LEADER                    │
                           │                   │
                      Processing               │
                      jobs...                  │
```

## Message Building

### Headers

Standard headers added to dispatched messages:

| Header | Description |
|--------|-------------|
| `X-FlowCatalyst-Event-Id` | Original event ID |
| `X-FlowCatalyst-Event-Type` | Event type |
| `X-FlowCatalyst-Dispatch-Job-Id` | Dispatch job ID |
| `X-FlowCatalyst-Subscription-Id` | Subscription ID |
| `X-FlowCatalyst-Client-Id` | Client ID |
| `X-FlowCatalyst-Timestamp` | Event timestamp |

### Message Group ID

Uses subscription ID for FIFO ordering:
- Messages for same subscription delivered in order
- Different subscriptions processed in parallel

### Deduplication ID

Uses dispatch job ID:
- Prevents duplicate delivery on retries
- SQS deduplication window: 5 minutes

## Scheduling Features

### Immediate Dispatch

Jobs with `scheduled_at <= now()` are dispatched immediately.

### Delayed Dispatch

Jobs can be scheduled for future delivery:
```rust
DispatchJob {
    scheduled_at: Utc::now() + Duration::minutes(30),
    // ...
}
```

### Retry Scheduling

Failed jobs are rescheduled with exponential backoff:
- Attempt 1: immediate
- Attempt 2: +1 minute
- Attempt 3: +5 minutes
- Attempt 4: +15 minutes
- Attempt 5: +1 hour

## Metrics

Prometheus metrics at `/metrics`:

| Metric | Type | Description |
|--------|------|-------------|
| `fc_scheduler_jobs_polled_total` | Counter | Total jobs polled |
| `fc_scheduler_jobs_queued_total` | Counter | Jobs successfully queued |
| `fc_scheduler_jobs_failed_total` | Counter | Jobs failed to queue |
| `fc_scheduler_poll_duration_seconds` | Histogram | Poll operation latency |
| `fc_scheduler_queue_duration_seconds` | Histogram | Queue publish latency |
| `fc_scheduler_pending_jobs` | Gauge | Current pending job count |
| `fc_scheduler_stale_jobs_recovered_total` | Counter | Stale jobs recovered |

## Error Handling

### Queue Publish Errors

| Error | Handling |
|-------|----------|
| Queue unavailable | Job stays PROCESSING, recovered as stale |
| Message too large | Job marked FAILED with error |
| Invalid payload | Job marked FAILED with error |

### Database Errors

| Error | Handling |
|-------|----------|
| MongoDB unavailable | Retry with backoff |
| Write conflict | Retry |
| Connection timeout | Reconnect and retry |

## Crate Structure

```
fc-scheduler/
├── src/
│   ├── lib.rs         # Main scheduler logic
│   └── config.rs      # Configuration handling
└── tests/
```

## Integration with fc-dev

The development monolith (`fc-dev`) does not include the scheduler by default. Jobs created by the stream processor need to be manually processed or use a separate scheduler instance.

For full local testing:
```bash
# Terminal 1: fc-dev (platform + stream processor)
cargo run -p fc-dev

# Terminal 2: scheduler
cargo run -p fc-scheduler-server
```

## Testing

```bash
# Unit tests
cargo test -p fc-scheduler

# Integration tests (requires MongoDB)
cargo test -p fc-scheduler --test integration_tests
```

## Dependencies

- `fc-common`: Message and job types
- `fc-config`: Configuration loading
- `fc-queue`: Queue publisher abstraction
- `fc-standby`: Leader election
- `mongodb`: MongoDB driver
