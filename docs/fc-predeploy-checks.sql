-- FlowCatalyst Rust cutover: pre-deploy checks (read-only SELECTs, no secrets).
-- Run against the production (Go) database. Each query lists what would stop
-- working, or change, when the security fixes on feat/functions ship.

-- 1. Ruling 3 (tenant pin): users of these domains can no longer sign in until
--    the mapping gets a required OIDC tenant id.
SELECT m.email_domain, p.code
FROM tnt_email_domain_mappings m
JOIN oauth_identity_providers p ON p.id = m.identity_provider_id
WHERE p.oidc_multi_tenant
  AND coalesce(btrim(m.required_oidc_tenant_id), '') = '';

-- 2. Ruling 15 (roles hold only their own application's permissions): these
--    roles will be refused on their next write or SDK sync.
SELECT r.application_code, r.name, p.permission
FROM iam_roles r
JOIN iam_role_permissions p ON p.role_id = r.id
WHERE split_part(p.permission, ':', 1) <> r.application_code
ORDER BY 1, 2, 3;

-- 3. S15 (client_credentials needs a SERVICE principal): these OAuth clients
--    can no longer mint tokens.
SELECT c.client_id, pr.type
FROM oauth_clients c
JOIN iam_principals pr ON pr.id = c.service_account_principal_id
WHERE pr.type <> 'SERVICE';

-- 4. S1 (ingest needs Go's batch permissions): active service principals and
--    whether any of their roles grants events-write / dispatch-jobs-write
--    (wildcards approximated with LIKE). Rows with both false can no longer
--    ingest; that matches Go, so an app working on Go today should show true.
SELECT pr.id, pr.name, sa.application_id,
       bool_or('platform:messaging:batch:events-write'
               LIKE replace(rp.permission, '*', '%')) AS events_write,
       bool_or('platform:messaging:batch:dispatch-jobs-write'
               LIKE replace(rp.permission, '*', '%')) AS dispatch_jobs_write
FROM iam_principals pr
LEFT JOIN iam_service_accounts sa ON sa.id = pr.service_account_id
LEFT JOIN iam_principal_roles ur ON ur.principal_id = pr.id
LEFT JOIN iam_roles r ON r.name = ur.role_name
LEFT JOIN iam_role_permissions rp ON rp.role_id = r.id
WHERE pr.type = 'SERVICE' AND pr.active
GROUP BY pr.id, pr.name, sa.application_id
ORDER BY events_write NULLS FIRST, pr.name;

-- 5. Ruling 17a (an application's events come only from a caller that may
--    sign as it): service principals NOT tied to an application. If one of
--    these is what an app uses to send its own events, it will be refused
--    unless it is a super-admin or an anchor granted that application.
SELECT pr.id, pr.name, pr.scope, string_agg(ur.role_name, ', ') AS roles
FROM iam_principals pr
LEFT JOIN iam_service_accounts sa ON sa.id = pr.service_account_id
LEFT JOIN iam_principal_roles ur ON ur.principal_id = pr.id
WHERE pr.type = 'SERVICE' AND pr.active AND sa.application_id IS NULL
GROUP BY pr.id, pr.name, pr.scope
ORDER BY pr.name;
