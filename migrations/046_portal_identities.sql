-- 046_portal_identities.sql
--
-- Mirrors flowcatalyst-go internal/migrate/sql/041_portal_identities.sql and
-- 043_drop_idp_portal_binding.sql (their Up sections). IF NOT EXISTS makes it
-- a no-op on a database Go has already migrated.
--
-- Portal identity plane: portal end-users are a SEPARATE identity population,
-- one row per (client, email) context, wholly unrelated to iam_principals.
-- The portal login endpoints are independent of the employee auth surface and
-- never touch fc_session.
CREATE TABLE IF NOT EXISTS portal_identities (
    id VARCHAR(17) PRIMARY KEY,
    client_id VARCHAR(17) NOT NULL,               -- portal-operator tenant client
    email VARCHAR(255) NOT NULL,                  -- stored lowercased
    name VARCHAR(255),
    password_hash VARCHAR(255),                   -- NULL until the invite completes (or SSO-only)
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE', -- ACTIVE | DISABLED
    source VARCHAR(20) NOT NULL,                  -- INVITE | JIT
    last_login_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_portal_identities_client_email UNIQUE (client_id, email)
);

CREATE INDEX IF NOT EXISTS idx_portal_identities_email
    ON portal_identities (email);

-- Short-lived single-use stash for the portal authorization flow: GET
-- /portal/authorize validates the (portal-flagged) OAuth client + redirect
-- URI + PKCE, parks the chain here, and bounces to the SPA portal login
-- page; the password/SSO handlers redeem the row to mint the code.
CREATE TABLE IF NOT EXISTS portal_login_flows (
    id VARCHAR(64) PRIMARY KEY,
    oauth_client_id VARCHAR(100) NOT NULL,        -- oauth_clients.client_id
    portal_client_id VARCHAR(17) NOT NULL,        -- owner tenant client (denormalized at stash time)
    redirect_uri VARCHAR(2000) NOT NULL,
    scope VARCHAR(500),
    state VARCHAR(500) NOT NULL,
    nonce VARCHAR(500),
    code_challenge VARCHAR(200),
    code_challenge_method VARCHAR(10),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

-- Non-NULL marks an OAuth client as a portal entry point owned by that
-- tenant client: it routes through /portal/authorize and its codes carry
-- portal-identity subjects.
ALTER TABLE oauth_clients
    ADD COLUMN IF NOT EXISTS portal_client_id VARCHAR(17);

-- Go's 041 bound identity providers to a portal and its 043 dropped that
-- binding again (an IdP authenticates the domains it owns for every login
-- surface). The net effect is no column; drop it should one exist.
ALTER TABLE oauth_identity_providers
    DROP COLUMN IF EXISTS portal_client_id;

-- Marks an OIDC login state as a PORTAL-plane handshake for the given owner
-- client: the OIDC callback then routes into the portal sink (JIT portal
-- identity + code issuance, no fc_session) instead of the employee flow.
ALTER TABLE oauth_oidc_login_states
    ADD COLUMN IF NOT EXISTS portal_client_id VARCHAR(17);

-- Go's 031 reset-token columns, which the portal invite path and 047's
-- backfill read (the definitions are Go's, so this is a no-op wherever they
-- already exist).
ALTER TABLE iam_password_reset_tokens
    ADD COLUMN IF NOT EXISTS purpose VARCHAR(20) NOT NULL DEFAULT 'reset';
ALTER TABLE iam_password_reset_tokens
    ADD COLUMN IF NOT EXISTS reset_2fa BOOLEAN NOT NULL DEFAULT FALSE;

-- Post-set-password redirect for portal invites. Reset tokens key portal
-- identities by their ptu_ id in principal_id: the two id spaces share the
-- column but never collide thanks to the TSID prefixes.
ALTER TABLE iam_password_reset_tokens
    ADD COLUMN IF NOT EXISTS redirect_uri VARCHAR(2000);
