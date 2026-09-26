# High Availability

FlowCatalyst supports active/standby HA via Redis leader election. This document covers what is and isn't gated, how failover works, and the operational implications.

For the standby crate's design: `crates/fc-standby/src/lib.rs`. For deployment topologies that use HA: [topologies.md](topologies.md).

---

## What's gated on leadership

| Subsystem | Behaviour when standby enabled |
|---|---|
| **Platform API** | Always on. Both nodes serve HTTP traffic. Load balancer sees both as healthy. |
| **Metrics + health** | Always on. So Prometheus / k8s probes always work on every node. |
| **Router** | Only on the leader. SQS polling pauses on standby nodes. |
| **Scheduler (dispatch)** | Only on the leader. Pending-job polling pauses on standby nodes. |
| **Scheduler (scheduled jobs)** | Only on the leader. Cron firings pause on standby. |
| **Stream processor** | Only on the leader. Projections + fan-out pause; partition manager pauses. |
| **Outbox processor** | Only on the leader (per outbox lock key). |
| **Migrations** | Run by every starting node. Idempotent — multiple concurrent starts are safe. |

The split has a reason for each entry:

- **Platform API stays on every node** because it's stateless given the database is shared. Putting it on the leader would force traffic to one node, defeating horizontal scaling.
- **Background work goes on the leader only** because background loops have shared in-process state (the scheduler's `MessageGroupDispatcher`, the router's `in_pipeline`) and racing against itself across nodes is unsafe.
- **Migrations are per-node** because they happen at startup, before subsystems run, and `_schema_migrations` provides serialisation. The risk of dual application is zero; the value of every node running them is "no special bootstrap step".

---

## Redis lock contract

`fc-standby` uses Redis as a distributed lock with lease renewal. The mechanism:

```
acquire:
    SET lock_key instance_id NX EX lock_ttl_seconds

renew (every refresh_interval_seconds, only by the leader):
    SET lock_key instance_id XX EX lock_ttl_seconds

release on shutdown:
    EVAL "if redis.call('GET', KEYS[1]) == ARGV[1] then redis.call('DEL', KEYS[1]) end" 1 lock_key instance_id
```

