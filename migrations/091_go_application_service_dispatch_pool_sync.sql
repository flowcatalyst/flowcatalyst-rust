-- Rust 091 mirrors Go 079_application_service_dispatch_pool_sync.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- F19 grant: platform:application-service gains platform:messaging:dispatch-pool:sync. A no-op where the role row does not exist yet (the seeder inserts roles at startup from the catalogue; catalogue parity is a separate batch).
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- The built-in platform:application-service role now carries
-- platform:messaging:dispatch-pool:sync so an application's deploy can sync its
-- own dispatch pools (docs/platform-bug-fixes.md F19). The seeder only inserts a
-- built-in role when it is missing (it never rewrites an existing role's
-- permissions), so existing deployments pick the grant up here, once. The sync
-- confines an application-scoped caller to its bound applications and to pools
-- that application owns (migration 078).
--
-- Tokens carry their permissions from mint time: an application service
-- account must mint a new token to hold the new permission.
INSERT INTO iam_role_permissions (role_id, permission)
SELECT r.id, 'platform:messaging:dispatch-pool:sync'
  FROM iam_roles r
 WHERE r.name = 'platform:application-service'
ON CONFLICT DO NOTHING;
