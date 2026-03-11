-- FlowCatalyst Auth State Tables
-- NOTE: These tables are superseded by 007_oauth_tables.sql which creates
-- oauth_oidc_login_states and oauth_oidc_payloads (matching the TypeScript reference).
-- The iam_* tables below were from an earlier Rust-only migration and are NOT used.
-- They are kept here only to avoid migration ordering issues on existing databases.
-- The Rust code uses oauth_oidc_login_states (from 007) for OIDC login state.
-- Refresh tokens and authorization codes are stored in oauth_oidc_payloads (from 007).

-- SUPERSEDED: Rust code now uses oauth_oidc_login_states from 007_oauth_tables.sql
CREATE TABLE IF NOT EXISTS iam_oidc_login_states (
    state VARCHAR(255) PRIMARY KEY,
    email_domain VARCHAR(255) NOT NULL,
    auth_config_id VARCHAR(17) NOT NULL,
    nonce TEXT NOT NULL,
    code_verifier TEXT NOT NULL,
    return_url TEXT,
    oauth_client_id VARCHAR(255),
    oauth_redirect_uri TEXT,
    oauth_scope TEXT,
    oauth_state TEXT,
    oauth_code_challenge TEXT,
    oauth_code_challenge_method VARCHAR(10),
    oauth_nonce TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_iam_oidc_login_states_expires ON iam_oidc_login_states (expires_at);

-- SUPERSEDED: Rust code now uses oauth_oidc_payloads from 007_oauth_tables.sql
CREATE TABLE IF NOT EXISTS iam_refresh_tokens (
    id VARCHAR(17) PRIMARY KEY,
    token_hash VARCHAR(255) NOT NULL,
    principal_id VARCHAR(17) NOT NULL,
    oauth_client_id VARCHAR(255),
    scopes TEXT,
    accessible_clients TEXT,
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    revoked_at TIMESTAMPTZ,
    token_family VARCHAR(255),
    replaced_by VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    last_used_at TIMESTAMPTZ,
    created_from_ip VARCHAR(45),
    user_agent TEXT
);

CREATE INDEX IF NOT EXISTS idx_iam_refresh_tokens_hash ON iam_refresh_tokens (token_hash);
CREATE INDEX IF NOT EXISTS idx_iam_refresh_tokens_principal ON iam_refresh_tokens (principal_id);
CREATE INDEX IF NOT EXISTS idx_iam_refresh_tokens_family ON iam_refresh_tokens (token_family);
CREATE INDEX IF NOT EXISTS idx_iam_refresh_tokens_expires ON iam_refresh_tokens (expires_at);
CREATE INDEX IF NOT EXISTS idx_iam_refresh_tokens_revoked ON iam_refresh_tokens (revoked);

-- SUPERSEDED: Rust code now uses oauth_oidc_payloads from 007_oauth_tables.sql
CREATE TABLE IF NOT EXISTS iam_authorization_codes (
    code VARCHAR(255) PRIMARY KEY,
    client_id VARCHAR(255) NOT NULL,
    principal_id VARCHAR(17) NOT NULL,
    redirect_uri TEXT NOT NULL,
    scope TEXT,
    code_challenge TEXT,
    code_challenge_method VARCHAR(10),
    nonce TEXT,
    state TEXT,
    context_client_id VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    used BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS idx_iam_auth_codes_client ON iam_authorization_codes (client_id);
CREATE INDEX IF NOT EXISTS idx_iam_auth_codes_principal ON iam_authorization_codes (principal_id);
CREATE INDEX IF NOT EXISTS idx_iam_auth_codes_expires ON iam_authorization_codes (expires_at);
