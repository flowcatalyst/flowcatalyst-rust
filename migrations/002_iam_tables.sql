-- FlowCatalyst IAM Tables
-- Matches TypeScript Drizzle schema exactly

-- =============================================================================
-- iam_principals - Users and service accounts
-- =============================================================================
CREATE TABLE IF NOT EXISTS iam_principals (
    id VARCHAR(17) PRIMARY KEY,
    type VARCHAR(20) NOT NULL,
    scope VARCHAR(20),
    client_id VARCHAR(17),
    application_id VARCHAR(17),
    name VARCHAR(255) NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    email VARCHAR(255),
    email_domain VARCHAR(100),
    idp_type VARCHAR(50),
    external_idp_id VARCHAR(255),
    password_hash VARCHAR(255),
    last_login_at TIMESTAMPTZ,
    service_account_id VARCHAR(17),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_iam_principals_type ON iam_principals (type);
CREATE INDEX IF NOT EXISTS idx_iam_principals_client_id ON iam_principals (client_id);
CREATE INDEX IF NOT EXISTS idx_iam_principals_active ON iam_principals (active);
CREATE UNIQUE INDEX IF NOT EXISTS idx_iam_principals_email ON iam_principals (email);
CREATE INDEX IF NOT EXISTS idx_iam_principals_email_domain ON iam_principals (email_domain);
CREATE UNIQUE INDEX IF NOT EXISTS idx_iam_principals_service_account_id ON iam_principals (service_account_id);

-- =============================================================================
-- iam_service_accounts - Service account details
-- =============================================================================
CREATE TABLE IF NOT EXISTS iam_service_accounts (
    id VARCHAR(17) PRIMARY KEY,
    code VARCHAR(100) NOT NULL UNIQUE,
    name VARCHAR(200) NOT NULL,
    description VARCHAR(500),
    application_id VARCHAR(17),
    active BOOLEAN NOT NULL DEFAULT TRUE,
    wh_auth_type VARCHAR(50),
    wh_auth_token_ref VARCHAR(500),
    wh_signing_secret_ref VARCHAR(500),
    wh_signing_algorithm VARCHAR(50),
    wh_credentials_created_at TIMESTAMPTZ,
    wh_credentials_regenerated_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_iam_service_accounts_code ON iam_service_accounts (code);
CREATE INDEX IF NOT EXISTS idx_iam_service_accounts_application_id ON iam_service_accounts (application_id);
CREATE INDEX IF NOT EXISTS idx_iam_service_accounts_active ON iam_service_accounts (active);

-- =============================================================================
-- iam_principal_roles - Principal role assignments (junction)
-- =============================================================================
CREATE TABLE IF NOT EXISTS iam_principal_roles (
    principal_id VARCHAR(17) NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
    role_name VARCHAR(100) NOT NULL,
    assignment_source VARCHAR(50),
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (principal_id, role_name)
);

CREATE INDEX IF NOT EXISTS idx_iam_principal_roles_role_name ON iam_principal_roles (role_name);
CREATE INDEX IF NOT EXISTS idx_iam_principal_roles_assigned_at ON iam_principal_roles (assigned_at);

-- =============================================================================
-- iam_roles - Role definitions
-- =============================================================================
CREATE TABLE IF NOT EXISTS iam_roles (
    id VARCHAR(17) PRIMARY KEY,
    application_id VARCHAR(17),
    application_code VARCHAR(50),
    name VARCHAR(255) NOT NULL UNIQUE,
    display_name VARCHAR(255) NOT NULL,
    description TEXT,
    source VARCHAR(50) NOT NULL DEFAULT 'DATABASE',
    client_managed BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_iam_roles_name ON iam_roles (name);
CREATE INDEX IF NOT EXISTS idx_iam_roles_application_id ON iam_roles (application_id);
CREATE INDEX IF NOT EXISTS idx_iam_roles_application_code ON iam_roles (application_code);
CREATE INDEX IF NOT EXISTS idx_iam_roles_source ON iam_roles (source);
CREATE INDEX IF NOT EXISTS idx_iam_roles_client_managed ON iam_roles (client_managed);

-- =============================================================================
-- iam_permissions - Permission definitions
-- =============================================================================
CREATE TABLE IF NOT EXISTS iam_permissions (
    id VARCHAR(17) PRIMARY KEY,
    code VARCHAR(255) NOT NULL UNIQUE,
    subdomain VARCHAR(50) NOT NULL,
    context VARCHAR(50) NOT NULL,
    aggregate VARCHAR(50) NOT NULL,
    action VARCHAR(50) NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_iam_permissions_code ON iam_permissions (code);
CREATE INDEX IF NOT EXISTS idx_iam_permissions_subdomain ON iam_permissions (subdomain);
CREATE INDEX IF NOT EXISTS idx_iam_permissions_context ON iam_permissions (context);

-- =============================================================================
-- iam_role_permissions - Role permissions (junction)
-- =============================================================================
CREATE TABLE IF NOT EXISTS iam_role_permissions (
    role_id VARCHAR(17) NOT NULL REFERENCES iam_roles(id) ON DELETE CASCADE,
    permission VARCHAR(255) NOT NULL,
    PRIMARY KEY (role_id, permission)
);

CREATE INDEX IF NOT EXISTS idx_iam_role_permissions_role_id ON iam_role_permissions (role_id);

-- =============================================================================
-- iam_principal_application_access - Principal application access (junction)
-- =============================================================================
CREATE TABLE IF NOT EXISTS iam_principal_application_access (
    principal_id VARCHAR(17) NOT NULL,
    application_id VARCHAR(17) NOT NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (principal_id, application_id)
);

CREATE INDEX IF NOT EXISTS idx_iam_principal_app_access_app_id ON iam_principal_application_access (application_id);

-- =============================================================================
-- iam_client_access_grants - Client access grants
-- =============================================================================
CREATE TABLE IF NOT EXISTS iam_client_access_grants (
    id VARCHAR(17) PRIMARY KEY,
    principal_id VARCHAR(17) NOT NULL,
    client_id VARCHAR(17) NOT NULL,
    granted_by VARCHAR(17) NOT NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_iam_client_access_grants_principal ON iam_client_access_grants (principal_id);
CREATE INDEX IF NOT EXISTS idx_iam_client_access_grants_client ON iam_client_access_grants (client_id);
CREATE UNIQUE INDEX IF NOT EXISTS uq_iam_client_access_grants_principal_client ON iam_client_access_grants (principal_id, client_id);
