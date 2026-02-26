# Outbox Processor

The Outbox Processor implements the transactional outbox pattern, reading messages from application database outbox tables and reliably publishing them to FlowCatalyst or directly to message queues.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Outbox Processor                                  │
│                                                                          │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                    Enhanced Processor Mode                        │   │
│  │                                                                   │   │
│  │  ┌────────────┐    ┌────────────┐    ┌────────────────────────┐  │   │
│  │  │  Outbox    │───▶│   Group    │───▶│  Message Group         │  │   │
│  │  │ Repository │    │Distributor │    │  Processors (N)        │  │   │
│  │  └────────────┘    └────────────┘    │  ┌──────────────────┐  │  │   │
│  │                                      │  │ Group A Worker   │  │  │   │
│  │                                      │  │ (sequential)     │  │  │   │
│  │                                      │  └──────────────────┘  │  │   │
│  │                                      │  ┌──────────────────┐  │  │   │
│  │                                      │  │ Group B Worker   │  │  │   │
│  │                                      │  │ (sequential)     │  │  │   │
│  │                                      │  └──────────────────┘  │  │   │
│  │                                      └────────────┬───────────┘  │   │
│  └───────────────────────────────────────────────────┬──────────────┘   │
│                                                      │                   │
│  ┌───────────────────────────────────────────────────▼──────────────┐   │
│  │                      HTTP Dispatcher                              │   │
│  │              POST to FlowCatalyst /api/router/publish             │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐   │
│  │                    SQS Mode (Legacy)                              │   │
│  │              Direct publish to AWS SQS                            │   │
│  └───────────────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────────────┘
         │                                                    │
         ▼                                                    ▼
┌─────────────────┐                                 ┌─────────────────┐
│ FlowCatalyst    │                                 │    AWS SQS      │
│ Router API      │                                 │                 │
└─────────────────┘                                 └─────────────────┘
```

## The Outbox Pattern

The outbox pattern ensures reliable event publishing alongside database transactions:

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Transaction                   │
│                                                              │
│   1. Update business data                                    │
│   2. Insert event into outbox table     ◀── Same transaction │
│   3. Commit                                                  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Outbox Processor                          │
│                                                              │
│   1. Poll outbox table for pending messages                  │
│   2. Publish to destination (FlowCatalyst/SQS)              │
│   3. Mark as published or delete                            │
└─────────────────────────────────────────────────────────────┘
```

**Benefits:**
- Atomic: Event publishing tied to business transaction
- Reliable: No lost events even if messaging system is down
- Ordered: Messages within groups maintain order
- Idempotent: Deduplication IDs prevent duplicates

## Components

### OutboxRepository Trait (`fc-outbox/src/repository.rs`)

Abstract interface for outbox storage:

```rust
#[async_trait]
pub trait OutboxRepository: Send + Sync {
    async fn fetch_pending(&self, limit: usize) -> Result<Vec<OutboxMessage>>;
    async fn mark_published(&self, ids: &[String]) -> Result<()>;
    async fn mark_failed(&self, id: &str, error: &str) -> Result<()>;
    async fn delete(&self, ids: &[String]) -> Result<()>;
}
```

### Repository Implementations

| Implementation | Database | Feature Flag |
|----------------|----------|--------------|
| `SqliteOutboxRepository` | SQLite | `sqlite` |
| `PostgresOutboxRepository` | PostgreSQL | `postgres` |
| `MongoOutboxRepository` | MongoDB | `mongo` |

### Enhanced Processor (`fc-outbox/src/enhanced.rs`)

Production-grade processor with:
- **Message group ordering**: Sequential processing within groups
- **Concurrent group processing**: Multiple groups processed in parallel
- **Global buffer**: Efficient memory management
- **Recovery handling**: Automatic retry of failed messages

### HTTP Dispatcher (`fc-outbox/src/http_dispatcher.rs`)

Dispatches messages to FlowCatalyst Router API:
- POST to `/api/router/publish`
- Batch publishing support
- Retry with exponential backoff
- Health checking

