# PostgreSQL

The platform holds all state in Postgres — tenants, IAM, events, dispatch jobs, audit logs, OAuth tokens. This document covers provisioning, sizing, migrations, partitioning, and credential management.

---

## Version and extensions

| Item | Requirement |
|---|---|
| PostgreSQL | 14 or newer. Tested on 14/15/16/17. Should work on 18+ given driver support. |
| Extensions | None required. |

We deliberately avoid `pg_partman`, `pg_cron`, and other extensions. RDS allowlists move on AWS's schedule, not ours, and `pg_partman_bgw` was already dropped from the RDS PG 18 allowlist between two RDS releases. Our partition manager runs in pure Rust (see [../architecture/partitioning.md](../architecture/partitioning.md)) — same behaviour in dev (fc-dev's embedded PG) and prod (RDS, self-hosted, anything).

---

## Provisioning

### Self-hosted

```sh
createdb flowcatalyst
createuser -P flowcatalyst_app
psql -c "GRANT ALL ON DATABASE flowcatalyst TO flowcatalyst_app"
psql -d flowcatalyst -c "GRANT ALL ON SCHEMA public TO flowcatalyst_app"
```

That's it. Migrations run at platform startup; no manual schema setup needed.

### AWS RDS (typical production)

Minimum recommended starting size:

| Setting | Value |
|---|---|
| Engine | PostgreSQL 16 (or newer) |
| Instance | `db.t4g.medium` (light) → `db.m7g.large` (steady prod) |
| Storage | gp3, 100 GB initial, autoscale enabled |
| Multi-AZ | yes |
| Backup retention | 7 days minimum |
| Performance Insights | enabled |
| Parameter group | default, with `max_connections` raised to at least 200 |

Right-sizing comes down to event throughput:

| Events/sec sustained | Instance class (rough) |
|---|---|
| ≤ 50 | t4g.medium |
| 50–500 | m7g.large |
| 500–2k | m7g.xlarge with provisioned IOPS |
| 2k+ | m7g.2xlarge+, consider read-replica for the read models |

Storage grows roughly 1 KB per event (write model) + 2 KB per event (read model with denormalised columns) + 0.5 KB per dispatch attempt. A workload of 100 events/sec averaging 3 subscriptions = 300 dispatch jobs/sec = ~30 GB/day before retention drops kick in. At 90-day retention (the partition manager default) that's ~2.7 TB steady-state — plan accordingly.

### Connection limits

The platform uses two pools:

- **Main pool** (`fc-server` API + scheduler): `max_connections` configurable, default 20.
- **Stream processor pool**: 4 connections, separate from the main pool to prevent projection loops contending with API traffic.

Per node: ~25 connections. Active/standby pair: ~50 connections. Add 10 per fc-outbox-processor instance, none for a router-only fc-server node (it opens no Postgres connection). So `max_connections = 200` accommodates roughly 4-8 platform nodes + outbox sidecars. Bump it as you scale; AWS RDS has a `max_connections` derived from instance class that you may need to override via parameter group.

---

## Credential management

