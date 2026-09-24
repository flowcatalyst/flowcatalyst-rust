-- 033_oauth_client_secret_grace.sql
--
-- Mirrors flowcatalyst-go internal/migrate/sql/046_oauth_client_secret_grace.sql
-- and 047_oauth_previous_secret_last_used.sql (their Up sections, verbatim).
-- IF NOT EXISTS makes it a no-op on a database Go has already migrated.
--
-- Overlap window for OAuth client secret rotation. previous_secret_ref holds
-- the immediately-prior secret and previous_secret_expires_at bounds how long
-- it stays acceptable. Verification tries the current ref first, then the
-- previous one while it is unexpired. Both are cleared when the overlap is
-- ended early (revoke-previous-secret) and by the periodic purger once
-- lapsed. previous_secret_last_used_at is stamped whenever a client
-- authenticates with the superseded secret.
--
-- Additive and nullable: existing rows read as "no overlap in flight".
ALTER TABLE oauth_clients
    ADD COLUMN IF NOT EXISTS previous_secret_ref TEXT,
    ADD COLUMN IF NOT EXISTS previous_secret_expires_at TIMESTAMPTZ;

-- Lets the purger walk in-flight overlaps by expiry instead of scanning every
-- client. Partial: only rows actually carrying a previous secret qualify.
CREATE INDEX IF NOT EXISTS idx_oauth_clients_previous_secret_expires_at
    ON oauth_clients (previous_secret_expires_at)
    WHERE previous_secret_ref IS NOT NULL;

ALTER TABLE oauth_clients
    ADD COLUMN IF NOT EXISTS previous_secret_last_used_at TIMESTAMPTZ;
