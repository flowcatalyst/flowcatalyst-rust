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
    require_2fa: bool,
    remember_device_enabled: bool,
    remember_device_days: i32,
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
            require_2fa: r.require_2fa,
            allowed_2fa_methods: Vec::new(), // loaded separately
            remember_device_enabled: r.remember_device_enabled,
            remember_device_days: r.remember_device_days,
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
        let (additional, granted, roles, methods) = tokio::try_join!(
            sqlx::query_scalar::<_, String>(
                "SELECT client_id FROM tnt_email_domain_mapping_additional_clients WHERE email_domain_mapping_id = $1"
            ).bind(&edm.id).fetch_all(&self.pool),
            sqlx::query_scalar::<_, String>(
                "SELECT client_id FROM tnt_email_domain_mapping_granted_clients WHERE email_domain_mapping_id = $1"
            ).bind(&edm.id).fetch_all(&self.pool),
            sqlx::query_scalar::<_, String>(
                "SELECT role_id FROM tnt_email_domain_mapping_allowed_roles WHERE email_domain_mapping_id = $1"
            ).bind(&edm.id).fetch_all(&self.pool),
            sqlx::query_scalar::<_, String>(
                "SELECT method FROM tnt_email_domain_mapping_2fa_methods WHERE email_domain_mapping_id = $1 ORDER BY id"
            ).bind(&edm.id).fetch_all(&self.pool),
        )?;
        edm.additional_client_ids = additional;
        edm.granted_client_ids = granted;
        edm.allowed_role_ids = roles;
        edm.allowed_2fa_methods = methods;
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
        #[derive(sqlx::FromRow)]
        struct MethodRow {
            email_domain_mapping_id: String,
            method: String,
        }

        let (additional_rows, granted_rows, role_rows, method_rows) = tokio::try_join!(
            sqlx::query_as::<_, ClientRow>(
                "SELECT email_domain_mapping_id, client_id FROM tnt_email_domain_mapping_additional_clients WHERE email_domain_mapping_id = ANY($1)"
            ).bind(&ids).fetch_all(&self.pool),
            sqlx::query_as::<_, ClientRow>(
                "SELECT email_domain_mapping_id, client_id FROM tnt_email_domain_mapping_granted_clients WHERE email_domain_mapping_id = ANY($1)"
            ).bind(&ids).fetch_all(&self.pool),
            sqlx::query_as::<_, RoleRow>(
                "SELECT email_domain_mapping_id, role_id FROM tnt_email_domain_mapping_allowed_roles WHERE email_domain_mapping_id = ANY($1)"
            ).bind(&ids).fetch_all(&self.pool),
            sqlx::query_as::<_, MethodRow>(
                "SELECT email_domain_mapping_id, method FROM tnt_email_domain_mapping_2fa_methods WHERE email_domain_mapping_id = ANY($1) ORDER BY id"
            ).bind(&ids).fetch_all(&self.pool),
        )?;

        let mut methods_map: HashMap<String, Vec<String>> = HashMap::new();
        for r in method_rows {
            methods_map
                .entry(r.email_domain_mapping_id)
                .or_default()
                .push(r.method);
        }

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
            if let Some(v) = methods_map.remove(&edm.id) {
                edm.allowed_2fa_methods = v;
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

    /// Whether `domain` is federated: mapped to an OIDC (external) identity
    /// provider. A mapping to an INTERNAL provider — the one Go's `fcdev
    /// init` creates for the anchor domain — is not federated.
    pub async fn is_federated_domain(&self, domain: &str) -> Result<bool> {
        let (federated,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM tnt_email_domain_mappings m \
             JOIN oauth_identity_providers p ON p.id = m.identity_provider_id \
             WHERE m.email_domain = $1 AND p.type = 'OIDC')",
        )
        .bind(domain)
        .fetch_one(&self.pool)
        .await?;
        Ok(federated)
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

    /// The mappings routed to `identity_provider_id`, by domain (Go
    /// `FindByIdentityProvider`).
    pub async fn find_by_identity_provider(
        &self,
        identity_provider_id: &str,
    ) -> Result<Vec<EmailDomainMapping>> {
        let rows = sqlx::query_as::<_, EmailDomainMappingRow>(
            "SELECT * FROM tnt_email_domain_mappings WHERE identity_provider_id = $1 \
             ORDER BY email_domain",
        )
        .bind(identity_provider_id)
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
        let mut tx = self.pool.begin().await?;
        {
            let mut db = crate::usecase::DbTx { inner: &mut tx };
            write_mapping(edm, &mut db).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn update(&self, edm: &EmailDomainMapping) -> Result<()> {
        self.insert(edm).await
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let deleted = {
            let mut db = crate::usecase::DbTx { inner: &mut tx };
            delete_mapping(id, &mut db).await?
        };
        tx.commit().await?;
        Ok(deleted)
    }
}

/// Upsert a mapping and replace its junction rows, inside the caller's
/// transaction: the one write path for `tnt_email_domain_mappings`.
async fn write_mapping(edm: &EmailDomainMapping, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO tnt_email_domain_mappings
            (id, email_domain, identity_provider_id, scope_type,
             primary_client_id, required_oidc_tenant_id, sync_roles_from_idp,
             require_2fa, remember_device_enabled, remember_device_days,
             created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW())
        ON CONFLICT (id) DO UPDATE SET
            email_domain = EXCLUDED.email_domain,
            identity_provider_id = EXCLUDED.identity_provider_id,
            scope_type = EXCLUDED.scope_type,
            primary_client_id = EXCLUDED.primary_client_id,
            required_oidc_tenant_id = EXCLUDED.required_oidc_tenant_id,
            sync_roles_from_idp = EXCLUDED.sync_roles_from_idp,
            require_2fa = EXCLUDED.require_2fa,
            remember_device_enabled = EXCLUDED.remember_device_enabled,
            remember_device_days = EXCLUDED.remember_device_days,
            updated_at = NOW()"#,
    )
    .bind(&edm.id)
    .bind(&edm.email_domain)
    .bind(&edm.identity_provider_id)
    .bind(edm.scope_type.as_str())
    .bind(&edm.primary_client_id)
    .bind(&edm.required_oidc_tenant_id)
    .bind(edm.sync_roles_from_idp)
    .bind(edm.require_2fa)
    .bind(edm.remember_device_enabled)
    .bind(edm.remember_device_days)
    .bind(edm.created_at)
    .execute(&mut **tx.inner)
    .await?;
    delete_junctions(&edm.id, tx).await?;
    for (table, column, values) in [
        (
            "tnt_email_domain_mapping_additional_clients",
            "client_id",
            &edm.additional_client_ids,
        ),
        (
            "tnt_email_domain_mapping_granted_clients",
            "client_id",
            &edm.granted_client_ids,
        ),
        (
            "tnt_email_domain_mapping_allowed_roles",
            "role_id",
            &edm.allowed_role_ids,
        ),
    ] {
        if values.is_empty() {
            continue;
        }
        sqlx::query(&format!(
            "INSERT INTO {table} (email_domain_mapping_id, {column}) \
             SELECT $1, v FROM UNNEST($2::varchar[]) AS v"
        ))
        .bind(&edm.id)
        .bind(values)
        .execute(&mut **tx.inner)
        .await?;
    }
    if !edm.allowed_2fa_methods.is_empty() {
        sqlx::query(
            "INSERT INTO tnt_email_domain_mapping_2fa_methods (email_domain_mapping_id, method) \
             SELECT $1, m FROM UNNEST($2::text[]) WITH ORDINALITY AS t(m, n) ORDER BY n",
        )
        .bind(&edm.id)
        .bind(&edm.allowed_2fa_methods)
        .execute(&mut **tx.inner)
        .await?;
    }
    Ok(())
}

async fn delete_junctions(id: &str, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
    for table in [
        "tnt_email_domain_mapping_additional_clients",
        "tnt_email_domain_mapping_granted_clients",
        "tnt_email_domain_mapping_allowed_roles",
        "tnt_email_domain_mapping_2fa_methods",
    ] {
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE email_domain_mapping_id = $1"
        ))
        .bind(id)
        .execute(&mut **tx.inner)
        .await?;
    }
    Ok(())
}

async fn delete_mapping(id: &str, tx: &mut crate::usecase::DbTx<'_>) -> Result<bool> {
    delete_junctions(id, tx).await?;
    let result = sqlx::query("DELETE FROM tnt_email_domain_mappings WHERE id = $1")
        .bind(id)
        .execute(&mut **tx.inner)
        .await?;
    Ok(result.rows_affected() > 0)
}

impl crate::usecase::unit_of_work::HasId for EmailDomainMapping {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait::async_trait]
impl crate::usecase::Persist<EmailDomainMapping> for EmailDomainMappingRepository {
    async fn persist(
        &self,
        edm: &EmailDomainMapping,
        tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        write_mapping(edm, tx).await
    }

    async fn delete(
        &self,
        edm: &EmailDomainMapping,
        tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        delete_mapping(&edm.id, tx).await.map(|_| ())
    }
}
