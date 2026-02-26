# Message Router

The Message Router is the core delivery engine of FlowCatalyst. It consumes messages from a queue and delivers them to webhook endpoints with sophisticated retry logic, circuit breakers, rate limiting, and FIFO ordering guarantees.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Message Router                                 │
│                                                                          │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────────────┐  │
│  │    Queue     │───▶│    Queue     │───▶│      Process Pools       │  │
│  │  Consumer    │    │   Manager    │    │  ┌────────────────────┐  │  │
│  │ (SQS/SQLite) │    │              │    │  │  Pool: default     │  │  │
│  └──────────────┘    └──────────────┘    │  │  - Concurrency: 10 │  │  │
│                                          │  │  - Rate Limit: 100 │  │  │
│                                          │  └────────────────────┘  │  │
│                                          │  ┌────────────────────┐  │  │
│                                          │  │  Pool: priority    │  │  │
│                                          │  │  - Concurrency: 20 │  │  │
│                                          │  │  - Rate Limit: 500 │  │  │
│                                          │  └────────────────────┘  │  │
│                                          └────────────┬─────────────┘  │
│                                                       │                 │
│                                                       ▼                 │
│                                          ┌──────────────────────────┐  │
│                                          │     HTTP Mediator        │  │
│                                          │  - Circuit Breaker       │  │
│                                          │  - Retry Logic           │  │
│                                          │  - Timeout Handling      │  │
│                                          └────────────┬─────────────┘  │
└──────────────────────────────────────────────────────┬──────────────────┘
                                                       │
                                                       ▼
                                              ┌─────────────────┐
                                              │    Webhook      │
                                              │   Endpoints     │
                                              └─────────────────┘
```

## Components

### Queue Manager (`fc-router/src/manager.rs`)

Central orchestrator that:
- Polls messages from the queue consumer
- Routes messages to appropriate processing pools based on `pool_code`
- Manages message lifecycle (ACK/NACK/visibility extension)
- Coordinates graceful shutdown

### Process Pool (`fc-router/src/pool.rs`)

Worker pool that processes messages with:
- **Configurable concurrency**: Maximum parallel dispatches
- **Rate limiting**: Token bucket algorithm to prevent overwhelming endpoints
- **FIFO ordering**: Messages within the same `message_group_id` processed sequentially
- **Backpressure**: Respects queue visibility timeouts

### HTTP Mediator (`fc-router/src/mediator.rs`)

Handles HTTP delivery with:
- **Circuit breaker**: Per-endpoint failure tracking with automatic recovery
- **Retry logic**: Configurable retry attempts with exponential backoff
- **Timeout handling**: Request and connection timeouts
- **HTTP/2 support**: Production mode uses HTTP/2 with keep-alive

### Lifecycle Manager (`fc-router/src/lifecycle.rs`)

Background tasks for:
- Visibility timeout extension for long-running messages
- Health check coordination
- Graceful shutdown orchestration

### Circuit Breaker Registry (`fc-router/src/circuit_breaker.rs`)

Tracks circuit breaker state per endpoint:
- **Closed**: Normal operation, requests flow through
- **Open**: Endpoint failing, requests rejected immediately
- **Half-Open**: Testing recovery, limited requests allowed

### Warning Service (`fc-router/src/warning.rs`)

In-memory storage for operational warnings:
- Endpoint failures
- Rate limit violations
- Circuit breaker trips

### Health Service (`fc-router/src/health.rs`)

System health monitoring:
- Rolling window success/failure tracking
- Queue depth monitoring
- Pool health aggregation

## Binaries

### fc-router (Production)

Consumes from AWS SQS in production.

```bash
cargo build -p fc-router-bin --release
./target/release/fc-router-bin
```

### fc-dev (Development)

Uses embedded SQLite queue for local development.

```bash
cargo run -p fc-dev
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FC_ROUTER_PORT` | `8081` | Router API port |
| `FC_METRICS_PORT` | `9090` | Metrics endpoint port |
| `QUEUE_URL` | - | SQS queue URL (production) |
| `FC_QUEUE_PATH` | `:memory:` | SQLite queue path (development) |
| `VISIBILITY_TIMEOUT` | `30` | SQS visibility timeout (seconds) |
| `POOL_CONCURRENCY` | `10` | Default pool concurrency |
| `RUST_LOG` | `info` | Log level |

### Pool Configuration

Pools are configured per-tenant/application. Default pool settings:

```rust
PoolConfig {
    pool_code: "default",
    concurrency: 10,
    rate_limit: Some(100),  // requests per second
    rate_limit_burst: Some(20),
}
```

## Message Format

Messages conform to the `Message` struct from `fc-common`:

```rust
pub struct Message {
    pub id: String,                    // Unique message ID
    pub pool_code: String,             // Target processing pool
    pub mediation_type: MediationType, // HTTP, SQS, etc.
    pub target: String,                // Webhook URL
    pub payload: Value,                // JSON payload
    pub headers: Option<HashMap<String, String>>,
    pub message_group_id: Option<String>,  // For FIFO ordering
    pub deduplication_id: Option<String>,
}
```

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/router/publish` | Publish message to queue |
| `GET` | `/api/router/health` | Basic health check |
| `GET` | `/api/monitoring` | Detailed monitoring metrics |
| `GET` | `/api/warnings` | Active warnings |
| `GET` | `/api/pools` | Pool statistics |
| `GET` | `/api/circuit-breakers` | Circuit breaker states |
| `GET` | `/q/live` | Kubernetes liveness |
| `GET` | `/q/ready` | Kubernetes readiness |

