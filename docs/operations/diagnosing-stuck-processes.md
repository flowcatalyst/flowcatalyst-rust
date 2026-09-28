# Diagnosing a stuck router, pipeline or function host

What to look at, in order, when messages stop moving: which endpoint answers
which question, which metric shows what, and how to turn on the deeper tools
(task dumps, tokio-console, OTLP traces). Written so that an operator, or an
agent holding a read-only token, can triage without shell access to the task.

Background: in Rust a stuck async task leaves no thread to dump, and a panic
inside a spawned task only ends that task. The surface below exists so that
the logs are not the only evidence.

---

## The surface at a glance

| Question | Where | Auth |
|---|---|---|
| Is the process's async runtime alive, saturated or blocked? | `GET /router/diagnostics/runtime?sampleMs=1000` (router role), `GET :9090/diagnostics/runtime` (any fc-server) | `router:view` |
| What is every task waiting on? | `GET /router/diagnostics/task-dump`, `GET :9090/diagnostics/task-dump` | `router:operate` |
| Where is message X now, and what happened to it? | `GET /router/diagnostics/messages/{messageId}` | `router:view` |
| What is group G doing? | `GET /router/diagnostics/groups/{group}?poolCode=` | `router:view` |
| What did the router do recently (filterable)? | `GET /router/diagnostics/events?messageId=&group=&poolCode=&kind=&limit=` | `router:view` |
| Runtime, process and panic metrics | `GET /router/metrics`, `GET :9090/metrics`, function host `GET :9090/metrics` (host-only) or `:9091/metrics` (beside other roles), fc-outbox-processor `:9090/metrics` | open (scrape) |
| Which message is each worker on? | `GET /router/monitoring/mediating` | `router:view` |
| Which groups are parked or blocked? | `GET /router/monitoring/blocked-groups` | `router:view` |
| Is the router holding message X? (SDK recovery) | `GET /router/monitoring/in-flight-messages/check?messageId=` | `router:view` |
| In-flight entry, with last-seen and retry state | `GET /router/monitoring/in-flight-messages/detail?messageId=` | `router:view` |
| Consumers, breakers, pools, queues, warnings | `GET /router/monitoring/consumer-health`, `…/circuit-breakers`, `…/pool-stats`, `…/queue-stats`, `…/warnings` | `router:view` |

`/router` is fc-server's default `FC_ROUTER_HTTP_PREFIX`. `router:view` is
`platform:messaging:router:view` and `router:operate` is
`platform:messaging:router:operate`: the platform bearer token the router API
already takes (owner ruling 2; the rules are in
`crates/fc-router/src/api/platform_auth.rs`). The diagnostics routes are
**never anonymous**:

