-- 031_principal_all_applications.sql
--
-- Mirrors flowcatalyst-go internal/migrate/sql/034_principal_all_applications.sql.
-- IF NOT EXISTS makes it a no-op on a database Go has already migrated.
--
-- Application access is its own axis, orthogonal to the client tier
-- (anchor/partner/client). A principal can reach an application iff
-- all_applications is true OR the app id is in iam_principal_application_access.
-- all_applications is the application-axis analogue of the anchor tier ("all,
-- including future apps") — stored, not derived from tier, so an anchor-tier
-- service account can still be pinned to a single application
-- (all_applications=false + one access row). Defaults true so every existing
-- and normally-created principal stays unrestricted (today's behaviour); only
-- the app-service-account provision opts into restriction.
ALTER TABLE iam_principals
    ADD COLUMN IF NOT EXISTS all_applications boolean NOT NULL DEFAULT true;
