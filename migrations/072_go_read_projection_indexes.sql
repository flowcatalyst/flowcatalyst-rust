-- Rust 072 mirrors Go 036_read_projection_indexes.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- The rich read-projection indexes on msg_dispatch_jobs_read and msg_events_read.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Align the CQRS read projections with their filtered-list access patterns
-- (see docs/db-index-plan.md). The write tables stay lean (migration 015
-- deliberately stripped query indexes from msg_dispatch_jobs); ALL rich,
-- user-facing query indexes live on the *_read projections. The dispatch-job
-- list / by-event / filter-options reads are repointed from the write table
-- to msg_dispatch_jobs_read in the same change.
--
-- NOTE: msg_events_read and msg_dispatch_jobs_read are RANGE-partitioned by
-- created_at. CREATE INDEX on the parent cascades to every partition under a
-- brief ACCESS EXCLUSIVE lock - fine for fc-dev's near-empty embedded PG and
-- for pre-launch volumes. On a populated production DB, build these
-- CONCURRENTLY per-partition and ATTACH to the parent instead (zero-downtime
-- path documented in docs/db-index-plan.md s.6).

-- -- msg_dispatch_jobs_read -----------------------------------------------
-- Already indexed (migration 019): status, client_id, application,
-- subscription_id, message_group, created_at. The reads repointed here also
-- need:

-- Drill-down: GET /api/dispatch-jobs/event/{eventId} (Repository.FindByEventID).
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_event_id
    ON msg_dispatch_jobs_read (event_id);

-- Equality filters + /filter-options DISTINCTs not previously covered.
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_dispatch_pool_id
    ON msg_dispatch_jobs_read (dispatch_pool_id);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_code
    ON msg_dispatch_jobs_read (code);

-- Dominant filter+sort combos: fold the list's ORDER BY created_at DESC into
-- the index so the two most common facets (tenant, status) avoid a sort.
-- These make the single-column client_id / status indexes above redundant for
-- these queries; the singles are kept for now and revisited (verify-then-drop)
-- against pg_stat_user_indexes - see docs/db-index-plan.md worklist #6.
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_client_created
    ON msg_dispatch_jobs_read (client_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_status_created
    ON msg_dispatch_jobs_read (status, created_at DESC);

-- -- msg_events_read ------------------------------------------------------
-- The events list already reads this projection, but its dominant sort column
-- (created_at, the ingest timestamp) had no index - only `time` (the event
-- timestamp, which nothing sorts or filters on) did. Without this the default
-- "recent events" view scans-and-sorts across partitions.
CREATE INDEX IF NOT EXISTS idx_msg_events_read_created_at
    ON msg_events_read (created_at);

-- Dominant filter+sort combos for the events list.
CREATE INDEX IF NOT EXISTS idx_msg_events_read_client_created
    ON msg_events_read (client_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_msg_events_read_type_created
    ON msg_events_read (type, created_at DESC);
