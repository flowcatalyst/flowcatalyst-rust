-- Each application delivers its own outbox with its own service-account
-- credentials, so the built-in platform:application-service role now carries
-- the three batch-ingest permissions. The seeder only inserts a built-in role
-- when it is missing (it never rewrites an existing role's permissions), so
-- existing deployments pick the grants up here, once. The batch endpoints
-- confine an application-scoped caller to its own applications' items.
--
-- ON CONFLICT DO NOTHING: re-running is a no-op.
INSERT INTO iam_role_permissions (role_id, permission)
SELECT r.id, p.permission
  FROM iam_roles r
 CROSS JOIN (VALUES
        ('platform:messaging:batch:events-write'),
        ('platform:messaging:batch:dispatch-jobs-write'),
        ('platform:admin:batch:audit-logs-write')
      ) AS p(permission)
 WHERE r.name = 'platform:application-service'
ON CONFLICT DO NOTHING;
