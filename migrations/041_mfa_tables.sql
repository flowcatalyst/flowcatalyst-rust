-- Two-factor authentication for internal (password) users, in Go's tables
-- (Go's 031_mfa_tables, verbatim apart from the reset-token columns, which
-- 042 adds): the per-domain policy on the email-domain mapping, the enrolled
-- factors (TOTP / email PIN), single-use recovery codes, pending email-PIN
-- challenges and remembered ("trusted") devices. Federated (OIDC) users never
-- get rows here.
--
-- A database Go has migrated already has all of it, so every statement is a
-- no-op there.

-- Per-domain 2FA enforcement + remember-device.
ALTER TABLE tnt_email_domain_mappings
    ADD COLUMN IF NOT EXISTS require_2fa BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE tnt_email_domain_mappings
    ADD COLUMN IF NOT EXISTS remember_device_enabled BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE tnt_email_domain_mappings
    ADD COLUMN IF NOT EXISTS remember_device_days INTEGER NOT NULL DEFAULT 30;

-- The domain's allowed methods ('TOTP' | 'EMAIL_PIN'); at least one when
-- require_2fa is set (checked by the application).
CREATE TABLE IF NOT EXISTS tnt_email_domain_mapping_2fa_methods (
    id SERIAL PRIMARY KEY,
    email_domain_mapping_id VARCHAR(17) NOT NULL,
    method VARCHAR(20) NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tnt_edm_2fa_methods_mapping
    ON tnt_email_domain_mapping_2fa_methods (email_domain_mapping_id);

-- Enrolled second factors. secret_encrypted is the encrypted TOTP secret
-- (NULL for EMAIL_PIN). confirmed_at is NULL until a first code verifies;
-- last_used_at is the start of the last accepted TOTP time-step (replay
-- guard).
CREATE TABLE IF NOT EXISTS iam_user_mfa_methods (
    id               VARCHAR(17) PRIMARY KEY,
    principal_id     VARCHAR(17) NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
    method           VARCHAR(20) NOT NULL,
    secret_encrypted TEXT,
    confirmed_at     TIMESTAMPTZ,
    last_used_at     TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_iam_user_mfa_methods_principal_method
    ON iam_user_mfa_methods (principal_id, method);
CREATE INDEX IF NOT EXISTS idx_iam_user_mfa_methods_principal
    ON iam_user_mfa_methods (principal_id);

-- Single-use backup codes (SHA-256 of the printable code).
CREATE TABLE IF NOT EXISTS iam_user_mfa_recovery_codes (
    id           VARCHAR(17) PRIMARY KEY,
    principal_id VARCHAR(17) NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
    code_hash    VARCHAR(64) NOT NULL,
    used_at      TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_iam_user_mfa_recovery_codes_principal
    ON iam_user_mfa_recovery_codes (principal_id);
CREATE INDEX IF NOT EXISTS idx_iam_user_mfa_recovery_codes_hash
    ON iam_user_mfa_recovery_codes (code_hash);

-- Pending email-PIN challenges ('login' or 'enroll'; SHA-256 of the PIN).
CREATE TABLE IF NOT EXISTS iam_mfa_email_pins (
    id           VARCHAR(17) PRIMARY KEY,
    principal_id VARCHAR(17) NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
    purpose      VARCHAR(20) NOT NULL DEFAULT 'login',
    pin_hash     VARCHAR(64) NOT NULL,
    attempts     INTEGER NOT NULL DEFAULT 0,
    expires_at   TIMESTAMPTZ NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_iam_mfa_email_pins_principal
    ON iam_mfa_email_pins (principal_id);
CREATE INDEX IF NOT EXISTS idx_iam_mfa_email_pins_expires
    ON iam_mfa_email_pins (expires_at);

-- Remembered devices (SHA-256 of the __Host-fc_td cookie token).
CREATE TABLE IF NOT EXISTS iam_mfa_trusted_devices (
    id           VARCHAR(17) PRIMARY KEY,
    principal_id VARCHAR(17) NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
    token_hash   VARCHAR(64) NOT NULL UNIQUE,
    label        VARCHAR(255),
    expires_at   TIMESTAMPTZ NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_iam_mfa_trusted_devices_principal
    ON iam_mfa_trusted_devices (principal_id);
CREATE INDEX IF NOT EXISTS idx_iam_mfa_trusted_devices_hash
    ON iam_mfa_trusted_devices (token_hash);
