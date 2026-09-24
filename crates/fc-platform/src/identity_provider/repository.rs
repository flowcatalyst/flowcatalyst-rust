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
            allowed_email_domains: Vec::new(), // loaded separately
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

pub struct IdentityProviderRepository {
    pool: PgPool,
}

impl IdentityProviderRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    async fn hydrate(&self, mut idp: IdentityProvider) -> Result<IdentityProvider> {
        idp.allowed_email_domains = sqlx::query_scalar::<_, String>(
            "SELECT email_domain FROM oauth_identity_provider_allowed_domains WHERE identity_provider_id = $1"
        )
        .bind(&idp.id)
        .fetch_all(&self.pool)
        .await?;
        Ok(idp)
    }

    /// Batch-hydrate allowed domains for multiple identity providers (avoids N+1)
    async fn hydrate_all(&self, mut idps: Vec<IdentityProvider>) -> Result<Vec<IdentityProvider>> {
        if idps.is_empty() {
            return Ok(idps);
        }

        let ids: Vec<&str> = idps.iter().map(|i| i.id.as_str()).collect();

        #[derive(sqlx::FromRow)]
        struct DomainRow {
            identity_provider_id: String,
            email_domain: String,
        }

        let all_domains = sqlx::query_as::<_, DomainRow>(
            "SELECT identity_provider_id, email_domain FROM oauth_identity_provider_allowed_domains WHERE identity_provider_id = ANY($1)"
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;

        let mut domain_map: HashMap<String, Vec<String>> = HashMap::new();
        for d in all_domains {
            domain_map
                .entry(d.identity_provider_id)
                .or_default()
                .push(d.email_domain);
        }

        for idp in &mut idps {
            if let Some(domains) = domain_map.remove(&idp.id) {
                idp.allowed_email_domains = domains;
            }
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

    pub async fn insert(&self, idp: &IdentityProvider) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO oauth_identity_providers
                (id, code, name, type, oidc_issuer_url, oidc_client_id,
                 oidc_client_secret_ref, oidc_multi_tenant, oidc_issuer_pattern,
                 created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())"#,
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
        .execute(&self.pool)
        .await?;
        self.save_allowed_domains(&idp.id, &idp.allowed_email_domains)
            .await?;
        Ok(())
    }

    pub async fn update(&self, idp: &IdentityProvider) -> Result<()> {
        sqlx::query(
            r#"UPDATE oauth_identity_providers SET
                code = $2, name = $3, type = $4, oidc_issuer_url = $5,
                oidc_client_id = $6, oidc_client_secret_ref = $7,
                oidc_multi_tenant = $8, oidc_issuer_pattern = $9,
                updated_at = NOW()
            WHERE id = $1"#,
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
        .execute(&self.pool)
        .await?;
        // Replace allowed domains
        self.delete_allowed_domains(&idp.id).await?;
        self.save_allowed_domains(&idp.id, &idp.allowed_email_domains)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        self.delete_allowed_domains(id).await?;
        let result = sqlx::query("DELETE FROM oauth_identity_providers WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn save_allowed_domains(&self, idp_id: &str, domains: &[String]) -> Result<()> {
        for domain in domains {
            sqlx::query(
                "INSERT INTO oauth_identity_provider_allowed_domains (identity_provider_id, email_domain) VALUES ($1, $2)"
            )
            .bind(idp_id)
            .bind(domain)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn delete_allowed_domains(&self, idp_id: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM oauth_identity_provider_allowed_domains WHERE identity_provider_id = $1",
        )
        .bind(idp_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
