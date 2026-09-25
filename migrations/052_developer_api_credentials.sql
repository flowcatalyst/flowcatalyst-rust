-- Self-service developer API credentials (Go's 039): a USER principal holding
-- platform:developer mints client_credentials tokens as themselves
-- (client_id = their principal id) with a dedicated, rotatable secret kept
-- here as a keyed hash, never their login password. A database Go migrated
-- already has both columns.
ALTER TABLE iam_principals ADD COLUMN IF NOT EXISTS dev_client_secret_ref TEXT;
ALTER TABLE iam_principals ADD COLUMN IF NOT EXISTS dev_client_secret_updated_at TIMESTAMPTZ;
