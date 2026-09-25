-- Reset-token columns Go's password flows read and write (Go's 031, 032, 033,
-- 041 and 051, on iam_password_reset_tokens only):
--
--   purpose          'reset' (forgot password / admin reset, 15 min) or
--                    'invite' (first-time "set your password", 72 h)
--   reset_2fa        the confirm also clears the user's second factors
--   requires_factor  the confirm also needs an authenticator (TOTP) code
--   factor_attempts  wrong factor codes against this token (burned at 5)
--   redirect_uri     where the SPA goes once the flow completes
--
-- A database Go has migrated already has every column and the CHECK, so each
-- statement is a no-op there.
ALTER TABLE iam_password_reset_tokens
    ADD COLUMN IF NOT EXISTS purpose VARCHAR(20) NOT NULL DEFAULT 'reset';
ALTER TABLE iam_password_reset_tokens
    ADD COLUMN IF NOT EXISTS reset_2fa BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE iam_password_reset_tokens
    ADD COLUMN IF NOT EXISTS requires_factor BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE iam_password_reset_tokens
    ADD COLUMN IF NOT EXISTS factor_attempts INT NOT NULL DEFAULT 0;
ALTER TABLE iam_password_reset_tokens
    ADD COLUMN IF NOT EXISTS redirect_uri VARCHAR(2000);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_iam_password_reset_tokens_purpose'
    ) THEN
        ALTER TABLE iam_password_reset_tokens
            ADD CONSTRAINT chk_iam_password_reset_tokens_purpose
            CHECK (purpose IN ('reset', 'invite'));
    END IF;
END $$;
