-- FlowCatalyst OAuth Tables
-- Matches TypeScript Drizzle schema exactly

-- =============================================================================
-- oauth_identity_providers - Identity providers
-- =============================================================================
CREATE TABLE IF NOT EXISTS oauth_identity_providers (
    id VARCHAR(17) PRIMARY KEY,
    code VARCHAR(50) NOT NULL,
    name VARCHAR(200) NOT NULL,
    type VARCHAR(20) NOT NULL,
    oidc_issuer_url VARCHAR(500),
    oidc_client_id VARCHAR(200),
    oidc_client_secret_ref VARCHAR(500),
    oidc_multi_tenant BOOLEAN NOT NULL DEFAULT FALSE,
    oidc_issuer_pattern VARCHAR(500),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_oauth_identity_providers_code ON oauth_identity_providers (code);

-- =============================================================================
-- oauth_identity_provider_allowed_domains - IDP allowed domains (junction)
-- =============================================================================
CREATE TABLE IF NOT EXISTS oauth_identity_provider_allowed_domains (
    id SERIAL PRIMARY KEY,
    identity_provider_id VARCHAR(17) NOT NULL,
    email_domain VARCHAR(255) NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_oauth_idp_allowed_domains_idp ON oauth_identity_provider_allowed_domains (identity_provider_id);

-- =============================================================================
-- oauth_idp_role_mappings - IDP role mappings
-- =============================================================================
CREATE TABLE IF NOT EXISTS oauth_idp_role_mappings (
    id VARCHAR(17) PRIMARY KEY,
    idp_role_name VARCHAR(200) NOT NULL,
    internal_role_name VARCHAR(200) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_oauth_idp_role_mappings_idp_role_name ON oauth_idp_role_mappings (idp_role_name);

-- =============================================================================
-- oauth_clients - OAuth 2.0 client applications
-- =============================================================================
CREATE TABLE IF NOT EXISTS oauth_clients (
    id VARCHAR(17) PRIMARY KEY,
    client_id VARCHAR(100) NOT NULL UNIQUE,
    client_name VARCHAR(255) NOT NULL,
    client_type VARCHAR(20) NOT NULL DEFAULT 'PUBLIC',
    client_secret_ref VARCHAR(500),
    default_scopes VARCHAR(500),
    pkce_required BOOLEAN NOT NULL DEFAULT TRUE,
    service_account_principal_id VARCHAR(17),
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS oauth_clients_client_id_idx ON oauth_clients (client_id);
CREATE INDEX IF NOT EXISTS oauth_clients_active_idx ON oauth_clients (active);

-- =============================================================================
-- oauth_client_redirect_uris - OAuth redirect URIs (junction)
-- =============================================================================
CREATE TABLE IF NOT EXISTS oauth_client_redirect_uris (
    oauth_client_id VARCHAR(17) NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    redirect_uri VARCHAR(500) NOT NULL,
    PRIMARY KEY (oauth_client_id, redirect_uri)
);

CREATE INDEX IF NOT EXISTS idx_oauth_client_redirect_uris_client ON oauth_client_redirect_uris (oauth_client_id);

-- =============================================================================
-- oauth_client_allowed_origins - OAuth allowed origins (junction)
-- =============================================================================
CREATE TABLE IF NOT EXISTS oauth_client_allowed_origins (
    oauth_client_id VARCHAR(17) NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    allowed_origin VARCHAR(200) NOT NULL,
    PRIMARY KEY (oauth_client_id, allowed_origin)
);

CREATE INDEX IF NOT EXISTS idx_oauth_client_allowed_origins_client ON oauth_client_allowed_origins (oauth_client_id);
CREATE INDEX IF NOT EXISTS idx_oauth_client_allowed_origins_origin ON oauth_client_allowed_origins (allowed_origin);

-- =============================================================================
-- oauth_client_grant_types - OAuth grant types (junction)
-- =============================================================================
CREATE TABLE IF NOT EXISTS oauth_client_grant_types (
    oauth_client_id VARCHAR(17) NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    grant_type VARCHAR(50) NOT NULL,
    PRIMARY KEY (oauth_client_id, grant_type)
);

CREATE INDEX IF NOT EXISTS idx_oauth_client_grant_types_client ON oauth_client_grant_types (oauth_client_id);

-- =============================================================================
-- oauth_client_application_ids - OAuth application IDs (junction)
-- =============================================================================
CREATE TABLE IF NOT EXISTS oauth_client_application_ids (
    oauth_client_id VARCHAR(17) NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    application_id VARCHAR(17) NOT NULL,
    PRIMARY KEY (oauth_client_id, application_id)
);

CREATE INDEX IF NOT EXISTS idx_oauth_client_application_ids_client ON oauth_client_application_ids (oauth_client_id);

-- =============================================================================
-- oauth_oidc_login_states - OIDC login flow state
-- =============================================================================
CREATE TABLE IF NOT EXISTS oauth_oidc_login_states (
    state VARCHAR(200) PRIMARY KEY,
    email_domain VARCHAR(255) NOT NULL,
    identity_provider_id VARCHAR(17) NOT NULL,
    email_domain_mapping_id VARCHAR(17) NOT NULL,
    nonce VARCHAR(200) NOT NULL,
    code_verifier VARCHAR(200) NOT NULL,
    return_url VARCHAR(2000),
    oauth_client_id VARCHAR(200),
    oauth_redirect_uri VARCHAR(2000),
    oauth_scope VARCHAR(500),
    oauth_state VARCHAR(500),
    oauth_code_challenge VARCHAR(500),
    oauth_code_challenge_method VARCHAR(20),
    oauth_nonce VARCHAR(500),
    interaction_uid VARCHAR(200),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_oauth_oidc_login_states_expires ON oauth_oidc_login_states (expires_at);

-- =============================================================================
-- oauth_oidc_payloads - OIDC provider artifacts storage
-- =============================================================================
CREATE TABLE IF NOT EXISTS oauth_oidc_payloads (
    id VARCHAR(128) PRIMARY KEY,
    type VARCHAR(64) NOT NULL,
    payload JSONB NOT NULL,
    grant_id VARCHAR(128),
    user_code VARCHAR(128),
    uid VARCHAR(128),
    expires_at TIMESTAMPTZ,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS oauth_oidc_payloads_grant_id_idx ON oauth_oidc_payloads (grant_id);
CREATE INDEX IF NOT EXISTS oauth_oidc_payloads_user_code_idx ON oauth_oidc_payloads (user_code);
CREATE INDEX IF NOT EXISTS oauth_oidc_payloads_uid_idx ON oauth_oidc_payloads (uid);
CREATE INDEX IF NOT EXISTS oauth_oidc_payloads_type_idx ON oauth_oidc_payloads (type);
CREATE INDEX IF NOT EXISTS oauth_oidc_payloads_expires_at_idx ON oauth_oidc_payloads (expires_at);
