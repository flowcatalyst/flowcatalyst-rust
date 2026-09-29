# Scheduled Jobs

Cron-triggered firings. Independent of event-driven dispatch — they're for "do something at this time", not "do something when X happens".

---

## Two modes

| Mode | What it does |
|---|---|
| `EVENT` (recommended) | When the cron fires, the platform emits a domain event into `msg_events`. Existing subscriptions handle the rest. |
| `WEBHOOK` | When the cron fires, the platform directly POSTs to a configured URL. Bypasses subscriptions. |

The EVENT mode is recommended because it keeps events as the integration mechanism — the scheduled task slots into the same fan-out / subscription / dispatch-job pipeline as everything else. The WEBHOOK mode exists for cases where you want a direct line without modelling the event in your schema.

---

## Defining a scheduled job

Two paths:

### Admin UI

`/scheduled-jobs` → "New scheduled job":

| Field | Description |
|---|---|
| Code | Stable identifier, e.g. `billing:nightly-rollup` |
| Cron expression | Standard 5-field cron syntax. The platform uses `croner` (DST/timezone-aware). |
| Timezone | IANA name, e.g. `Australia/Brisbane`, `UTC` |
| Target mode | `EVENT` or `WEBHOOK` |
| Event type / Target URL | Depending on mode |
| Payload | JSON payload included in the event (or POST body) |
| Active | Toggle without delete |

### SDK definition sync

Same idea, declarative:

```ts
sync.defineApplication("billing")
  .withScheduledJobs([
    {
      code: "billing:nightly-rollup",
      cron: "0 2 * * *",
      timezone: "Australia/Brisbane",
      target: { kind: "event", eventType: "billing:invoicing:rollup:requested" },
      payload: { currency: "AUD" },
    },
  ])
  .build();
```

