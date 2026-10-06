//! Authentication Configuration Repositories — PostgreSQL via SQLx

use crate::auth::config_entity::AuthConfigType;
use crate::auth::config_entity::AuthProvider;
use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::AnchorDomainId;
use fc_platform_core::shared::id::ClientAccessGrantId;
use fc_platform_core::shared::id::ClientAuthConfigId;
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::IdpRoleMappingId;
use fc_platform_core::shared::id::PrincipalId;
use sqlx::PgPool;

use crate::auth::config_entity::{AnchorDomain, ClientAuthConfig, IdpRoleMapping};
use crate::principal::entity::ClientAccessGrant;
use fc_platform_core::shared::enum_str::Stored;
use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::usecase;
use fc_platform_core::usecase::unit_of_work::HasId;
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::Persist;

// ── Row types ────────────────────────────────────────────────────────────────

struct AnchorDomainRow {
    id: AnchorDomainId,
    domain: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<AnchorDomainRow> for AnchorDomain {
    fn from(r: AnchorDomainRow) -> Self {
        Self {
            id: r.id,
            domain: r.domain,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

struct ClientAuthConfigRow {
    id: ClientAuthConfigId,
    email_domain: String,
    config_type: Stored<AuthConfigType>,
    primary_client_id: Option<ClientId>,
    additional_client_ids: serde_json::Value,
    granted_client_ids: serde_json::Value,
    auth_provider: Stored<AuthProvider>,
    oidc_issuer_url: Option<String>,
    oidc_client_id: Option<String>,
    oidc_multi_tenant: bool,
    oidc_issuer_pattern: Option<String>,
    oidc_client_secret_ref: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ClientAuthConfigRow> for ClientAuthConfig {
    type Error = PlatformError;
    fn try_from(r: ClientAuthConfigRow) -> Result<Self> {
        let config_type =
            r.config_type
                .decode("tnt_client_auth_configs", "config_type", r.id.as_str())?;
        let auth_provider =
            r.auth_provider
                .decode("tnt_client_auth_configs", "auth_provider", r.id.as_str())?;
        let additional_client_ids: Vec<ClientId> =
            serde_json::from_value(r.additional_client_ids).unwrap_or_default();
        let granted_client_ids: Vec<ClientId> =
            serde_json::from_value(r.granted_client_ids).unwrap_or_default();
        Ok(Self {
            id: r.id,
            email_domain: r.email_domain,
            config_type,
            primary_client_id: r.primary_client_id,
            additional_client_ids,
            granted_client_ids,
            auth_provider,
            oidc_issuer_url: r.oidc_issuer_url,
            oidc_client_id: r.oidc_client_id,
            oidc_multi_tenant: r.oidc_multi_tenant,
            oidc_issuer_pattern: r.oidc_issuer_pattern,
            oidc_client_secret_ref: r.oidc_client_secret_ref,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

struct ClientAccessGrantRow {
    id: ClientAccessGrantId,
    principal_id: PrincipalId,
    client_id: ClientId,
    granted_by: String,
    granted_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ClientAccessGrantRow> for ClientAccessGrant {
    fn from(r: ClientAccessGrantRow) -> Self {
        Self {
            id: r.id,
            principal_id: r.principal_id,
            client_id: r.client_id,
            granted_by: r.granted_by,
            granted_at: r.granted_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

struct IdpRoleMappingRow {
    id: IdpRoleMappingId,
    idp_role_name: String,
    internal_role_name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<IdpRoleMappingRow> for IdpRoleMapping {
    fn from(r: IdpRoleMappingRow) -> Self {
        Self {
            id: r.id,
            idp_type: "OIDC".to_string(), // DB table doesn't store idp_type separately
            idp_role_name: r.idp_role_name,
            platform_role_name: r.internal_role_name,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// ── AnchorDomainRepository ───────────────────────────────────────────────────

pub struct AnchorDomainRepository {
    pool: PgPool,
}

impl AnchorDomainRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, domain: &AnchorDomain) -> Result<()> {
        let now = Utc::now();
        sqlx::query!(
            "INSERT INTO tnt_anchor_domains (id, domain, created_at, updated_at)
             VALUES ($1, $2, $3, $4)",
            &domain.id as &AnchorDomainId,
            &domain.domain,
            now,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &AnchorDomainId) -> Result<Option<AnchorDomain>> {
        let row = sqlx::query_as!(
            AnchorDomainRow,
            "SELECT id AS \"id: AnchorDomainId\", domain, created_at, updated_at \
                    FROM tnt_anchor_domains WHERE id = $1",
            id as &AnchorDomainId
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(AnchorDomain::from))
    }

    pub async fn find_by_domain(&self, domain: &str) -> Result<Option<AnchorDomain>> {
        let row = sqlx::query_as!(
            AnchorDomainRow,
            "SELECT id AS \"id: AnchorDomainId\", domain, created_at, updated_at \
                    FROM tnt_anchor_domains WHERE domain = $1",
            domain.to_lowercase()
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(AnchorDomain::from))
    }

    pub async fn find_all(&self) -> Result<Vec<AnchorDomain>> {
        let rows = sqlx::query_as!(
            AnchorDomainRow,
            "SELECT id AS \"id: AnchorDomainId\", domain, created_at, updated_at \
                    FROM tnt_anchor_domains ORDER BY domain ASC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(AnchorDomain::from).collect())
    }

    pub async fn is_anchor_domain(&self, domain: &str) -> Result<bool> {
        let row = sqlx::query_scalar!(
            "SELECT COUNT(*) AS \"count!\" FROM tnt_anchor_domains WHERE domain = $1",
            domain.to_lowercase()
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row > 0)
    }

    pub async fn update(&self, domain: &AnchorDomain) -> Result<()> {
        let now = Utc::now();
        sqlx::query!(
            "UPDATE tnt_anchor_domains SET domain = $2, updated_at = $3 WHERE id = $1",
            &domain.id as &AnchorDomainId,
            &domain.domain,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &AnchorDomainId) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM tnt_anchor_domains WHERE id = $1",
            id as &AnchorDomainId
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ── ClientAuthConfigRepository ───────────────────────────────────────────────

pub struct ClientAuthConfigRepository {
    pool: PgPool,
}

impl ClientAuthConfigRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, config: &ClientAuthConfig) -> Result<()> {
        let now = Utc::now();
        let additional_ids_json =
            serde_json::to_value(&config.additional_client_ids).unwrap_or_default();
        let granted_ids_json = serde_json::to_value(&config.granted_client_ids).unwrap_or_default();

        sqlx::query!(
            "INSERT INTO tnt_client_auth_configs
                (id, email_domain, config_type, primary_client_id, additional_client_ids,
                 granted_client_ids, auth_provider, oidc_issuer_url, oidc_client_id,
                 oidc_multi_tenant, oidc_issuer_pattern, oidc_client_secret_ref,
                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
            &config.id as &ClientAuthConfigId,
            &config.email_domain,
            config.config_type as AuthConfigType,
            &config.primary_client_id as &Option<ClientId>,
            &additional_ids_json,
            &granted_ids_json,
            config.auth_provider as AuthProvider,
            config.oidc_issuer_url.as_ref(),
            config.oidc_client_id.as_ref(),
            config.oidc_multi_tenant,
            config.oidc_issuer_pattern.as_ref(),
            config.oidc_client_secret_ref.as_ref(),
            now,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &ClientAuthConfigId) -> Result<Option<ClientAuthConfig>> {
        let row = sqlx::query_as!(
            ClientAuthConfigRow,
            "SELECT id AS \"id: ClientAuthConfigId\", email_domain, \
                    config_type AS \"config_type: Stored<AuthConfigType>\", \
                    primary_client_id AS \"primary_client_id: ClientId\", \
                    additional_client_ids AS \"additional_client_ids: serde_json::Value\", \
                    granted_client_ids AS \"granted_client_ids: serde_json::Value\", \
                    auth_provider AS \"auth_provider: Stored<AuthProvider>\", \
                    oidc_issuer_url, oidc_client_id, oidc_multi_tenant, \
                    oidc_issuer_pattern, oidc_client_secret_ref, created_at, updated_at \
                    FROM tnt_client_auth_configs WHERE id = $1",
            id as &ClientAuthConfigId
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(ClientAuthConfig::try_from).transpose()
    }

    pub async fn find_by_email_domain(&self, domain: &str) -> Result<Option<ClientAuthConfig>> {
        let row = sqlx::query_as!(
            ClientAuthConfigRow,
            "SELECT id AS \"id: ClientAuthConfigId\", email_domain, \
                    config_type AS \"config_type: Stored<AuthConfigType>\", \
                    primary_client_id AS \"primary_client_id: ClientId\", \
                    additional_client_ids AS \"additional_client_ids: serde_json::Value\", \
                    granted_client_ids AS \"granted_client_ids: serde_json::Value\", \
                    auth_provider AS \"auth_provider: Stored<AuthProvider>\", \
                    oidc_issuer_url, oidc_client_id, oidc_multi_tenant, \
                    oidc_issuer_pattern, oidc_client_secret_ref, created_at, updated_at \
                    FROM tnt_client_auth_configs WHERE email_domain = $1",
            domain.to_lowercase()
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(ClientAuthConfig::try_from).transpose()
    }

    pub async fn find_by_client_id(&self, client_id: &ClientId) -> Result<Vec<ClientAuthConfig>> {
        let rows = sqlx::query_as!(
            ClientAuthConfigRow,
            "SELECT id AS \"id: ClientAuthConfigId\", email_domain, \
                    config_type AS \"config_type: Stored<AuthConfigType>\", \
                    primary_client_id AS \"primary_client_id: ClientId\", \
                    additional_client_ids AS \"additional_client_ids: serde_json::Value\", \
                    granted_client_ids AS \"granted_client_ids: serde_json::Value\", \
                    auth_provider AS \"auth_provider: Stored<AuthProvider>\", \
                    oidc_issuer_url, oidc_client_id, oidc_multi_tenant, \
                    oidc_issuer_pattern, oidc_client_secret_ref, created_at, updated_at \
                    FROM tnt_client_auth_configs WHERE primary_client_id = $1",
            client_id as &ClientId
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(ClientAuthConfig::try_from).collect()
    }

    pub async fn find_all(&self) -> Result<Vec<ClientAuthConfig>> {
        let rows = sqlx::query_as!(
            ClientAuthConfigRow,
            "SELECT id AS \"id: ClientAuthConfigId\", email_domain, \
                    config_type AS \"config_type: Stored<AuthConfigType>\", \
                    primary_client_id AS \"primary_client_id: ClientId\", \
                    additional_client_ids AS \"additional_client_ids: serde_json::Value\", \
                    granted_client_ids AS \"granted_client_ids: serde_json::Value\", \
                    auth_provider AS \"auth_provider: Stored<AuthProvider>\", \
                    oidc_issuer_url, oidc_client_id, oidc_multi_tenant, \
                    oidc_issuer_pattern, oidc_client_secret_ref, created_at, updated_at \
                    FROM tnt_client_auth_configs ORDER BY email_domain ASC"
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(ClientAuthConfig::try_from).collect()
    }

    pub async fn update(&self, config: &ClientAuthConfig) -> Result<()> {
        let now = Utc::now();
        let additional_ids_json =
            serde_json::to_value(&config.additional_client_ids).unwrap_or_default();
        let granted_ids_json = serde_json::to_value(&config.granted_client_ids).unwrap_or_default();

        sqlx::query!(
            "UPDATE tnt_client_auth_configs SET
                email_domain = $2, config_type = $3, primary_client_id = $4,
                additional_client_ids = $5, granted_client_ids = $6,
                auth_provider = $7, oidc_issuer_url = $8, oidc_client_id = $9,
                oidc_multi_tenant = $10, oidc_issuer_pattern = $11,
                oidc_client_secret_ref = $12, updated_at = $13
             WHERE id = $1",
            &config.id as &ClientAuthConfigId,
            &config.email_domain,
            config.config_type as AuthConfigType,
            &config.primary_client_id as &Option<ClientId>,
            &additional_ids_json,
            &granted_ids_json,
            config.auth_provider as AuthProvider,
            config.oidc_issuer_url.as_ref(),
            config.oidc_client_id.as_ref(),
            config.oidc_multi_tenant,
            config.oidc_issuer_pattern.as_ref(),
            config.oidc_client_secret_ref.as_ref(),
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &ClientAuthConfigId) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM tnt_client_auth_configs WHERE id = $1",
            id as &ClientAuthConfigId
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ── ClientAccessGrantRepository ──────────────────────────────────────────────

pub struct ClientAccessGrantRepository {
    pool: PgPool,
}

impl ClientAccessGrantRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, grant: &ClientAccessGrant) -> Result<()> {
        sqlx::query!(
            "INSERT INTO iam_client_access_grants
                (id, principal_id, client_id, granted_by, granted_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &grant.id as &ClientAccessGrantId,
            &grant.principal_id as &PrincipalId,
            &grant.client_id as &ClientId,
            &grant.granted_by,
            grant.granted_at,
            grant.created_at,
            grant.updated_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &ClientAccessGrantId) -> Result<Option<ClientAccessGrant>> {
        let row = sqlx::query_as!(
            ClientAccessGrantRow,
            "SELECT id AS \"id: ClientAccessGrantId\", \
                    principal_id AS \"principal_id: PrincipalId\", \
                    client_id AS \"client_id: ClientId\", granted_by, granted_at, \
                    created_at, updated_at \
                    FROM iam_client_access_grants WHERE id = $1",
            id as &ClientAccessGrantId
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ClientAccessGrant::from))
    }

    /// A principal's grants, oldest first (Go `FindByPrincipal`).
    pub async fn find_by_principal(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<Vec<ClientAccessGrant>> {
        let rows = sqlx::query_as!(
            ClientAccessGrantRow,
            "SELECT id AS \"id: ClientAccessGrantId\", \
                    principal_id AS \"principal_id: PrincipalId\", \
                    client_id AS \"client_id: ClientId\", granted_by, granted_at, \
                    created_at, updated_at \
                    FROM iam_client_access_grants WHERE principal_id = $1
             ORDER BY granted_at, id",
            principal_id as &PrincipalId
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(ClientAccessGrant::from).collect())
    }

    pub async fn find_by_client(&self, client_id: &ClientId) -> Result<Vec<ClientAccessGrant>> {
        let rows = sqlx::query_as!(
            ClientAccessGrantRow,
            "SELECT id AS \"id: ClientAccessGrantId\", \
                    principal_id AS \"principal_id: PrincipalId\", \
                    client_id AS \"client_id: ClientId\", granted_by, granted_at, \
                    created_at, updated_at \
                    FROM iam_client_access_grants WHERE client_id = $1",
            client_id as &ClientId
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(ClientAccessGrant::from).collect())
    }

    pub async fn find_by_principal_and_client(
        &self,
        principal_id: &PrincipalId,
        client_id: &ClientId,
    ) -> Result<Option<ClientAccessGrant>> {
        let row = sqlx::query_as!(
            ClientAccessGrantRow,
            "SELECT id AS \"id: ClientAccessGrantId\", \
                    principal_id AS \"principal_id: PrincipalId\", \
                    client_id AS \"client_id: ClientId\", granted_by, granted_at, \
                    created_at, updated_at \
                    FROM iam_client_access_grants WHERE principal_id = $1 AND client_id = $2",
            principal_id as &PrincipalId,
            client_id as &ClientId
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ClientAccessGrant::from))
    }

    pub async fn delete(&self, id: &ClientAccessGrantId) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM iam_client_access_grants WHERE id = $1",
            id as &ClientAccessGrantId
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_by_principal_and_client(
        &self,
        principal_id: &PrincipalId,
        client_id: &ClientId,
    ) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM iam_client_access_grants WHERE principal_id = $1 AND client_id = $2",
            principal_id as &PrincipalId,
            client_id as &ClientId
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ── Persist<ClientAccessGrant> ───────────────────────────────────────────────

impl HasId for ClientAccessGrant {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

impl Persist<ClientAccessGrant> for ClientAccessGrantRepository {
    async fn persist(&self, g: &ClientAccessGrant, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "INSERT INTO iam_client_access_grants (id, principal_id, client_id, granted_by, granted_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (id) DO UPDATE SET
                granted_by = EXCLUDED.granted_by,
                updated_at = EXCLUDED.updated_at",
            &g.id as &ClientAccessGrantId,
            &g.principal_id as &PrincipalId,
            &g.client_id as &ClientId,
            &g.granted_by,
            g.granted_at,
            g.created_at,
            g.updated_at
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, g: &ClientAccessGrant, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM iam_client_access_grants WHERE id = $1",
            &g.id as &ClientAccessGrantId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}

// ── Persist<AnchorDomain> ────────────────────────────────────────────────────

impl HasId for AnchorDomain {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

impl Persist<AnchorDomain> for AnchorDomainRepository {
    async fn persist(&self, d: &AnchorDomain, tx: &mut DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        sqlx::query!(
            "INSERT INTO tnt_anchor_domains (id, domain, created_at, updated_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE SET
                domain = EXCLUDED.domain,
                updated_at = EXCLUDED.updated_at",
            &d.id as &AnchorDomainId,
            &d.domain,
            now,
            now
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, d: &AnchorDomain, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM tnt_anchor_domains WHERE id = $1",
            &d.id as &AnchorDomainId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}

// ── Persist<ClientAuthConfig> ────────────────────────────────────────────────

impl HasId for ClientAuthConfig {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

impl Persist<ClientAuthConfig> for ClientAuthConfigRepository {
    async fn persist(&self, c: &ClientAuthConfig, tx: &mut DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        let additional_client_ids_json =
            serde_json::to_value(&c.additional_client_ids).unwrap_or_default();
        let granted_client_ids_json =
            serde_json::to_value(&c.granted_client_ids).unwrap_or_default();
        sqlx::query!(
            "INSERT INTO tnt_client_auth_configs (id, email_domain, config_type, primary_client_id, additional_client_ids, granted_client_ids, auth_provider, oidc_issuer_url, oidc_client_id, oidc_multi_tenant, oidc_issuer_pattern, oidc_client_secret_ref, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT (id) DO UPDATE SET
                email_domain = EXCLUDED.email_domain,
                config_type = EXCLUDED.config_type,
                primary_client_id = EXCLUDED.primary_client_id,
                additional_client_ids = EXCLUDED.additional_client_ids,
                granted_client_ids = EXCLUDED.granted_client_ids,
                auth_provider = EXCLUDED.auth_provider,
                oidc_issuer_url = EXCLUDED.oidc_issuer_url,
                oidc_client_id = EXCLUDED.oidc_client_id,
                oidc_multi_tenant = EXCLUDED.oidc_multi_tenant,
                oidc_issuer_pattern = EXCLUDED.oidc_issuer_pattern,
                oidc_client_secret_ref = EXCLUDED.oidc_client_secret_ref,
                updated_at = EXCLUDED.updated_at",
            &c.id as &ClientAuthConfigId,
            &c.email_domain,
            c.config_type as AuthConfigType,
            &c.primary_client_id as &Option<ClientId>,
            &additional_client_ids_json,
            &granted_client_ids_json,
            c.auth_provider as AuthProvider,
            c.oidc_issuer_url.as_ref(),
            c.oidc_client_id.as_ref(),
            c.oidc_multi_tenant,
            c.oidc_issuer_pattern.as_ref(),
            c.oidc_client_secret_ref.as_ref(),
            now,
            now
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, c: &ClientAuthConfig, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM tnt_client_auth_configs WHERE id = $1",
            &c.id as &ClientAuthConfigId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}

// ── IdpRoleMappingRepository ─────────────────────────────────────────────────

pub struct IdpRoleMappingRepository {
    pool: PgPool,
}

impl IdpRoleMappingRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, mapping: &IdpRoleMapping) -> Result<()> {
        let now = Utc::now();
        sqlx::query!(
            "INSERT INTO oauth_idp_role_mappings
                (id, idp_role_name, internal_role_name, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5)",
            &mapping.id as &IdpRoleMappingId,
            &mapping.idp_role_name,
            &mapping.platform_role_name,
            now,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &IdpRoleMappingId) -> Result<Option<IdpRoleMapping>> {
        let row = sqlx::query_as!(
            IdpRoleMappingRow,
            "SELECT id AS \"id: IdpRoleMappingId\", idp_role_name, internal_role_name, \
                    created_at, updated_at \
                    FROM oauth_idp_role_mappings WHERE id = $1",
            id as &IdpRoleMappingId
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(IdpRoleMapping::from))
    }

    pub async fn find_by_idp_role(
        &self,
        _idp_type: &str,
        idp_role_name: &str,
    ) -> Result<Option<IdpRoleMapping>> {
        let row = sqlx::query_as!(
            IdpRoleMappingRow,
            "SELECT id AS \"id: IdpRoleMappingId\", idp_role_name, internal_role_name, \
                    created_at, updated_at \
                    FROM oauth_idp_role_mappings WHERE idp_role_name = $1",
            idp_role_name
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(IdpRoleMapping::from))
    }

    pub async fn find_by_idp_type(&self, _idp_type: &str) -> Result<Vec<IdpRoleMapping>> {
        // DB doesn't have idp_type column — return all
        self.find_all().await
    }

    pub async fn find_all(&self) -> Result<Vec<IdpRoleMapping>> {
        let rows = sqlx::query_as!(
            IdpRoleMappingRow,
            "SELECT id AS \"id: IdpRoleMappingId\", idp_role_name, internal_role_name, \
                    created_at, updated_at \
                    FROM oauth_idp_role_mappings ORDER BY idp_role_name ASC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(IdpRoleMapping::from).collect())
    }

    pub async fn find_idp_role_mapping(
        &self,
        idp_role_name: &str,
    ) -> Result<Option<IdpRoleMapping>> {
        let row = sqlx::query_as!(
            IdpRoleMappingRow,
            "SELECT id AS \"id: IdpRoleMappingId\", idp_role_name, internal_role_name, \
                    created_at, updated_at \
                    FROM oauth_idp_role_mappings WHERE idp_role_name = $1",
            idp_role_name
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(IdpRoleMapping::from))
    }

    pub async fn delete(&self, id: &IdpRoleMappingId) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM oauth_idp_role_mappings WHERE id = $1",
            id as &IdpRoleMappingId
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

impl usecase::HasId for IdpRoleMapping {
    fn id(&self) -> &str {
        self.id.as_str()
    }
}

impl Persist<IdpRoleMapping> for IdpRoleMappingRepository {
    async fn persist(&self, m: &IdpRoleMapping, tx: &mut DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        sqlx::query!(
            "INSERT INTO oauth_idp_role_mappings (id, idp_role_name, internal_role_name, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (id) DO UPDATE SET
                idp_role_name = EXCLUDED.idp_role_name,
                internal_role_name = EXCLUDED.internal_role_name,
                updated_at = EXCLUDED.updated_at",
            &m.id as &IdpRoleMappingId,
            &m.idp_role_name,
            &m.platform_role_name,
            now,
            now
        )
        .execute(&mut **tx.inner).await?;
        Ok(())
    }

    async fn delete(&self, m: &IdpRoleMapping, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM oauth_idp_role_mappings WHERE id = $1",
            &m.id as &IdpRoleMappingId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}
