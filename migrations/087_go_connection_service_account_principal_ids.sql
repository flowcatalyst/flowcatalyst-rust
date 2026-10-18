-- Rust 087 mirrors Go 075_connection_service_account_principal_ids.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- Data fix F17: connections holding a prn_ principal id in service_account_id are re-pointed at the sac_ service account behind it.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- 075_connection_service_account_principal_ids.sql
--
-- SyncConnections used to store the application's service_account_id as the
-- connection's service_account_id. The application column holds the service
-- account's PRINCIPAL id (prn_..., iam_principals.id; migration 028), while a
-- connection's column is the service account's OWN id (sac_...,
-- iam_service_accounts.id) -- the id the delivery signer looks up. Every
-- connection a code-first sync created therefore named a service account that
-- does not exist, and its deliveries went out unsigned
-- (docs/platform-bug-fixes.md F17).
--
-- Re-point those rows at the service account behind the principal. A
-- principal id never equals a service account id, so rows already correct are
-- untouched; a principal with no service account behind it is left as it is
-- (the next sync refuses it loudly).
UPDATE msg_connections c
   SET service_account_id = p.service_account_id,
       updated_at = NOW()
  FROM iam_principals p
 WHERE c.service_account_id = p.id
   AND p.service_account_id IS NOT NULL;
