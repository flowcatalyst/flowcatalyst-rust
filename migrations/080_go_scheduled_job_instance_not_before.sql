-- Rust 080 mirrors Go 060_scheduled_job_instance_not_before.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- msg_scheduled_job_instances.not_before: a 429-deferred instance is not asked again before it.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- A scheduled-job instance can now be deferred: a target that answers 429
-- with Retry-After is asked again no sooner than not_before, without the
-- attempt counting against delivery_max_attempts (dispatch jobs already
-- treat 429 this way). NULL means deliverable now, which is every existing
-- row. The dispatcher's queued-instance query filters on it.
ALTER TABLE msg_scheduled_job_instances ADD COLUMN IF NOT EXISTS not_before TIMESTAMPTZ;
