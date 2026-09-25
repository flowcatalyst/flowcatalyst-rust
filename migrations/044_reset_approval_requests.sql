-- Lost-device password-reset requests awaiting a client administrator
-- (Go's 032): a self-service reset for a user with no strong factor is filed
-- here instead of emailing a link when the stricter reset policy is on (off
-- by default, as Go). A database Go migrated already has the table.
CREATE TABLE IF NOT EXISTS iam_reset_approval_requests (
    id           VARCHAR(17) PRIMARY KEY,
    principal_id VARCHAR(17) NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
    client_id    VARCHAR(17),
    status       VARCHAR(20) NOT NULL DEFAULT 'PENDING', -- PENDING|APPROVED|DENIED|EXPIRED
    reset_2fa    BOOLEAN NOT NULL DEFAULT TRUE,
    note         VARCHAR(255),
    decided_by   VARCHAR(17),
    decided_at   TIMESTAMPTZ,
    expires_at   TIMESTAMPTZ NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_iam_reset_approval_client_status
    ON iam_reset_approval_requests (client_id, status);
CREATE INDEX IF NOT EXISTS idx_iam_reset_approval_principal
    ON iam_reset_approval_requests (principal_id);
