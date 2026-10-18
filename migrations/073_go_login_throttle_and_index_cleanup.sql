-- Rust 073 mirrors Go 037_login_throttle_and_index_cleanup.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- The login throttle index, and the five single-column read-projection indexes made redundant by Go 036.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Index tuning follow-ups (docs/db-index-plan.md s.7 items 5-6):
--   1. A purpose-built index for the login brute-force throttle.
--   2. Drop one dead index + four singles made redundant by migration 036's
--      composites. All drops are structurally safe - see each note.

-- -- 1. iam_login_attempts: brute-force throttle --------------------------
-- The three hot failure-counting queries (loginattempt.go: CountRecentFailures,
-- FailureCountByIdentifierSince, FailureStatsByIdentifierIPSince) all share the
-- predicate `outcome='FAILURE' AND identifier=$1 AND attempted_at >= $cutoff`.
-- A partial composite seeks the identifier and range-scans the time bound in
-- one index; the per-(identifier,IP) variant filters ip_address as a cheap
-- residual (few failures per identifier in the window). The single-column
-- identifier / outcome / attempted_at indexes can't serve this as well.
CREATE INDEX IF NOT EXISTS idx_iam_login_attempts_failure_throttle
    ON iam_login_attempts (identifier, attempted_at)
    WHERE outcome = 'FAILURE';

-- -- 2. Drop dead / redundant indexes -------------------------------------
-- These are partitioned indexes (created on the parent in migration 019);
-- DROP INDEX cascades to every child partition.

-- Dead: nothing filters or sorts on msg_events_read.time (the event-time
-- field). All list filters + the keyset sort use created_at (the ingest time);
-- DistinctValues never offers `time`. Verified by code audit.
DROP INDEX IF EXISTS idx_msg_events_read_time;

-- Redundant: each is a strict prefix of a (col, created_at DESC) composite
-- added in migration 036, which serves every query the single did (equality
-- on the leading column) plus the list's ORDER BY created_at DESC. Dropping
-- the singles removes write amplification on the high-volume projections.
DROP INDEX IF EXISTS idx_msg_dispatch_jobs_read_client_id;  -- subset of idx_msg_dispatch_jobs_read_client_created
DROP INDEX IF EXISTS idx_msg_dispatch_jobs_read_status;     -- subset of idx_msg_dispatch_jobs_read_status_created
DROP INDEX IF EXISTS idx_msg_events_read_client_id;         -- subset of idx_msg_events_read_client_created
DROP INDEX IF EXISTS idx_msg_events_read_type;              -- subset of idx_msg_events_read_type_created