Three modes, see [configuration.md](configuration.md#database). For production we recommend **AWS Secrets Manager mode** with RDS-managed rotation:

1. Create the RDS instance with a Secrets Manager-managed master password.
2. Create a separate application user (don't use the master).
3. Create a Secrets Manager secret for the app user, enable rotation.
4. Set `DB_HOST`, `DB_NAME`, `DB_SECRET_ARN` on the platform processes.
5. The platform's `AwsSecretProvider` polls the secret every `DB_SECRET_REFRESH_INTERVAL_MS` (default 5 min) and swaps the pool's `connect_options` when the password changes.

A few caveats:

- **Every `PgPool` needs the refresh task registered.** The main pool registers it automatically when `secret_provider` is set in `resolve_database_url`. The stream-processor pool gets it registered in `spawn_stream_processor`. If you add a new dedicated pool, register it: `fc_platform::shared::database::start_secret_refresh(provider, pool, url, interval)`. Otherwise rotation will kill that pool silently while the others keep working.
- **The refresh polls the secret; it doesn't subscribe.** Worst-case 5 minutes between rotation and our pool picking it up. Existing connections continue to work until they're closed (postgres doesn't enforce password on existing sessions). New connections opened after rotation but before refresh will fail with auth errors — the pool will retry until refresh catches up.
- **Rotation strategy.** RDS's "single-user" rotation is the safest model. Multi-user is fine but adds complexity (the app user identity changes across rotations).

See [secrets-and-rotation.md](secrets-and-rotation.md) for the broader picture.

---

## Migrations

`migrations/` directory at the repo root. Run automatically by `shared/database.rs::run_migrations` at every startup, in order. Tracked in `_schema_migrations` (id, name, applied_at, checksum).

### The cardinal rule

**Never edit a shipped migration.** The runner stores a sha256 of each migration's content. If you edit a migration that's already been applied somewhere:

- The runner detects the drift via checksum mismatch.
- It does **not** re-run the SQL — that would be silent data loss.
- It logs a WARN visible in the startup output:

```
WARN  Migration content changed since it was applied. The new SQL has NOT
been executed — migrations are immutable once shipped. If you intended a
schema change, write a new migration. If the edit was benign (e.g.
comment-only) you can silence this warning with:
UPDATE _schema_migrations SET checksum = '<current>' WHERE migration_id = '<id>';
```

The warning is informational, not fatal — the bad outcome is silent drift, not refused deployment. For an intentional schema change, write a new migration: `NNN_alter_<table>_<change>.sql` with `ALTER TABLE ... ADD COLUMN IF NOT EXISTS ...`.

### Partition-key constraint

Seven tables are RANGE-partitioned on `created_at`:

```
msg_events
msg_events_read
msg_dispatch_jobs
msg_dispatch_jobs_read
msg_dispatch_job_attempts
msg_scheduled_job_instances
msg_scheduled_job_instance_logs
```

Postgres rejects UNIQUE constraints / PRIMARY KEYs on partitioned tables that don't include the partition key. So every UNIQUE on these tables must include `created_at`. The PK on these is `(id, created_at)`, not just `id`.

If you write a migration that adds a UNIQUE on a partitioned table without including `created_at`, the migration will fail at apply time with:

```
ERROR  unique constraint on partitioned table must include all partitioning columns
```

### Manual migration tracker fix

`_schema_migrations` has a `checksum` column. If you intentionally renamed a migration file (e.g. fixing a typo in the name) but didn't change the SQL, you'll trip the drift warning forever. Reset:

```sql
UPDATE _schema_migrations SET checksum = '<new-sha256>' WHERE migration_id = '<id>';
```

The runner emits the expected checksum in its WARN log, so you can copy-paste it.

---

## Partitioning

Detailed in [../architecture/partitioning.md](../architecture/partitioning.md). Operationally:

- **Forward partitions:** 3 monthly partitions are kept ahead of current month. Created daily by the in-Rust `PartitionManagerService`, gated by the standby leader.
- **Retention:** partitions whose range is older than 90 days get dropped. `DROP TABLE IF EXISTS` is O(1) — there's no row-by-row delete pass to compete with ingest I/O.

To change retention in production: edit `bin/fc-server/src/main.rs::spawn_stream_processor`, set `PartitionManagerConfig { retention_days: 180, ... }`. Rebuild and deploy.

Verify partition state manually:

```sql
SELECT child.relname
FROM pg_inherits i
JOIN pg_class p ON i.inhparent = p.oid
JOIN pg_class child ON i.inhrelid = child.oid
WHERE p.relname = 'msg_events'
ORDER BY child.relname;
```

Expect partitions from `(this month - 1)` through `(this month + 3)`.

---

## The dispatch queue table

`msg_dispatch_queue` holds exactly one row for every PENDING dispatch job that is waiting to be claimed (keyed by `job_id`, ordered by `idx_dispatch_queue_order` on `(message_group NULLS LAST, sequence, job_created_at, job_id)`). The dispatch-job lifecycle writes it in the same SQL statement as the job (migrations 064 and 065; the Go and Java platforms run the same DDL against the same database). The scheduler claims from it and never scans `msg_dispatch_jobs` for PENDING. Anything other than the lifecycle writing either table is a bug; the reconcile sweep repairs and reports drift.

**The claim is two plain statements, and it deletes.** `SELECT job_id ... ORDER BY ... LIMIT` over the order index, then `DELETE ... WHERE job_id = ANY(...) RETURNING ...` by primary key; what the DELETE returns is the claim. A claimed job is PENDING with no queue row until it is marked QUEUED. A job that is not published (failed publish, failed mark, dropped as poisoned, withheld, held back by an earlier job of its group) has its row **restored** from the job table (`INSERT ... SELECT FROM msg_dispatch_jobs ... WHERE status = 'PENDING' ON CONFLICT DO NOTHING`), so a job that has moved on is not resurrected. `claimed_at` is unused and always NULL (it will be dropped).

**Crash recovery is the reconcile "insert missing" pass.** A job whose claimer died has no queue row. A new leader restores every PENDING job without a queue row (no age guard, in batches of 5,000 until none are left) before its first claim; every minute the leader runs the reconcile pass with its age guards (jobs not updated for 60 s), excluding the ids it is publishing.

Indexes on `msg_dispatch_jobs` after 065: the primary key, `idx_dispatch_jobs_status_group` on `(status, message_group, sequence, created_at, id)` (hold-back by FAILED / ERROR, the stale sweeps, the reaper, the reconcile sweep) and the projector's `idx_msg_dispatch_jobs_dirty` (the one partial index left on the table). No other dispatch index is partial except the projection feed's three.

The queue table is small and churns: it has `fillfactor = 70` and vacuums and analyzes after 2,000 changes (`autovacuum_*_scale_factor = 0`).

### The scheduler's own connection pool

The scheduler's pool (`create_scheduler_pool`, used by `fc-server` and `fc-dev`; no other pool) sets `plan_cache_mode = force_custom_plan` and `enable_sort = off` on every connection, and keeps them across a credential refresh. Measured on PostgreSQL:

- A single-statement claim (`WITH c AS (SELECT ... LIMIT) UPDATE/DELETE ... FROM c`) is planned as a quadratic nested loop when the queue was drained and then analysed with its pages still allocated (240 ms / 2.4 s / 10 s per claim at 5k / 50k / 200k rows). An idle queue followed by a burst is the normal case; hence two plain statements.
- A cached GENERIC plan made while the queue is empty is a seq scan and is reused after a burst (about 0.5 s per claim at 100,000 rows) until the next autoanalyze. sqlx prepares and caches every statement per connection, so `force_custom_plan` is what prevents it.
- With no statistics, or statistics taken while the queue was empty, the planner prefers a seq scan plus a sort of the whole queue for `ORDER BY ... LIMIT`; `enable_sort = off` makes it walk `idx_dispatch_queue_order`.

Every statement on that pool (the claim, the restore, mark-QUEUED, the hold-back, the stale sweeps, the reaper, the reconcile sweep, the caches) is covered by the plan tests with these two settings applied (`dispatch_queue_plan_test`).

---

---

## Query rules

Three CLAUDE.md rules that matter for ops because violations cause sustained DB load:

1. **No queries in loops.** N+1 patterns destroy throughput. Reviewers catch this in PRs; if you see DB CPU spiking after a deploy and recent changes touched an iteration over results, that's the first place to look.
2. **`fetch_optional` over `fetch_one`.** `fetch_one` panics on empty result. If you see "stream of restart logs" without a clear cause, panic on an unwrap is the candidate. Grep the codebase for `.fetch_one` to spot risks.
3. **Shallow queries for list endpoints.** A "list clients" dropdown that hydrates connections, subscriptions, and event types per client will choke on a large tenant. The `*_shallow()` repository methods exist for this; they should be used wherever the consumer only needs id + name.

---

## Read replicas

Not required, not currently used in production. The CQRS projection model means the API reads from `*_read` tables that are tuned for query workload (lots of indexes); the write tables stay lean.

If you do introduce a read replica, the natural seam is the API tier reading from the replica (with replication lag accepted) while the scheduler/stream/router continue reading-and-writing the primary. The current code doesn't have a "read pool vs write pool" distinction, but adding one is a few-hour change. The bigger question is whether you actually need it; most deployments don't.

---

## Backup and restore

### Self-hosted

`pg_dump` for logical backups, `pg_basebackup` + WAL archiving for PITR. Standard PG fare.

The platform has **no state outside Postgres** other than:

- JWT signing keys (regenerate or restore from secret store)
- Encryption key `FLOWCATALYST_APP_KEY` (must match — encrypted OIDC client secrets are unrecoverable without it)
- Redis (lock state — re-acquired on restart, no recovery needed)
- SQS in-flight messages (will redeliver on visibility timeout)

So a Postgres restore + matching `FLOWCATALYST_APP_KEY` + JWT keys is a complete restore.

### AWS RDS

Default backups + Point-in-Time Recovery (35-day retention max on RDS). Snapshots before any operation that could lose data (manual schema change, big migration, partition cleanup outside normal cadence).

Encryption keys must be preserved alongside the database. Treating them as a unit prevents the worst-case "we have a snapshot but no app key" scenario.

---

## Observability

Useful PG-level metrics to watch:

| Metric | Why |
|---|---|
| `pg_stat_database.numbackends` (connections in use) | Pool exhaustion |
| `pg_stat_database.xact_commit / xact_rollback` | Transaction rate |
| `pg_stat_activity.state_change` for long-running queries | Stuck queries |
| `pg_stat_bgwriter.checkpoints_*` | WAL pressure |
| Replication lag (if replica) | Read-staleness |
| Disk used | Trending vs partition retention |

The platform itself exposes a number of Postgres-touching metrics:

| Metric | Source |
|---|---|
| `fc_stream_events_projected_total` | event projection throughput |
| `fc_stream_dispatch_jobs_projected_total` | dispatch-job projection throughput |
| `fc_stream_events_fanned_out_total` | fan-out throughput |
| `fc_scheduler_jobs_polled_total` | scheduler poll throughput |
| `fc_scheduler_jobs_queued_total` | scheduler publish throughput |
| `fc_scheduler_pending_jobs` | jobs claimed by the last poll, at most the batch size (gauge); not the backlog |
| `fc_scheduler_poll_full_batches_total` | polls that filled the batch (a backlog is draining) |
| `fc_scheduler_queued_jobs` | depth of QUEUED (gauge) |
| `fc_scheduler_stale_jobs_recovered_total` | recovery firings (should usually be 0) |
| `fc_scheduler_queue_backlog` | due rows in the dispatch queue (gauge): the dispatch backlog |

If `fc_scheduler_poll_full_batches_total` rises continuously and `fc_scheduler_jobs_queued_total` lags it: the publish path is slow or stuck (`fc_scheduler_pending_jobs` is capped at the batch size and cannot show a backlog).
If `fc_scheduler_stale_jobs_recovered_total` increments routinely: the router isn't completing dispatches within the 15-minute window — investigate.
If `fc_stream_events_fanned_out_total` lags `fc_events_received_total`: fan-out is starving.

See [observability.md](observability.md).

---

## Code references

- Pool creation: `crates/fc-platform/src/shared/database.rs::create_pool`.
- Secret refresh: `crates/fc-platform/src/shared/database.rs::start_secret_refresh`.
- Migration runner: `crates/fc-platform/src/shared/database.rs::run_migrations`.
- Migration files: `migrations/*.sql`.
- Partition manager: `crates/fc-stream/src/partition_manager.rs`.
- Built-in role seeding: `crates/fc-platform/src/shared/database.rs::seed_builtin_roles`, `crates/fc-platform-iam/src/role/entity.rs::roles`.