### SQS Publisher (Legacy Mode)

Direct publishing to AWS SQS:
- FIFO queue support
- Message deduplication
- Batch publishing (up to 10 messages)

## Message Format

### Outbox Table Schema

```sql
CREATE TABLE outbox (
    id VARCHAR(36) PRIMARY KEY,
    message_group_id VARCHAR(255),      -- For FIFO ordering
    deduplication_id VARCHAR(255),       -- For idempotency
    pool_code VARCHAR(255) NOT NULL,
    target VARCHAR(1024) NOT NULL,       -- Webhook URL
    payload JSONB NOT NULL,
    headers JSONB,
    created_at TIMESTAMP NOT NULL,
    published_at TIMESTAMP,
    status VARCHAR(20) DEFAULT 'pending',
    retry_count INT DEFAULT 0,
    last_error TEXT
);

CREATE INDEX idx_outbox_status ON outbox(status);
CREATE INDEX idx_outbox_created ON outbox(created_at);
CREATE INDEX idx_outbox_group ON outbox(message_group_id);
```

### OutboxMessage Struct

```rust
pub struct OutboxMessage {
    pub id: String,
    pub message_group_id: Option<String>,
    pub deduplication_id: Option<String>,
    pub pool_code: String,
    pub target: String,
    pub payload: Value,
    pub headers: Option<HashMap<String, String>>,
    pub created_at: DateTime<Utc>,
    pub retry_count: u32,
}
```

## Processing Modes

### Enhanced Mode (Default)

HTTP-based publishing with message group support:

```
┌─────────────────────────────────────────────────────────────┐
│                    Message Groups                            │
│                                                              │
│   Group "order-123":  [msg1] → [msg2] → [msg3]  (sequential)│
│   Group "order-456":  [msg4] → [msg5]           (sequential)│
│   No Group:           [msg6], [msg7]            (parallel)  │
│                                                              │
│   Groups process in parallel, messages within groups         │
│   process sequentially                                       │
└─────────────────────────────────────────────────────────────┘
```

### SQS Mode (Legacy)

Direct SQS publishing for backwards compatibility:
- Publishes directly to SQS FIFO queue
- Uses SQS message groups for ordering
- Suitable when FlowCatalyst Router runs separately

## Binary

### fc-outbox-processor

```bash
cargo build -p fc-outbox-processor --release
./target/release/fc-outbox-processor
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FC_OUTBOX_MODE` | `enhanced` | Processing mode: `enhanced` or `sqs` |
| `FC_OUTBOX_DB_TYPE` | `postgres` | Database: `sqlite`, `postgres`, `mongo` |
| `FC_OUTBOX_DB_URL` | **required** | Database connection URL |
| `FC_OUTBOX_MONGO_DB` | `flowcatalyst` | MongoDB database (if mongo) |
| `FC_OUTBOX_MONGO_COLLECTION` | `outbox` | MongoDB collection (if mongo) |
| `FC_OUTBOX_POLL_INTERVAL_MS` | `1000` | Poll interval in milliseconds |
| `FC_OUTBOX_BATCH_SIZE` | `100` | Max messages per poll |
| `FC_OUTBOX_CONCURRENCY` | `10` | Concurrent group processors |
| `FC_ROUTER_URL` | `http://localhost:8081` | FlowCatalyst Router URL |
| `FC_QUEUE_URL` | - | SQS queue URL (SQS mode) |
| `FC_METRICS_PORT` | `9090` | Metrics/health port |
| `RUST_LOG` | `info` | Log level |

### Database Connection Examples

**SQLite:**
```bash
export FC_OUTBOX_DB_TYPE=sqlite
export FC_OUTBOX_DB_URL=sqlite:./outbox.db
```

**PostgreSQL:**
```bash
export FC_OUTBOX_DB_TYPE=postgres
export FC_OUTBOX_DB_URL=postgres://user:pass@localhost:5432/mydb
```

