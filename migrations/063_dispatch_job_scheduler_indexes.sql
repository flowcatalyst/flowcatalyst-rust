-- msg_dispatch_jobs: the index set the dispatch paths actually use.
--
-- Every statement that touches the table was run with EXPLAIN (ANALYZE,
-- BUFFERS) against a 2,000,000-row copy (three monthly partitions: 50,000
-- PENDING, 19,500 QUEUED, 4,900 PROCESSING, 30,700 FAILED/ERROR, the rest
-- COMPLETED) before and after this change. Plain CREATE/DROP INDEX: the
-- production table is small, so the brief lock is acceptable. On the
-- partitioned parent each statement cascades to every partition, and
-- partitions created later inherit the indexes.

-- 1. The scheduler's claim.
--      WHERE status = 'PENDING' AND ... ORDER BY message_group NULLS LAST,
--      sequence, created_at, id LIMIT n FOR UPDATE SKIP LOCKED
--    The old index stopped at created_at, so the claim's total order needed a
--    sort on top of the index: an incremental sort when rows had distinct
--    created_at, and a sort of every tied row when many shared one (a batch
--    ingested in one statement) — or, when statistics lagged a burst of
--    inserts, a sort of the whole PENDING set on every claim. With id as the
--    last key the index yields the claim's order exactly, across partitions
--    (a Merge Append of ordered index scans), and no plan needs a sort.
DROP INDEX IF EXISTS idx_dispatch_jobs_pending_poll;
CREATE INDEX IF NOT EXISTS idx_dispatch_jobs_pending_poll
    ON msg_dispatch_jobs (message_group NULLS LAST, sequence, created_at, id)
    WHERE status = 'PENDING';

-- 2. The hold-back checks (claim time, delivery time, and the reaper's join):
--      WHERE message_group = ... AND (status IN ('FAILED', 'ERROR')
--            OR (status = 'PENDING' AND scheduled_for IS NOT NULL
--                AND scheduled_for > NOW()))
--      [AND (sequence, created_at, id) < (...)]
--    idx_dispatch_jobs_blocked_groups covered only the FAILED/ERROR arm, and the
--    backoff arm read every PENDING row of the group through the poll index.
--    This index holds exactly the rows that can hold a group, in the
--    positional order the checks compare by. A job without a message_group
--    never holds anything, so it is left out.
DROP INDEX IF EXISTS idx_dispatch_jobs_blocked_groups;
CREATE INDEX IF NOT EXISTS idx_dispatch_jobs_group_holders
    ON msg_dispatch_jobs (message_group, sequence, created_at, id)
    WHERE message_group IS NOT NULL
      AND (status IN ('FAILED', 'ERROR') OR (status = 'PENDING' AND scheduled_for IS NOT NULL));

-- 3. Stale recovery and the reaper's sweep:
--      WHERE status = 'QUEUED' AND updated_at < $1
--      WHERE status = 'PROCESSING' AND updated_at < $1
--      WHERE status IN ('QUEUED', 'PROCESSING') AND mode = 'BLOCK_ON_ERROR' ...
--    idx_dispatch_jobs_stale_queued was keyed on queued_at, which no query
--    filters or orders by (the sweeps use updated_at), and it did not cover
--    PROCESSING at all: that sweep scanned every partition once a minute.
DROP INDEX IF EXISTS idx_dispatch_jobs_stale_queued;
CREATE INDEX IF NOT EXISTS idx_dispatch_jobs_in_flight
    ON msg_dispatch_jobs (status, updated_at)
    WHERE status IN ('QUEUED', 'PROCESSING');

-- 4. The dispatch-job projector's claim:
--      WHERE projected_at IS NULL OR updated_at > projected_at
--      ORDER BY created_at LIMIT n
--    idx_msg_dispatch_jobs_unprojected covered only the first arm, so the
--    planner could not use it for the OR and every projector poll, including
--    the idle one each second, scanned every partition (141 ms on the
--    2,000,000-row copy, 0.4 ms with this index). A partial index on the
--    whole dirty predicate, written as the query writes it. It only ever
--    holds the rows not yet projected.
DROP INDEX IF EXISTS idx_msg_dispatch_jobs_unprojected;
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_dirty
    ON msg_dispatch_jobs (created_at)
    WHERE projected_at IS NULL OR updated_at > projected_at;
