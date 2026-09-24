-- 032_service_account_scope_and_client_ids.sql
--
-- Mirrors the iam_service_accounts part of flowcatalyst-go
-- internal/migrate/sql/035_persist_boundary_columns.sql. IF NOT EXISTS makes
-- it a no-op on a database Go has already migrated.
--
-- scope holds the scope requested for the service account, as sent; the
-- token tier lives on the linked principal and follows client_ids (none →
-- ANCHOR, one → CLIENT, several → PARTNER). client_ids holds the account's
-- client links. Both nullable: rows written before the columns existed read
-- back NULL.
ALTER TABLE iam_service_accounts ADD COLUMN IF NOT EXISTS scope VARCHAR(20);
ALTER TABLE iam_service_accounts ADD COLUMN IF NOT EXISTS client_ids TEXT[];
