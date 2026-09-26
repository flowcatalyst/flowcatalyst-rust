//! Principal Repository — PostgreSQL via SQLx
//!
//! Roles are loaded from iam_principal_roles junction table.
//! Assigned clients are loaded from iam_client_access_grants.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::{ExternalIdentity, Principal, PrincipalType, UserIdentity, UserScope};
use crate::service_account::entity::RoleAssignment;
use crate::shared::enum_str::{decode, decode_opt};
use crate::shared::error::{PlatformError, Result};
use crate::usecase::unit_of_work::HasId;

// ── Row types ────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct PrincipalRow {
    id: String,
    #[sqlx(rename = "type")]
    principal_type: String,
    scope: Option<String>,
    client_id: Option<String>,
    application_id: Option<String>,
    name: String,
    active: bool,
    email: Option<String>,
    #[allow(dead_code)]
    email_domain: Option<String>,
    idp_type: Option<String>,
    external_idp_id: Option<String>,
    password_hash: Option<String>,
    last_login_at: Option<DateTime<Utc>>,
    service_account_id: Option<String>,
    all_applications: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<PrincipalRow> for Principal {
    type Error = PlatformError;
    fn try_from(r: PrincipalRow) -> Result<Self> {
        let principal_type = decode(&r.principal_type, "iam_principals", "type", &r.id)?;
        // A NULL scope is an unscoped row, read as CLIENT (the narrowest).
        let scope = decode_opt(r.scope.as_deref(), "iam_principals", "scope", &r.id)?
            .unwrap_or(UserScope::Client);

        let user_identity = if principal_type == PrincipalType::User {
            r.email.as_ref().map(|email| UserIdentity {
                email: email.clone(),
                email_verified: false,
                first_name: None,
                last_name: None,
                picture_url: None,
                phone: None,
                external_id: r.external_idp_id.clone(),
                provider: r.idp_type.clone(),
                password_hash: r.password_hash.clone(),
                last_login_at: r.last_login_at,
            })
        } else {
            None
        };

        let external_identity = r.external_idp_id.as_ref().map(|ext_id| ExternalIdentity {
            provider_id: r.idp_type.clone().unwrap_or_default(),
            external_id: ext_id.clone(),
        });

        Ok(Self {
            id: r.id,
            principal_type,
            scope,
            client_id: r.client_id,
            application_id: r.application_id,
            name: r.name,
            active: r.active,
            user_identity,
            service_account_id: r.service_account_id,
            roles: vec![],
            assigned_clients: vec![],
            client_identifier_map: std::collections::HashMap::new(),
            accessible_application_ids: vec![],
            application_code_map: std::collections::HashMap::new(),
            all_applications: r.all_applications,
            created_at: r.created_at,
            updated_at: r.updated_at,
            external_identity,
        })
    }
}

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

#[derive(sqlx::FromRow)]
struct ClientAccessGrantRow {
    principal_id: String,
    client_id: String,
}

#[derive(sqlx::FromRow)]
struct ClientIdentifierRow {
    id: String,
    identifier: String,
}

#[derive(sqlx::FromRow)]
struct PrincipalApplicationAccessRow {
    principal_id: String,
    application_id: String,
    /// The application's code; NULL when the application row is gone (the
    /// junction has no FK), as Go's LEFT JOIN reads it.
    application_code: Option<String>,
}

/// The application-access facts for one principal, without hydrating the
/// rest of it: its `all_applications` flag and its explicit
/// `iam_principal_application_access` grants.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PrincipalApplicationBinding {
    pub all_applications: bool,
    pub granted_application_ids: Vec<String>,
}

// ── Repository ───────────────────────────────────────────────────────────────

pub struct PrincipalRepository {
    pool: PgPool,
}

impl PrincipalRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, principal: &Principal) -> Result<()> {
        let now = Utc::now();
        let email_domain = principal
            .user_identity
            .as_ref()
            .map(|i| i.email.split('@').nth(1).unwrap_or("").to_string());
        let email = principal.user_identity.as_ref().map(|i| i.email.clone());
        let idp_type = principal
            .user_identity
            .as_ref()
            .and_then(|i| i.provider.clone())
            .or_else(|| {
                if principal.is_user() {
                    Some("INTERNAL".to_string())
                } else {
                    None
                }
            });
        let external_idp_id = principal
            .external_identity
            .as_ref()
            .map(|e| e.external_id.clone());
        let password_hash = principal
            .user_identity
            .as_ref()
            .and_then(|i| i.password_hash.clone());
        let last_login_at = principal
            .user_identity
            .as_ref()
            .and_then(|i| i.last_login_at);

