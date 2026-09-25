-- OAuth clients flagged `api_access` (Go's migration 042): a trusted
-- first-party client whose interactive logins (authorization_code and its
-- refresh) receive an authority-bearing access token (`token_use: api`),
-- narrowed to the client's applications. Every other client's logins receive
-- an identity-only token, which the platform API refuses as a bearer.
--
-- A database Go migrated already has the column (the probe backfills this
-- migration there); a Rust-created one gets it here. Re-running is a no-op.
ALTER TABLE oauth_clients
    ADD COLUMN IF NOT EXISTS api_access BOOLEAN NOT NULL DEFAULT FALSE;
