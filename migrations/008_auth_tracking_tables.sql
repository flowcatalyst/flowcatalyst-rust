-- FlowCatalyst Auth Tracking Tables
-- Login attempts and password reset tokens

-- =============================================================================
-- iam_login_attempts - Login attempt tracking
-- =============================================================================
CREATE TABLE IF NOT EXISTS iam_login_attempts (
    id VARCHAR(17) PRIMARY KEY,
    attempt_type VARCHAR(20) NOT NULL,
    outcome VARCHAR(20) NOT NULL,
    failure_reason VARCHAR(100),
    identifier VARCHAR(255),
    principal_id VARCHAR(17),
    ip_address VARCHAR(45),
    user_agent TEXT,
    attempted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_iam_login_attempts_type ON iam_login_attempts (attempt_type);
CREATE INDEX IF NOT EXISTS idx_iam_login_attempts_outcome ON iam_login_attempts (outcome);
CREATE INDEX IF NOT EXISTS idx_iam_login_attempts_identifier ON iam_login_attempts (identifier);
CREATE INDEX IF NOT EXISTS idx_iam_login_attempts_principal ON iam_login_attempts (principal_id);
CREATE INDEX IF NOT EXISTS idx_iam_login_attempts_at ON iam_login_attempts (attempted_at);

-- =============================================================================
-- iam_password_reset_tokens - Password reset token storage
-- =============================================================================
CREATE TABLE IF NOT EXISTS iam_password_reset_tokens (
    id VARCHAR(17) PRIMARY KEY,
    principal_id VARCHAR(17) NOT NULL,
    token_hash VARCHAR(64) NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_iam_password_reset_token_hash ON iam_password_reset_tokens (token_hash);
CREATE INDEX IF NOT EXISTS idx_iam_password_reset_principal ON iam_password_reset_tokens (principal_id);
