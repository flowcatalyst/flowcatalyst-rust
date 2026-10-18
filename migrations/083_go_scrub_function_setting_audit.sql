-- Rust 083 mirrors Go 063_scrub_function_setting_audit.sql (Go internal/migrate/sql/), verbatim below this header
-- apart from the goose directives, the Down section and ASCII-folded comments.
-- Mapping table: docs/go-mirror/migration-mapping.md.
-- Data fix: masks SECRET and DB function-setting values Go wrote to aud_logs. A no-op where there is no PutSettingCommand audit row.
-- Idempotent, so a database Go already migrated and a database Rust created both
-- end in the same schema; the runner's probe for this id (fc-migrations lib.rs)
-- records it as applied, without running it, where its effect is already visible.
-- Do not edit after shipping: the tracker stores the checksum.

-- Scrub SECRET and DB function-setting values from aud_logs.
--
-- PutSettingCommand did not mask `value`, so every SECRET and DB setting
-- written through PUT .../secrets/{key} or .../db/{name} was stored in plaintext in
-- aud_logs.operation_json. The command now masks it (AuditMaskedFields); this
-- migration removes what was already written. CONFIG rows are left alone.
--
-- Scrubbing the audit log does not un-leak a value: every secret and DSN ever
-- written through these endpoints must still be rotated.
--
-- Read-only impact check, to run before deploying:
--   SELECT COUNT(*) FROM aud_logs
--    WHERE operation = 'PutSettingCommand'
--      AND operation_json ? 'functionId'
--      AND operation_json->>'kind' IN ('SECRET', 'DB')
--      AND operation_json->>'value' IS DISTINCT FROM '***';
UPDATE aud_logs
   SET operation_json = jsonb_set(operation_json, '{value}', '"***"'::jsonb)
 WHERE operation = 'PutSettingCommand'
   AND operation_json ? 'functionId'
   AND operation_json->>'kind' IN ('SECRET', 'DB')
   AND operation_json ? 'value'
   AND operation_json->>'value' IS DISTINCT FROM '***';
