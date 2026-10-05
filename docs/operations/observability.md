# Observability

Metrics, logs, and health endpoints for FlowCatalyst. Each binary exposes the same surface — Prometheus metrics + JSON health.

---

## Health endpoints

Every binary listens on two ports: the API port (varies) and the metrics port (default 9090).

### Per-binary

| Endpoint | Port | Returns | Use |
|---|---|---|---|
| `GET /health` | metrics | combined JSON | Detailed status — leader, version, per-subsystem state |
| `GET /metrics` | metrics | Prometheus text | Scrape target |
| `GET /ready` | metrics | `{"status":"READY"}` | Lightweight readiness |
| `GET /q/live` | API | minimal | Kubernetes liveness probe (router/standalone) |
| `GET /q/ready` | API | minimal | Kubernetes readiness probe |

### fc-server combined `/health`

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

| State | Meaning |
|---|---|
| `UP` | Enabled and currently the leader (or no standby) |
| `STANDBY` | Enabled, but not leader — subsystem paused |
| `DISABLED` | Toggle is off |

For kubernetes:

```yaml
livenessProbe:
  httpGet:
    path: /q/live
    port: 3000
  initialDelaySeconds: 10
  periodSeconds: 30
readinessProbe:
  httpGet:
    path: /q/ready
    port: 3000
  periodSeconds: 5
```

Liveness should be loose (a stuck SQS connection isn't a reason to restart the process). Readiness should be tight (a stuck DB pool means the node shouldn't receive traffic).

---

## Metrics

Runtime diagnostics (tokio runtime and process series, panics, supervised
task restarts, task dumps, the router's flight recorder and the span fields
every log line carries) are in
[diagnosing-stuck-processes.md](diagnosing-stuck-processes.md).

All binaries expose Prometheus metrics at `:9090/metrics`. Scrape with Prometheus, push to whatever (Cortex, Mimir, Cloudwatch via the exporter, Datadog Agent, etc.).

### Router metrics

The names and labels are the same as the Go router's, so dashboards and
alerts work against either. They are rendered from snapshots at each scrape
(`crates/fc-router/src/api/prometheus.rs`), so a removed pool or queue drops
out of the next scrape rather than keeping its last value.

```
# Per pool (label: pool)
fc_pool_queue_size                       # gauge   messages buffered awaiting dispatch
fc_pool_active_workers                   # gauge   deliveries in flight
fc_pool_message_groups                   # gauge   groups holding buffered work
fc_messages_submitted_total              # counter routed to the pool
fc_messages_processed_total{success,result}      # counter, result = SUCCESS, ERROR_CONFIG, ...
fc_messages_rejected_total{reason}       # counter capacity | stopped | released | blocked | suppressed
fc_rate_limit_exceeded_total             # counter
fc_mediation_duration_seconds            # histogram, buckets 5ms .. 10s

# Per queue (label: queue, or consumer)
fc_queue_pending_messages                # gauge   approximate, from the broker
fc_queue_in_flight_messages              # gauge
fc_consumer_messages_received_total{consumer}    # counter
fc_queue_messages_total{outcome}         # counter acked | nacked | deferred
fc_consumer_polls_total                  # counter
fc_consumer_errors_total{type}           # counter poll

# Circuit breakers (label: target)
fc_circuit_breaker_open                  # gauge   1 when OPEN
fc_circuit_breaker_calls_total{outcome}  # counter success | failure

# Global
fc_in_pipeline_messages                  # gauge
fc_router_panics_recovered_total         # counter every panic in the process
fc_mediation_http_version_total{version} # counter negotiated HTTP version
```

