-- 047_portal_apps.sql
--
-- Mirrors flowcatalyst-go internal/migrate/sql/053_portal_apps.sql (its Up
-- section, verbatim). IF NOT EXISTS makes it a no-op on a database Go has
-- already migrated.
--
-- Portal applications: a client may run several portals (e.g. a customer
-- portal and a supplier portal). Each is a named portal_apps row with a
-- code the portal app itself uses on the admin API; OAuth clients link to
-- the app they front (oauth_clients.portal_app_id) and the id_token of a
-- login through that OAuth client carries the app's code back.
--
-- Identities stay one per (client, email) — one password across a client's
-- portals — and are GRANTED per app (portal_identity_apps). A login through
-- an app-linked OAuth client requires a grant for that app.
CREATE TABLE IF NOT EXISTS portal_apps (
    id VARCHAR(17) PRIMARY KEY,
    client_id VARCHAR(17) NOT NULL,               -- owning tenant client
    code VARCHAR(100) NOT NULL,                   -- stable identifier the portal app sends
    name VARCHAR(255) NOT NULL,
    description VARCHAR(1000),
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_portal_apps_client_code UNIQUE (client_id, code)
);

CREATE TABLE IF NOT EXISTS portal_identity_apps (
    identity_id VARCHAR(17) NOT NULL REFERENCES portal_identities(id) ON DELETE CASCADE,
    portal_app_id VARCHAR(17) NOT NULL REFERENCES portal_apps(id) ON DELETE CASCADE,
    source VARCHAR(20) NOT NULL,                  -- INVITE | JIT | ADMIN
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (identity_id, portal_app_id)
);

CREATE INDEX IF NOT EXISTS idx_portal_identity_apps_app
    ON portal_identity_apps (portal_app_id);

-- Non-NULL links a portal-flagged OAuth client to the portal app it fronts.
-- portal_client_id stays the plane gate (and must equal the app's client).
-- NULL on a portal client = legacy client-wide portal: no app grant is
-- required and no app code is reported.
ALTER TABLE oauth_clients
    ADD COLUMN IF NOT EXISTS portal_app_id VARCHAR(17);

-- Invite bookkeeping, so the admin surface can show INVITED vs
-- INVITE_EXPIRED vs ACTIVE without joining the reset-token table.
-- invite_expires_at NULL with invited_at set = an SSO invite (no expiry).
ALTER TABLE portal_identities
    ADD COLUMN IF NOT EXISTS invited_at TIMESTAMPTZ;
ALTER TABLE portal_identities
    ADD COLUMN IF NOT EXISTS invite_expires_at TIMESTAMPTZ;

-- Backfill: a live portal invite token supplies the real dates; any other
-- password-less, never-logged-in INVITE identity gets its creation time plus
-- the 72h invite lifetime (it was invited on create).
UPDATE portal_identities pi
SET invited_at = t.created_at, invite_expires_at = t.expires_at
FROM iam_password_reset_tokens t
WHERE t.principal_id = pi.id AND t.purpose = 'invite' AND pi.invited_at IS NULL;

UPDATE portal_identities
SET invited_at = created_at, invite_expires_at = created_at + INTERVAL '72 hours'
WHERE invited_at IS NULL AND source = 'INVITE'
  AND password_hash IS NULL AND last_login_at IS NULL;

-- Prefix search (TERM%) on email and name within a client. email is stored
-- lower-cased; name is matched on lower(name).
CREATE INDEX IF NOT EXISTS idx_portal_identities_client_email_prefix
    ON portal_identities (client_id, email text_pattern_ops);
CREATE INDEX IF NOT EXISTS idx_portal_identities_client_name_prefix
    ON portal_identities (client_id, lower(name) text_pattern_ops);
