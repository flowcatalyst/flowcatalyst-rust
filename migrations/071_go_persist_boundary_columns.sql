-- Rust 071 mirrors Go 035_persist_boundary_columns.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- Only oauth_idp_role_mappings.idp_type is new here; the other columns Go 035 adds are already created by Rust 032, 053 and 054, and ADD COLUMN IF NOT EXISTS skips them.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Persist-boundary columns for fields the entities and API DTOs already
-- carry but the schema silently dropped (found by the operation-test
-- campaign). All additive + nullable: the legacy-platform twin's explicit column
-- lists ignore them, and its rows simply read back NULL.

ALTER TABLE msg_event_types ADD COLUMN IF NOT EXISTS created_by VARCHAR(17);
ALTER TABLE msg_subscriptions ADD COLUMN IF NOT EXISTS created_by VARCHAR(17);

ALTER TABLE iam_service_accounts ADD COLUMN IF NOT EXISTS scope VARCHAR(20);
ALTER TABLE iam_service_accounts ADD COLUMN IF NOT EXISTS client_ids TEXT[];

ALTER TABLE oauth_idp_role_mappings ADD COLUMN IF NOT EXISTS idp_type VARCHAR(50);