- On the router API they sit behind the same guard as `/monitoring/*`. On a
  router left open outside dev mode (the transitional `AUTH_MODE=NONE`,
  decision #43) they answer `403 DIAGNOSTICS_REQUIRE_AUTH` rather than open.
  In dev mode (`fc-dev`, `FLOWCATALYST_DEV_MODE=true`) they answer.
- On fc-server's metrics port (`FC_METRICS_PORT`, 9090) they verify tokens
  against `FC_DIAGNOSTICS_PLATFORM_URL`, else `FC_ROUTER_PLATFORM_URL`, else
  the platform in the same process. With none of these every call is `401`.
  This is what a platform-only or worker node (scheduler, stream, outbox)
  has: no router API to ask.
- The task dump needs `router:operate` even though it is a `GET`: it pauses
  every runtime worker while it walks the tasks.

```sh
TOKEN=…   # a platform API token with platform:messaging:router:view
curl -s -H "Authorization: Bearer $TOKEN" https://router.internal/router/diagnostics/runtime | jq
```

---

## Triage: a stuck router

Work down the list; each step says what the answer means.

1. **Does the process answer at all?** `GET /router/health` (open). No answer
   while the container is up: the runtime is wedged (every worker blocked),
   see the last paragraph of step 2.

2. **Is the runtime healthy?** `GET /router/diagnostics/runtime?sampleMs=1000`.
   The handler samples the tokio runtime for a second:
   - `runtime.tokio.workersNeverParked`: workers that ran the whole window
     without going idle. One or two under load is normal; the same worker in
     every sample, with `busyRatio` near 0 (tokio folds busy time in only
     when a worker parks), is a **blocked worker**: a blocking call or a busy
     loop on an async thread. Take a task dump (step 7).
   - `runtime.tokio.globalQueueDepth` staying above a few hundred: runnable
     tasks are waiting for a worker; the workers are saturated or blocked.
   - `runtime.tokio.aliveTasks` climbing across samples without traffic
     growing: tasks are spawned faster than they finish (a hung downstream,
     a leak).
   - `runtime.panics`, `runtime.taskRestarts`: panics since start, and which
     supervised background loops have been restarted (see "Panics").
   - `runtime.process`: RSS, open fds against `maxFds`, threads.
   - `router`: in-flight count, messages in workers, live and parked groups,
     the flight recorder's fill.

   If this endpoint itself hangs, the runtime cannot schedule the handler:
   every worker is blocked. The last scraped `tokio_runtime_*` series and the
   logs are then the evidence; restart the task. A task dump request in that
   state answers `504` after its timeout, which confirms it.

3. **Is intake running?** `GET /router/monitoring/consumer-health` (each
   consumer's last poll), `GET /router/monitoring/warnings` (look for
   "paused — its destination pools are at capacity", "Consumer … stalled",
   breaker and config warnings), `GET /router/monitoring/standby-status` (a
   standby does not poll).

4. **Where are the messages?**
   - `GET /router/monitoring/mediating`: every message a worker holds now,
     longest first, with its target and `attempts`. A delivery here for
     minutes is a slow or hanging target (breakers:
     `GET /router/monitoring/circuit-breakers`).
   - `GET /router/monitoring/blocked-groups`: every live ordered group:
     `buffered`, `working` (a drainer owns it), `parkedAt`, suppression, and
     the pool's `concurrency` / `rateLimitPerMinute`. `working: false` with
     `buffered > 0` is a parked group; the lifecycle reaper restarts such a
     group every 5 minutes (`Restarted parked message groups` WARN). It
     should never happen.
   - `GET /router/monitoring/in-flight-messages?limit=50`: the tracker,
     oldest first. `lastSeenElapsedMs` growing past the broker's visibility
     timeout means the broker stopped redelivering it (a phantom entry).

5. **One message.** `GET /router/diagnostics/messages/{messageId}` answers
   `status`: `MEDIATING` (in a worker: `mediating` says since when, the
   target and attempts), `BUFFERED` (waiting behind its group's head:
   `buffered` gives the position and whether a drainer runs),
   `RETRY_BACKOFF` (waiting out an in-place retry), `TRACKED_IDLE` (tracked
   but in no worker or buffer: settling, or a phantom), or
   `NOT_IN_PIPELINE`. `history` is what the flight recorder saw, oldest
   first (see "The flight recorder"), so it also answers for a message that
   has already left.

6. **One group.** `GET /router/diagnostics/groups/{group}`: per pool holding
   it, whether a drainer owns it, the message in the worker, what is
   buffered behind it (`[messageId, attempts]`), and the group's recent
   events (the `GROUP_DECISION` events say why it stopped: `RETRY_HEAD`,
   `RETURN_GROUP`, `BLOCK_GROUP`).

7. **Task dump.** `GET /router/diagnostics/task-dump?timeoutMs=5000`
   (`router:operate`) returns every task's async backtrace as text: a hung
   delivery shows the mediator's HTTP frames under a pool worker, a consumer
   parked on capacity shows `wait_for_capacity_or_cancel`. `501`: the build
   has no task dumps (below). `504`: a worker is blocked and the runtime
   could not pause; that is the answer to step 2.

8. **Act.** Reset a breaker (`POST …/circuit-breakers/{name}/reset`),
   force-ack a phantom (`POST …/in-flight-messages/{id}/ack`), lift a group
   flush (`POST …/group-flushes/{pool}/{group}/clear`), reload config
   (`POST /router/config/reload`), or restart the task. All need
   `router:operate`.

## Triage: a stuck pipeline (scheduler, stream, outbox)

These roles have no router API; use fc-server's metrics port.

- `GET :9090/diagnostics/runtime` and `GET :9090/diagnostics/task-dump`, as
  above.
- `GET :9090/metrics`: the scheduler's `scheduler_*` series (served whatever
  roles run; the port used to serve a constant), plus the runtime and
  process series.
- Logs: every scheduler tick runs in a `scheduler.poll` span (`claimed`,
  `published`), each publish in `scheduler.publish` (`jobs`, `unpublished`),
  each stale sweep in `scheduler.stale_recovery`. A delivery through
  `/api/dispatch/process` runs in `dispatch.process` (`job_id`,
  `subscription_id`, `group`, `client_id`, `attempt`); `job_id` is the id the
  router logs as `message_id`, so one search follows a job from the
  scheduler through the router to the webhook.
