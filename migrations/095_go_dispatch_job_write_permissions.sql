-- Rust 095 mirrors Go 085_dispatch_job_write_permissions.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- Data: the write actions on a dispatch job (requeue, cancel, complete, edit-payload) are granted to every role that holds dispatch-job:view, except platform:viewer. A no-op where no role holds view.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- The single platform:messaging:dispatch-job:view permission used to gate both
-- reading dispatch jobs and every write on them (requeue, cancel, complete,
-- edit payload). Each write action now has its own permission; view keeps the
-- reads. So that nobody loses an ability on upgrade, every role that holds
-- view is granted the four ordinary write permissions here, once:
--
--   platform:messaging:dispatch-job:requeue
--   platform:messaging:dispatch-job:cancel
--   platform:messaging:dispatch-job:complete
--   platform:messaging:dispatch-job:edit-payload
--
-- Not granted to anyone: platform:messaging:dispatch-job:redrive-completed,
-- which re-delivers jobs that already succeeded (a requeue never touches a
-- COMPLETED job without it). Only platform:super-admin (the platform:*:*:*
-- wildcard) holds it until a role is given it deliberately.
--
-- Not granted to platform:viewer, the built-in read-only role: it holds view
-- only to read, and writing was never its intent. Everything else holding view
-- (platform:messaging-admin, custom roles, application roles) is granted.
-- The seeder never rewrites an existing role, so the built-in
-- platform:messaging-admin gets its grants here too.
--
-- Tokens carry their permissions from mint time: a user must sign in again
-- (or a service account mint a new token) to hold the new permissions.
INSERT INTO iam_role_permissions (role_id, permission)
SELECT rp.role_id, p.permission
  FROM iam_role_permissions rp
  JOIN iam_roles r ON r.id = rp.role_id
 CROSS JOIN (VALUES
        ('platform:messaging:dispatch-job:requeue'),
        ('platform:messaging:dispatch-job:cancel'),
        ('platform:messaging:dispatch-job:complete'),
        ('platform:messaging:dispatch-job:edit-payload')
      ) AS p(permission)
 WHERE rp.permission = 'platform:messaging:dispatch-job:view'
   AND r.name <> 'platform:viewer'
ON CONFLICT DO NOTHING;
