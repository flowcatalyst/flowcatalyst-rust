-- Rust 093 mirrors Go 081_function_version_requeue_revision.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- F22: fng_versions.requeued_at_revision, the pool revision a re-publish reset a FAILED version at.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Re-publishing a digest whose version is FAILED resets it to PUBLISHED so a
-- runner retries it (docs/function-runner-plan.md s.6.5). A runner that has
-- not yet seen the reset still reports its OLD FAILED outcome in heartbeats;
-- applying that stale report would immediately re-fail the version.
--
-- requeued_at_revision is the pool revision the reset published. A FAILED
-- report only counts when the runner has applied at least that revision
-- (heartbeat.revision >= requeued_at_revision), i.e. when it already holds
-- the post-reset desired state. 0 for every version that was never reset.
ALTER TABLE fng_versions ADD COLUMN IF NOT EXISTS requeued_at_revision BIGINT NOT NULL DEFAULT 0;