- `fc_task_restarts_total{task="scheduler.poller"}` or
  `{task="scheduler.stale_recovery"}` above zero: that loop panicked and was
  restarted (the panic line has the backtrace).
- Outbox: `outbox.poll` (`claimed`), `outbox.group` (`group`) and
  `outbox.forward` (`outbox_id`, `group`, `item_type`, or `count` and
  `first_id` for an ungrouped batch) spans; the group admin API on
  `FC_OUTBOX_ADMIN_PORT` (localhost) lists paused and blocked groups.

## Triage: a stuck function host

- The host's observability listener (`/health`, `/ready`, `/metrics`) runs
  on its own OS thread and runtime, so it answers even when the host's main
  runtime is wedged. Its `/metrics` carries the **main** runtime's
  `tokio_runtime_*` series and the process series: a main runtime whose park
  counters stopped moving is blocked.
- Every invocation logs in an `invocation` span (`function`, `version`,
  `execution_id`, `correlation_id`).
- Beside other roles (`FC_FUNCTION_HOST_ENABLED` with others), fc-server's
  `:9090/diagnostics/*` covers the host too: it is the same runtime. A
  host-only node has no authenticated diagnostics endpoint of its own; use
  its `/metrics`, the logs, and tokio-console when needed.

---

## Metrics

