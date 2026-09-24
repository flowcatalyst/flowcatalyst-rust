//! ServiceAccount Repository
//!
//! PostgreSQL persistence for ServiceAccount entities using SQLx.
//! Queries through iam_principals (type=SERVICE) as the source of truth,
//! hydrating webhook credentials from iam_service_accounts.
//! This matches the TypeScript implementation.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::principal::entity::UserScope;
use crate::service_account::entity::{RoleAssignment, WebhookAuthType, WebhookCredentials};
use crate::shared::enum_str::decode_opt;
use crate::shared::error::{PlatformError, Result};
use crate::usecase::unit_of_work::HasId;
use crate::ServiceAccount;

/// Row mapping for iam_principals table (SERVICE type rows)
#[derive(sqlx::FromRow, Clone)]
struct PrincipalRow {
    id: String,
    #[sqlx(rename = "type")]
    #[allow(dead_code)]
    principal_type: String,
    scope: Option<String>,
    client_id: Option<String>,
    application_id: Option<String>,
    name: String,
    active: bool,
    service_account_id: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Row mapping for iam_service_accounts table (webhook credentials side)
#[derive(sqlx::FromRow, Clone)]
struct ServiceAccountRow {
    id: String,
    code: String,
    #[allow(dead_code)]
    name: String,
    description: Option<String>,
    #[allow(dead_code)]
    application_id: Option<String>,
    #[allow(dead_code)]
    active: bool,
    wh_auth_type: Option<String>,
    wh_auth_token_ref: Option<String>,
    wh_signing_secret_ref: Option<String>,
    wh_signing_algorithm: Option<String>,
    last_used_at: Option<DateTime<Utc>>,
    #[allow(dead_code)]
    created_at: DateTime<Utc>,
    #[allow(dead_code)]
    updated_at: DateTime<Utc>,
}

/// Row mapping for iam_client_access_grants (a PARTNER account's clients)
#[derive(sqlx::FromRow)]
struct ClientGrantRow {
    principal_id: String,
    client_id: String,
}

/// Row mapping for iam_principal_roles junction table
#[derive(sqlx::FromRow)]
struct PrincipalRoleRow {
    principal_id: String,
    role_name: String,
    assignment_source: Option<String>,
    assigned_at: DateTime<Utc>,
}

impl TryFrom<PrincipalRoleRow> for RoleAssignment {
    type Error = PlatformError;
    fn try_from(r: PrincipalRoleRow) -> Result<Self> {
        let assignment_source = decode_opt(
            r.assignment_source.as_deref(),
            "iam_principal_roles",
            "assignment_source",
            &format!("{}/{}", r.principal_id, r.role_name),
        )?;
        Ok(RoleAssignment {
            role: r.role_name,
            client_id: None,
            assignment_source,
            assigned_at: r.assigned_at,
            assigned_by: None,
        })
    }
}

pub struct ServiceAccountRepository {
    pool: PgPool,
}

impl ServiceAccountRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, account: &ServiceAccount) -> Result<()> {
        let now = Utc::now();
        let wh = &account.webhook_credentials;
        let sa_id = account
            .service_account_table_id
            .as_ref()
            .unwrap_or(&account.id);

