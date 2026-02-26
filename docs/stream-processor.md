# Stream Processor

The Stream Processor watches MongoDB change streams for new events and creates dispatch jobs by matching events to subscriptions.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Stream Processor                                  │
│                                                                          │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                    Change Stream Watcher                          │   │
│  │                                                                   │   │
│  │   MongoDB events collection  ───▶  Change Stream Consumer        │   │
│  │   (insert operations)              (resume token tracking)       │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                    │                                     │
│                                    ▼                                     │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                   Subscription Matcher                            │   │
│  │                                                                   │   │
│  │   Event Type: orders:fulfillment:shipment:shipped                │   │
│  │                        │                                          │   │
│  │                        ▼                                          │   │
│  │   ┌─────────────────────────────────────────────────────────┐    │   │
│  │   │  Subscription Patterns:                                  │    │   │
│  │   │    orders:fulfillment:shipment:shipped  ✓ exact match   │    │   │
│  │   │    orders:fulfillment:*:*               ✓ wildcard      │    │   │
│  │   │    orders:*:*:*                         ✓ wildcard      │    │   │
│  │   │    inventory:*:*:*                      ✗ no match      │    │   │
│  │   └─────────────────────────────────────────────────────────┘    │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                    │                                     │
│                                    ▼                                     │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                  Dispatch Job Creator                             │   │
│  │                                                                   │   │
│  │   For each matching subscription:                                 │   │
│  │     - Create DispatchJob with event reference                    │   │
│  │     - Set initial status: PENDING                                │   │
│  │     - Link to subscription's webhook target                      │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                    │                                     │
│                                    ▼                                     │
│                          ┌─────────────────┐                            │
│                          │   MongoDB       │                            │
│                          │ dispatch_jobs   │                            │
│                          └─────────────────┘                            │
└─────────────────────────────────────────────────────────────────────────┘
```

## Event Processing Flow

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│   Event      │    │   Change     │    │ Subscription │    │   Dispatch   │
│  Published   │───▶│   Stream     │───▶│   Matching   │───▶│    Jobs      │
│  (MongoDB)   │    │   Detected   │    │              │    │   Created    │
└──────────────┘    └──────────────┘    └──────────────┘    └──────────────┘
                                                                    │
                                                                    ▼
                    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
                    │   Message    │◀───│   Scheduler  │◀───│   Pending    │
                    │   Router     │    │   Polls Jobs │    │    Jobs      │
                    └──────────────┘    └──────────────┘    └──────────────┘
```

## Components

### Change Stream Watcher (`fc-stream/src/lib.rs`)

Watches MongoDB for new events:
- Subscribes to `insert` operations on `events` collection
- Maintains resume token for crash recovery
- Handles reconnection on network failures

### Subscription Matcher

Matches events to subscriptions using:
- **Exact matching**: Event type exactly matches subscription pattern
- **Wildcard matching**: `*` wildcards in pattern segments
- **Client filtering**: Events only matched to same-client subscriptions
- **Active filtering**: Only active subscriptions considered

### Event Type Pattern Matching

Event types follow a hierarchical format: `domain:category:resource:action`

Pattern matching rules:
| Pattern | Matches |
|---------|---------|
| `orders:fulfillment:shipment:shipped` | Exact event type only |
| `orders:fulfillment:shipment:*` | Any shipment action |
| `orders:fulfillment:*:*` | Any fulfillment event |
| `orders:*:*:*` | Any orders event |
| `*:*:*:*` | All events |

### Dispatch Job Creator

Creates `DispatchJob` documents for each match:
```rust
pub struct DispatchJob {
    pub id: String,
    pub event_id: String,
    pub subscription_id: String,
    pub client_id: String,
    pub status: DispatchStatus,
    pub target: String,              // From subscription
    pub pool_code: String,           // From subscription
    pub scheduled_at: DateTime<Utc>,
    pub attempts: u32,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
}
```

### Health Service (`fc-stream/src/health.rs`)

Monitors stream processor health:
- Change stream connection status
- Processing lag metrics
- Error rate tracking

### Projection Builder (`fc-stream/src/projection.rs`)

Builds read models for efficient querying:
- Event counts by type
- Subscription statistics
- Client activity summaries

### Index Initializer (`fc-stream/src/index_initializer.rs`)

