-- Rust 092 mirrors Go 080_application_service_admin_pages.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- F20 grants: platform:application-service gains six admin-page permissions. A no-op where the role row does not exist yet (see 091).
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- The built-in platform:application-service role now carries the permissions an
-- application's own admin pages need when they call the platform as the
-- application's service account (docs/platform-bug-fixes.md F20): read its own
-- events and dispatch jobs, read and invite its own users, and list, resend and
-- revoke its own pending invitations. The seeder only inserts a built-in role
-- when it is missing (it never rewrites an existing role's permissions), so
-- existing deployments pick the grants up here, once. Every endpoint confines
-- an application-scoped caller to the applications it is bound to, so the
-- grants do not widen reach.
--
-- Tokens carry their permissions from mint time: an application service
-- account must mint a new token to hold the new permissions.
INSERT INTO iam_role_permissions (role_id, permission)
SELECT r.id, p.permission
  FROM iam_roles r
 CROSS JOIN (VALUES
        ('platform:application-service:event:view'),
        ('platform:application-service:dispatch-job:view'),
        ('platform:application-service:user:view'),
        ('platform:application-service:user:create'),
        ('platform:application-service:invitation:view'),
        ('platform:application-service:invitation:manage')
      ) AS p(permission)
 WHERE r.name = 'platform:application-service'
ON CONFLICT DO NOTHING;
