-- FlowCatalyst Audit Tables
-- Matches TypeScript Drizzle schema exactly

-- =============================================================================
-- aud_logs - Audit logs
-- =============================================================================
CREATE TABLE IF NOT EXISTS aud_logs (
    id VARCHAR(17) PRIMARY KEY,
    entity_type VARCHAR(100) NOT NULL,
    entity_id VARCHAR(17) NOT NULL,
    operation VARCHAR(100) NOT NULL,
    operation_json JSONB,
    principal_id VARCHAR(100),
    performed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_aud_logs_entity ON aud_logs (entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_aud_logs_performed ON aud_logs (performed_at);
CREATE INDEX IF NOT EXISTS idx_aud_logs_principal ON aud_logs (principal_id);
CREATE INDEX IF NOT EXISTS idx_aud_logs_operation ON aud_logs (operation);