Ensures MongoDB indexes exist:
- Events collection indexes
- Dispatch jobs indexes
- Subscription indexes

## Binary

### fc-stream-processor

```bash
cargo build -p fc-stream-processor --release
./target/release/fc-stream-processor
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FC_MONGO_URL` | `mongodb://localhost:27017` | MongoDB connection URL |
| `FC_MONGO_DB` | `flowcatalyst` | MongoDB database name |
| `FC_METRICS_PORT` | `9090` | Metrics/health port |
| `FC_STREAM_BATCH_SIZE` | `100` | Max events per processing batch |
| `FC_STREAM_RESUME_TOKEN_KEY` | `stream-processor` | Redis key for resume token |
| `FC_REDIS_URL` | - | Redis URL for resume token storage |
| `RUST_LOG` | `info` | Log level |

### Resume Token Storage

Resume tokens enable crash recovery:

**Redis (recommended for production):**
```bash
export FC_REDIS_URL=redis://localhost:6379
```

**In-memory (development only):**
```bash
# No FC_REDIS_URL set - uses in-memory storage
# Warning: Resume token lost on restart
```

## MongoDB Change Streams

### Requirements

- MongoDB 3.6+ (change streams)
- Replica set deployment (required for change streams)
- Read concern `majority`

### Change Stream Pipeline

The processor watches with this pipeline:
```javascript
[
  { $match: { operationType: "insert" } },
  { $project: { fullDocument: 1, _id: 1 } }
]
```

### Resume Behavior

1. On startup, check for stored resume token
2. If found, resume from that point
3. If not found, start from current point
4. After each batch, persist new resume token

## High Availability

For HA deployments, use leader election:

```bash
export FC_STANDBY_ENABLED=true
export FC_STANDBY_REDIS_URL=redis://localhost:6379
export FC_STANDBY_LOCK_KEY=stream-processor-lock
export FC_STANDBY_INSTANCE_ID=stream-1
```

Only the leader instance watches the change stream; others remain on standby.

## Subscription Filtering

Subscriptions define which events they receive:

```rust
pub struct Subscription {
    pub id: String,
    pub client_id: String,
    pub name: String,
    pub event_type_pattern: String,  // e.g., "orders:*:*:*"
    pub target: String,               // Webhook URL
    pub pool_code: String,            // Processing pool
    pub active: bool,
    pub filters: Option<EventFilters>,
}
```

### Additional Filters

Beyond event type matching, subscriptions can filter on:
- Event payload fields (JSONPath expressions)
- Event source
- Event metadata

## Metrics

Prometheus metrics at `/metrics`:

| Metric | Type | Description |
|--------|------|-------------|
| `fc_stream_events_processed_total` | Counter | Total events processed |
| `fc_stream_dispatch_jobs_created_total` | Counter | Total dispatch jobs created |
| `fc_stream_processing_duration_seconds` | Histogram | Event processing latency |
| `fc_stream_change_stream_lag_seconds` | Gauge | Lag behind MongoDB oplog |
| `fc_stream_subscriptions_matched_total` | Counter | Subscription matches |

## Error Handling

### Change Stream Errors

| Error | Handling |
|-------|----------|
| Network disconnect | Automatic reconnection with backoff |
| Invalid resume token | Start from current position, log warning |
| MongoDB unavailable | Retry with exponential backoff |

### Processing Errors

| Error | Handling |
|-------|----------|
| Invalid event format | Skip event, log error |
| Database write failure | Retry with backoff |
| Subscription lookup failure | Retry with backoff |

## Crate Structure

```
fc-stream/
├── src/
│   ├── lib.rs                # Main processor logic
│   ├── health.rs             # Health monitoring
│   ├── projection.rs         # Read model building
│   └── index_initializer.rs  # Index management
└── tests/
```

## Integration with fc-dev

The development monolith (`fc-dev`) includes the stream processor:

```bash
cargo run -p fc-dev
# Stream processor automatically watches events collection
```

## Testing

```bash
# Unit tests
cargo test -p fc-stream

# Integration tests (requires MongoDB replica set)
cargo test -p fc-stream --test integration_tests
```

## Dependencies

- `fc-common`: Event and subscription types
- `fc-platform`: Repository access
- `fc-standby`: Leader election
- `mongodb`: MongoDB driver with change streams
- `redis`: Resume token storage
- `futures`: Async stream handling