## Message Flow

1. **Receive**: Queue consumer polls messages from SQS/SQLite
2. **Route**: Queue manager routes to pool based on `pool_code`
3. **Queue**: Pool queues message, respecting FIFO ordering within groups
4. **Rate Limit**: Token bucket checks if request can proceed
5. **Dispatch**: HTTP mediator sends request to webhook
6. **Circuit Check**: Circuit breaker allows/rejects based on endpoint health
7. **Retry**: On failure, message is retried with backoff
8. **ACK/NACK**: Success ACKs message; exhausted retries NACK for redelivery

## FIFO Ordering

Messages with the same `message_group_id` are processed sequentially:

```
Group A: [msg1] → [msg2] → [msg3]  (sequential)
Group B: [msg4] → [msg5]          (sequential)
         ↓         ↓
    (Groups A and B process in parallel)
```

## Circuit Breaker States

```
     ┌─────────────────────────────────────┐
     │                                     │
     ▼                                     │
┌─────────┐  failure threshold  ┌─────────┐
│ CLOSED  │────────────────────▶│  OPEN   │
└─────────┘                     └────┬────┘
     ▲                               │
     │         timeout               │
     │                               ▼
     │                         ┌──────────┐
     └─────── success ─────────│HALF-OPEN │
                               └──────────┘
```

## Metrics

Prometheus metrics exposed at `/metrics`:

| Metric | Type | Description |
|--------|------|-------------|
| `fc_router_messages_processed_total` | Counter | Total messages processed |
| `fc_router_messages_failed_total` | Counter | Total failed messages |
| `fc_router_dispatch_duration_seconds` | Histogram | Dispatch latency |
| `fc_router_pool_queue_depth` | Gauge | Messages waiting per pool |
| `fc_router_circuit_breaker_state` | Gauge | Circuit breaker states |
| `fc_router_rate_limit_rejections_total` | Counter | Rate limit rejections |

## Error Handling

### Retryable Errors
- Network timeouts
- 5xx server errors
- Connection refused
- Circuit breaker half-open test failures

### Non-Retryable Errors
- 4xx client errors (except 429)
- Invalid payload
- Malformed URL

### 429 Too Many Requests
- Respected with `Retry-After` header
- Falls back to exponential backoff if header missing

## Graceful Shutdown

On SIGTERM/SIGINT:
1. Stop accepting new messages
2. Extend visibility of in-flight messages
3. Wait for current dispatches to complete (with timeout)
4. NACK any incomplete messages for redelivery
5. Close connections cleanly

## Testing

```bash
# Unit tests
cargo test -p fc-router

# Integration tests (requires infrastructure)
cargo test -p fc-router --test integration_tests

# Specific test suites
cargo test -p fc-router --test pool_tests
cargo test -p fc-router --test rate_limit_tests
cargo test -p fc-router --test fifo_tests
cargo test -p fc-router --test mediator_tests
```

## Crate Dependencies

- `fc-common`: Message types, configuration
- `fc-queue`: Queue consumer/publisher abstraction
- `fc-api`: HTTP API layer (when used with API server)