Served on every `/metrics` listed above, alongside the existing series.
Label cardinality is bounded: a worker index (the runtime's worker count)
and a supervised-task name (a fixed set).

| Series | Type | Meaning |
|---|---|---|
| `tokio_runtime_workers` | gauge | Worker threads of the main runtime |
| `tokio_runtime_alive_tasks` | gauge | Tasks spawned and not finished |
| `tokio_runtime_global_queue_depth` | gauge | Runnable tasks waiting for a worker |
| `tokio_runtime_worker_busy_seconds_total{worker}` | counter | Time each worker ran tasks (folded in when it parks) |
| `tokio_runtime_worker_park_total{worker}` | counter | Times each worker went idle; flat while the worker is active = saturated or blocked |
| `process_cpu_seconds_total` | counter | User + system CPU |
| `process_resident_memory_bytes`, `process_virtual_memory_bytes` | gauge | Linux |
| `process_max_resident_memory_bytes` | gauge | Peak RSS |
| `process_open_fds`, `process_max_fds` | gauge | Descriptors and the soft limit |
| `process_threads` | gauge | OS threads (Linux) |
| `process_start_time_seconds` | gauge | Start time |
| `fc_process_panics_total` | counter | Panics on any thread (each logged, see "Panics") |
| `fc_task_restarts_total{task}` | counter | Panics caught in supervised loops, by loop |
| `fc_messages_rejected_total{reason}` | counter | Router: messages removed without delivery: `malformed` (could not be decoded; also a WARN line, a CONFIGURATION/ERROR warning and a `REJECTED` event) or `strict_routing` |

Only tokio's **stable** metrics are used (no `tokio_unstable` needed), read
straight from the runtime; the process figures come from `/proc/self` and
`getrusage`, with no new dependency.

Useful alerts: `increase(fc_process_panics_total[10m]) > 0`;
`increase(fc_task_restarts_total[10m]) > 0`;
`tokio_runtime_global_queue_depth > 1000 for 5m`;
`process_open_fds / process_max_fds > 0.8`;
`changes(tokio_runtime_worker_park_total[2m]) == 0` per worker (a worker
that has not parked for two minutes).

## Logs and spans

Every line logged inside a message's processing carries its identifiers, from
the span it runs in. fc-server logs JSON; each line has `span` (the innermost
span and its fields) and `spans` (the whole stack, outermost first):

| Span | Fields | Where |
|---|---|---|
| `router.consumer` | `queue`, `generation` | a consumer's poll loop |
| `router.route_batch` | `queue`, `batch`, `size` | routing one polled batch |
| `router.dispatch` | `message_id`, `pool`, `group`, `queue`, `attempt` | a message in a pool worker: mediation, ack/nack, group decisions |
| `dispatch.process` | `job_id`, `subscription_id`, `group`, `client_id`, `attempt` | `/api/dispatch/process` delivering a claimed job |
| `scheduler.poll` / `scheduler.publish` / `scheduler.stale_recovery` | `claimed`, `published` / `jobs`, `unpublished` / none | the dispatch scheduler |
| `outbox.poll` / `outbox.group` / `outbox.forward` | `claimed` / `group` / `outbox_id`, `group`, `item_type` (or `count`, `first_id`) | the outbox processor |
| `invocation` | `function`, `version`, `execution_id`, `correlation_id` | a function invocation |

CloudWatch Logs Insights, everything about one message:

```
fields @timestamp, level, message, span.name
| filter span.message_id = "evt_0HZXEQ5Y8JY5Z" or span.job_id = "evt_0HZXEQ5Y8JY5Z"
| sort @timestamp asc
```

Spans do not log themselves: `FC_LOG_SPAN_EVENTS=close` adds a line (with
its duration) whenever a span closes, which is a line per message: for a
debugging session only.

## The flight recorder

The router keeps, in memory, the last `FC_ROUTER_FLIGHT_RECORDER_EVENTS`
(default 16 384, about 3 MB; `0` turns it off) things it did, per message:

| Kind | Facts |
|---|---|
| `ROUTED` | pool, group, queue, `batch` |
| `REDELIVERED` | a broker redelivery while in the pipeline (receipt refreshed) |
| `DEFERRED_CAPACITY` | handed back because the pool was full |
| `REJECTED` | strict-routing malformed, pool creation or submit failed, behind a failed submit |
| `SUPPRESSED` / `DUPLICATE_ACKED` | acked without delivery (group flushed / another copy owns it) |
| `DISPATCH_STARTED` | `attempt` |
| `DISPATCH_FINISHED` | `attempt`, `outcome`, `status`, `durationMs`, `action` (`Ack`/`Release`/`Retry`), `delaySecs`, `detail` (the error) |
| `GROUP_DECISION` | `RETRY_HEAD` / `RETURN_GROUP` / `BLOCK_GROUP`, with the siblings affected |
| `ACKED` / `ACK_FAILED` / `NACKED` | settlement with the broker (`delaySecs`) |
| `ABANDONED` / `PANICKED` | the callback dropped unresolved; the mediator panicked (message released) |
| `UNTRACKED` / `RELEASED_AT_SHUTDOWN` | reaped or force-acked; handed back at shutdown |

It is Java's `Dispatch` / `GroupDecision` / `MessageSettled` JFR events,
always on and queryable (`/diagnostics/messages/{id}`,
`/diagnostics/groups/{group}`, `/diagnostics/events`). It restarts empty.
Its cost is under "Overhead".

## Panics

- A process-wide panic hook logs every panic as one `ERROR` line with target
  `panic`: `panic_message`, `panic_location`, `thread`, a `backtrace` (at
  most ten per ten seconds; later ones say `backtrace_suppressed`), and,
  because the line is written on the panicking thread, the span context of
  whatever was running (a message's `message_id`, a job's `job_id`). It is
  counted in `fc_process_panics_total`. Nothing goes to bare stderr once
  logging is up.
- A mediator that panics no longer unwinds its worker: the message is
  released like an unavailable target (and, in an ordered group, everything
  buffered behind it, in order), recorded as `PANICKED`.
- A panic while routing a polled batch is logged and the consumer keeps
  polling; the messages already handed to a pool are settled by their
  workers, the rest come back at the broker's visibility timeout.
- Every long-lived background loop runs supervised: logged, counted in
  `fc_task_restarts_total{task}`, and restarted after a backoff (1 s doubling
  to 60 s). The router's `router.memory_health`, `router.consumer_watchdog`,
  `router.stall_detector`, `router.queue_health_monitor`,
  `router.warning_cleanup`, `router.health_report`, `router.stale_reaper`,
  `router.config_sync`, `router.breaker_eviction`,
  `router.leadership_monitor`, `router.leadership_follower`,
  `router.in_pipeline_reaper`, `router.notification_batch`; the scheduler's
  `scheduler.poller` and `scheduler.stale_recovery`. Consumer poll loops keep
  their contract: a loop that dies is rebuilt by the consumer watchdog.
- The outbox counts panics in a send (`outbox.group_send`,
  `outbox.forward`), a poll or a recovery pass and carries on: the group
  stops and releases what it holds rather than stranding, and the rows the
  send held are released from memory, so recovery can re-send them.

---

## Task dumps: how they are built

`tokio::runtime::Handle::dump` needs tokio's `taskdump` feature, which in
turn needs `--cfg tokio_unstable`, on Linux x86/x86_64/aarch64. A cfg flag
cannot be set per Cargo profile on stable, so it is a build choice:

- **The production image has them.** `Dockerfile` builds with
  `RUSTFLAGS="--cfg tokio_unstable"` and `--features taskdump` unless
  `--build-arg FC_TASKDUMP=0` (then `/diagnostics/task-dump` answers 501).
  The cook and the build use the same flags, so the dependency layer caches.
- **Every other build is stable tokio**: local builds, CI, `fc-dev`. They
  answer 501. To build one by hand on Linux:
  `RUSTFLAGS="--cfg tokio_unstable" cargo build --release -p fc-server --features taskdump`
  (`RUSTFLAGS` replaces `.cargo/config.toml`'s linker flags; add
  `-C link-arg=-fuse-ld=lld` if you rely on them).
- **Cost.** Tokio's documentation: enabling the feature "imposes virtually
  no additional runtime overhead", while calling `Handle::dump` is
  expensive. Measured under "Overhead". A dump pauses every worker while it
  re-polls each task in tracing mode: take one when stuck, not on a
  schedule. It adds three crates to the image (`backtrace`, `addr2line`,
  `gimli`).

## tokio-console (live task inspection)

Not in the production image: its instrumentation records every task's
spawns, polls and wakes, which costs throughput. Build a debugging image with
it and run that in place of the stuck task's image. In the builder stage:

```sh
RUSTFLAGS="--cfg tokio_unstable" \
  cargo build --release -p fc-server --bin fc-server --features taskdump,tokio-console
```

Run with `FC_TOKIO_CONSOLE=true`. The console's gRPC server binds
`FC_TOKIO_CONSOLE_BIND`, default `127.0.0.1:6669`, **localhost only**; never
put it on a public interface. Reach it through an SSM port-forward (ECS Exec
enabled on the service):

```sh
aws ssm start-session \
  --target "ecs:${CLUSTER}_${TASK_ID}_${RUNTIME_ID}" \
  --document-name AWS-StartPortForwardingSession \
  --parameters '{"portNumber":["6669"],"localPortNumber":["6669"]}'
# then, locally:
tokio-console http://127.0.0.1:6669
```

(`RUNTIME_ID` is the container's runtime id from `aws ecs describe-tasks`.)
In the console, sort tasks by busy time to find one that holds a worker, and
look for idle tasks with no wakers to find one nothing will wake.
`fc-dev --features tokio-console` works the same way locally. The feature
adds three crates (`console-subscriber`, `console-api`, `humantime`).

## OpenTelemetry traces

Off by default. Build with `--features otel` (fc-server, fc-dev,
fc-outbox-processor) and run with `FC_OTEL_ENABLED=true`: every span above is
exported over OTLP/HTTP (protobuf) to `OTEL_EXPORTER_OTLP_ENDPOINT` (default
`http://localhost:4318`, a collector sidecar such as ADOT) as service
`OTEL_SERVICE_NAME` (default the binary's name), filtered like the logs. The
exporter reuses the workspace's `reqwest`; the feature adds six crates
(`opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`,
`opentelemetry-http`, `opentelemetry-proto`, `tracing-opentelemetry`).
Spans are flushed at shutdown.

---

## Environment

| Variable | Default | Where | Meaning |
|---|---|---|---|
| `FC_ROUTER_FLIGHT_RECORDER_EVENTS` | `16384` | router | Events the flight recorder keeps; `0` = off |
| `FC_DIAGNOSTICS_PLATFORM_URL` | `FC_ROUTER_PLATFORM_URL`, else the in-process platform | fc-server | The platform whose JWKS verifies metrics-port diagnostics tokens |
| `FC_LOG_SPAN_EVENTS` | unset | all | `close`: a line per closed span, with its duration; `full`: open/enter/exit/close |
| `FC_TOKIO_CONSOLE` | `false` | builds with `tokio-console` | Start the console server |
| `FC_TOKIO_CONSOLE_BIND` | `127.0.0.1:6669` | same | Its address |
| `FC_OTEL_ENABLED` | `false` | builds with `otel` | Export spans over OTLP/HTTP |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4318` | same | Collector (`/v1/traces` is appended) |
| `OTEL_SERVICE_NAME` | the binary's name | same | `service.name` |

Build: `FC_TASKDUMP` (Docker build argument, default `1`); Cargo features
`taskdump`, `tokio-console`, `otel` on fc-server (fc-dev and
fc-outbox-processor: `tokio-console`, `otel`).

## Overhead

Measured with `crates/fc-router/tests/throughput_bench.rs`: in process, with
a target that answers at once, so only the router's own work counts
(routing, tracking, the pool, spans, the recorder).

OVERHEAD_TABLE

A real delivery is an HTTP exchange costing tens of microseconds of CPU and
milliseconds of wall time, so the added per-message cost is a small fraction
of a deployed router's work.
