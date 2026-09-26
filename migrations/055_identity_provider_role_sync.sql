-- Role-sync configuration lives on the identity provider (Go's 040): whether
-- logins through the provider reconcile the user's IDP_SYNC roles from the
-- token's `roles` claim, and which platform roles (by id) it may confer. The
-- email-domain mapping's `sync_roles_from_idp` column and its
-- `tnt_email_domain_mapping_allowed_roles` junction are no longer read.
--
-- A database Go migrated already has both (the probe marks this migration
-- applied there without running it). On a database Rust created, the column
-- is added and filled the way Go's 040 filled it: OIDC providers keep syncing
-- (the old per-domain flag was never consulted at login), and each provider's
-- allow-list is the union of its mappings' lists, except that a provider with
-- any unrestricted mapping stays unrestricted. The fill runs only when the
-- column is new, so a re-run never overwrites an administrator's choice.

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'oauth_identity_providers'
          AND column_name = 'sync_roles_from_idp'
    ) THEN
        ALTER TABLE oauth_identity_providers
            ADD COLUMN sync_roles_from_idp BOOLEAN NOT NULL DEFAULT FALSE;
        UPDATE oauth_identity_providers SET sync_roles_from_idp = TRUE WHERE type = 'OIDC';

        CREATE TABLE IF NOT EXISTS oauth_identity_provider_allowed_roles (
            id SERIAL PRIMARY KEY,
            identity_provider_id VARCHAR(17) NOT NULL,
            role_id VARCHAR(17) NOT NULL
        );

        INSERT INTO oauth_identity_provider_allowed_roles (identity_provider_id, role_id)
        SELECT DISTINCT m.identity_provider_id, ar.role_id
        FROM tnt_email_domain_mappings m
        JOIN tnt_email_domain_mapping_allowed_roles ar
            ON ar.email_domain_mapping_id = m.id
        WHERE m.identity_provider_id NOT IN (
            SELECT m2.identity_provider_id
            FROM tnt_email_domain_mappings m2
            WHERE NOT EXISTS (
                SELECT 1 FROM tnt_email_domain_mapping_allowed_roles ar2
                WHERE ar2.email_domain_mapping_id = m2.id
            )
        );
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS oauth_identity_provider_allowed_roles (
    id SERIAL PRIMARY KEY,
    identity_provider_id VARCHAR(17) NOT NULL,
    role_id VARCHAR(17) NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_oauth_idp_allowed_roles_idp
    ON oauth_identity_provider_allowed_roles (identity_provider_id);
