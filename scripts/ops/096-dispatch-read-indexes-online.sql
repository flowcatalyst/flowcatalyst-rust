-- Migration 096 (Go 086) PRODUCTION ONLINE INDEX BUILD (optional, run BEFORE deploying 096).
--
-- 096_go_dispatch_job_read_filter_indexes.sql creates three indexes on the partitioned
-- projection msg_dispatch_jobs_read:
--   idx_msg_dispatch_jobs_read_code_created          (code, created_at DESC)
--   idx_msg_dispatch_jobs_read_subscription_created  (subscription_id, created_at DESC)
--   idx_msg_dispatch_jobs_read_connection_created    (connection_id, created_at DESC) WHERE connection_id IS NOT NULL
-- CREATE INDEX on a partitioned parent builds every partition under one ACCESS EXCLUSIVE
-- lock (writes to the projection stall for the whole build), which is fine on a small
-- database and not on 90 days of a busy one. This script builds them the online way
-- (the same pattern as any partitioned index build): the parent index ON ONLY (invalid, no build), each
-- partition's index CONCURRENTLY, then ATTACH. Afterwards migration 096's CREATE ... IF NOT
-- EXISTS statements find the parent indexes and do nothing; only its DROP of the two
-- superseded single-column indexes runs (instant).
--
-- Run with psql (the \gexec statements must run outside a transaction):
--     psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f scripts/ops/096-dispatch-read-indexes-online.sql
-- Re-runnable: a finished step is skipped. A failed CONCURRENTLY build leaves an INVALID
-- index on that partition; drop it (DROP INDEX CONCURRENTLY <name>) and re-run.
--
-- The new column first (instant: nullable, no default):
ALTER TABLE msg_dispatch_jobs_read ADD COLUMN IF NOT EXISTS connection_id VARCHAR(17);

CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_code_created
    ON ONLY msg_dispatch_jobs_read (code, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_subscription_created
    ON ONLY msg_dispatch_jobs_read (subscription_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_connection_created
    ON ONLY msg_dispatch_jobs_read (connection_id, created_at DESC) WHERE connection_id IS NOT NULL;

-- One concurrent build per partition per index.
SELECT format('CREATE INDEX CONCURRENTLY IF NOT EXISTS %I ON %I %s', c.relname || '_' || i.suffix, c.relname, i.cols)
  FROM pg_inherits h
  JOIN pg_class c ON c.oid = h.inhrelid
  CROSS JOIN (VALUES
        ('code_created_idx',         '(code, created_at DESC)'),
        ('subscription_created_idx', '(subscription_id, created_at DESC)'),
        ('connection_created_idx',   '(connection_id, created_at DESC) WHERE connection_id IS NOT NULL')
  ) AS i(suffix, cols)
 WHERE h.inhparent = 'msg_dispatch_jobs_read'::regclass
 ORDER BY c.relname, i.suffix
\gexec

-- Attach each partition's index to its parent index (skipping the ones already attached).
SELECT format('ALTER INDEX %I ATTACH PARTITION %I', p.parent_index, c.relname || '_' || p.suffix)
  FROM pg_inherits h
  JOIN pg_class c ON c.oid = h.inhrelid
  CROSS JOIN (VALUES
        ('idx_msg_dispatch_jobs_read_code_created',         'code_created_idx'),
        ('idx_msg_dispatch_jobs_read_subscription_created', 'subscription_created_idx'),
        ('idx_msg_dispatch_jobs_read_connection_created',   'connection_created_idx')
  ) AS p(parent_index, suffix)
 WHERE h.inhparent = 'msg_dispatch_jobs_read'::regclass
   AND NOT EXISTS (
        SELECT 1 FROM pg_inherits ih
         WHERE ih.inhrelid = (c.relname || '_' || p.suffix)::regclass
           AND ih.inhparent = p.parent_index::regclass)
 ORDER BY c.relname, p.suffix
\gexec

-- Optional backfill of connection_id for function-dispatch jobs created before 096 (rows
-- the projector has not touched since). Bounded: only write rows that name a connection.
-- Run per month if the write table is large; the write table has no connection_id index,
-- so each statement scans one monthly partition of msg_dispatch_jobs.
--
--   UPDATE msg_dispatch_jobs_read r SET connection_id = j.connection_id
--     FROM msg_dispatch_jobs j
--    WHERE j.connection_id IS NOT NULL AND r.connection_id IS NULL
--      AND j.id = r.id AND j.created_at = r.created_at
--      AND j.created_at >= '2026-10-01' AND j.created_at < '2026-11-01';
--
-- Check afterwards: every partition index valid and attached.
SELECT c.relname AS partition_index, i.indisvalid AS valid, p.relname AS parent_index
  FROM pg_index i
  JOIN pg_class c ON c.oid = i.indexrelid
  LEFT JOIN pg_inherits ih ON ih.inhrelid = c.oid
  LEFT JOIN pg_class p ON p.oid = ih.inhparent
 WHERE i.indrelid IN (SELECT inhrelid FROM pg_inherits WHERE inhparent = 'msg_dispatch_jobs_read'::regclass)
   AND (c.relname LIKE '%\_code\_created\_idx' OR c.relname LIKE '%\_subscription\_created\_idx'
        OR c.relname LIKE '%\_connection\_created\_idx')
 ORDER BY 1;
