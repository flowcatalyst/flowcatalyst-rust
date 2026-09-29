# Debugging

Where to look when something doesn't behave as expected. Organised by symptom — find the symptom that matches what you're seeing.

---

## Symptom: I published an event but nothing happened downstream

**Step 1.** Did the event land? Check `/events` (or filter by event type code). The event should appear within ~1 second of your POST.

- **Event not in the list.** Your POST didn't succeed or wasn't authenticated. Check the HTTP response from your call.
- **Event in the list.** Move to step 2.

**Step 2.** Was a subscription matched?

Open the event detail (`/events/{id}`) — there's a "Dispatch Jobs" section. If empty:

- No subscription matched the event type. Either you don't have a subscription, or its pattern doesn't match.
- Check the subscription's `active` flag, `event_type` pattern, and `client_id` (subscriptions are scoped to clients — events for client A don't fan out to subscriptions belonging to client B).

If you have a dispatch job listed, move to step 3.

**Step 3.** What's the dispatch job status?

| Status | Meaning |
|---|---|
| `PENDING` | Waiting for the scheduler. Either the scheduler is paused, or the connection is paused, or the dispatch job is in a blocked message group. |
| `QUEUED` | Scheduled to SQS. Either the router isn't running or it's behind on processing. |
| `PROCESSING` | Router is actively delivering. If it's been here a long time (> minutes), the webhook receiver is slow or hanging. |
| `COMPLETED` | Done. Receiver returned success. |
| `FAILED` | All retries exhausted, or 4xx received. Check the attempts. |
| `CANCELLED` | Operator cancelled. |
| `EXPIRED` | TTL exceeded. |

