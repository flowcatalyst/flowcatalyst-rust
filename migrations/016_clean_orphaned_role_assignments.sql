-- One-time cleanup: remove orphaned rows from iam_principal_roles whose
-- role_name no longer matches any iam_roles.name.
--
-- Pre-this-migration, several delete paths removed iam_roles rows without
-- cascading to iam_principal_roles (the junction has no DB-level FK — by
-- design, integrity is managed in code). Over time that produced orphan
-- assignments; users holding one of those orphans could no longer save
-- their role list because assign_roles.rs validates every submitted role
-- against iam_roles.
--
-- The code fix (role/repository.rs cascade + delete-guards on all role
-- sync paths) prevents new orphans. This migration clears the existing
-- backlog. Idempotent.

DELETE FROM iam_principal_roles pr
 WHERE NOT EXISTS (
   SELECT 1 FROM iam_roles r WHERE r.name = pr.role_name
 );
