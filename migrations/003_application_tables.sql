-- FlowCatalyst Application Tables
-- Matches TypeScript Drizzle schema exactly

-- =============================================================================
-- app_applications - Applications
-- =============================================================================
CREATE TABLE IF NOT EXISTS app_applications (
    id VARCHAR(17) PRIMARY KEY,
    type VARCHAR(50) NOT NULL DEFAULT 'APPLICATION',
    code VARCHAR(50) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    icon_url VARCHAR(500),
    website VARCHAR(500),
    logo TEXT,
    logo_mime_type VARCHAR(100),
    default_base_url VARCHAR(500),
    service_account_id VARCHAR(17),
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_app_applications_code ON app_applications (code);
CREATE INDEX IF NOT EXISTS idx_app_applications_type ON app_applications (type);
CREATE INDEX IF NOT EXISTS idx_app_applications_active ON app_applications (active);

-- =============================================================================
-- app_client_configs - Application client configurations
-- =============================================================================
CREATE TABLE IF NOT EXISTS app_client_configs (
    id VARCHAR(17) PRIMARY KEY,
    application_id VARCHAR(17) NOT NULL,
    client_id VARCHAR(17) NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_app_client_configs_app ON app_client_configs (application_id);
CREATE INDEX IF NOT EXISTS idx_app_client_configs_clt ON app_client_configs (client_id);
CREATE UNIQUE INDEX IF NOT EXISTS uq_app_client_configs_app_clt ON app_client_configs (application_id, client_id);

-- =============================================================================
-- app_platform_configs - Platform configurations
-- =============================================================================
CREATE TABLE IF NOT EXISTS app_platform_configs (
    id VARCHAR(17) PRIMARY KEY,
    application_code VARCHAR(100) NOT NULL,
    section VARCHAR(100) NOT NULL,
    property VARCHAR(100) NOT NULL,
    scope VARCHAR(20) NOT NULL,
    client_id VARCHAR(17),
    value_type VARCHAR(20) NOT NULL,
    value TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_app_platform_config_key ON app_platform_configs (application_code, section, property, scope, client_id);
CREATE INDEX IF NOT EXISTS idx_app_platform_configs_lookup ON app_platform_configs (application_code, section, scope, client_id);
CREATE INDEX IF NOT EXISTS idx_app_platform_configs_app_section ON app_platform_configs (application_code, section);

-- =============================================================================
-- app_platform_config_access - Platform config access control
-- =============================================================================
CREATE TABLE IF NOT EXISTS app_platform_config_access (
    id VARCHAR(17) PRIMARY KEY,
    application_code VARCHAR(100) NOT NULL,
    role_code VARCHAR(200) NOT NULL,
    can_read BOOLEAN NOT NULL DEFAULT TRUE,
    can_write BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_app_config_access_role ON app_platform_config_access (application_code, role_code);
CREATE INDEX IF NOT EXISTS idx_app_config_access_app ON app_platform_config_access (application_code);
CREATE INDEX IF NOT EXISTS idx_app_config_access_role ON app_platform_config_access (role_code);
