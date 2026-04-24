//! Authentication Configuration Repositories — PostgreSQL via SQLx

use async_trait::async_trait;
use sqlx::PgPool;
use chrono::{DateTime, Utc};

use crate::auth::config_entity::{AnchorDomain, AuthConfigType, AuthProvider, ClientAuthConfig, IdpRoleMapping};
use crate::principal::entity::ClientAccessGrant;
use crate::shared::error::Result;
use crate::usecase::unit_of_work::HasId;

// ── Row types ────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct AnchorDomainRow {
    id: String,
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

#[derive(sqlx::FromRow)]
struct ClientAuthConfigRow {
    id: String,
    email_domain: String,
    config_type: String,
    primary_client_id: Option<String>,
    additional_client_ids: serde_json::Value,
    granted_client_ids: serde_json::Value,
    auth_provider: String,
    oidc_issuer_url: Option<String>,
    oidc_client_id: Option<String>,
    oidc_multi_tenant: bool,
    oidc_issuer_pattern: Option<String>,
    oidc_client_secret_ref: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ClientAuthConfigRow> for ClientAuthConfig {
    fn from(r: ClientAuthConfigRow) -> Self {
        let additional_client_ids: Vec<String> =
            serde_json::from_value(r.additional_client_ids).unwrap_or_default();
        let granted_client_ids: Vec<String> =
            serde_json::from_value(r.granted_client_ids).unwrap_or_default();
        Self {
            id: r.id,
            email_domain: r.email_domain,
            config_type: AuthConfigType::from_str(&r.config_type),
            primary_client_id: r.primary_client_id,
            additional_client_ids,
            granted_client_ids,
            auth_provider: AuthProvider::from_str(&r.auth_provider),
            oidc_issuer_url: r.oidc_issuer_url,
            oidc_client_id: r.oidc_client_id,
            oidc_multi_tenant: r.oidc_multi_tenant,
            oidc_issuer_pattern: r.oidc_issuer_pattern,
            oidc_client_secret_ref: r.oidc_client_secret_ref,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ClientAccessGrantRow {
    id: String,
    principal_id: String,
    client_id: String,
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

#[derive(sqlx::FromRow)]
struct IdpRoleMappingRow {
    id: String,
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
        sqlx::query(
            "INSERT INTO tnt_anchor_domains (id, domain, created_at, updated_at)
             VALUES ($1, $2, $3, $4)"
        )
        .bind(&domain.id)
        .bind(&domain.domain)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<AnchorDomain>> {
        let row = sqlx::query_as::<_, AnchorDomainRow>(
            "SELECT * FROM tnt_anchor_domains WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(AnchorDomain::from))
    }

    pub async fn find_by_domain(&self, domain: &str) -> Result<Option<AnchorDomain>> {
        let row = sqlx::query_as::<_, AnchorDomainRow>(
            "SELECT * FROM tnt_anchor_domains WHERE domain = $1"
        )
        .bind(domain.to_lowercase())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(AnchorDomain::from))
    }

    pub async fn find_all(&self) -> Result<Vec<AnchorDomain>> {
        let rows = sqlx::query_as::<_, AnchorDomainRow>(
            "SELECT * FROM tnt_anchor_domains ORDER BY domain ASC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(AnchorDomain::from).collect())
    }

    pub async fn is_anchor_domain(&self, domain: &str) -> Result<bool> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM tnt_anchor_domains WHERE domain = $1"
        )
        .bind(domain.to_lowercase())
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }

    pub async fn update(&self, domain: &AnchorDomain) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE tnt_anchor_domains SET domain = $2, updated_at = $3 WHERE id = $1"
        )
        .bind(&domain.id)
        .bind(&domain.domain)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM tnt_anchor_domains WHERE id = $1")
            .bind(id)
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
        let additional_ids_json = serde_json::to_value(&config.additional_client_ids).unwrap_or_default();
        let granted_ids_json = serde_json::to_value(&config.granted_client_ids).unwrap_or_default();

        sqlx::query(
            "INSERT INTO tnt_client_auth_configs
                (id, email_domain, config_type, primary_client_id, additional_client_ids,
                 granted_client_ids, auth_provider, oidc_issuer_url, oidc_client_id,
                 oidc_multi_tenant, oidc_issuer_pattern, oidc_client_secret_ref,
                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"
        )
        .bind(&config.id)
        .bind(&config.email_domain)
        .bind(config.config_type.as_str())
        .bind(&config.primary_client_id)
        .bind(&additional_ids_json)
        .bind(&granted_ids_json)
        .bind(config.auth_provider.as_str())
        .bind(&config.oidc_issuer_url)
        .bind(&config.oidc_client_id)
        .bind(config.oidc_multi_tenant)
        .bind(&config.oidc_issuer_pattern)
        .bind(&config.oidc_client_secret_ref)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<ClientAuthConfig>> {
        let row = sqlx::query_as::<_, ClientAuthConfigRow>(
            "SELECT * FROM tnt_client_auth_configs WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ClientAuthConfig::from))
    }

    pub async fn find_by_email_domain(&self, domain: &str) -> Result<Option<ClientAuthConfig>> {
        let row = sqlx::query_as::<_, ClientAuthConfigRow>(
            "SELECT * FROM tnt_client_auth_configs WHERE email_domain = $1"
        )
        .bind(domain.to_lowercase())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ClientAuthConfig::from))
    }

    pub async fn find_by_client_id(&self, client_id: &str) -> Result<Vec<ClientAuthConfig>> {
        let rows = sqlx::query_as::<_, ClientAuthConfigRow>(
            "SELECT * FROM tnt_client_auth_configs WHERE primary_client_id = $1"
        )
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(ClientAuthConfig::from).collect())
    }

    pub async fn find_all(&self) -> Result<Vec<ClientAuthConfig>> {
        let rows = sqlx::query_as::<_, ClientAuthConfigRow>(
            "SELECT * FROM tnt_client_auth_configs ORDER BY email_domain ASC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(ClientAuthConfig::from).collect())
    }

    pub async fn update(&self, config: &ClientAuthConfig) -> Result<()> {
        let now = Utc::now();
        let additional_ids_json = serde_json::to_value(&config.additional_client_ids).unwrap_or_default();
        let granted_ids_json = serde_json::to_value(&config.granted_client_ids).unwrap_or_default();

        sqlx::query(
            "UPDATE tnt_client_auth_configs SET
                email_domain = $2, config_type = $3, primary_client_id = $4,
                additional_client_ids = $5, granted_client_ids = $6,
                auth_provider = $7, oidc_issuer_url = $8, oidc_client_id = $9,
                oidc_multi_tenant = $10, oidc_issuer_pattern = $11,
                oidc_client_secret_ref = $12, updated_at = $13
             WHERE id = $1"
        )
        .bind(&config.id)
        .bind(&config.email_domain)
        .bind(config.config_type.as_str())
        .bind(&config.primary_client_id)
        .bind(&additional_ids_json)
        .bind(&granted_ids_json)
        .bind(config.auth_provider.as_str())
        .bind(&config.oidc_issuer_url)
        .bind(&config.oidc_client_id)
        .bind(config.oidc_multi_tenant)
        .bind(&config.oidc_issuer_pattern)
        .bind(&config.oidc_client_secret_ref)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM tnt_client_auth_configs WHERE id = $1")
            .bind(id)
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
        sqlx::query(
            "INSERT INTO iam_client_access_grants
                (id, principal_id, client_id, granted_by, granted_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)"
        )
        .bind(&grant.id)
        .bind(&grant.principal_id)
        .bind(&grant.client_id)
        .bind(&grant.granted_by)
        .bind(grant.granted_at)
        .bind(grant.created_at)
        .bind(grant.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<ClientAccessGrant>> {
        let row = sqlx::query_as::<_, ClientAccessGrantRow>(
            "SELECT * FROM iam_client_access_grants WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ClientAccessGrant::from))
    }

    pub async fn find_by_principal(&self, principal_id: &str) -> Result<Vec<ClientAccessGrant>> {
        let rows = sqlx::query_as::<_, ClientAccessGrantRow>(
            "SELECT * FROM iam_client_access_grants WHERE principal_id = $1"
        )
        .bind(principal_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(ClientAccessGrant::from).collect())
    }

    pub async fn find_by_client(&self, client_id: &str) -> Result<Vec<ClientAccessGrant>> {
        let rows = sqlx::query_as::<_, ClientAccessGrantRow>(
            "SELECT * FROM iam_client_access_grants WHERE client_id = $1"
        )
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(ClientAccessGrant::from).collect())
    }

    pub async fn find_by_principal_and_client(&self, principal_id: &str, client_id: &str) -> Result<Option<ClientAccessGrant>> {
        let row = sqlx::query_as::<_, ClientAccessGrantRow>(
            "SELECT * FROM iam_client_access_grants WHERE principal_id = $1 AND client_id = $2"
        )
        .bind(principal_id)
        .bind(client_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ClientAccessGrant::from))
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM iam_client_access_grants WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_by_principal_and_client(&self, principal_id: &str, client_id: &str) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM iam_client_access_grants WHERE principal_id = $1 AND client_id = $2"
        )
        .bind(principal_id)
        .bind(client_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ── Persist<ClientAccessGrant> ───────────────────────────────────────────────

impl HasId for ClientAccessGrant {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl crate::usecase::Persist<ClientAccessGrant> for ClientAccessGrantRepository {
    async fn persist(&self, g: &ClientAccessGrant, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO iam_client_access_grants (id, principal_id, client_id, granted_by, granted_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (id) DO UPDATE SET
                granted_by = EXCLUDED.granted_by,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&g.id)
        .bind(&g.principal_id)
        .bind(&g.client_id)
        .bind(&g.granted_by)
        .bind(g.granted_at)
        .bind(g.created_at)
        .bind(g.updated_at)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, g: &ClientAccessGrant, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM iam_client_access_grants WHERE id = $1")
            .bind(&g.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

// ── Persist<AnchorDomain> ────────────────────────────────────────────────────

impl HasId for AnchorDomain {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl crate::usecase::Persist<AnchorDomain> for AnchorDomainRepository {
    async fn persist(&self, d: &AnchorDomain, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO tnt_anchor_domains (id, domain, created_at, updated_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE SET
                domain = EXCLUDED.domain,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&d.id)
        .bind(&d.domain)
        .bind(now)
        .bind(now)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, d: &AnchorDomain, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM tnt_anchor_domains WHERE id = $1")
            .bind(&d.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

// ── Persist<ClientAuthConfig> ────────────────────────────────────────────────

impl HasId for ClientAuthConfig {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl crate::usecase::Persist<ClientAuthConfig> for ClientAuthConfigRepository {
    async fn persist(&self, c: &ClientAuthConfig, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        let additional_client_ids_json = serde_json::to_value(&c.additional_client_ids).unwrap_or_default();
        let granted_client_ids_json = serde_json::to_value(&c.granted_client_ids).unwrap_or_default();
        sqlx::query(
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
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&c.id)
        .bind(&c.email_domain)
        .bind(c.config_type.as_str())
        .bind(&c.primary_client_id)
        .bind(&additional_client_ids_json)
        .bind(&granted_client_ids_json)
        .bind(c.auth_provider.as_str())
        .bind(&c.oidc_issuer_url)
        .bind(&c.oidc_client_id)
        .bind(c.oidc_multi_tenant)
        .bind(&c.oidc_issuer_pattern)
        .bind(&c.oidc_client_secret_ref)
        .bind(now)
        .bind(now)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(&self, c: &ClientAuthConfig, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM tnt_client_auth_configs WHERE id = $1")
            .bind(&c.id)
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
        sqlx::query(
            "INSERT INTO oauth_idp_role_mappings
                (id, idp_role_name, internal_role_name, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(&mapping.id)
        .bind(&mapping.idp_role_name)
        .bind(&mapping.platform_role_name)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<IdpRoleMapping>> {
        let row = sqlx::query_as::<_, IdpRoleMappingRow>(
            "SELECT * FROM oauth_idp_role_mappings WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(IdpRoleMapping::from))
    }

    pub async fn find_by_idp_role(&self, _idp_type: &str, idp_role_name: &str) -> Result<Option<IdpRoleMapping>> {
        let row = sqlx::query_as::<_, IdpRoleMappingRow>(
            "SELECT * FROM oauth_idp_role_mappings WHERE idp_role_name = $1"
        )
        .bind(idp_role_name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(IdpRoleMapping::from))
    }

    pub async fn find_by_idp_type(&self, _idp_type: &str) -> Result<Vec<IdpRoleMapping>> {
        // DB doesn't have idp_type column — return all
        self.find_all().await
    }

    pub async fn find_all(&self) -> Result<Vec<IdpRoleMapping>> {
        let rows = sqlx::query_as::<_, IdpRoleMappingRow>(
            "SELECT * FROM oauth_idp_role_mappings ORDER BY idp_role_name ASC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(IdpRoleMapping::from).collect())
    }

    pub async fn find_idp_role_mapping(&self, idp_role_name: &str) -> Result<Option<IdpRoleMapping>> {
        let row = sqlx::query_as::<_, IdpRoleMappingRow>(
            "SELECT * FROM oauth_idp_role_mappings WHERE idp_role_name = $1"
        )
        .bind(idp_role_name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(IdpRoleMapping::from))
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM oauth_idp_role_mappings WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

impl crate::usecase::HasId for IdpRoleMapping {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl crate::usecase::Persist<IdpRoleMapping> for IdpRoleMappingRepository {
    async fn persist(&self, m: &IdpRoleMapping, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO oauth_idp_role_mappings (id, idp_role_name, internal_role_name, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (id) DO UPDATE SET
                idp_role_name = EXCLUDED.idp_role_name,
                internal_role_name = EXCLUDED.internal_role_name,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&m.id)
        .bind(&m.idp_role_name)
        .bind(&m.platform_role_name)
        .bind(now)
        .bind(now)
        .execute(&mut **tx.inner).await?;
        Ok(())
    }

    async fn delete(&self, m: &IdpRoleMapping, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM oauth_idp_role_mappings WHERE id = $1")
            .bind(&m.id)
            .execute(&mut **tx.inner).await?;
        Ok(())
    }
}
