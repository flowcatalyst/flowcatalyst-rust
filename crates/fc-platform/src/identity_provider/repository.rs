//! IdentityProvider Repository — PostgreSQL via SQLx

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;

use super::entity::IdentityProvider;
use crate::shared::enum_str::decode;
use crate::shared::error::{PlatformError, Result};

// ── Row structs ─────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct IdentityProviderRow {
    id: String,
    code: String,
    name: String,
    r#type: String,
    oidc_issuer_url: Option<String>,
    oidc_client_id: Option<String>,
    oidc_client_secret_ref: Option<String>,
    oidc_multi_tenant: bool,
    oidc_issuer_pattern: Option<String>,
    /// Go's 040 (this platform's 055).
    #[sqlx(default)]
    sync_roles_from_idp: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<IdentityProviderRow> for IdentityProvider {
    type Error = PlatformError;
    fn try_from(r: IdentityProviderRow) -> Result<Self> {
        let r#type = decode(&r.r#type, "oauth_identity_providers", "type", &r.id)?;
        Ok(Self {
            id: r.id,
            code: r.code,
            name: r.name,
            r#type,
            oidc_issuer_url: r.oidc_issuer_url,
            oidc_client_id: r.oidc_client_id,
            oidc_client_secret_ref: r.oidc_client_secret_ref,
            oidc_multi_tenant: r.oidc_multi_tenant,
            oidc_issuer_pattern: r.oidc_issuer_pattern,
            allowed_email_domains: Vec::new(), // hydrated
            sync_roles_from_idp: r.sync_roles_from_idp,
            allowed_role_ids: Vec::new(), // hydrated
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

impl crate::usecase::unit_of_work::HasId for IdentityProvider {
    fn id(&self) -> &str {
        &self.id
    }
}

/// Go's identity-provider repository (identityprovider/repository.go):
/// `oauth_identity_providers` plus the allowed-roles junction. The routed
/// email domains are read from `tnt_email_domain_mappings` (the one source
/// of domain → provider routing); the legacy
/// `oauth_identity_provider_allowed_domains` junction is no longer read or
/// written, only cleared when a provider is deleted.
pub struct IdentityProviderRepository {
    pool: PgPool,
}

impl IdentityProviderRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    async fn hydrate(&self, idp: IdentityProvider) -> Result<IdentityProvider> {
        let mut all = self.hydrate_all(vec![idp]).await?;
        Ok(all.remove(0))
    }

    /// Batch-hydrate the mapped domains and the allowed roles (two queries,
    /// whatever the number of providers).
    async fn hydrate_all(&self, mut idps: Vec<IdentityProvider>) -> Result<Vec<IdentityProvider>> {
        if idps.is_empty() {
            return Ok(idps);
        }
        let ids: Vec<&str> = idps.iter().map(|i| i.id.as_str()).collect();

        let domains = sqlx::query_as::<_, (String, String)>(
            "SELECT identity_provider_id, email_domain FROM tnt_email_domain_mappings \
             WHERE identity_provider_id = ANY($1) ORDER BY identity_provider_id, email_domain",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;
        let roles = sqlx::query_as::<_, (String, String)>(
            "SELECT identity_provider_id, role_id FROM oauth_identity_provider_allowed_roles \
             WHERE identity_provider_id = ANY($1) ORDER BY identity_provider_id, role_id",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;

        let mut domain_map: HashMap<String, Vec<String>> = HashMap::new();
        for (idp_id, domain) in domains {
            domain_map.entry(idp_id).or_default().push(domain);
        }
        let mut role_map: HashMap<String, Vec<String>> = HashMap::new();
        for (idp_id, role_id) in roles {
            role_map.entry(idp_id).or_default().push(role_id);
        }
        for idp in &mut idps {
            idp.allowed_email_domains = domain_map.remove(&idp.id).unwrap_or_default();
            idp.allowed_role_ids = role_map.remove(&idp.id).unwrap_or_default();
        }
        Ok(idps)
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<IdentityProvider>> {
        let row = sqlx::query_as::<_, IdentityProviderRow>(
            "SELECT * FROM oauth_identity_providers WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok(Some(self.hydrate(IdentityProvider::try_from(r)?).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<IdentityProvider>> {
        let row = sqlx::query_as::<_, IdentityProviderRow>(
            "SELECT * FROM oauth_identity_providers WHERE code = $1",
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok(Some(self.hydrate(IdentityProvider::try_from(r)?).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_all(&self) -> Result<Vec<IdentityProvider>> {
        let rows = sqlx::query_as::<_, IdentityProviderRow>(
            "SELECT * FROM oauth_identity_providers ORDER BY code",
        )
        .fetch_all(&self.pool)
        .await?;
        let idps: Vec<IdentityProvider> = rows
            .into_iter()
            .map(IdentityProvider::try_from)
            .collect::<Result<_>>()?;
        self.hydrate_all(idps).await
    }

    /// `id → name` for the given providers, in one query (the mapping
    /// responses' `identityProviderName`).
    pub async fn find_names_by_ids(&self, ids: &[String]) -> Result<HashMap<String, String>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT id, name FROM oauth_identity_providers WHERE id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }

    /// The names of the roles a provider's allow-list names (Go resolves the
    /// stored role ids to names at login, skipping a role that no longer
    /// exists).
    pub async fn allowed_role_names(&self, role_ids: &[String]) -> Result<Vec<String>> {
        if role_ids.is_empty() {
            return Ok(Vec::new());
        }
        let names = sqlx::query_scalar::<_, String>(
            "SELECT name FROM iam_roles WHERE id = ANY($1) ORDER BY name",
        )
        .bind(role_ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(names)
    }

    /// Insert outside a unit of work (the startup seed of the internal
    /// provider; tests).
    pub async fn insert(&self, idp: &IdentityProvider) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        {
            let mut db = crate::usecase::DbTx { inner: &mut tx };
            write_provider(idp, &mut db).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Update outside a unit of work.
    pub async fn update(&self, idp: &IdentityProvider) -> Result<()> {
        self.insert(idp).await
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let deleted = {
            let mut db = crate::usecase::DbTx { inner: &mut tx };
            delete_provider(id, &mut db).await?
        };
        tx.commit().await?;
        Ok(deleted)
    }
}

/// Upsert the provider row and replace its allowed roles (Go `Persist`).
/// The routed domains are the mappings', never written here.
async fn write_provider(idp: &IdentityProvider, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO oauth_identity_providers
            (id, code, name, type, oidc_issuer_url, oidc_client_id,
             oidc_client_secret_ref, oidc_multi_tenant, oidc_issuer_pattern,
             sync_roles_from_idp, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW())
        ON CONFLICT (id) DO UPDATE SET
            code = EXCLUDED.code, name = EXCLUDED.name, type = EXCLUDED.type,
            oidc_issuer_url = EXCLUDED.oidc_issuer_url,
            oidc_client_id = EXCLUDED.oidc_client_id,
            oidc_client_secret_ref = EXCLUDED.oidc_client_secret_ref,
            oidc_multi_tenant = EXCLUDED.oidc_multi_tenant,
            oidc_issuer_pattern = EXCLUDED.oidc_issuer_pattern,
            sync_roles_from_idp = EXCLUDED.sync_roles_from_idp,
            updated_at = NOW()"#,
    )
    .bind(&idp.id)
    .bind(&idp.code)
    .bind(&idp.name)
    .bind(idp.r#type.as_str())
    .bind(&idp.oidc_issuer_url)
    .bind(&idp.oidc_client_id)
    .bind(&idp.oidc_client_secret_ref)
    .bind(idp.oidc_multi_tenant)
    .bind(&idp.oidc_issuer_pattern)
    .bind(idp.sync_roles_from_idp)
    .bind(idp.created_at)
    .execute(&mut **tx.inner)
    .await?;
    sqlx::query(
        "DELETE FROM oauth_identity_provider_allowed_roles WHERE identity_provider_id = $1",
    )
    .bind(&idp.id)
    .execute(&mut **tx.inner)
    .await?;
    if !idp.allowed_role_ids.is_empty() {
        sqlx::query(
            "INSERT INTO oauth_identity_provider_allowed_roles (identity_provider_id, role_id) \
             SELECT $1, r FROM UNNEST($2::varchar[]) AS r",
        )
        .bind(&idp.id)
        .bind(&idp.allowed_role_ids)
        .execute(&mut **tx.inner)
        .await?;
    }
    Ok(())
}

/// Delete the provider and clear its junctions (the allowed roles, and the
/// legacy allowed-domains rows so an old install keeps no orphans).
async fn delete_provider(id: &str, tx: &mut crate::usecase::DbTx<'_>) -> Result<bool> {
    sqlx::query(
        "DELETE FROM oauth_identity_provider_allowed_domains WHERE identity_provider_id = $1",
    )
    .bind(id)
    .execute(&mut **tx.inner)
    .await?;
    sqlx::query(
        "DELETE FROM oauth_identity_provider_allowed_roles WHERE identity_provider_id = $1",
    )
    .bind(id)
    .execute(&mut **tx.inner)
    .await?;
    let result = sqlx::query("DELETE FROM oauth_identity_providers WHERE id = $1")
        .bind(id)
        .execute(&mut **tx.inner)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[async_trait::async_trait]
impl crate::usecase::Persist<IdentityProvider> for IdentityProviderRepository {
    async fn persist(
        &self,
        idp: &IdentityProvider,
        tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        write_provider(idp, tx).await
    }

    async fn delete(
        &self,
        idp: &IdentityProvider,
        tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        delete_provider(&idp.id, tx).await.map(|_| ())
    }
}
