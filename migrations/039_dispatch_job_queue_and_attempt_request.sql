-- Two columns Go already has on a shared database, needed by the dispatch
-- pipeline's port of Go's model (owner decision 28).
--
-- msg_dispatch_jobs.queue (Go's 054): a dispatch job's own priority claim,
-- DEFAULT | HIGH_PRIORITY | legacy text, nullable (absent is the legacy
-- state). The scheduler publishes a job to its tenant's queue for this
-- priority when it names a recognised value, else its subscription's, else
-- DEFAULT. The event fan-out copies the raising subscription's value.
--
-- msg_dispatch_job_attempts.request_info (Go's 057): what the platform SENT
-- on an attempt — which service account signed it, whether a signature and
-- a bearer were attached, the signing timestamp, the header names — or why
-- it went out unsigned. Never a secret.
--
-- ADD COLUMN IF NOT EXISTS: a database Go already migrated keeps its
-- columns, and re-running is a no-op.
ALTER TABLE msg_dispatch_jobs
    ADD COLUMN IF NOT EXISTS queue VARCHAR(255);

ALTER TABLE msg_dispatch_job_attempts
    ADD COLUMN IF NOT EXISTS request_info JSONB;
