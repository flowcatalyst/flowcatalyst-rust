-- Go's 057 (the parts 039 did not already add): a dispatch job's
-- descriptor, and the job's metadata on the read projection. Column names,
-- types, nullability and defaults are Go's; Go adds no index.
--
-- msg_dispatch_jobs.descriptor / msg_dispatch_jobs_read.descriptor: what
-- the job IS, in words. For a job the event fan-out raises, the raising
-- subscription's name ("Notify Value of user logins" reads on the
-- dispatch-jobs grid where "value:iam:user:logged-in" does not); a directly
-- created job (POST /api/dispatch-jobs(/batch)) may supply its own.
-- Nullable; absent is the legacy state.
--
-- msg_dispatch_jobs_read.metadata: the job's key/value tags
-- ([{key, value}]), projected from the write row so the grid shows them
-- (the payload stays out of the projection). The fan-out copies the raising
-- event's context_data onto the job's metadata, so a job shows the same
-- "additional data" its event does.
--
-- Go's 057 also adds msg_dispatch_job_attempts.request_info; Rust's 039
-- already did.
--
-- ADD COLUMN IF NOT EXISTS: a database Go already migrated keeps its
-- columns, and re-running is a no-op.
ALTER TABLE msg_dispatch_jobs
    ADD COLUMN IF NOT EXISTS descriptor VARCHAR(255);

ALTER TABLE msg_dispatch_jobs_read
    ADD COLUMN IF NOT EXISTS descriptor VARCHAR(255),
    ADD COLUMN IF NOT EXISTS metadata JSONB NOT NULL DEFAULT '[]'::jsonb;
