-- Rust 089 mirrors Go 077_backfill_role_application_id.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- Data fix F18: iam_roles.application_id is set from app_applications.code where it is NULL.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- 077_backfill_role_application_id.sql
--
-- iam_roles.application_id was left NULL by the admin role create and by the
-- platform role seed/sync, although application_code names an application
-- that exists (docs/platform-bug-fixes.md F18). A role with no application id
-- is invisible to the application-scoped lookups (role sync finds an
-- application's roles by id) and is dropped from tokens narrowed to an
-- application.
--
-- Set it from app_applications.code wherever it is NULL and application_code
-- names an application. app_applications.code is unique, so the match is
-- unambiguous by construction. Roles whose application_code matches no
-- application, or is empty, are left alone (nothing is guessed from the role
-- name); list them with
--   SELECT id, name, application_code FROM iam_roles WHERE application_id IS NULL;
UPDATE iam_roles r
   SET application_id = a.id
  FROM app_applications a
 WHERE r.application_id IS NULL
   AND r.application_code IS NOT NULL
   AND r.application_code <> ''
   AND a.code = r.application_code;
