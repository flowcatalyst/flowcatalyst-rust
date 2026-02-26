-- FlowCatalyst Tenant Tables
-- Matches TypeScript Drizzle schema exactly

-- =============================================================================
-- tnt_clients - Tenant/Client organizations
-- =============================================================================
CREATE TABLE IF NOT EXISTS tnt_clients (
    id VARCHAR(17) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    identifier VARCHAR(100) NOT NULL UNIQUE,
    status VARCHAR(50) NOT NULL DEFAULT 'ACTIVE',
    status_reason VARCHAR(255),
    status_changed_at TIMESTAMPTZ,
    notes JSONB DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tnt_clients_identifier ON tnt_clients (identifier);
CREATE INDEX IF NOT EXISTS idx_tnt_clients_status ON tnt_clients (status);

-- =============================================================================
-- tnt_anchor_domains - Anchor (admin) email domains
-- =============================================================================
CREATE TABLE IF NOT EXISTS tnt_anchor_domains (
    id VARCHAR(17) PRIMARY KEY,
    domain VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS tnt_anchor_domains_domain_idx ON tnt_anchor_domains (domain);

-- =============================================================================
-- tnt_cors_allowed_origins - CORS allowed origins
-- =============================================================================
CREATE TABLE IF NOT EXISTS tnt_cors_allowed_origins (
    id VARCHAR(17) PRIMARY KEY,
    origin VARCHAR(500) NOT NULL UNIQUE,
    description TEXT,
    created_by VARCHAR(17),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS tnt_cors_allowed_origins_origin_idx ON tnt_cors_allowed_origins (origin);

-- =============================================================================
-- tnt_client_auth_configs - Client authentication configuration
-- =============================================================================
CREATE TABLE IF NOT EXISTS tnt_client_auth_configs (
    id VARCHAR(17) PRIMARY KEY,
    email_domain VARCHAR(255) NOT NULL UNIQUE,
    config_type VARCHAR(50) NOT NULL,
    primary_client_id VARCHAR(17),
    additional_client_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    granted_client_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    auth_provider VARCHAR(50) NOT NULL,
    oidc_issuer_url VARCHAR(500),
    oidc_client_id VARCHAR(255),
    oidc_multi_tenant BOOLEAN NOT NULL DEFAULT FALSE,
    oidc_issuer_pattern VARCHAR(500),
    oidc_client_secret_ref VARCHAR(1000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS tnt_client_auth_configs_email_domain_idx ON tnt_client_auth_configs (email_domain);
CREATE INDEX IF NOT EXISTS tnt_client_auth_configs_config_type_idx ON tnt_client_auth_configs (config_type);
CREATE INDEX IF NOT EXISTS tnt_client_auth_configs_primary_client_id_idx ON tnt_client_auth_configs (primary_client_id);

-- =============================================================================
-- tnt_email_domain_mappings - Email domain to identity provider mappings
-- =============================================================================
CREATE TABLE IF NOT EXISTS tnt_email_domain_mappings (
    id VARCHAR(17) PRIMARY KEY,
    email_domain VARCHAR(255) NOT NULL,
    identity_provider_id VARCHAR(17) NOT NULL,
    scope_type VARCHAR(20) NOT NULL,
    primary_client_id VARCHAR(17),
    required_oidc_tenant_id VARCHAR(100),
    sync_roles_from_idp BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_tnt_email_domain_mappings_domain ON tnt_email_domain_mappings (email_domain);
CREATE INDEX IF NOT EXISTS idx_tnt_email_domain_mappings_idp ON tnt_email_domain_mappings (identity_provider_id);
CREATE INDEX IF NOT EXISTS idx_tnt_email_domain_mappings_scope ON tnt_email_domain_mappings (scope_type);

-- =============================================================================
-- tnt_email_domain_mapping_additional_clients - Junction table
-- =============================================================================
CREATE TABLE IF NOT EXISTS tnt_email_domain_mapping_additional_clients (
    id SERIAL PRIMARY KEY,
    email_domain_mapping_id VARCHAR(17) NOT NULL,
    client_id VARCHAR(17) NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tnt_edm_additional_clients_mapping ON tnt_email_domain_mapping_additional_clients (email_domain_mapping_id);

-- =============================================================================
-- tnt_email_domain_mapping_granted_clients - Junction table
-- =============================================================================
CREATE TABLE IF NOT EXISTS tnt_email_domain_mapping_granted_clients (
    id SERIAL PRIMARY KEY,
    email_domain_mapping_id VARCHAR(17) NOT NULL,
    client_id VARCHAR(17) NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tnt_edm_granted_clients_mapping ON tnt_email_domain_mapping_granted_clients (email_domain_mapping_id);

-- =============================================================================
-- tnt_email_domain_mapping_allowed_roles - Junction table
-- =============================================================================
CREATE TABLE IF NOT EXISTS tnt_email_domain_mapping_allowed_roles (
    id SERIAL PRIMARY KEY,
    email_domain_mapping_id VARCHAR(17) NOT NULL,
    role_id VARCHAR(17) NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tnt_edm_allowed_roles_mapping ON tnt_email_domain_mapping_allowed_roles (email_domain_mapping_id);
