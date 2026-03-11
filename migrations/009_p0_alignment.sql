-- P0 Alignment: Schema fixes to match TypeScript reference
--
-- 1. Add connection_id to msg_subscriptions (replaces target)
-- 2. Add connection_id to msg_dispatch_jobs (when migrated to PG)
-- 3. Add application_id and client_id to aud_logs

-- =============================================================================
-- msg_subscriptions: add connection_id column
-- =============================================================================
ALTER TABLE msg_subscriptions ADD COLUMN IF NOT EXISTS connection_id VARCHAR(17);
CREATE INDEX IF NOT EXISTS idx_msg_subscriptions_connection_id ON msg_subscriptions (connection_id);

-- =============================================================================
-- aud_logs: add application_id and client_id columns
-- =============================================================================
ALTER TABLE aud_logs ADD COLUMN IF NOT EXISTS application_id VARCHAR(17);
ALTER TABLE aud_logs ADD COLUMN IF NOT EXISTS client_id VARCHAR(17);
CREATE INDEX IF NOT EXISTS idx_aud_logs_application_id ON aud_logs (application_id);
CREATE INDEX IF NOT EXISTS idx_aud_logs_client_id ON aud_logs (client_id);
