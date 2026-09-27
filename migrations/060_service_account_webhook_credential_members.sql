-- The webhook-credential members Go accepts on a service account but has no
-- column for: the basic-auth username and password, the API-key header
-- name and the signature header name.
--
-- Go's `WebhookCredentialsDTO` (`PUT /api/service-accounts/{id}`) carries
-- `authType, token, username, password, headerName, signingSecret,
-- signingAlgorithm, signatureHeader`; Go stores the first, the token, the
-- signing secret and the algorithm (wh_*) and silently drops the other four.
-- This platform stores them too, so an update keeps everything it was sent.
-- wh_password_ref, like wh_auth_token_ref and wh_signing_secret_ref, holds an
-- `encrypted:` reference, never the password. All four are write-only: no
-- read answers them.
--
-- ADD COLUMN IF NOT EXISTS: re-running is a no-op.
ALTER TABLE iam_service_accounts
    ADD COLUMN IF NOT EXISTS wh_username VARCHAR(255),
    ADD COLUMN IF NOT EXISTS wh_password_ref VARCHAR(500),
    ADD COLUMN IF NOT EXISTS wh_header_name VARCHAR(100),
    ADD COLUMN IF NOT EXISTS wh_signature_header VARCHAR(100);