**MongoDB:**
```bash
export FC_OUTBOX_DB_TYPE=mongo
export FC_OUTBOX_DB_URL=mongodb://localhost:27017
export FC_OUTBOX_MONGO_DB=myapp
export FC_OUTBOX_MONGO_COLLECTION=outbox
```

## Integration with fc-dev

The development monolith (`fc-dev`) includes an optional outbox processor:

```bash
# Enable outbox processor in development
export FC_OUTBOX_ENABLED=true
export FC_OUTBOX_DB_TYPE=sqlite
export FC_OUTBOX_DB_URL=sqlite::memory:

cargo run -p fc-dev
```

## Message Flow

### Enhanced Mode

```
1. Application inserts message into outbox table
2. Processor polls for pending messages
3. Messages grouped by message_group_id
4. GroupDistributor assigns groups to workers
5. Each worker processes messages sequentially within group
6. HttpDispatcher POSTs to FlowCatalyst Router
7. On success: message marked published/deleted
8. On failure: retry count incremented, re-queued
```

### SQS Mode

```
1. Application inserts message into outbox table
2. Processor polls for pending messages
3. Messages batched (up to 10)
4. SqsPublisher sends batch to SQS
5. Successful messages deleted from outbox
6. Failed messages remain for retry
```

## High Availability

For HA deployments, use leader election:

```bash
export FC_STANDBY_ENABLED=true
export FC_STANDBY_REDIS_URL=redis://localhost:6379
export FC_STANDBY_LOCK_KEY=outbox-processor-lock
export FC_STANDBY_INSTANCE_ID=outbox-1
```

Only the leader instance processes messages; others remain on standby.

## Metrics

Prometheus metrics at `/metrics`:

| Metric | Type | Description |
|--------|------|-------------|
| `fc_outbox_messages_processed_total` | Counter | Total messages processed |
| `fc_outbox_messages_failed_total` | Counter | Total failed messages |
| `fc_outbox_poll_duration_seconds` | Histogram | Poll operation latency |
| `fc_outbox_publish_duration_seconds` | Histogram | Publish operation latency |
| `fc_outbox_pending_messages` | Gauge | Current pending message count |
| `fc_outbox_groups_active` | Gauge | Active message groups |

## Error Handling

### Retry Logic

Messages are retried with exponential backoff:
- Retry 1: 1 second delay
- Retry 2: 2 second delay
- Retry 3: 4 second delay
- ...up to max retries (default 10)

### Dead Letter Handling

After max retries, messages are:
1. Marked as `failed` with error details
2. Available for manual inspection/replay
3. Not deleted automatically

### Recovery Task

Background task that:
- Identifies stuck messages (processing too long)
- Resets them for reprocessing
- Handles processor crashes/restarts

## Crate Structure

```
fc-outbox/
├── src/
│   ├── lib.rs                    # Module exports
│   ├── processor.rs              # Basic processor
│   ├── enhanced.rs               # Enhanced processor
│   ├── http_dispatcher.rs        # HTTP publishing
│   ├── repository.rs             # Repository trait
│   ├── sqlite.rs                 # SQLite implementation
│   ├── postgres.rs               # PostgreSQL implementation
│   ├── mongo.rs                  # MongoDB implementation
│   ├── group_processor.rs        # Message group handling
│   ├── group_distributor.rs      # Work distribution
│   ├── global_buffer.rs          # Memory management
│   └── recovery.rs               # Recovery task
└── tests/
```

## Testing

```bash
# Unit tests
cargo test -p fc-outbox

# With specific backend
cargo test -p fc-outbox --features sqlite
cargo test -p fc-outbox --features postgres
cargo test -p fc-outbox --features mongo
```

## Dependencies

- `fc-common`: Message types
- `fc-queue`: SQS publisher (legacy mode)
- `fc-standby`: Leader election (optional)
- `sqlx`: Database connectivity
- `mongodb`: MongoDB driver