        sqlx::query(
            "INSERT INTO iam_service_accounts
                (id, code, name, description, application_id, active,
                 wh_auth_type, wh_auth_token_ref, wh_signing_secret_ref, wh_signing_algorithm,
                 wh_credentials_created_at, wh_credentials_regenerated_at,
                 last_used_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NULL, $12, $13, $14)",
        )
        .bind(sa_id)
        .bind(&account.code)
        .bind(&account.name)
        .bind(&account.description)
        .bind(&account.application_id)
        .bind(account.active)
        .bind(Some(wh.auth_type.as_str()))
        .bind(&wh.token)
        .bind(&wh.signing_secret)
        .bind(wh.signing_algorithm.map(|a| a.as_str()))
        .bind(Some(now)) // wh_credentials_created_at
        .bind(account.last_used_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Find by principal ID (the ID returned in API responses).
    pub async fn find_by_id(&self, id: &str) -> Result<Option<ServiceAccount>> {
        let principal = sqlx::query_as::<_, PrincipalRow>(
            "SELECT id, type, scope, client_id, application_id, name, active, \
             service_account_id, created_at, updated_at \
             FROM iam_principals WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match principal {
            Some(p) => self.hydrate(p).await.map(Some),
            None => Ok(None),
        }
    }

    /// Find by service account code.
    pub async fn find_by_code(&self, code: &str) -> Result<Option<ServiceAccount>> {
        // Look up the service_account_id from iam_service_accounts, then find the principal
        let sa = sqlx::query_as::<_, ServiceAccountRow>(
            "SELECT id, code, name, description, application_id, active, \
             wh_auth_type, wh_auth_token_ref, wh_signing_secret_ref, wh_signing_algorithm, \
             last_used_at, created_at, updated_at \
             FROM iam_service_accounts WHERE code = $1",
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?;

        match sa {
            Some(sa_row) => {
                let principal = sqlx::query_as::<_, PrincipalRow>(
                    "SELECT id, type, scope, client_id, application_id, name, active, \
                     service_account_id, created_at, updated_at \
                     FROM iam_principals WHERE service_account_id = $1",
                )
                .bind(&sa_row.id)
                .fetch_optional(&self.pool)
                .await?;
                match principal {
                    Some(p) => self.hydrate_with_sa(p, sa_row).await.map(Some),
                    None => Ok(None),
                }
            }
            None => Ok(None),
        }
    }

    /// Find all active service account principals.
    pub async fn find_active(&self) -> Result<Vec<ServiceAccount>> {
        let principals = sqlx::query_as::<_, PrincipalRow>(
            "SELECT id, type, scope, client_id, application_id, name, active, \
             service_account_id, created_at, updated_at \
             FROM iam_principals WHERE type = 'SERVICE' AND active = true",
        )
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_many(principals).await
    }

    /// Find service accounts by application ID.
    pub async fn find_by_application(&self, application_id: &str) -> Result<Vec<ServiceAccount>> {
        let principals = sqlx::query_as::<_, PrincipalRow>(
            "SELECT id, type, scope, client_id, application_id, name, active, \
             service_account_id, created_at, updated_at \
             FROM iam_principals WHERE type = 'SERVICE' AND application_id = $1",
        )
        .bind(application_id)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_many(principals).await
    }

    /// Find service accounts by client ID.
    pub async fn find_by_client(&self, client_id: &str) -> Result<Vec<ServiceAccount>> {
        let principals = sqlx::query_as::<_, PrincipalRow>(
            "SELECT id, type, scope, client_id, application_id, name, active, \
             service_account_id, created_at, updated_at \
             FROM iam_principals WHERE type = 'SERVICE' AND client_id = $1 AND active = true",
        )
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_many(principals).await
    }

    /// Find service accounts with a specific role.
    pub async fn find_with_role(&self, role: &str) -> Result<Vec<ServiceAccount>> {
        let principals = sqlx::query_as::<_, PrincipalRow>(
            "SELECT p.id, p.type, p.scope, p.client_id, p.application_id, p.name, p.active, \
             p.service_account_id, p.created_at, p.updated_at \
             FROM iam_principals p
             INNER JOIN iam_principal_roles pr ON pr.principal_id = p.id
             WHERE p.type = 'SERVICE' AND p.active = true AND pr.role_name = $1",
        )
        .bind(role)
        .fetch_all(&self.pool)
        .await?;

        self.hydrate_many(principals).await
    }

    pub async fn update(&self, account: &ServiceAccount) -> Result<()> {
        let now = Utc::now();
        if let Some(ref sa_table_id) = account.service_account_table_id {
            let wh = &account.webhook_credentials;
            sqlx::query(
                "UPDATE iam_service_accounts SET
                    code = $2, name = $3, description = $4, application_id = $5, active = $6,
                    wh_auth_type = $7, wh_auth_token_ref = $8, wh_signing_secret_ref = $9,
                    wh_signing_algorithm = $10, last_used_at = $11, updated_at = $12
                 WHERE id = $1",
            )
            .bind(sa_table_id)
            .bind(&account.code)
            .bind(&account.name)
            .bind(&account.description)
            .bind(&account.application_id)
            .bind(account.active)
            .bind(Some(wh.auth_type.as_str()))
            .bind(&wh.token)
            .bind(&wh.signing_secret)
            .bind(wh.signing_algorithm.map(|a| a.as_str()))
            .bind(account.last_used_at)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        // Delete the principal (CASCADE will clean up roles)
        let result = sqlx::query("DELETE FROM iam_principals WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // ── Hydration ──────────────────────────────────────────────

    /// Hydrate a single principal into a ServiceAccount by loading
    /// webhook credentials from iam_service_accounts and roles from iam_principal_roles.
    async fn hydrate(&self, principal: PrincipalRow) -> Result<ServiceAccount> {
        let sa_row = if let Some(ref sa_id) = principal.service_account_id {
            sqlx::query_as::<_, ServiceAccountRow>(
                "SELECT id, code, name, description, application_id, active, \
                 wh_auth_type, wh_auth_token_ref, wh_signing_secret_ref, wh_signing_algorithm, \
                 last_used_at, created_at, updated_at \
                 FROM iam_service_accounts WHERE id = $1",
            )
            .bind(sa_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            None
        };
        let (roles, granted) = tokio::try_join!(
            self.load_roles(&principal.id),
            self.load_granted_clients(&principal.id),
        )?;
        Self::build_service_account_sync(principal, sa_row.as_ref(), roles, granted)
    }

    /// Hydrate when we already have both rows.
    async fn hydrate_with_sa(
        &self,
        principal: PrincipalRow,
        sa_row: ServiceAccountRow,
    ) -> Result<ServiceAccount> {
        let (roles, granted) = tokio::try_join!(
            self.load_roles(&principal.id),
            self.load_granted_clients(&principal.id),
        )?;
        Self::build_service_account_sync(principal, Some(&sa_row), roles, granted)
    }

    /// Hydrate multiple principals into ServiceAccounts (batch).
    async fn hydrate_many(&self, principals: Vec<PrincipalRow>) -> Result<Vec<ServiceAccount>> {
        if principals.is_empty() {
            return Ok(vec![]);
        }

        let principal_ids: Vec<String> = principals.iter().map(|p| p.id.clone()).collect();

        // Batch-load service account details
        let sa_ids: Vec<String> = principals
            .iter()
            .filter_map(|p| p.service_account_id.clone())
            .collect();

        let sa_rows: std::collections::HashMap<String, ServiceAccountRow> = if !sa_ids.is_empty() {
            sqlx::query_as::<_, ServiceAccountRow>(
                "SELECT id, code, name, description, application_id, active, \
                 wh_auth_type, wh_auth_token_ref, wh_signing_secret_ref, wh_signing_algorithm, \
                 last_used_at, created_at, updated_at \
                 FROM iam_service_accounts WHERE id = ANY($1)",
            )
            .bind(&sa_ids)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect()
        } else {
            std::collections::HashMap::new()
        };

        // Batch-load roles
        let all_roles = sqlx::query_as::<_, PrincipalRoleRow>(
            "SELECT principal_id, role_name, assignment_source, assigned_at \
             FROM iam_principal_roles WHERE principal_id = ANY($1)",
        )
        .bind(&principal_ids)
        .fetch_all(&self.pool)
        .await?;

        let mut role_map: std::collections::HashMap<String, Vec<RoleAssignment>> =
            std::collections::HashMap::new();
        for r in all_roles {
            role_map
                .entry(r.principal_id.clone())
                .or_default()
                .push(RoleAssignment::try_from(r)?);
        }

        // Batch-load client grants
        let all_grants = sqlx::query_as::<_, ClientGrantRow>(
            "SELECT principal_id, client_id FROM iam_client_access_grants \
             WHERE principal_id = ANY($1) ORDER BY granted_at, client_id",
        )
        .bind(&principal_ids)
        .fetch_all(&self.pool)
        .await?;
        let mut grant_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for g in all_grants {
            grant_map
                .entry(g.principal_id)
                .or_default()
                .push(g.client_id);
        }

        // Build ServiceAccount entities
        principals
            .into_iter()
            .map(|p| {
                let id = p.id.clone();
                let sa_row = p
                    .service_account_id
                    .as_ref()
                    .and_then(|sa_id| sa_rows.get(sa_id));
                let roles = role_map.remove(&id).unwrap_or_default();
                let granted = grant_map.remove(&id).unwrap_or_default();

                Self::build_service_account_sync(p, sa_row, roles, granted)
            })
            .collect()
    }

    /// Synchronous builder (no DB calls).
    fn build_service_account_sync(
        principal: PrincipalRow,
        sa_row: Option<&ServiceAccountRow>,
        roles: Vec<RoleAssignment>,
        granted_clients: Vec<String>,
    ) -> Result<ServiceAccount> {
        // Read exactly as the principal repository (and Go) read the column:
        // NULL is an unscoped row, read as CLIENT, the narrowest; anything
        // unrecognised is a loud error (X-06).
        let scope = decode_opt(
            principal.scope.as_deref(),
            "iam_principals",
            "scope",
            &principal.id,
        )?
        .unwrap_or(UserScope::Client);
        // The clients the principal actually reaches at that scope. A stale
        // `client_id` on an ANCHOR row reaches nothing extra, so it isn't
        // reported.
        let client_ids = match scope {
            UserScope::Anchor => vec![],
            UserScope::Client => principal.client_id.clone().into_iter().collect(),
            UserScope::Partner => granted_clients,
        };

        let webhook_credentials = match sa_row {
            Some(sa) => {
                let signing_algorithm = decode_opt(
                    sa.wh_signing_algorithm.as_deref(),
                    "iam_service_accounts",
                    "wh_signing_algorithm",
                    &sa.id,
                )?;
                // An unknown auth type must never read as NONE: that would
                // deliver webhooks unauthenticated (X-06).
                let auth_type: Option<WebhookAuthType> = decode_opt(
                    sa.wh_auth_type.as_deref(),
                    "iam_service_accounts",
                    "wh_auth_type",
                    &sa.id,
                )?;
                WebhookCredentials {
                    auth_type: auth_type.unwrap_or_default(),
                    token: sa.wh_auth_token_ref.clone(),
                    username: None,
                    password: None,
                    header_name: None,
                    signing_secret: sa.wh_signing_secret_ref.clone(),
                    signing_algorithm,
                    signature_header: None,
                }
            }
            None => WebhookCredentials::default(),
        };

        let code = sa_row
            .map(|sa| sa.code.clone())
            .unwrap_or_else(|| principal.name.clone());

        Ok(ServiceAccount {
            // The principal ID is what gets returned to clients
            id: principal.id,
            code,
            name: principal.name,
            description: sa_row.and_then(|sa| sa.description.clone()),
            active: principal.active,
            client_ids,
            application_id: principal.application_id,
            scope,
            webhook_credentials,
            roles,
            service_account_table_id: principal.service_account_id,
            last_used_at: sa_row.and_then(|sa| sa.last_used_at),
            created_at: principal.created_at,
            updated_at: principal.updated_at,
        })
    }

    /// Load roles for a principal from the junction table.
    async fn load_roles(&self, principal_id: &str) -> Result<Vec<RoleAssignment>> {
        let rows = sqlx::query_as::<_, PrincipalRoleRow>(
            "SELECT principal_id, role_name, assignment_source, assigned_at \
             FROM iam_principal_roles WHERE principal_id = $1",
        )
        .bind(principal_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(RoleAssignment::try_from).collect()
    }

    /// Clients granted to a principal (a PARTNER account's clients).
    async fn load_granted_clients(&self, principal_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query_as::<_, ClientGrantRow>(
            "SELECT principal_id, client_id FROM iam_client_access_grants \
             WHERE principal_id = $1 ORDER BY granted_at, client_id",
        )
        .bind(principal_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|g| g.client_id).collect())
    }
}

// ── Persist<ServiceAccount> ──────────────────────────────────────────────────

impl HasId for ServiceAccount {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait]
impl crate::usecase::Persist<ServiceAccount> for ServiceAccountRepository {
    async fn persist(&self, sa: &ServiceAccount, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        // The account's reach, as the principal carries it: a CLIENT account's
        // one client is its home client; a PARTNER account's clients are
        // grants; an ANCHOR account has neither.
        let home_client_id = match sa.scope {
            UserScope::Client => sa.client_ids.first(),
            UserScope::Anchor | UserScope::Partner => None,
        };
        let granted_client_ids: &[String] = match sa.scope {
            UserScope::Partner => &sa.client_ids,
            UserScope::Anchor | UserScope::Client => &[],
        };
        let sa_table_id = sa
            .service_account_table_id
            .clone()
            .unwrap_or_else(|| sa.id.clone());
        let wh = &sa.webhook_credentials;

        // 1. Upsert iam_principals (SERVICE type principal)
        sqlx::query(
            "INSERT INTO iam_principals (id, type, scope, client_id, application_id, name, active, email, email_domain, idp_type, external_idp_id, password_hash, last_login_at, service_account_id, created_at, updated_at)
             VALUES ($1, 'SERVICE', $2, $3, $4, $5, $6, NULL, NULL, NULL, NULL, NULL, NULL, $7, $8, $9)
             ON CONFLICT (id) DO UPDATE SET
                scope = EXCLUDED.scope,
                name = EXCLUDED.name,
                active = EXCLUDED.active,
                client_id = EXCLUDED.client_id,
                application_id = EXCLUDED.application_id,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&sa.id)
        .bind(sa.scope.as_str())
        .bind(home_client_id)
        .bind(&sa.application_id)
        .bind(&sa.name)
        .bind(sa.active)
        .bind(Some(&sa.id))
        .bind(sa.created_at)
        .bind(now)
        .execute(&mut **tx.inner).await?;

        // 2. Upsert iam_service_accounts (webhook credentials)
        sqlx::query(
            "INSERT INTO iam_service_accounts (id, code, name, description, application_id, active, wh_auth_type, wh_auth_token_ref, wh_signing_secret_ref, wh_signing_algorithm, wh_credentials_created_at, last_used_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT (id) DO UPDATE SET
                code = EXCLUDED.code,
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                application_id = EXCLUDED.application_id,
                active = EXCLUDED.active,
                wh_auth_type = EXCLUDED.wh_auth_type,
                wh_auth_token_ref = EXCLUDED.wh_auth_token_ref,
                wh_signing_secret_ref = EXCLUDED.wh_signing_secret_ref,
                wh_signing_algorithm = EXCLUDED.wh_signing_algorithm,
                last_used_at = EXCLUDED.last_used_at,
                updated_at = EXCLUDED.updated_at"
        )
        .bind(&sa_table_id)
        .bind(&sa.code)
        .bind(&sa.name)
        .bind(&sa.description)
        .bind(&sa.application_id)
        .bind(sa.active)
        .bind(Some(wh.auth_type.as_str()))
        .bind(&wh.token)
        .bind(&wh.signing_secret)
        .bind(wh.signing_algorithm.map(|a| a.as_str()))
        .bind(Some(now))
        .bind(sa.last_used_at)
        .bind(now)
        .bind(now)
        .execute(&mut **tx.inner).await?;

        // 3. Sync client grants: exactly the PARTNER account's clients.
        sqlx::query("DELETE FROM iam_client_access_grants WHERE principal_id = $1")
            .bind(&sa.id)
            .execute(&mut **tx.inner)
            .await?;
        if !granted_client_ids.is_empty() {
            let grant_ids: Vec<String> = granted_client_ids
                .iter()
                .map(|_| crate::shared::tsid::generate(crate::EntityType::ClientAccessGrant))
                .collect();
            sqlx::query(
                "INSERT INTO iam_client_access_grants
                    (id, principal_id, client_id, granted_by, granted_at, created_at, updated_at)
                 SELECT g.id, $1, g.client_id, $1, $4, $4, $4
                 FROM UNNEST($2::text[], $3::text[]) AS g(id, client_id)",
            )
            .bind(&sa.id)
            .bind(&grant_ids)
            .bind(granted_client_ids)
            .bind(now)
            .execute(&mut **tx.inner)
            .await?;
        }

        // 4. Sync roles to iam_principal_roles using the principal ID
        sqlx::query("DELETE FROM iam_principal_roles WHERE principal_id = $1")
            .bind(&sa.id)
            .execute(&mut **tx.inner)
            .await?;
        for r in &sa.roles {
            sqlx::query(
                "INSERT INTO iam_principal_roles (principal_id, role_name, assignment_source, assigned_at)
                 VALUES ($1, $2, $3, $4)"
            )
            .bind(&sa.id)
            .bind(&r.role)
            .bind(r.assignment_source.map(|s| s.as_str()))
            .bind(r.assigned_at)
            .execute(&mut **tx.inner).await?;
        }

        Ok(())
    }

    async fn delete(&self, sa: &ServiceAccount, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        // Delete any OAuth client wired to this service account principal.
        // Migration 027 adds an FK with ON DELETE CASCADE, which would
        // make this row-level delete redundant — but we keep it here as
        // defense-in-depth for installs that haven't migrated yet and so
        // the order of deletes is explicit in the use-case path.
        // `oauth_clients`'s junction tables (redirect_uris, allowed_origins,
        // grant_types, application_ids) already cascade from oauth_clients.id.
        sqlx::query("DELETE FROM oauth_clients WHERE service_account_principal_id = $1")
            .bind(&sa.id)
            .execute(&mut **tx.inner)
            .await?;
        // Clear any application pointer at this SA. Without this, the
        // application keeps `service_account_id` set to a dead principal
        // and the provision-service-account handler refuses to mint a
        // replacement with "already has a service account provisioned".
        // Migration 028 adds an `ON DELETE SET NULL` FK so the DB does
        // this automatically; the explicit UPDATE here is defense in
        // depth for pre-migration installs.
        sqlx::query(
            "UPDATE app_applications SET service_account_id = NULL WHERE service_account_id = $1",
        )
        .bind(&sa.id)
        .execute(&mut **tx.inner)
        .await?;
        if let Some(ref sa_id) = sa.service_account_table_id {
            sqlx::query("DELETE FROM iam_service_accounts WHERE id = $1")
                .bind(sa_id)
                .execute(&mut **tx.inner)
                .await?;
        }
        sqlx::query("DELETE FROM iam_principals WHERE id = $1")
            .bind(&sa.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal_row() -> PrincipalRow {
        PrincipalRow {
            id: "prn_1".to_string(),
            principal_type: "SERVICE".to_string(),
            scope: None,
            client_id: None,
            application_id: None,
            name: "svc".to_string(),
            active: true,
            service_account_id: Some("sac_1".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn sa_row(auth_type: Option<&str>, algorithm: Option<&str>) -> ServiceAccountRow {
        ServiceAccountRow {
            id: "sac_1".to_string(),
            code: "svc".to_string(),
            name: "svc".to_string(),
            description: None,
            application_id: None,
            active: true,
            wh_auth_type: auth_type.map(str::to_string),
            wh_auth_token_ref: None,
            wh_signing_secret_ref: None,
            wh_signing_algorithm: algorithm.map(str::to_string),
            last_used_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn unknown_stored_webhook_auth_type_is_a_read_error_not_none() {
        let row = sa_row(Some("OAUTH_MAGIC"), None);
        let err = ServiceAccountRepository::build_service_account_sync(
            principal_row(),
            Some(&row),
            vec![],
            vec![],
        )
        .unwrap_err()
        .to_string();
        for part in ["iam_service_accounts.wh_auth_type", "sac_1", "OAUTH_MAGIC"] {
            assert!(err.contains(part), "{err} should mention {part}");
        }
    }

    #[test]
    fn stored_values_decode_including_legacy_signing_algorithm() {
        for alg in ["HMAC_SHA256", "SHA256"] {
            let row = sa_row(Some("BEARER_TOKEN"), Some(alg));
            let sa = ServiceAccountRepository::build_service_account_sync(
                principal_row(),
                Some(&row),
                vec![],
                vec![],
            )
            .unwrap();
            assert_eq!(
                sa.webhook_credentials.auth_type,
                WebhookAuthType::BearerToken
            );
            assert_eq!(
                sa.webhook_credentials.signing_algorithm,
                Some(crate::SigningAlgorithm::HmacSha256)
            );
        }
    }

    fn build(principal: PrincipalRow, granted: &[&str]) -> Result<ServiceAccount> {
        ServiceAccountRepository::build_service_account_sync(
            principal,
            Some(&sa_row(Some("BEARER_TOKEN"), None)),
            vec![],
            granted.iter().map(|s| s.to_string()).collect(),
        )
    }

    fn scoped(scope: Option<&str>, client_id: Option<&str>) -> PrincipalRow {
        PrincipalRow {
            scope: scope.map(str::to_string),
            client_id: client_id.map(str::to_string),
            ..principal_row()
        }
    }

    #[test]
    fn scope_reads_from_the_principal_row_with_the_links_it_reaches() {
        let anchor = build(scoped(Some("ANCHOR"), Some("clt_stale")), &["clt_g"]).unwrap();
        assert_eq!(anchor.scope, UserScope::Anchor);
        assert!(anchor.client_ids.is_empty());

        let client = build(scoped(Some("CLIENT"), Some("clt_home")), &["clt_g"]).unwrap();
        assert_eq!(client.scope, UserScope::Client);
        assert_eq!(client.client_ids, vec!["clt_home"]);

        let partner = build(scoped(Some("PARTNER"), None), &["clt_1", "clt_2"]).unwrap();
        assert_eq!(partner.scope, UserScope::Partner);
        assert_eq!(partner.client_ids, vec!["clt_1", "clt_2"]);
    }

    #[test]
    fn null_scope_reads_as_client_never_anchor() {
        // Same reading as the principal repository and Go's principal read.
        let sa = build(scoped(None, None), &[]).unwrap();
        assert_eq!(sa.scope, UserScope::Client);
        assert!(sa.client_ids.is_empty());
    }

    #[test]
    fn unknown_or_miscased_stored_scope_is_a_read_error() {
        for bad in ["anchor", "ROOT"] {
            let err = build(scoped(Some(bad), None), &[]).unwrap_err().to_string();
            for part in ["iam_principals.scope", "prn_1", bad] {
                assert!(err.contains(part), "{err} should mention {part}");
            }
        }
    }
}