`fc_messages_processed_total` counts only outcomes the pool counts as a
success or a failure. A transient error the broker will redeliver is not
counted, so the series sums to the pool's success and failure totals.
`fc_consumer_errors_total` has no `panic` type: a poll that panics is not
caught separately here (Go's is).

### Scheduler metrics

```
fc_scheduler_jobs_polled_total
fc_scheduler_jobs_queued_total
fc_scheduler_jobs_failed_to_queue_total
fc_scheduler_poll_duration_seconds         # histogram
fc_scheduler_publish_duration_seconds       # histogram
fc_scheduler_pending_jobs                   # gauge — jobs claimed by the last poll (<= batch size); NOT the PENDING backlog
fc_scheduler_jobs_claimed_total
fc_scheduler_poll_full_batches_total        # claim filled the batch: a backlog is likely waiting
fc_scheduler_poll_errors_total
fc_scheduler_poll_last_success_timestamp_seconds   # gauge — alert when it stops advancing on the leader
fc_scheduler_publish_timeouts_total
fc_scheduler_queued_jobs                    # gauge — depth of QUEUED
fc_scheduler_stale_jobs_recovered_total     # should usually be flat at 0 (QUEUED > 15 min plus PROCESSING > 75 min)
fc_scheduler_stale_jobs_queued_recovered_total       # the QUEUED part
fc_scheduler_stale_jobs_processing_recovered_total   # the PROCESSING part
fc_scheduler_active_groups                  # gauge

# The PENDING backlog, sampled by the leader every 30 s (a bounded count)
fc_scheduler_pending_backlog                # gauge — PENDING jobs, saturating at 100000
fc_scheduler_pending_oldest_age_seconds     # gauge — age of the first PENDING job in claim order

# Hold-back and withholds
fc_scheduler_jobs_held_total                # claimed but found held back by an earlier job of their group (they stay PENDING)
fc_scheduler_held_groups                    # gauge — groups remembered as held (skipped by the claim for 5 s each)
fc_scheduler_jobs_withheld_total            # claimed but not submitted (behind a doomed job of their group; they stay PENDING)
fc_scheduler_claim_in_flight_duplicates_total  # a row returned for an id already in flight (the claim excludes them): should stay 0
```

### Stream processor metrics

```
fc_stream_events_projected_total
fc_stream_dispatch_jobs_projected_total
fc_stream_events_fanned_out_total
fc_stream_dispatch_jobs_created_total      # via fan-out
fc_stream_subscriptions_matched_total
fc_stream_processing_duration_seconds      # histogram per service
fc_stream_processing_lag_seconds            # gauge — projection lag
fc_stream_partition_manager_creates_total
fc_stream_partition_manager_drops_total
```

### Outbox processor metrics

```
fc_outbox_messages_processed_total
fc_outbox_messages_failed_total{status}
fc_outbox_messages_recovered_total          # via stuck-item recovery
fc_outbox_poll_duration_seconds             # histogram
fc_outbox_publish_duration_seconds           # histogram (HTTP POST)
fc_outbox_pending_items                      # gauge — depth of PENDING
fc_outbox_in_flight_items                    # gauge — buffer + processors
fc_outbox_active_groups                      # gauge
```

### Platform API metrics

Standard HTTP request metrics (axum + tower-http instrumentation):

```
http_requests_total{method, status, route}
http_request_duration_seconds{method, route}    # histogram
http_in_flight_requests
```

Plus auth-specific:

```
fc_platform_auth_token_validations_total{result}
fc_platform_auth_token_cache_hits_total
fc_platform_auth_token_cache_misses_total
fc_platform_oidc_callbacks_total{provider, result}
fc_platform_login_attempts_total{result}
```

### Postgres-facing (sqlx)

```
sqlx_pool_size                     # gauge
sqlx_pool_idle                      # gauge
sqlx_pool_acquire_duration_seconds  # histogram
```

If `sqlx_pool_acquire_duration_seconds` grows, the pool is exhausted. Either raise `FC_MAX_DB_CONNECTIONS` (we don't expose this directly today — code change) or add more nodes.

---

## Dashboards

### Minimum useful dashboards

1. **Dispatch throughput.** `rate(fc_scheduler_jobs_queued_total[5m])` + `rate(fc_router_messages_processed_total[5m])`. Difference is the in-flight buffer between scheduler and router; should be tiny in steady state.
2. **End-to-end latency.** `fc_router_dispatch_duration_seconds` p99 per pool.
3. **Backlog.** `rate(fc_scheduler_poll_full_batches_total[5m])` (the scheduler's claim keeps filling) + `fc_outbox_pending_items`. (`fc_scheduler_pending_jobs` is the last claim's size, not a depth.) Sustained growth = something is broken downstream of the producer.
4. **Failure rate.** `rate(fc_router_messages_failed_total[5m]) / rate(fc_router_messages_processed_total[5m])` per pool. Per-pool view shows which endpoint is bleeding.
5. **Leadership.** `up{role="fc-server"}` + a gauge showing who's leader. Useful during failover postmortems.

### Recommended alerts

| Alert | Condition | Severity |
|---|---|---|
| Scheduler saturated | `rate(fc_scheduler_poll_full_batches_total[5m]) > 0 for 30m` | Warning |
| Scheduler stalled | `time() - max(fc_scheduler_poll_last_success_timestamp_seconds) > 120` | Critical |
| Stale recovery firing | `rate(fc_scheduler_stale_jobs_recovered_total[5m]) > 0 for 10m` | Warning — investigate why jobs are stuck in QUEUED |
| Router pool 95th latency | `histogram_quantile(0.95, fc_router_dispatch_duration_seconds) > 30 for 5m` per pool | Warning per pool |
| Circuit breaker open | `fc_router_circuit_breaker_state == 1 for 5m` per endpoint | Warning |
| Leader missing | All `up{role="fc-server",leader="true"}` are 0 for > 1 min | Critical |
| Postgres connections saturated | `sqlx_pool_idle == 0 for 5m` | Warning |
| Projection lag | `fc_stream_processing_lag_seconds > 60 for 10m` | Warning |
| Login failure spike | `rate(fc_platform_login_attempts_total{result="failure"}[5m]) > N` (N = your baseline × 3) | Warning — credential stuffing? |

---

## Logging

JSON-formatted via `tracing-subscriber` (override with `FC_LOG_FORMAT=text` for human eyes).

### Levels

- `error` — actionable failure. Page if you see these in volume.
- `warn` — degraded state, not yet failure. Worth dashboarding.
- `info` — major lifecycle (startup, leadership transitions, config sync results). Quiet during steady state.
- `debug` — internal state. Enable per-module: `RUST_LOG=info,fc_router=debug`.
- `trace` — very verbose; rarely useful in prod.

### Useful filters

```
RUST_LOG=info,fc_router=info,fc_platform=info,fc_stream=info
```

Production defaults.

```
RUST_LOG=warn,fc_router::pool=info,fc_platform::auth=info
```

Quiet baseline with explicit info-level on hot subsystems.

```
RUST_LOG=info,fc_router::mediator=debug
```

Debug a specific endpoint delivery problem.

### Correlation

Every request that hits the platform gets a request ID (`X-Request-ID` if supplied, else auto-generated). It's logged on every log line related to the request via `tracing::Span`. Use it to follow a single request through the system.

Domain events also carry `correlation_id` and `causation_id`. Events emitted as part of an HTTP request inherit the request's correlation ID; events emitted from background processing have a fresh one. Dispatch jobs propagate these into webhook headers (`X-FlowCatalyst-Correlation-Id`) so receivers can log them too.

---

## Tracing

OpenTelemetry export is **not** currently wired. The `tracing` crate produces the spans, but there's no exporter to OTLP/Jaeger/Tempo in the default build. Adding one is straightforward (`tracing-opentelemetry` plus an OTLP exporter), but no production deployments do this today — Prometheus metrics + correlation-ID-tagged logs have been sufficient.

If you wire it up, the natural span boundaries are:

- HTTP request handlers (already a span).
- Use case execution (already a span via `#[instrument]` on `UseCase::run`).
- Router pool dispatch (already a span).
- Scheduler poll cycle (currently not instrumented; would be a useful add).

---

## Router-specific: the monitoring API

The router role exposes a dedicated monitoring API beyond Prometheus, under `fc-server`'s `FC_ROUTER_HTTP_PREFIX` (default `/router`, so `/router/monitoring/health`). It's a JSON dashboard surface:

| Endpoint | Returns |
|---|---|
| `GET /monitoring/health` | Detailed health: pool statuses, queue health, CB summary |
| `GET /monitoring/pools` | Pool list with per-pool metrics |
| `PUT /monitoring/pools/{code}` | Update concurrency at runtime |
| `GET /monitoring/queues` | Queue depths, consumer health |
| `GET /monitoring/circuit-breakers` | Per-endpoint state and recent failure rate |
| `GET /monitoring/pool-stats` | HdrHistogram-backed p50/p95/p99 per pool |
| `GET /monitoring/warnings` | Active operational warnings |
| `GET /monitoring/consumer-health` | Lag distribution per consumer |
| `GET /monitoring/standby-status` | Leader / standby state |

These need a platform bearer token holding `platform:messaging:router:view` (the
`platform:router-operator`, `platform:viewer` and `platform:application-service` roles hold it);
see `AUTH_MODE` in `configuration.md`. Useful for an operator dashboard that wants more detail than Prometheus exposes (Prometheus metrics are point-in-time; the warning service tracks warnings over an 8h window).

The platform's admin UI consumes these endpoints when displaying router status.

---

## Logs to grep for

Some recurring messages whose presence signals specific issues:

| Pattern | Meaning |
|---|---|
| `Stale entry reaper evicted` | A callback never fired — possible router bug. Should be rare. |
| `Migration content changed` | Someone edited a shipped migration. Drift; see [postgres.md](postgres.md). |
| `Failed to refresh database credentials` | Secret rotation broken or transient SM outage. |
| `Circuit breaker opened` | Endpoint failing. Look at `fc_router_circuit_breaker_state` and the warning service for the endpoint URL. |
| `Lost leadership` | Standby transition; expected during deploys. Frequent occurrence outside deploys = Redis flakiness. |
| `Pool at capacity` | Backpressure firing. Sustained = pool is undersized or endpoint is slower than budget. |
| `Recovery task reset stuck items` | Outbox-side stuck-message recovery firing. Investigation worthwhile. |
| `Pending-job filter caught paused connection` | Normal — informational, just confirming the pause filter is active. |

---

## Code references

- Metrics: `crates/fc-router/src/metrics.rs`, `crates/fc-router/src/router_metrics.rs`, `crates/fc-platform/src/shared/monitoring_api.rs`.
- Health: `bin/fc-server/src/main.rs::combined_health_handler`, `crates/fc-router/src/health.rs`.
- Logging init: `crates/fc-common/src/logging.rs`.
- Monitoring API: `crates/fc-router/src/api/`.
- Warning service: `crates/fc-router/src/warning.rs`.
