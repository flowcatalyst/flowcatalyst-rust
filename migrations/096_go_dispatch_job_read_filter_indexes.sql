-- Rust 096 mirrors Go 086_dispatch_job_read_filter_indexes.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- msg_dispatch_jobs_read.connection_id (projected from the write row) and the index swap behind the event type, subscription and connection filters of the dispatch-job list.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Filters of the dispatch-job list that had no index leading with the filtered
-- column and ending in the list's sort: event type (the job's `code`), the job's
-- subscription, and the job's own connection.
--
-- Rule: SWAP, never add. msg_dispatch_jobs_read is rewritten on every status
-- change (status is indexed, so the update is never HOT and every secondary
-- index gets a new entry), so each index costs on every transition of every job.
--
--   * (code, created_at DESC) REPLACES idx_msg_dispatch_jobs_read_code. The
--     leading column is unchanged, so every query the single served is still
--     served (equality / IN on code, the filter-options DISTINCT), and now
--     "this code, newest first" is read in order from the index. `code` is the
--     event type for a job the event fan-out raises (fan_out.go: Code = the
--     event's type), and the caller's own job code for an SDK job or a
--     function's fn.dispatch; no new column is needed.
--   * (subscription_id, created_at DESC) REPLACES idx_msg_dispatch_jobs_read_subscription_id,
--     same reasoning: "this subscription's jobs, newest first".
--   * connection_id is NEW on the read projection and its index is PARTIAL
--     (WHERE connection_id IS NOT NULL). Only a job that names its own connection
--     (a function's fn.dispatch; migration 082) carries one, so the index holds
--     entries for those jobs only and costs the event fan-out jobs, the bulk of
--     the table, nothing. A subscription's jobs are found by the subscription
--     (the list resolves a connection to its subscription ids first), not by a
--     connection value copied onto every job.
--
-- idx_msg_dispatch_jobs_read_application, _message_group, _event_id,
-- _dispatch_pool_id, _created_at, (client_id, created_at DESC) and
-- (status, created_at DESC) are untouched: each still serves a query of the
-- list, the by-event drill-down or the id walk.
--
-- msg_dispatch_jobs_read is RANGE-partitioned: these are partitioned indexes
-- (built on every partition, ACCESS EXCLUSIVE on the parent while they build).
-- Fine for a small or fresh database. On a populated one, build them first, online,
-- with scripts/ops/096-dispatch-read-indexes-online.sql; IF NOT EXISTS then
-- makes this migration's CREATEs no-ops and only the DROPs run.
--
-- Rows projected before this migration have connection_id NULL; the projector
-- fills it on a job's next projection (it is in the upsert's DO UPDATE), and the
-- ops script above has an optional backfill. No blocking backfill here.
ALTER TABLE msg_dispatch_jobs_read
    ADD COLUMN IF NOT EXISTS connection_id VARCHAR(17);

CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_code_created
    ON msg_dispatch_jobs_read (code, created_at DESC);
DROP INDEX IF EXISTS idx_msg_dispatch_jobs_read_code;

CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_subscription_created
    ON msg_dispatch_jobs_read (subscription_id, created_at DESC);
DROP INDEX IF EXISTS idx_msg_dispatch_jobs_read_subscription_id;

CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_connection_created
    ON msg_dispatch_jobs_read (connection_id, created_at DESC)
    WHERE connection_id IS NOT NULL;