On sync, the platform upserts the job. Deleting it from your manifest deactivates the job (doesn't delete history of past firings).

---

## Cron expression

Standard 5-field: `minute hour day-of-month month day-of-week`.

| Expression | Meaning |
|---|---|
| `0 2 * * *` | 02:00 daily |
| `*/15 * * * *` | every 15 minutes |
| `0 9 * * 1-5` | 09:00 Monday-Friday |
| `0 0 1 * *` | first day of every month |

DST handling is via the `croner` library — when DST changes, a "02:00 daily" job correctly handles the missing or duplicated hour. The `timezone` field controls what timezone the cron is evaluated against; it's separate from server time.

---

## What happens when it fires

```
1. Cron evaluation (every minute in the scheduled-job scheduler — separate from the dispatch scheduler)
        │
        ▼
2. Insert msg_scheduled_job_instances row (status PENDING)
        │
        ▼
3. (Target mode EVENT)
   Emit domain event:
        type:    event_type field
        source:  /scheduled-jobs/{code}
        data:    payload field
        message_group: scheduled-job-instance-id    (so multiple firings stay ordered)
   Status → COMPLETED.

   (Target mode WEBHOOK)
   POST target_url with body:
        {
            "scheduled_job_code": "billing:nightly-rollup",
            "fired_at": "...",
            "instance_id": "msj_...",
            "payload": { ... }
        }
   Status → RUNNING then COMPLETED or FAILED based on response.
```

For EVENT mode, the event flows through the normal pipeline. If you have a subscription that matches `event_type`, you'll see a dispatch job within seconds. If you don't, the event is stored but nothing else happens.

---

## Manual firing

For testing, force a firing without waiting for the cron:

```
POST /api/scheduled-jobs/{id}/fire
```

Same outcome as a cron-triggered firing, plus a `ScheduledJobFiredManually` audit event recording who triggered it.

---

## Instances and logs

Each firing produces a row in `msg_scheduled_job_instances`. Subordinate `msg_scheduled_job_instance_logs` rows accumulate output from the firing (useful for WEBHOOK mode — captures the HTTP response).

Browse via:

- UI: scheduled-job detail page shows recent instances.
- API: `GET /api/scheduled-jobs/{id}/instances`, `GET /api/scheduled-jobs/instances/{id}/logs`.

Both tables are partitioned (90-day retention by default).

---

## Missed firings (platform downtime)

If the platform is down during a firing window:

- The job's `next_fire_at` advances normally on restart based on the cron expression.
- **Missed firings are not retroactively fired.** If you needed the 02:00 firing and the platform was down 01:55–02:05, no firing happens for that day.
- The job's "last fired" timestamp shows the most recent successful firing — you can detect gaps.

This is intentional. The alternative ("fire every missed instance on restart") creates cascading-firing scenarios that almost never reflect operator intent. If you need missed-firing recovery, monitor `last_fired_at` and trigger a manual firing when you see a gap.

The same applies to "platform was down for a week, was the daily rollup supposed to fire 7 times?" — no, only the next scheduled firing fires.

---

## Operational properties

| Property | Behaviour |
|---|---|
| Standby leader | Only the leader runs the scheduled-job scheduler. On failover, the new leader picks up the cron evaluation. |
| Time skew | The scheduler ticks once per minute. A firing scheduled for HH:MM:30 fires at the next minute boundary (HH:MM+1:00) — not at HH:MM:30 exactly. |
| Concurrent firings | Two firings of the same job (extremely rare — would require >1 minute clock disagreement) deduplicate by `(job_id, fire_at)` unique constraint. |
| Throughput | Limited by the once-per-minute scheduler tick. Sub-minute granularity isn't supported. |
| Audit | Every firing produces a `ScheduledJobFired` audit log entry. Manual firings get `ScheduledJobFiredManually`. |

For sub-minute scheduling, you're outside the scheduled-job model. Use a different mechanism (an in-app timer, an external scheduler emitting events, etc.).

---

## Worked example — nightly billing rollup

Goal: every night at 02:00 Brisbane time, calculate yesterday's billing totals for each customer.

1. Define the event type via SDK sync (or admin UI):

   ```
   event_type: billing:invoicing:rollup:requested
   schema: { date: string (ISO), currency: string }
   ```

2. Define the scheduled job:

   ```
   code: billing:nightly-rollup
   cron: "0 2 * * *"
   timezone: Australia/Brisbane
   target_mode: EVENT
   event_type: billing:invoicing:rollup:requested
   payload: { date: "{{ yesterday }}", currency: "AUD" }
   ```

   (The platform doesn't yet substitute templates like `{{ yesterday }}` — pass a static payload, and have the receiver derive the date from `event.time`.)

3. Define a subscription:

   ```
   event_type: billing:invoicing:rollup:requested
   connection: con_billing_internal
   dispatch_pool: dpl_billing
   dispatch_mode: IMMEDIATE
   ```

4. The receiver (`con_billing_internal.endpoint`) is your billing service's webhook. It computes the rollup and stores results.

Flow on each firing:

```
02:00 Brisbane time
   ├─ Scheduler emits billing:invoicing:rollup:requested event
   │
   ├─ Fan-out creates dispatch job for the subscription
   │
   ├─ Router POSTs to billing service
   │
   └─ Billing service computes rollup, returns 200
```

Why not just call the billing service directly with target_mode=WEBHOOK? Because:

- The EVENT version is observable in the standard event list (you can see "rollup requested" without looking at scheduled-job logs).
- Other subscriptions could consume the same event (e.g. an audit/analytics service that wants to know every time the rollup runs).
- You can pause the connection if the receiver is down, and PENDING dispatch jobs accumulate; with WEBHOOK mode, missed firings are just missed.

---

## Quick reference

| Question | Answer |
|---|---|
| How precise is the firing time? | Minute granularity. Fires within one minute of the cron expression. |
| Are missed firings replayed? | No. Only the next scheduled firing fires after downtime. |
| Can I see what fired? | Yes — `/scheduled-jobs/{id}/instances` in the UI. |
| Can I cancel a firing in flight? | A firing in `RUNNING` state can be cancelled via API. Already-completed firings are immutable. |
| Multiple platforms / multi-region? | Each cluster has its own scheduler. The job exists per cluster. Cross-region coordination isn't built in. |

---

## Code references

- Scheduled-job aggregate: `crates/fc-platform/src/scheduled_job/`.
- Scheduler service: `crates/fc-platform-scheduled-jobs/src/scheduled_job/scheduler/`.
- Cron parser: the `croner` crate (DST/timezone-aware).
- Instance & log tables: `migrations/021_scheduled_jobs.sql`, `migrations/022_partition_scheduled_job_history.sql`, `migrations/024_scheduled_jobs_add_target_url.sql`.
- Leader-gated start: `bin/fc-server/src/main.rs::spawn_scheduled_job_scheduler`.
