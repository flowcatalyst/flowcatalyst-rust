-- Go's 056_connection_application_scope (flowcatalyst-go
-- internal/migrate/sql/056_connection_application_scope.sql, Up section,
-- verbatim): connections become application-scoped for the SDK
-- connections sync (POST /api/applications/{appCode}/connections/sync).
--
-- - msg_connections.application_code: the owning application (NULL = a
--   shared connection); no FK, as in Go.
-- - msg_connections.source: CODE | API | UI (existing rows are UI; a sync
--   touches only API/CODE rows).
-- - Uniqueness moves from (code, client_id) to (application_code,
--   client_id, code), NULLs folded to '' so they compare, on connections
--   and subscriptions alike. A database with rows that would collide
--   refuses the migration with the offending keys, as Go's does.
--
-- Idempotent: a database Go has migrated already has all of it.
ALTER TABLE msg_connections ADD COLUMN IF NOT EXISTS application_code VARCHAR(100);

ALTER TABLE msg_connections ADD COLUMN IF NOT EXISTS source VARCHAR(20) NOT NULL DEFAULT 'UI';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_msg_connections_source'
    ) THEN
        ALTER TABLE msg_connections
            ADD CONSTRAINT chk_msg_connections_source
            CHECK (source IN ('CODE', 'API', 'UI'));
    END IF;
END $$;

DO $$
DECLARE
    dupes TEXT;
BEGIN
    SELECT string_agg(
               format('(application_code=%L, client_id=%L, code=%L, count=%s)',
                      NULLIF(application_code, ''), NULLIF(client_id, ''), code, cnt),
               ', ')
      INTO dupes
      FROM (
          SELECT COALESCE(application_code, '') AS application_code,
                 COALESCE(client_id, '') AS client_id,
                 code,
                 COUNT(*) AS cnt
            FROM msg_connections
           GROUP BY 1, 2, 3
          HAVING COUNT(*) > 1
      ) d;

    IF dupes IS NOT NULL THEN
        RAISE EXCEPTION 'msg_connections has rows that collide under the new (application_code, client_id, code) uniqueness key — resolve these before re-running this migration: %', dupes;
    END IF;
END $$;

DO $$
DECLARE
    dupes TEXT;
BEGIN
    SELECT string_agg(
               format('(application_code=%L, client_id=%L, code=%L, count=%s)',
                      NULLIF(application_code, ''), NULLIF(client_id, ''), code, cnt),
               ', ')
      INTO dupes
      FROM (
          SELECT COALESCE(application_code, '') AS application_code,
                 COALESCE(client_id, '') AS client_id,
                 code,
                 COUNT(*) AS cnt
            FROM msg_subscriptions
           GROUP BY 1, 2, 3
          HAVING COUNT(*) > 1
      ) d;

    IF dupes IS NOT NULL THEN
        RAISE EXCEPTION 'msg_subscriptions has rows that collide under the new (application_code, client_id, code) uniqueness key — resolve these before re-running this migration: %', dupes;
    END IF;
END $$;

DROP INDEX IF EXISTS idx_msg_connections_code_client;
DROP INDEX IF EXISTS idx_msg_subscriptions_code_client;

CREATE UNIQUE INDEX IF NOT EXISTS uq_msg_connections_app_client_code
    ON msg_connections (COALESCE(application_code, ''), COALESCE(client_id, ''), code);
CREATE UNIQUE INDEX IF NOT EXISTS uq_msg_subscriptions_app_client_code
    ON msg_subscriptions (COALESCE(application_code, ''), COALESCE(client_id, ''), code);