        sqlx::query(
            "INSERT INTO iam_principals
                (id, type, scope, client_id, application_id, name, active, email, email_domain,
                 idp_type, external_idp_id, password_hash, last_login_at, service_account_id,
                 created_at, updated_at, all_applications)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(&principal.id)
        .bind(principal.principal_type.as_str())
        .bind(Some(principal.scope.as_str()))
        .bind(&principal.client_id)
        .bind(&principal.application_id)
        .bind(&principal.name)
        .bind(principal.active)
        .bind(&email)
        .bind(&email_domain)
        .bind(&idp_type)
        .bind(&external_idp_id)
        .bind(&password_hash)
        .bind(last_login_at)
        .bind(&principal.service_account_id)
        .bind(now)
        .bind(now)
        .bind(principal.all_applications)
        .execute(&self.pool)
        .await?;

        // Insert roles into junction table
        self.insert_roles(&principal.id, &principal.roles).await?;

        // Insert application access grants
        if !principal.accessible_application_ids.is_empty() {
            sqlx::query(
                "INSERT INTO iam_principal_application_access (principal_id, application_id, granted_at)
                 SELECT $1, a, $3 FROM UNNEST($2::varchar[]) AS a",
            )
            .bind(&principal.id)
            .bind(&principal.accessible_application_ids)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Principal>> {
        let row = sqlx::query_as::<_, PrincipalRow>("SELECT * FROM iam_principals WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(r) => Ok(Some(self.hydrate_principal(r).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<Principal>> {
        let row = sqlx::query_as::<_, PrincipalRow>(
            "SELECT * FROM iam_principals WHERE type = 'USER' AND email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(self.hydrate_principal(r).await?)),
            None => Ok(None),
        }
    }

    /// Replace a principal's stored password hash — infrastructure, not a
    /// domain change: the lazy re-encoding of a hash the user just proved
    /// they know the password for (a migrated bcrypt hash, or older argon2
    /// parameters), made on login. Go writes it the same way, as a direct
    /// UPDATE with no event or audit (principal/repository.go:760-770,
    /// called from auth/login/endpoint.go:519-525).
    pub async fn update_password_hash(&self, principal_id: &str, hash: &str) -> Result<()> {
        sqlx::query("UPDATE iam_principals SET password_hash = $1, updated_at = $2 WHERE id = $3")
            .bind(hash)
            .bind(Utc::now())
            .bind(principal_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The USER principals with these emails (matched case-insensitively),
    /// hydrated, in one query plus the batch hydration.
    pub async fn find_users_by_emails(&self, emails: &[String]) -> Result<Vec<Principal>> {
        if emails.is_empty() {
            return Ok(vec![]);
        }
        let lowered: Vec<String> = emails.iter().map(|e| e.to_lowercase()).collect();
        let rows = sqlx::query_as::<_, PrincipalRow>(
            "SELECT * FROM iam_principals WHERE type = 'USER' AND lower(email) = ANY($1)",
        )
        .bind(&lowered)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_principals(rows).await
    }

    /// Load the all-applications flag and grants for one principal in a
    /// single indexed query (both lookups hit a primary key). `None` when no
    /// such principal exists.
    pub async fn find_application_binding(
        &self,
        principal_id: &str,
    ) -> Result<Option<PrincipalApplicationBinding>> {
        let row = sqlx::query_as::<_, PrincipalApplicationBinding>(
            "SELECT p.all_applications,
                    ARRAY(SELECT a.application_id FROM iam_principal_application_access a
                          WHERE a.principal_id = p.id) AS granted_application_ids
             FROM iam_principals p WHERE p.id = $1",
        )
        .bind(principal_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn find_by_service_account(
        &self,
        service_account_id: &str,
    ) -> Result<Option<Principal>> {
        let row = sqlx::query_as::<_, PrincipalRow>(
            "SELECT * FROM iam_principals WHERE type = 'SERVICE' AND service_account_id = $1",
        )
        .bind(service_account_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(self.hydrate_principal(r).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_all(&self) -> Result<Vec<Principal>> {
        let rows = sqlx::query_as::<_, PrincipalRow>("SELECT * FROM iam_principals")
            .fetch_all(&self.pool)
            .await?;
        self.hydrate_principals(rows).await
    }

    pub async fn find_active(&self) -> Result<Vec<Principal>> {
        let rows =
            sqlx::query_as::<_, PrincipalRow>("SELECT * FROM iam_principals WHERE active = true")
                .fetch_all(&self.pool)
                .await?;
        self.hydrate_principals(rows).await
    }

    pub async fn find_users(&self) -> Result<Vec<Principal>> {
        let rows = sqlx::query_as::<_, PrincipalRow>(
            "SELECT * FROM iam_principals WHERE type = 'USER' AND active = true",
        )
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_principals(rows).await
    }

    pub async fn find_services(&self) -> Result<Vec<Principal>> {
        let rows = sqlx::query_as::<_, PrincipalRow>(
            "SELECT * FROM iam_principals WHERE type = 'SERVICE' AND active = true",
        )
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_principals(rows).await
    }

    pub async fn find_by_client(&self, client_id: &str) -> Result<Vec<Principal>> {
        // Find principals that either have this client_id OR have a grant for it
        let rows = sqlx::query_as::<_, PrincipalRow>(
            "SELECT DISTINCT p.* FROM iam_principals p
             LEFT JOIN iam_client_access_grants g ON g.principal_id = p.id
             WHERE p.active = true AND (p.client_id = $1 OR g.client_id = $1)",
        )
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_principals(rows).await
    }

    pub async fn find_by_scope(&self, scope: UserScope) -> Result<Vec<Principal>> {
        let rows = sqlx::query_as::<_, PrincipalRow>(
            "SELECT * FROM iam_principals WHERE scope = $1 AND active = true",
        )
        .bind(scope.as_str())
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_principals(rows).await
    }

    /// Combinable filter query — applies all provided filters at the DB level (AND logic).
    /// For `client_id`, includes principals whose home client matches OR who have a grant for it.
    /// For `search`, applies ILIKE on name and email (OR).
    pub async fn find_with_filters(
        &self,
        client_id: Option<&str>,
        scope: Option<UserScope>,
        principal_type: Option<PrincipalType>,
        active: Option<bool>,
        search: Option<&str>,
        email: Option<&str>,
    ) -> Result<Vec<Principal>> {
        let needs_join = client_id.is_some();
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(if needs_join {
            "SELECT DISTINCT p.* FROM iam_principals p \
             LEFT JOIN iam_client_access_grants g ON g.principal_id = p.id"
        } else {
            "SELECT p.* FROM iam_principals p"
        });

        let mut has_where = false;
        let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
            qb.push(if *has_where { " AND " } else { " WHERE " });
            *has_where = true;
        };

        if let Some(cid) = client_id {
            push_where(&mut qb, &mut has_where);
            let cid_owned = cid.to_string();
            qb.push("(p.client_id = ")
                .push_bind(cid_owned.clone())
                .push(" OR g.client_id = ")
                .push_bind(cid_owned)
                .push(")");
        }
        if let Some(s) = scope {
            push_where(&mut qb, &mut has_where);
            qb.push("p.scope = ").push_bind(s.as_str());
        }
        if let Some(pt) = principal_type {
            push_where(&mut qb, &mut has_where);
            qb.push("p.type = ").push_bind(pt.as_str());
        }
        if let Some(a) = active {
            push_where(&mut qb, &mut has_where);
            qb.push("p.active = ").push_bind(a);
        }
        if let Some(q) = search {
            if !q.is_empty() {
                push_where(&mut qb, &mut has_where);
                let pattern = format!("%{}%", q);
                qb.push("(p.name ILIKE ")
                    .push_bind(pattern.clone())
                    .push(" OR p.email ILIKE ")
                    .push_bind(pattern)
                    .push(")");
            }
        }
        if let Some(em) = email {
            if !em.is_empty() {
                push_where(&mut qb, &mut has_where);
                qb.push("LOWER(p.email) = ").push_bind(em.to_lowercase());
            }
        }

        let rows: Vec<PrincipalRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        self.hydrate_principals(rows).await
    }

    pub async fn find_anchors(&self) -> Result<Vec<Principal>> {
        let rows = sqlx::query_as::<_, PrincipalRow>(
            "SELECT * FROM iam_principals WHERE scope = 'ANCHOR' AND active = true",
        )
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_principals(rows).await
    }

    pub async fn find_by_application(&self, application_id: &str) -> Result<Vec<Principal>> {
        let rows = sqlx::query_as::<_, PrincipalRow>(
            "SELECT * FROM iam_principals WHERE application_id = $1",
        )
        .bind(application_id)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_principals(rows).await
    }

    pub async fn find_with_role(&self, role: &str) -> Result<Vec<Principal>> {
        let rows = sqlx::query_as::<_, PrincipalRow>(
            "SELECT p.* FROM iam_principals p
             INNER JOIN iam_principal_roles r ON r.principal_id = p.id
             WHERE r.role_name = $1 AND p.active = true",
        )
        .bind(role)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_principals(rows).await
    }

    pub async fn update(&self, principal: &Principal) -> Result<()> {
        let now = Utc::now();
        let email_domain = principal
            .user_identity
            .as_ref()
            .map(|i| i.email.split('@').nth(1).unwrap_or("").to_string());
        let email = principal.user_identity.as_ref().map(|i| i.email.clone());
        let idp_type = principal
            .user_identity
            .as_ref()
            .and_then(|i| i.provider.clone())
            .or_else(|| {
                if principal.is_user() {
                    Some("INTERNAL".to_string())
                } else {
                    None
                }
            });
        let external_idp_id = principal
            .external_identity
            .as_ref()
            .map(|e| e.external_id.clone());
        let password_hash = principal
            .user_identity
            .as_ref()
            .and_then(|i| i.password_hash.clone());
        let last_login_at = principal
            .user_identity
            .as_ref()
            .and_then(|i| i.last_login_at);

        sqlx::query(
            "UPDATE iam_principals SET
                type = $2, scope = $3, client_id = $4, application_id = $5, name = $6,
                active = $7, email = $8, email_domain = $9, idp_type = $10,
                external_idp_id = $11, password_hash = $12, last_login_at = $13,
                service_account_id = $14, updated_at = $15, all_applications = $16
             WHERE id = $1",
        )
        .bind(&principal.id)
        .bind(principal.principal_type.as_str())
        .bind(Some(principal.scope.as_str()))
        .bind(&principal.client_id)
        .bind(&principal.application_id)
        .bind(&principal.name)
        .bind(principal.active)
        .bind(&email)
        .bind(&email_domain)
        .bind(&idp_type)
        .bind(&external_idp_id)
        .bind(&password_hash)
        .bind(last_login_at)
        .bind(&principal.service_account_id)
        .bind(now)
        .bind(principal.all_applications)
        .execute(&self.pool)
        .await?;

        // Sync roles: delete all then re-insert
        sqlx::query("DELETE FROM iam_principal_roles WHERE principal_id = $1")
            .bind(&principal.id)
            .execute(&self.pool)
            .await?;
        self.insert_roles(&principal.id, &principal.roles).await?;

        // Sync application access
        sqlx::query("DELETE FROM iam_principal_application_access WHERE principal_id = $1")
            .bind(&principal.id)
            .execute(&self.pool)
            .await?;

        if !principal.accessible_application_ids.is_empty() {
            let count = principal.accessible_application_ids.len();
            let principal_ids: Vec<String> =
                std::iter::repeat_n(principal.id.clone(), count).collect();
            let app_ids: Vec<String> = principal.accessible_application_ids.clone();
            let granted_ats: Vec<DateTime<Utc>> = std::iter::repeat_n(now, count).collect();

            sqlx::query(
                "INSERT INTO iam_principal_application_access (principal_id, application_id, granted_at)
                 SELECT * FROM UNNEST($1::varchar[], $2::varchar[], $3::timestamptz[])"
            )
            .bind(&principal_ids)
            .bind(&app_ids)
            .bind(&granted_ats)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Delete a principal and cascade the non-FK junctions. Mirrors the
    /// tx-aware `Persist<Principal>::delete` — both paths MUST cascade or
    /// we leak orphaned role assignments / client access / app access rows.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM iam_principal_roles WHERE principal_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM iam_client_access_grants WHERE principal_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM iam_principal_application_access WHERE principal_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query("DELETE FROM iam_principals WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    /// Grant a single client access to a principal. Idempotent via ON CONFLICT.
    /// Returns true if a new row was inserted, false if the grant already existed.
    pub async fn grant_client_access(&self, principal_id: &str, client_id: &str) -> Result<bool> {
        let now = Utc::now();
        let result = sqlx::query(
            "INSERT INTO iam_client_access_grants
                (id, principal_id, client_id, granted_by, granted_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (principal_id, client_id) DO NOTHING",
        )
        .bind(crate::shared::tsid::generate(crate::EntityType::Principal))
        .bind(principal_id)
        .bind(client_id)
        .bind(principal_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Search principals by name or email (case-insensitive partial match)
    pub async fn search(&self, term: &str) -> Result<Vec<Principal>> {
        let pattern = format!("%{}%", term);
        let rows = sqlx::query_as::<_, PrincipalRow>(
            "SELECT * FROM iam_principals WHERE name LIKE $1 OR email LIKE $1",
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_principals(rows).await
    }

    /// Batch-lookup principal names by IDs. Returns a map of id -> name.
    pub async fn find_names_by_ids(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, String>> {
        if ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, name FROM iam_principals WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().collect())
    }

    /// Count principals with email ending in the given domain
    pub async fn count_by_email_domain(&self, domain: &str) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM iam_principals WHERE type = 'USER' AND email_domain = $1",
        )
        .bind(domain.to_lowercase())
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    /// Insert roles into the junction table via UNNEST
    async fn insert_roles(&self, principal_id: &str, roles: &[RoleAssignment]) -> Result<()> {
        if roles.is_empty() {
            return Ok(());
        }

        let count = roles.len();
        let pids: Vec<String> = std::iter::repeat_n(principal_id.to_string(), count).collect();
        let role_names: Vec<String> = roles.iter().map(|r| r.role.clone()).collect();
        let sources: Vec<Option<&str>> = roles
            .iter()
            .map(|r| r.assignment_source.map(|s| s.as_str()))
            .collect();
        let assigned_ats: Vec<DateTime<Utc>> = roles.iter().map(|r| r.assigned_at).collect();

        sqlx::query(
            "INSERT INTO iam_principal_roles (principal_id, role_name, assignment_source, assigned_at)
             SELECT * FROM UNNEST($1::varchar[], $2::varchar[], $3::varchar[], $4::timestamptz[])"
        )
        .bind(&pids)
        .bind(&role_names)
        .bind(&sources as &[Option<&str>])
        .bind(&assigned_ats)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Hydrate a single principal with roles, client grants, and application access
    async fn hydrate_principal(&self, row: PrincipalRow) -> Result<Principal> {
        let id = row.id.clone();
        let home_client_id = row.client_id.clone();
        let mut principal = Principal::try_from(row)?;

        // Load roles
        let role_rows = sqlx::query_as::<_, PrincipalRoleRow>(
            "SELECT principal_id, role_name, assignment_source, assigned_at
             FROM iam_principal_roles WHERE principal_id = $1 ORDER BY assigned_at",
        )
        .bind(&id)
        .fetch_all(&self.pool)
        .await?;
        principal.roles = role_rows
            .into_iter()
            .map(RoleAssignment::try_from)
            .collect::<Result<_>>()?;

        // Load client access grants
        let grant_rows = sqlx::query_as::<_, ClientAccessGrantRow>(
            "SELECT principal_id, client_id FROM iam_client_access_grants WHERE principal_id = $1",
        )
        .bind(&id)
        .fetch_all(&self.pool)
        .await?;
        let client_ids: Vec<String> = grant_rows.into_iter().map(|g| g.client_id).collect();

        // Collect all client IDs for identifier lookup (grant + home)
        let mut all_client_ids: std::collections::HashSet<String> =
            client_ids.iter().cloned().collect();
        if let Some(ref cid) = home_client_id {
            all_client_ids.insert(cid.clone());
        }

        // Batch-load client identifiers
        let mut identifier_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        if !all_client_ids.is_empty() {
            let ids_vec: Vec<String> = all_client_ids.into_iter().collect();
            let client_rows = sqlx::query_as::<_, ClientIdentifierRow>(
                "SELECT id, identifier FROM tnt_clients WHERE id = ANY($1)",
            )
            .bind(&ids_vec)
            .fetch_all(&self.pool)
            .await?;
            for c in client_rows {
                identifier_map.insert(c.id, c.identifier);
            }
        }
        principal.assigned_clients = client_ids;
        principal.client_identifier_map = identifier_map;

        // Load application access
        // Go's hydrateAppAccess (principal/repository.go:286-322): LEFT JOIN
        // for the code, ordered by application id.
        let app_rows = sqlx::query_as::<_, PrincipalApplicationAccessRow>(
            "SELECT a.principal_id, a.application_id, ap.code AS application_code
             FROM iam_principal_application_access a
             LEFT JOIN app_applications ap ON ap.id = a.application_id
             WHERE a.principal_id = $1
             ORDER BY a.application_id",
        )
        .bind(&id)
        .fetch_all(&self.pool)
        .await?;
        for a in app_rows {
            if let Some(code) = a.application_code.filter(|c| !c.is_empty()) {
                principal
                    .application_code_map
                    .insert(a.application_id.clone(), code);
            }
            principal.accessible_application_ids.push(a.application_id);
        }

        Ok(principal)
    }

    /// Hydrate multiple principals with roles, client grants, and application access (batch)
    async fn hydrate_principals(&self, rows: Vec<PrincipalRow>) -> Result<Vec<Principal>> {
        if rows.is_empty() {
            return Ok(vec![]);
        }

        let principal_ids: Vec<String> = rows.iter().map(|m| m.id.clone()).collect();

        // Batch-load roles
        let all_roles = sqlx::query_as::<_, PrincipalRoleRow>(
            "SELECT principal_id, role_name, assignment_source, assigned_at
             FROM iam_principal_roles WHERE principal_id = ANY($1) ORDER BY assigned_at",
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

        // Batch-load client access grants
        let all_grants = sqlx::query_as::<_, ClientAccessGrantRow>(
            "SELECT principal_id, client_id FROM iam_client_access_grants WHERE principal_id = ANY($1)"
        )
        .bind(&principal_ids)
        .fetch_all(&self.pool)
        .await?;

        let mut grant_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut all_client_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for g in &all_grants {
            all_client_ids.insert(g.client_id.clone());
        }
        // Also include home client IDs so Client-scoped users get "id:identifier"
        for m in &rows {
            if let Some(ref cid) = m.client_id {
                all_client_ids.insert(cid.clone());
            }
        }
        for g in all_grants {
            grant_map
                .entry(g.principal_id)
                .or_default()
                .push(g.client_id);
        }

        // Batch-load client identifiers
        let mut client_id_to_identifier: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        if !all_client_ids.is_empty() {
            let ids_vec: Vec<String> = all_client_ids.into_iter().collect();
            let client_rows = sqlx::query_as::<_, ClientIdentifierRow>(
                "SELECT id, identifier FROM tnt_clients WHERE id = ANY($1)",
            )
            .bind(&ids_vec)
            .fetch_all(&self.pool)
            .await?;
            for c in client_rows {
                client_id_to_identifier.insert(c.id, c.identifier);
            }
        }

        // Batch-load application access
        let all_app_access = sqlx::query_as::<_, PrincipalApplicationAccessRow>(
            "SELECT a.principal_id, a.application_id, ap.code AS application_code
             FROM iam_principal_application_access a
             LEFT JOIN app_applications ap ON ap.id = a.application_id
             WHERE a.principal_id = ANY($1)
             ORDER BY a.application_id",
        )
        .bind(&principal_ids)
        .fetch_all(&self.pool)
        .await?;

        let mut app_access_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut app_code_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for a in all_app_access {
            if let Some(code) = a.application_code.filter(|c| !c.is_empty()) {
                app_code_map.insert(a.application_id.clone(), code);
            }
            app_access_map
                .entry(a.principal_id)
                .or_default()
                .push(a.application_id);
        }

        // Build domain entities
        let principals = rows
            .into_iter()
            .map(|m| {
                let id = m.id.clone();
                let mut principal = Principal::try_from(m)?;
                if let Some(roles) = role_map.remove(&id) {
                    principal.roles = roles;
                }
                // Build client identifier map — include both grant clients and home client
                let mut id_map = std::collections::HashMap::new();
                if let Some(ref home_cid) = principal.client_id {
                    if let Some(ident) = client_id_to_identifier.get(home_cid) {
                        id_map.insert(home_cid.clone(), ident.clone());
                    }
                }
                if let Some(clients) = grant_map.remove(&id) {
                    for cid in &clients {
                        if let Some(ident) = client_id_to_identifier.get(cid) {
                            id_map.insert(cid.clone(), ident.clone());
                        }
                    }
                    principal.assigned_clients = clients;
                }
                principal.client_identifier_map = id_map;
                if let Some(apps) = app_access_map.remove(&id) {
                    principal.application_code_map = apps
                        .iter()
                        .filter_map(|a| app_code_map.get(a).map(|c| (a.clone(), c.clone())))
                        .collect();
                    principal.accessible_application_ids = apps;
                }
                Ok(principal)
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(principals)
    }
}

// ── Persist<PrincipalSyncBatch> ────────────────────────────────────────────
//
// A platform-level user sync writes each user's `iam_principals` row and
// replaces its role set — Go's `RolesPersister` over `CommitSync`
// (principal/operations/sync_principals.go:241-245) — in one UNNEST upsert
// and one UNNEST role insert, whatever the batch size. Client grants and
// application access are not touched by a sync.

impl HasId for crate::principal::entity::PrincipalSyncBatch {
    fn id(&self) -> &str {
        "principals"
    }
}

#[async_trait]
impl crate::usecase::Persist<crate::principal::entity::PrincipalSyncBatch> for PrincipalRepository {
    async fn persist(
        &self,
        batch: &crate::principal::entity::PrincipalSyncBatch,
        tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        let ps = &batch.principals;
        if ps.is_empty() {
            return Ok(());
        }
        let now = Utc::now();
        let ids: Vec<&str> = ps.iter().map(|p| p.id.as_str()).collect();
        let types: Vec<&str> = ps.iter().map(|p| p.principal_type.as_str()).collect();
        let scopes: Vec<&str> = ps.iter().map(|p| p.scope.as_str()).collect();
        let client_ids: Vec<Option<&str>> = ps.iter().map(|p| p.client_id.as_deref()).collect();
        let names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
        let actives: Vec<bool> = ps.iter().map(|p| p.active).collect();
        let emails: Vec<Option<&str>> = ps.iter().map(|p| p.email()).collect();
        let domains: Vec<Option<String>> = ps
            .iter()
            .map(|p| {
                p.email()
                    .map(|e| e.split('@').nth(1).unwrap_or("").to_string())
            })
            .collect();
        let idp_types: Vec<Option<String>> = ps
            .iter()
            .map(|p| {
                p.user_identity
                    .as_ref()
                    .and_then(|i| i.provider.clone())
                    .or_else(|| p.is_user().then(|| "INTERNAL".to_string()))
            })
            .collect();
        let hashes: Vec<Option<&str>> = ps
            .iter()
            .map(|p| {
                p.user_identity
                    .as_ref()
                    .and_then(|i| i.password_hash.as_deref())
            })
            .collect();
        let created: Vec<DateTime<Utc>> = ps.iter().map(|p| p.created_at).collect();
        let all_apps: Vec<bool> = ps.iter().map(|p| p.all_applications).collect();

        sqlx::query(
            "INSERT INTO iam_principals (id, type, scope, client_id, name, active, email, email_domain, idp_type, password_hash, created_at, updated_at, all_applications)
             SELECT u.id, u.type, u.scope, u.client_id, u.name, u.active, u.email, u.email_domain, u.idp_type, u.password_hash, u.created_at, $13, u.all_applications
             FROM UNNEST($1::varchar[], $2::varchar[], $3::varchar[], $4::varchar[], $5::varchar[], $6::bool[], $7::varchar[], $8::varchar[], $9::varchar[], $10::varchar[], $11::timestamptz[], $12::bool[])
                  AS u(id, type, scope, client_id, name, active, email, email_domain, idp_type, password_hash, created_at, all_applications)
             ON CONFLICT (id) DO UPDATE SET
                name = EXCLUDED.name,
                active = EXCLUDED.active,
                password_hash = EXCLUDED.password_hash,
                updated_at = EXCLUDED.updated_at",
        )
        .bind(&ids)
        .bind(&types)
        .bind(&scopes)
        .bind(&client_ids)
        .bind(&names)
        .bind(&actives)
        .bind(&emails)
        .bind(&domains)
        .bind(&idp_types)
        .bind(&hashes)
        .bind(&created)
        .bind(&all_apps)
        .bind(now)
        .execute(&mut **tx.inner)
        .await?;

        sqlx::query("DELETE FROM iam_principal_roles WHERE principal_id = ANY($1)")
            .bind(&ids)
            .execute(&mut **tx.inner)
            .await?;
        let mut role_pids: Vec<&str> = Vec::new();
        let mut role_names: Vec<&str> = Vec::new();
        let mut role_sources: Vec<Option<&str>> = Vec::new();
        let mut role_ats: Vec<DateTime<Utc>> = Vec::new();
        for p in ps {
            for r in &p.roles {
                role_pids.push(&p.id);
                role_names.push(&r.role);
                role_sources.push(r.assignment_source.map(|s| s.as_str()));
                role_ats.push(r.assigned_at);
            }
        }
        if !role_pids.is_empty() {
            sqlx::query(
                "INSERT INTO iam_principal_roles (principal_id, role_name, assignment_source, assigned_at)
                 SELECT * FROM UNNEST($1::varchar[], $2::varchar[], $3::varchar[], $4::timestamptz[])
                 ON CONFLICT DO NOTHING",
            )
            .bind(&role_pids)
            .bind(&role_names)
            .bind(&role_sources)
            .bind(&role_ats)
            .execute(&mut **tx.inner)
            .await?;
        }
        Ok(())
    }

    async fn delete(
        &self,
        _batch: &crate::principal::entity::PrincipalSyncBatch,
        _tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        Err(PlatformError::Internal {
            message: "a principal sync batch is never deleted".to_string(),
        })
    }
}

// ── Persist<Principal> ─────────────────────────────────────────────────────
//
// Per CLAUDE.md § "Layering Rules": the repository persists the aggregate,
// not the other way round. `Principal` itself has no knowledge of how it
// gets stored — all SQL lives here.

impl HasId for Principal {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait]
impl crate::usecase::Persist<Principal> for PrincipalRepository {
    async fn persist(&self, p: &Principal, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();
        let email_domain = p
            .user_identity
            .as_ref()
            .map(|i| i.email.split('@').nth(1).unwrap_or("").to_string());
        let email = p.user_identity.as_ref().map(|i| i.email.clone());
        let idp_type = p
            .user_identity
            .as_ref()
            .and_then(|i| i.provider.clone())
            .or_else(|| {
                if p.is_user() {
                    Some("INTERNAL".to_string())
                } else {
                    None
                }
            });
        let external_idp_id = p.external_identity.as_ref().map(|e| e.external_id.clone());
        let password_hash = p
            .user_identity
            .as_ref()
            .and_then(|i| i.password_hash.clone());
        let last_login_at = p.user_identity.as_ref().and_then(|i| i.last_login_at);

        // 1. Upsert main row
        sqlx::query(
            "INSERT INTO iam_principals (id, type, scope, client_id, application_id, name, active, email, email_domain, idp_type, external_idp_id, password_hash, last_login_at, service_account_id, created_at, updated_at, all_applications)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
             ON CONFLICT (id) DO UPDATE SET
                type = EXCLUDED.type,
                scope = EXCLUDED.scope,
                client_id = EXCLUDED.client_id,
                application_id = EXCLUDED.application_id,
                name = EXCLUDED.name,
                active = EXCLUDED.active,
                email = EXCLUDED.email,
                email_domain = EXCLUDED.email_domain,
                idp_type = EXCLUDED.idp_type,
                external_idp_id = EXCLUDED.external_idp_id,
                password_hash = EXCLUDED.password_hash,
                last_login_at = EXCLUDED.last_login_at,
                service_account_id = EXCLUDED.service_account_id,
                updated_at = EXCLUDED.updated_at,
                all_applications = EXCLUDED.all_applications"
        )
        .bind(&p.id)
        .bind(p.principal_type.as_str())
        .bind(Some(p.scope.as_str()))
        .bind(&p.client_id)
        .bind(&p.application_id)
        .bind(&p.name)
        .bind(p.active)
        .bind(&email)
        .bind(&email_domain)
        .bind(&idp_type)
        .bind(&external_idp_id)
        .bind(&password_hash)
        .bind(last_login_at)
        .bind(&p.service_account_id)
        .bind(now)
        .bind(now)
        .bind(p.all_applications)
        .execute(&mut **tx.inner).await?;

        // 2. Sync roles: delete then re-insert
        sqlx::query("DELETE FROM iam_principal_roles WHERE principal_id = $1")
            .bind(&p.id)
            .execute(&mut **tx.inner)
            .await?;
        for r in &p.roles {
            sqlx::query(
                "INSERT INTO iam_principal_roles (principal_id, role_name, assignment_source, assigned_at)
                 VALUES ($1, $2, $3, $4)"
            )
            .bind(&p.id)
            .bind(&r.role)
            .bind(r.assignment_source.map(|s| s.as_str()))
            .bind(r.assigned_at)
            .execute(&mut **tx.inner).await?;
        }

        // 3. Sync client access grants: delete then re-insert
        sqlx::query("DELETE FROM iam_client_access_grants WHERE principal_id = $1")
            .bind(&p.id)
            .execute(&mut **tx.inner)
            .await?;
        for client_id in &p.assigned_clients {
            sqlx::query(
                "INSERT INTO iam_client_access_grants (id, principal_id, client_id, granted_by, granted_at, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)"
            )
            .bind(crate::shared::tsid::generate(crate::EntityType::Principal))
            .bind(&p.id)
            .bind(client_id)
            .bind(&p.id) // granted_by = self
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(&mut **tx.inner).await?;
        }

        // 4. Sync application access: delete then re-insert
        sqlx::query("DELETE FROM iam_principal_application_access WHERE principal_id = $1")
            .bind(&p.id)
            .execute(&mut **tx.inner)
            .await?;
        for app_id in &p.accessible_application_ids {
            sqlx::query(
                "INSERT INTO iam_principal_application_access (principal_id, application_id, granted_at)
                 VALUES ($1, $2, $3)"
            )
            .bind(&p.id)
            .bind(app_id)
            .bind(now)
            .execute(&mut **tx.inner).await?;
        }

        Ok(())
    }

    async fn delete(&self, p: &Principal, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM iam_principal_roles WHERE principal_id = $1")
            .bind(&p.id)
            .execute(&mut **tx.inner)
            .await?;
        sqlx::query("DELETE FROM iam_client_access_grants WHERE principal_id = $1")
            .bind(&p.id)
            .execute(&mut **tx.inner)
            .await?;
        sqlx::query("DELETE FROM iam_principal_application_access WHERE principal_id = $1")
            .bind(&p.id)
            .execute(&mut **tx.inner)
            .await?;
        sqlx::query("DELETE FROM iam_principals WHERE id = $1")
            .bind(&p.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

// ── Developer API credential (Go's dev_client_secret_ref) ──────────────────

impl HasId for crate::developer_credential::DeveloperCredential {
    fn id(&self) -> &str {
        &self.principal_id
    }
}

/// A USER principal's self-service client_credentials secret lives on its
/// `iam_principals` row, so the principal repository writes it: set or
/// rotate stamps `dev_client_secret_updated_at`, revoke clears both columns
/// (Go `SetDevClientSecretRef` / `ClearDevClientSecretRef`).
#[async_trait::async_trait]
impl crate::usecase::Persist<crate::developer_credential::DeveloperCredential>
    for PrincipalRepository
{
    async fn persist(
        &self,
        c: &crate::developer_credential::DeveloperCredential,
        tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE iam_principals SET dev_client_secret_ref = $2, \
             dev_client_secret_updated_at = CASE WHEN $2::text IS NULL THEN NULL ELSE NOW() END, \
             updated_at = NOW() WHERE id = $1",
        )
        .bind(&c.principal_id)
        .bind(&c.secret_ref)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    async fn delete(
        &self,
        c: &crate::developer_credential::DeveloperCredential,
        tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE iam_principals SET dev_client_secret_ref = NULL, \
             dev_client_secret_updated_at = NULL, updated_at = NOW() WHERE id = $1",
        )
        .bind(&c.principal_id)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}

impl PrincipalRepository {
    /// A principal's developer secret ref and when it was last set.
    pub async fn find_developer_secret(
        &self,
        principal_id: &str,
    ) -> Result<Option<(Option<String>, Option<chrono::DateTime<chrono::Utc>>)>> {
        let row = sqlx::query_as::<_, (Option<String>, Option<chrono::DateTime<chrono::Utc>>)>(
            "SELECT dev_client_secret_ref, dev_client_secret_updated_at FROM iam_principals \
             WHERE id = $1",
        )
        .bind(principal_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// When each of `ids` last set a developer secret (absent: none set).
    pub async fn find_developer_secret_times(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>> {
        let rows = sqlx::query_as::<_, (String, Option<chrono::DateTime<chrono::Utc>>)>(
            "SELECT id, dev_client_secret_updated_at FROM iam_principals \
             WHERE id = ANY($1) AND dev_client_secret_ref IS NOT NULL",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, at)| (id, at.unwrap_or_default()))
            .collect())
    }

    /// Rewrite only the developer secret ref: the lazy at-rest format upgrade
    /// after `/oauth/token` verified an older shape. Like the password
    /// rehash, a storage-format change, not a credential change, so no
    /// event (Go `RewriteDevClientSecretRef`). Callers treat errors as
    /// non-fatal.
    pub async fn rewrite_developer_secret_ref(&self, principal_id: &str, new_ref: &str) -> Result<()> {
        sqlx::query(
            "UPDATE iam_principals SET dev_client_secret_ref = $2, \
             dev_client_secret_updated_at = NOW(), updated_at = NOW() WHERE id = $1",
        )
        .bind(principal_id)
        .bind(new_ref)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

impl PrincipalRepository {
    /// Batch-lookup `(name, email)` by id (email empty for a service
    /// account): the shallow read list pages need.
    pub async fn find_names_and_emails_by_ids(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, (String, String)>> {
        if ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows: Vec<(String, String, Option<String>)> =
            sqlx::query_as("SELECT id, name, email FROM iam_principals WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, email)| (id, (name, email.unwrap_or_default())))
            .collect())
    }
}

impl PrincipalRepository {
    /// The OIDC-federated users of an email domain (Go
    /// `FindUsersByEmailDomain` filtered to `idp_type = 'OIDC'`), for an
    /// email-domain mapping moving to an internal provider.
    pub async fn find_oidc_user_ids_by_email_domain(&self, domain: &str) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM iam_principals \
             WHERE type = 'USER' AND email_domain = lower($1) AND idp_type = 'OIDC' \
             ORDER BY id",
        )
        .bind(domain)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Hand federated users back to the internal provider inside `tx` (Go
    /// `MoveMappingTx`): provider INTERNAL, no external identity, and the
    /// roles their IdP synced dropped.
    pub async fn reset_to_internal_in_tx(
        &self,
        ids: &[String],
        tx: &mut crate::usecase::DbTx<'_>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "UPDATE iam_principals SET idp_type = 'INTERNAL', external_idp_id = NULL, \
             updated_at = NOW() WHERE id = ANY($1)",
        )
        .bind(ids)
        .execute(&mut **tx.inner)
        .await?;
        sqlx::query(
            "DELETE FROM iam_principal_roles \
             WHERE principal_id = ANY($1) AND assignment_source = 'IDP_SYNC'",
        )
        .bind(ids)
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}

impl PrincipalRepository {
    /// Go `LookupVersion` (principal/repository.go:74): the later of the
    /// principal's own `updated_at` and its roles' newest `updated_at`.
    pub async fn lookup_version(&self, id: &str) -> Result<Option<DateTime<Utc>>> {
        let row: Option<(Option<DateTime<Utc>>,)> = sqlx::query_as(
            "SELECT GREATEST(p.updated_at, COALESCE((SELECT MAX(r.updated_at) \
                 FROM iam_principal_roles pr JOIN iam_roles r ON r.name = pr.role_name \
                 WHERE pr.principal_id = p.id), p.updated_at)) \
             FROM iam_principals p WHERE p.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|(v,)| v))
    }

    /// The emails of `emails` that already belong to a principal (lower-cased).
    pub async fn existing_emails(&self, emails: &[String]) -> Result<Vec<String>> {
        if emails.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT lower(email) FROM iam_principals WHERE lower(email) = ANY($1)")
                .bind(emails)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(e,)| e).collect())
    }
}