For PENDING and QUEUED stuck for > 1 minute, check [Subsystem health](#subsystem-health) below.

For FAILED, check the attempts — each attempt records the HTTP status and response body.

---

## Symptom: my webhook receiver isn't being called

**Step 1.** Confirm a dispatch job exists for the event (see above).

**Step 2.** Confirm the connection isn't paused. `/connections/{id}` — the status should be ACTIVE. If PAUSED, dispatch jobs accumulate as PENDING and won't deliver until you un-pause.

**Step 3.** Check the connection's `endpoint_url` is reachable from the platform.

```sh
# from a platform node
curl -v https://your-receiver.example.com/webhook
```

A 404 or 5xx is fine for this test — what matters is whether the platform can reach the endpoint at all. Connection refused / DNS failure → networking issue.

**Step 4.** Check the dispatch job's recent attempts. `/dispatch-jobs/{id}` → "Attempts" section. Each attempt shows the response code, response body excerpt, and any error.

Common patterns:

- 401 / 403 → auth misconfigured. Check the connection's auth_token or signing_secret.
- 404 → the URL is wrong.
- 5xx → receiver is failing. Check receiver logs.
- timeout → receiver too slow (> 15 min default).
- DNS error → endpoint hostname is unresolvable.

---

## Symptom: webhook is being called but the receiver says signature invalid

99% of the time this is the receiver's signature verification doing something wrong with the body bytes.

Common causes:

- **Re-encoding the JSON before verifying.** Frameworks that parse JSON for you give you a dict/object, then your validation re-serialises it. The result has different whitespace from what the platform signed. Verify against the **raw bytes** of the request body, not the re-serialised form.
- **Wrong secret.** Confirm the connection's signing_secret in the admin UI matches what your receiver is using.
- **Timestamp window too tight.** A request that takes > 5 minutes to reach your receiver (clock skew, queue) would fail timestamp verification. Loosen to 10 min if you have clock skew.
- **Wrong algorithm.** It's HMAC-SHA256, not SHA1. Hex-encoded, not base64.

Test verification manually:

```python
import hmac, hashlib
secret = "your-secret"
timestamp = "2026-05-13T15:42:08.331Z"   # X-FLOWCATALYST-TIMESTAMP
body = '{"id":"mev_...","type":"...","data":{...}}'   # raw body bytes

expected = hmac.new(secret.encode(), (timestamp + body).encode(), hashlib.sha256).hexdigest()
# Compare against X-FLOWCATALYST-SIGNATURE
```

If `expected` matches the header, your secret + algorithm are right and the bug is in how you're reading the body.

---

## Symptom: dispatches are slow

Latency = scheduler-poll-cadence + SQS-roundtrip + router-pool-wait + receiver-roundtrip.

**Diagnose:**

- Look at the dispatch job's timeline: `created_at` (when fan-out created the job) → `queued_at` (when scheduler published to SQS) → first attempt timestamp.
- Big gap between `created_at` and `queued_at`? Scheduler is behind. Check `fc_scheduler_pending_jobs` metric — is the backlog growing?
- Big gap between `queued_at` and first attempt? Router is behind, or the pool's concurrency cap is saturated. Check `fc_router_pool_queue_depth` per pool.
- Big gap between first attempt timestamp and completion? The receiver itself is slow.

**Act:**

- Scheduler backlog → raise `FC_SCHEDULER_MAX_CONCURRENT_GROUPS` or `FC_SCHEDULER_BATCH_SIZE`.
- Router pool saturated → raise pool concurrency in the admin UI (`/dispatch-pools`).
- Slow receiver → not the platform's problem; investigate the receiver.

---

## Symptom: I see jobs in QUEUED that aren't moving

The router isn't picking them up. Causes:

1. **Router not running.** Check `/health` on the metrics port — does it show `router: UP`?
2. **Router on standby.** If running HA, only the leader processes. Check `leader: true` in `/health`.
3. **Pool config missing.** If the job's `pool_code` doesn't match any pool in router config, jobs would be NACKed. Check the router logs for "unknown pool" errors. The `DEFAULT-POOL` should always exist as a fallback.
4. **SQS issue.** Router is up but can't reach SQS — check `fc_router_consumer_lag_seconds`. AWS console for SQS messages-available vs messages-in-flight.
5. **Capacity gate.** All pools at concurrency cap → router defers polling. Sustained = real backlog problem; raise concurrency.

The scheduler's stale-recovery loop will reclaim jobs that have been QUEUED > 15 min (default), so the issue is bounded — but you shouldn't rely on stale-recovery as the primary path.

---

## Symptom: subscription "should" match but doesn't fan out

**Step 1.** Confirm subscription state:

- `active: true`
- `event_type` pattern actually matches the event type code (segment by segment, with `*` as wildcard)
- `client_id` matches the event's `client_id`

**Step 2.** Check the fan-out cache lag.

The stream processor caches subscriptions and refreshes every 5 seconds. A subscription you just created takes up to 5 s before fan-out sees it. Wait, retry, see if it works on a freshly-published event.

**Step 3.** Check the fan-out service is running.

`/health` on the metrics port → `stream_processor` should be `UP` on the leader. If it's STANDBY or DISABLED, fan-out isn't running on this node.

**Step 4.** Verify with a debug query:

The event's `fanned_out_at` column (in `msg_events_read`) should be non-null shortly after the event is created. If it's null indefinitely, the fan-out service isn't claiming the event.

---

## Symptom: receiver gets duplicate webhook calls

The platform delivers **at-least-once**. Duplicates can happen:

- The receiver returned a transient error after starting to process — the message gets retried even though the work was done.
- The router's stale-recovery republishes a job whose original SQS message turned out to still be in flight.
- A network interruption between receiver and platform — the receiver returned 200 but the platform never got it.

**Mitigation:** make your receiver idempotent. Use `X-FlowCatalyst-Dispatch-Job-Id` (stable across retries of the same job) as your dedup key. See [receiving-webhooks.md](receiving-webhooks.md#idempotency).

If duplicates are appearing without an obvious cause:

- Check the dispatch job's attempt count. If > 1, the duplicate is a retry — your receiver returned non-200 or timed out on attempt N-1.
- If the duplicates have **different** dispatch job IDs but the same event ID: fan-out is somehow creating multiple jobs per (event, subscription). Investigate — this would be a bug.

---

## Subsystem health

Quick way to check every subsystem at once:

```sh
curl http://platform:9090/health | jq .
```

```json
{
  "status": "UP",
  "leader": true,
  "components": {
    "platform":         "UP",
    "router":           "UP",
    "scheduler":        "UP",
    "stream_processor": "UP",
    "outbox":           "DISABLED"
  }
}
```

| State | Means |
|---|---|
| UP | Running on this node |
| STANDBY | Enabled but this node is not the leader |
| DISABLED | Toggle is off |

If a component you expect to be UP is STANDBY: this isn't the leader. Either query the leader (`redis-cli GET fc:server:leader` to find out who) or force failover (kill the leader's lock).

For per-component metrics (router pool depth, scheduler backlog, etc.), see [../operations/observability.md](../operations/observability.md).

---

## Useful debug pages

The admin UI has dedicated debug views for high-volume data:

- **`/debug/events`** — firehose of raw events. No pagination — use `?size=N` to control batch.
- **`/debug/dispatch-jobs`** — firehose of dispatch jobs.
- **`/audit-logs`** — every UoW write across the platform, including operator actions.

The dashboard at `/dashboard` shows aggregates: throughput rates, recent failures, backlog gauges. Good for "is the system breathing?" at a glance.

---

## Useful API queries

Find every dispatch job for a specific event:

```
GET /api/events/{event_id}/dispatch-jobs
```

Find every attempt for a dispatch job:

```
GET /api/dispatch-jobs/{job_id}/attempts
```

Find events of a specific type in a time range:

```
GET /api/events?type=orders.fulfillment.shipment.shipped&from=2026-05-13T00:00:00Z&to=2026-05-13T23:59:59Z
```

Audit log filtered by entity:

```
GET /api/audit-logs?entity_type=subscription&entity_id=sub_...
```

---

## When all else fails

- Restart the platform. Most stuck subsystems recover gracefully — the scheduler's stale-recovery, the outbox's recovery task, and the router's lifecycle reaper all run on startup.
- Bump log level. `RUST_LOG=debug` or `RUST_LOG=info,fc_router=debug,fc_platform::scheduler=debug` for targeted noise.
- Compare with a known-good environment. If staging works and prod doesn't with the same setup, look at infrastructure diffs (SG rules, IAM, secrets).

For platform bugs (something the platform itself is doing wrong), the audit log and the per-component metrics together tell most of the story. Open an issue with: the event ID, dispatch job ID, time window, what you expected, what you saw.

---

## Code references

- API request handlers (BFF, used by the admin UI): `crates/fc-platform/src/shared/bff_*_api.rs`.
- Audit log emission (every UoW write): `crates/fc-platform-core/src/usecase/unit_of_work.rs::persist_audit`.
- Combined health: `bin/fc-server/src/main.rs::combined_health_handler`.
- Router monitoring API: `crates/fc-router/src/api/`.
- Operations-side runbooks: [../operations/runbooks.md](../operations/runbooks.md).
