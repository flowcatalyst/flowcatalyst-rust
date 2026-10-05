-- msg_dispatch_queue: one row for every dispatch job that is PENDING, and none
-- for any other job. A small, unpartitioned table with an ordinary index,
-- kept exact by explicit writes from the dispatch-job lifecycle
-- (fc_common::dispatch_lifecycle): the same statement that creates a job or
-- moves it into / out of PENDING writes (or deletes) its queue row.
--
-- Nothing reads it for dispatching yet: the scheduler's claim, the hold-back
-- checks and the indexes on msg_dispatch_jobs are unchanged by this
-- migration.
--
-- Shared physical database: the Go and Java platforms create the same table
-- with the same DDL (same column types, same index name). No foreign key:
-- msg_dispatch_jobs is partitioned and its partitions are dropped.
--
--   job_id / job_created_at  address the partitioned job row
--   version                  the job's updated_at when this row was written
--   claimed_at               set by the scheduler's claim (a later step)
CREATE TABLE IF NOT EXISTS msg_dispatch_queue (
    job_id           VARCHAR(13)  PRIMARY KEY,
    job_created_at   TIMESTAMPTZ  NOT NULL,
    message_group    VARCHAR(200),
    sequence         INTEGER      NOT NULL,
    scheduled_for    TIMESTAMPTZ,
    subscription_id  VARCHAR(17),
    dispatch_pool_id VARCHAR(17),
    client_id        VARCHAR(17),
    mode             VARCHAR(30)  NOT NULL,
    queue            VARCHAR(255),
    version          TIMESTAMPTZ  NOT NULL,
    claimed_at       TIMESTAMPTZ,
    enqueued_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_dispatch_queue_order
    ON msg_dispatch_queue (message_group NULLS LAST, sequence, job_created_at, job_id);

-- Backfill: the jobs that are PENDING now. Re-running is a no-op.
INSERT INTO msg_dispatch_queue
    (job_id, job_created_at, message_group, sequence, scheduled_for,
     subscription_id, dispatch_pool_id, client_id, mode, queue, version)
SELECT id, created_at, message_group, sequence, scheduled_for,
       subscription_id, dispatch_pool_id, client_id, mode, queue, updated_at
  FROM msg_dispatch_jobs
 WHERE status = 'PENDING'
ON CONFLICT (job_id) DO NOTHING;

-- Rollback: DROP TABLE msg_dispatch_queue;
