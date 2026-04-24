# Adaptive Concurrency — Per-Pool Mediators

**Status:** design, not yet implemented.
**Scope:** `crates/fc-router` — `HttpMediator`, `ProcessPool`, and the pool
concurrency limiter.

---

## Why per-pool mediators

Today there's one `HttpMediator` (one reqwest `Client`, one connection pool)
shared across every `ProcessPool`. Two problems with that:

### 1. AWS HTTP/2 stream cap

AWS ALB / API Gateway / NLB cap HTTP/2 connections at **128 concurrent
streams**. With a single shared mediator, every pool multiplexes over the
same handful of connections to the same origin. Once we saturate those
128 slots, **all pools queue behind each other**, regardless of how
concurrency is configured per pool.

Per-pool mediators give each pool its own connection pool. N pools → up to
N × (connections-per-origin) parallel H/2 connections = N × 128 streams.
Pool isolation at the transport layer.

### 2. Adaptive concurrency needs per-pool signal

The concurrency limit for a pool should react to that pool's own observed
backend behaviour — not an average across mixed workloads. One adaptive
limiter per pool, watching one workload, is the shape that works.

---

## Vegas algorithm (the adaptive strategy)

Named after TCP Vegas. Implemented in Netflix's
[concurrency-limits](https://github.com/Netflix/concurrency-limits).

### Signal

- **`rtt_min`** — the best (lowest) observed response time. Proxy for
  "backend is idle, no queueing".
- **`rtt_current`** — smoothed average (or median) RTT in the current
  sample window.
- **`limit`** — current cap on in-flight requests for the pool.

### Update rule (per sample window)

```
queue_size = limit * (1 - rtt_min / rtt_current)
```

`queue_size` is an estimate of how many requests are queued behind the
backend's actual capacity. When `rtt_current ≈ rtt_min`, queue is ~0 and
we can grow. When `rtt_current >> rtt_min`, queue is large and we should
shrink.

```
if queue_size < alpha:         limit += 1          # grow — backend idle
elif queue_size > beta:        limit -= 1          # shrink — backend queuing
else:                          limit  = limit      # hold — in the pocket
```

Typical values: `alpha = 3`, `beta = 6`. These are the "dead zone" edges.

### Why this is the right shape

- **Proactive, not reactive.** Latency starts rising *before* the backend
  starts returning 5xx. Vegas sees queueing in the `rtt_current / rtt_min`
  ratio well before AIMD would see failures. No cliff-crossing.
- **Runs forever in steady state.** If backend capacity doesn't change,
  the limit sits in the dead zone and stops oscillating. AIMD can't hold
  still; it increments until it hits the cliff again.
- **Recovers quickly from backend improvements.** A deploy that makes the
  backend faster lowers `rtt_current`, queue estimate drops, limit grows.
  No waiting for a failure window.

### Why it's subtle

- **`rtt_min` can drift upward.** If the pool is never idle, every
  observed RTT is already under some load, and `rtt_min` creeps up. The
  limiter then under-reacts. Mitigation: periodically reset `rtt_min`
  (e.g. every 10 minutes) and re-probe. Or accept the bias — a
  consistently-busy pool doesn't have a meaningful "idle" baseline
  anyway.
- **Needs workload homogeneity** (see next section).
- **Cold start:** the first few samples set `rtt_min`. Start with a
  conservative limit (e.g. concurrency/2) and let it grow in.

---

## The homogeneity precondition

Vegas reads RTT as the congestion signal. **That only works if RTT means
the same thing across requests.**

Bad pool setup:

> Mixing 20ms cache lookups with 2s PDF generation in one pool.

Here `rtt_min = 20ms`, occasional PDF jobs raise `rtt_current` into the
seconds, the limiter reads that as "backend is queuing" and shrinks —
starving the cache lookups for no good reason. The PDF job wasn't
queueing; it's just slow.

Good pool setup:

> All requests in a pool have similar nominal latency, even if the
> throughput varies.

Operator responsibility: **segregate fast work from slow work at the pool
boundary.** This is already the shape of pool config — you pick which
events/subscriptions route to which pool — it just becomes *load-bearing*
instead of advisory once adaptive concurrency is on.

---

## Why not AIMD

AIMD (TCP Reno style): `limit += 1` per success window, `limit /= 2` on
any failure.

Sound on paper — the algorithm that runs the internet. But:

1. **Only learns from failures.** It has to push past the capacity
   boundary and observe the backend falling over to know where the
   boundary is. Then it cuts in half and walks back up. This repeats
   forever in steady state.
2. **Always-oscillating.** AIMD never holds still. The sawtooth is a
   feature on best-effort IP networks where flows compete; it's a bug
   for a pool where you want a stable in-flight count.
3. **Wastes capacity on overshoot.** Every "cut in half" costs you half
   your concurrency for a recovery window, even if the backend only
   briefly blipped.

The core objection — *"I don't like systematically waiting for things to
break"* — is structural. AIMD literally cannot tell the limit is correct
without probing past it.

Vegas reads latency changes. Latency rises before failures start. That's
the gap AIMD ignores and Vegas exploits.

---

## Operator UX — the complexity concern

Adaptive concurrency adds one more thing operators have to understand.
Mitigations:

### 1. Opt-in per pool, not global

Pool config gets a `concurrency_mode`:

```
concurrency_mode: "static"       # current behaviour, explicit cap (default)
concurrency_mode: "vegas"        # adaptive, requires homogeneous workload
```

Default stays `static` with the existing `concurrency: N`. Nothing
changes for existing configs. Vegas is opt-in and operators must
acknowledge the precondition.

### 2. Expose the control loop

Per-pool metrics:
- `pool.limit.current` — gauge
- `pool.rtt.min` — gauge
- `pool.rtt.current` — gauge
- `pool.queue_estimate` — gauge

Operators can see the algorithm working. If `rtt_current / rtt_min` is
chaotic (2×, 10×, 2× across samples), the pool isn't homogeneous enough
— they read that in the dashboard and move traffic out.

### 3. Hard floor + ceiling

Even in Vegas mode, operators set `min_concurrency` and `max_concurrency`.
Prevents runaway shrink on a backend blip and runaway growth on a badly-
configured pool.

### 4. Don't silently start in Vegas mode

First-time setup with Vegas should log a WARN listing the preconditions:

> Pool `X` started with adaptive concurrency (Vegas). Verify requests in
> this pool have similar nominal latency. See
> docs/adaptive-concurrency.md.

---

## What "done" looks like

**Phase 1 — per-pool mediator.** `ProcessPool` owns its own `HttpMediator`.
`HttpMediatorConfig` moves into pool config (probably with sane shared
defaults at the router level). Circuit breaker registry stays shared
(breakers are keyed by endpoint, not pool).

**Phase 2 — Vegas limiter.** Replace `Semaphore(N)` in the pool with a
`VegasLimiter`. Gate with `concurrency_mode: "vegas"`; static remains
default. Metrics land at the same time — a Vegas limiter without
observability is a black box.

**Phase 3 — docs + dashboards.** Operator-facing runbook: how to tell if
your pool is a good Vegas candidate, how to read the dashboard, how to
fall back to static if things go sideways.

Phases should land as separate PRs. Per-pool mediator on its own is
useful (fixes the 128-stream cap) even if Vegas never ships.

---

## Open questions

- **minRTT refresh cadence.** 10min? On every deploy event? Tied to
  circuit breaker state transitions?
- **Sample window size.** Too short = noise dominates. Too long = slow
  reaction. Netflix uses 100 samples or 1 second, whichever first.
- **Interaction with circuit breaker.** When breaker trips, freeze
  `limit` or let it shrink to min? Probably freeze — breaker trip isn't
  a latency signal.
- **Metric labels.** Per-pool, obviously. But do we also want per-origin?
  A pool can hit multiple endpoints.
