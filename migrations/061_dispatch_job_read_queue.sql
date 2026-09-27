-- A dispatch job's own priority claim on the read projection:
-- msg_dispatch_jobs_read.queue, copied from msg_dispatch_jobs.queue (Go's
-- 054, this platform's 039) by the projector, the same type.
--
-- Go documents `priority` on the list row (`DispatchJobRead`) but never
-- fills it, and its projection has no column to fill it from. This platform
-- answers it from this column: 1 for a job that claims HIGH_PRIORITY, 0 for
-- one that claims DEFAULT, absent for a job with no claim of its own (it
-- dispatches at its subscription's priority) and for rows projected before
-- this column, which are left NULL rather than backfilled across every
-- partition.
--
-- ADD COLUMN IF NOT EXISTS: re-running is a no-op.
ALTER TABLE msg_dispatch_jobs_read
    ADD COLUMN IF NOT EXISTS queue VARCHAR(255);
