//! EmailDomainMapping Repository — PostgreSQL via SQLx

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;

use super::entity::EmailDomainMapping;
use crate::shared::enum_str::decode;
use crate::shared::error::{PlatformError, Result};

// ── Row structs ─────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct EmailDomainMappingRow {
    id: String,
    email_domain: String,
    identity_provider_id: String,
    scope_type: String,
    primary_client_id: Option<String>,
    required_oidc_tenant_id: Option<String>,
    sync_roles_from_idp: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<EmailDomainMappingRow> for EmailDomainMapping {
    type Error = PlatformError;
    fn try_from(r: EmailDomainMappingRow) -> Result<Self> {
        let scope_type = decode(
            &r.scope_type,
            "tnt_email_domain_mappings",
            "scope_type",
            &r.id,
        )?;
        Ok(Self {
            id: r.id,
            email_domain: r.email_domain,
            identity_provider_id: r.identity_provider_id,
            scope_type,
            primary_client_id: r.primary_client_id,
            additional_client_ids: Vec::new(), // loaded separately
            granted_client_ids: Vec::new(),    // loaded separately
            required_oidc_tenant_id: r.required_oidc_tenant_id,
            allowed_role_ids: Vec::new(), // loaded separately
            sync_roles_from_idp: r.sync_roles_from_idp,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

pub struct EmailDomainMappingRepository {
    pool: PgPool,
}

impl EmailDomainMappingRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    async fn hydrate(&self, mut edm: EmailDomainMapping) -> Result<EmailDomainMapping> {
        let (additional, granted, roles) = tokio::try_join!(
            sqlx::query_scalar::<_, String>(
                "SELECT client_id FROM tnt_email_domain_mapping_additional_clients WHERE email_domain_mapping_id = $1"
            ).bind(&edm.id).fetch_all(&self.pool),
            sqlx::query_scalar::<_, String>(
                "SELECT client_id FROM tnt_email_domain_mapping_granted_clients WHERE email_domain_mapping_id = $1"
            ).bind(&edm.id).fetch_all(&self.pool),
            sqlx::query_scalar::<_, String>(
                "SELECT role_id FROM tnt_email_domain_mapping_allowed_roles WHERE email_domain_mapping_id = $1"
            ).bind(&edm.id).fetch_all(&self.pool),
        )?;
        edm.additional_client_ids = additional;
        edm.granted_client_ids = granted;
        edm.allowed_role_ids = roles;
        Ok(edm)
    }

    /// Batch-hydrate junction tables for multiple email domain mappings (avoids N+1)
    async fn hydrate_all(
        &self,
        mut edms: Vec<EmailDomainMapping>,
    ) -> Result<Vec<EmailDomainMapping>> {
        if edms.is_empty() {
            return Ok(edms);
        }

        let ids: Vec<&str> = edms.iter().map(|e| e.id.as_str()).collect();

        #[derive(sqlx::FromRow)]
        struct ClientRow {
            email_domain_mapping_id: String,
            client_id: String,
        }
        #[derive(sqlx::FromRow)]
        struct RoleRow {
            email_domain_mapping_id: String,
            role_id: String,
        }

        let (additional_rows, granted_rows, role_rows) = tokio::try_join!(
            sqlx::query_as::<_, ClientRow>(
                "SELECT email_domain_mapping_id, client_id FROM tnt_email_domain_mapping_additional_clients WHERE email_domain_mapping_id = ANY($1)"
            ).bind(&ids).fetch_all(&self.pool),
            sqlx::query_as::<_, ClientRow>(
                "SELECT email_domain_mapping_id, client_id FROM tnt_email_domain_mapping_granted_clients WHERE email_domain_mapping_id = ANY($1)"
            ).bind(&ids).fetch_all(&self.pool),
            sqlx::query_as::<_, RoleRow>(
                "SELECT email_domain_mapping_id, role_id FROM tnt_email_domain_mapping_allowed_roles WHERE email_domain_mapping_id = ANY($1)"
            ).bind(&ids).fetch_all(&self.pool),
        )?;

        let mut additional_map: HashMap<String, Vec<String>> = HashMap::new();
        for r in additional_rows {
            additional_map
                .entry(r.email_domain_mapping_id)
                .or_default()
                .push(r.client_id);
        }

        let mut granted_map: HashMap<String, Vec<String>> = HashMap::new();
        for r in granted_rows {
            granted_map
                .entry(r.email_domain_mapping_id)
                .or_default()
                .push(r.client_id);
        }

        let mut roles_map: HashMap<String, Vec<String>> = HashMap::new();
        for r in role_rows {
            roles_map
                .entry(r.email_domain_mapping_id)
                .or_default()
                .push(r.role_id);
        }

        for edm in &mut edms {
            if let Some(v) = additional_map.remove(&edm.id) {
                edm.additional_client_ids = v;
            }
            if let Some(v) = granted_map.remove(&edm.id) {
                edm.granted_client_ids = v;
            }
            if let Some(v) = roles_map.remove(&edm.id) {
                edm.allowed_role_ids = v;
            }
        }

        Ok(edms)
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<EmailDomainMapping>> {
        let row = sqlx::query_as::<_, EmailDomainMappingRow>(
            "SELECT * FROM tnt_email_domain_mappings WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok(Some(self.hydrate(EmailDomainMapping::try_from(r)?).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_email_domain(&self, domain: &str) -> Result<Option<EmailDomainMapping>> {
        let row = sqlx::query_as::<_, EmailDomainMappingRow>(
            "SELECT * FROM tnt_email_domain_mappings WHERE email_domain = $1",
        )
        .bind(domain)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok(Some(self.hydrate(EmailDomainMapping::try_from(r)?).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_all(&self) -> Result<Vec<EmailDomainMapping>> {
        let rows = sqlx::query_as::<_, EmailDomainMappingRow>(
            "SELECT * FROM tnt_email_domain_mappings ORDER BY email_domain",
        )
        .fetch_all(&self.pool)
        .await?;
        let edms: Vec<EmailDomainMapping> = rows
            .into_iter()
            .map(EmailDomainMapping::try_from)
            .collect::<Result<_>>()?;
        self.hydrate_all(edms).await
    }

    /// The domains routed to `identity_provider_id` whose mapping pins no
    /// OIDC tenant (a null or blank `required_oidc_tenant_id`), sorted.
    pub async fn find_unpinned_domains_for_identity_provider(
        &self,
        identity_provider_id: &str,
    ) -> Result<Vec<String>> {
        let domains = sqlx::query_scalar::<_, String>(
            "SELECT email_domain FROM tnt_email_domain_mappings
             WHERE identity_provider_id = $1
               AND coalesce(btrim(required_oidc_tenant_id), '') = ''
             ORDER BY email_domain",
        )
        .bind(identity_provider_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(domains)
    }

    pub async fn insert(&self, edm: &EmailDomainMapping) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO tnt_email_domain_mappings
                (id, email_domain, identity_provider_id, scope_type,
                 primary_client_id, required_oidc_tenant_id, sync_roles_from_idp,
                 created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())"#,
        )
        .bind(&edm.id)
        .bind(&edm.email_domain)
        .bind(&edm.identity_provider_id)
        .bind(edm.scope_type.as_str())
        .bind(&edm.primary_client_id)
        .bind(&edm.required_oidc_tenant_id)
        .bind(edm.sync_roles_from_idp)
        .execute(&self.pool)
        .await?;
        self.save_junctions(&edm.id, edm).await?;
        Ok(())
    }

    pub async fn update(&self, edm: &EmailDomainMapping) -> Result<()> {
        sqlx::query(
            r#"UPDATE tnt_email_domain_mappings SET
                email_domain = $2, identity_provider_id = $3, scope_type = $4,
                primary_client_id = $5, required_oidc_tenant_id = $6,
                sync_roles_from_idp = $7, updated_at = NOW()
            WHERE id = $1"#,
        )
        .bind(&edm.id)
        .bind(&edm.email_domain)
        .bind(&edm.identity_provider_id)
        .bind(edm.scope_type.as_str())
        .bind(&edm.primary_client_id)
        .bind(&edm.required_oidc_tenant_id)
        .bind(edm.sync_roles_from_idp)
        .execute(&self.pool)
        .await?;
        self.delete_junctions(&edm.id).await?;
        self.save_junctions(&edm.id, edm).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        self.delete_junctions(id).await?;
        let result = sqlx::query("DELETE FROM tnt_email_domain_mappings WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn save_junctions(&self, id: &str, edm: &EmailDomainMapping) -> Result<()> {
        for cid in &edm.additional_client_ids {
            sqlx::query(
                "INSERT INTO tnt_email_domain_mapping_additional_clients (email_domain_mapping_id, client_id) VALUES ($1, $2)"
            )
            .bind(id)
            .bind(cid)
            .execute(&self.pool)
            .await?;
        }
        for cid in &edm.granted_client_ids {
            sqlx::query(
                "INSERT INTO tnt_email_domain_mapping_granted_clients (email_domain_mapping_id, client_id) VALUES ($1, $2)"
            )
            .bind(id)
            .bind(cid)
            .execute(&self.pool)
            .await?;
        }
        for rid in &edm.allowed_role_ids {
            sqlx::query(
                "INSERT INTO tnt_email_domain_mapping_allowed_roles (email_domain_mapping_id, role_id) VALUES ($1, $2)"
            )
            .bind(id)
            .bind(rid)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn delete_junctions(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM tnt_email_domain_mapping_additional_clients WHERE email_domain_mapping_id = $1")
            .bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM tnt_email_domain_mapping_granted_clients WHERE email_domain_mapping_id = $1")
            .bind(id).execute(&self.pool).await?;
        sqlx::query(
            "DELETE FROM tnt_email_domain_mapping_allowed_roles WHERE email_domain_mapping_id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