`NX` ensures we only acquire when no leader holds the lock. `XX` ensures renewal fails if someone else stole the lock (which shouldn't happen under normal operation but is theoretically possible during clock-skew + partition scenarios).

### Configuration

| Env var | Default | Behaviour |
|---|---|---|
| `FC_STANDBY_ENABLED` | `false` | Master toggle |
| `FC_STANDBY_REDIS_URL` | `redis://127.0.0.1:6379` | Redis URL (`redis+tls://` for TLS) |
| `FC_STANDBY_LOCK_KEY` | `fc:server:leader` | The lock key. **Must differ per cluster role.** |
| `FC_STANDBY_LOCK_TTL_SECONDS` | `30` | Lock TTL — worst-case failover lag |
| `FC_STANDBY_REFRESH_INTERVAL_SECONDS` | `10` | How often the leader renews |
| `FC_STANDBY_INSTANCE_ID` | hostname | This node's UUID; used to prevent accidental refresh of someone else's lock |

`lock_ttl_seconds / refresh_interval_seconds ≥ 3` is the sane ratio. The leader has at least three renewal chances within one TTL window before losing the lock. Default 30/10 gives that comfortably.

### Choosing the lock key

Use distinct keys for distinct cluster roles. Some examples:

- `fc:server:leader` — one fc-server cluster's leader.
- `fc:router:leader` — a router tier's leader (router-only `fc-server` nodes, when routers are deployed separately).
- `fc:processors:leader` — split deployment where scheduler + stream + outbox share a leader.
- `app-myapp-outbox-leader` — application-side outbox processor's leader (one per application database).

Multiple keys mean multiple independent locks. Two fc-server clusters sharing one Redis but using different lock keys won't conflict.

### TLS

If your Redis is behind TLS (Elasticache with in-transit encryption, Redis Cloud, etc.):

```sh
FC_STANDBY_REDIS_URL=rediss://:password@redis.example.com:6379
```

(Note: `rediss://` with two `s` for TLS.) Auth is via password in the URL or `AUTH` command — the `redis` Rust crate handles either.

---

## Failover walkthrough

Two nodes, fc-server-1 (leader) and fc-server-2 (standby). fc-server-1 crashes.

```
T0    fc-server-1 holds lock.
      fc-server-2 sees status=Standby (subscribed to lock changes via redis pub/sub).

T1    fc-server-1 process dies (OOM, SIGKILL, machine reboot, network partition).

T1+10s  fc-server-1 would have renewed at this point. Renewal fails (process dead).

T1+30s (lock TTL elapsed)
      Redis expires the lock.

T1+30..40s (next refresh tick of fc-server-2)
      fc-server-2 attempts SET NX → succeeds.
      fc-server-2 transitions to Leader.
      Background subsystems' `watch::Receiver<bool>` flips to true.
      Router starts polling SQS.
      Scheduler starts polling PENDING jobs.
      Stream processor starts projecting.

Total dispatch pause: 30–40 seconds worst case.
```

During the pause:
- New events keep landing in Postgres via the platform API on fc-server-2.
- New dispatch jobs accumulate in `PENDING`.
- SQS messages that were in-flight on fc-server-1 stay there; SQS will redeliver after the visibility timeout expires (15 min default).
- Scheduler's stale-recovery loop will catch any jobs stuck in `QUEUED` past 15 min once fc-server-2 takes leadership.

After the pause:
- The new leader picks up where the old leader left off. PENDING jobs drain via the normal scheduler poll.
- SQS in-flight messages either complete on redelivery to the new leader, or stale-recovery republishes them.
- No work is lost. Some jobs may be delivered slightly out of order across the failover boundary — message_group FIFO is preserved within a group, but across groups the resumption order isn't deterministic. Almost never observable, but technically possible.

### Failover with leader still alive (split-brain attempt)

If the network partition is between fc-server-1 and Redis (but fc-server-1 is still running and reachable from its other dependencies):

- fc-server-1's renewals fail. After one failure, it self-demotes to Standby and pauses subsystems.
- Redis expires the lock at TTL.
- fc-server-2 acquires the lock and starts subsystems.

The window where both think they might be leader is at most `lock_ttl` seconds — once fc-server-1's renewal fails, it immediately pauses itself rather than waiting for the lock to actually expire. So worst case: fc-server-2 takes leadership while fc-server-1 has just paused. There's a small overlap window of zero leaders, which is the same as the regular failover case.

The "two leaders simultaneously" scenario would require fc-server-1 to keep operating without realising its lock has expired. The code makes that nearly impossible — every subsystem's main loop checks the active channel state on every iteration; a Redis disconnection flips it to false immediately.

---

## Redis sizing and reliability

Redis is on the critical path for failover, **not** on the critical path for normal operation. Once the leader has the lock, it just renews every 10 s. A Redis outage of < `lock_ttl_seconds` is invisible (the renewal retries; the leader keeps working). A Redis outage longer than that demotes the leader.

So Redis can be small — a `db.t4g.micro` ElastiCache or equivalent is plenty. The traffic is ~6 set commands/minute per cluster role, regardless of platform load.

Use ElastiCache (or your cloud's equivalent) with:

- **Cluster mode disabled** (we use a single key per cluster role — no benefit to sharding).
- **Multi-AZ enabled with automatic failover** if HA of Redis itself matters. Otherwise a single-node Redis is fine; loss of Redis just degrades you to a brief no-leader state followed by re-acquisition by the surviving Redis once it's restored.
- **In-transit encryption** if your IAM model requires it.

You **do not** need persistence on Redis — the lock is regenerated on each acquisition; nothing in Redis is precious.

---

## Verifying leadership

```sh
curl http://fc-server-1:9090/health | jq .
```

```json
{
  "status": "UP",
  "leader": true,
  "version": "0.4.0",
  "components": {
    "platform":         "UP",
    "router":           "UP",
    "scheduler":        "UP",
    "stream_processor": "UP",
    "outbox":           "DISABLED"
  }
}
```

On the standby:

```json
{
  "status": "UP",
  "leader": false,
  "components": {
    "platform":         "UP",
    "router":           "STANDBY",
    "scheduler":        "STANDBY",
    "stream_processor": "STANDBY",
    "outbox":           "DISABLED"
  }
}
```

Programmatic check (e.g. for a custom monitoring tool):

```sh
redis-cli -h redis.internal GET fc:server:leader
# returns the leader's instance_id (hostname by default), or (nil)
```

---

## Forced failover

To deliberately move leadership (planned maintenance):

```sh
# On the current leader, kill the lock key
redis-cli -h redis.internal DEL fc:server:leader
```

The leader notices it doesn't hold the lock anymore on its next renewal attempt and pauses subsystems. Whichever node acquires next becomes the new leader.

A cleaner alternative is to SIGTERM the leader. The graceful-shutdown path releases the lock explicitly (via the EVAL above) so the standby can pick up immediately, no TTL wait.

---

## ALB integration (`alb` build feature)

Optional. When `FC_ALB_ENABLED=true`:

- On leadership acquisition: the router registers the local instance in an AWS ALB target group.
- On leadership loss: deregisters the local instance.

This lets an external client treat an active/standby pair as a single endpoint, with the ALB always routing to the current leader. Useful when external systems can't be told to load-balance across two endpoints themselves.

Configuration:

```sh
FC_ALB_ENABLED=true
FC_ALB_TARGET_GROUP_ARN=arn:aws:elasticloadbalancing:...:targetgroup/fc-router/...
FC_ALB_TARGET_ID=i-0123456789abcdef0    # EC2 instance ID
FC_ALB_TARGET_PORT=8080
```

The traffic watcher is gated on standby being enabled — without standby there's no leadership transitions to react to.

---

## Multi-region

Not natively supported. Each region runs an independent FlowCatalyst cluster pointing at its own Postgres + SQS + Redis. Cross-region event replication is the application's responsibility (publish into both regions' platforms, dedupe at the receiver via `eventId`).

The bottleneck is Postgres — there's no built-in support for cross-region writes, and adding logical replication for the messaging tables is non-trivial (they're partitioned, schema is large). Most multi-region needs are better served by region-local clusters with application-layer fan-out.

---

## Code references

- Leader election: `crates/fc-standby/src/lib.rs`.
- Active/standby wiring in fc-server: `bin/fc-server/src/main.rs::main` — search for `leader_election`, `active_tx`, `active_rx`.
- Per-subsystem leader gates: `bin/fc-server/src/main.rs::start_router`, `::spawn_scheduler`, `::spawn_stream_processor`, `::spawn_outbox_processor`.
- Combined health endpoint: `bin/fc-server/src/main.rs::combined_health_handler`.
- Router standby integration: `crates/fc-router/src/standby.rs`.
- ALB traffic watcher: `crates/fc-router/src/traffic.rs`.
