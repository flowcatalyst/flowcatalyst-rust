//! Portal identity plane — PostgreSQL via SQLx.
//!
//! Every portal SQL statement lives here (Go `portalidentity/{repository,
//! app}.go`, `portalauth/flow.go`, and the portal slices of the reset-token,
//! OAuth-client and OIDC-login-state stores).
//!
//! Aggregates (`PortalIdentity`, `PortalApp`) are written only through their
//! `Persist` impls. The rest are the plane's infrastructure rows, written
//! directly as Go does: login flows and OIDC states (auth-flow plumbing),
//! reset tokens, the invite bookkeeping, the password set by a confirmed
//! invite/reset, and the last-login stamp.

use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::IdentityProviderId;
use fc_platform_core::shared::id::OAuthClientId;
use fc_platform_core::shared::id::PasswordResetTokenId;
use fc_platform_core::shared::id::PortalAppId;
use fc_platform_core::shared::id::PortalIdentityId;
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{
    AppGrant, IdentitySource, IdentityStatus, LinkedOAuthClient, LoginFlow, PortalApp,
    PortalIdentity, PortalOAuthClient,
};
use fc_platform_core::shared::enum_str::Stored;
use fc_platform_core::shared::error::{PlatformError, Result};
use fc_platform_core::usecase::DbTx;
use fc_platform_core::usecase::Persist;

// ── Portal identities ─────────────────────────────────────────────────────

struct IdentityRow {
    id: PortalIdentityId,
    client_id: ClientId,
    email: String,
    name: Option<String>,
    password_hash: Option<String>,
    status: Stored<IdentityStatus>,
    source: Stored<IdentitySource>,
    last_login_at: Option<DateTime<Utc>>,
    invited_at: Option<DateTime<Utc>>,
    invite_expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<IdentityRow> for PortalIdentity {
    type Error = PlatformError;
    fn try_from(r: IdentityRow) -> Result<Self> {
        // This table's own wording, kept: it names the two statuses.
        let status = r.status.into_result().map_err(|raw| {
            PlatformError::internal(format!(
                "portal_identities.status of {} is not ACTIVE or DISABLED: {raw}",
                r.id
            ))
        })?;
        Ok(PortalIdentity::from_parts(
            r.id,
            r.client_id,
            r.email,
            r.name,
            r.password_hash,
            status,
            // An unrecognised source reads as INVITE (what Go's ensure writes).
            r.source.known().unwrap_or(IdentitySource::Invite),
            r.last_login_at,
            r.invited_at,
            r.invite_expires_at,
            r.created_at,
            r.updated_at,
        ))
    }
}

struct GrantRow {
    identity_id: PortalIdentityId,
    portal_app_id: PortalAppId,
    source: Stored<IdentitySource>,
    granted_at: DateTime<Utc>,
}

/// Narrows a client's identities for the admin list (Go `SearchFilter`).
#[derive(Debug, Clone)]
pub struct IdentitySearch {
    pub client_id: ClientId,
    /// Prefix (TERM%) matched case-insensitively against email and name.
    pub query: String,
    /// Only identities granted this app.
    pub app_id: Option<PortalAppId>,
    /// Only identities granted no app at all.
    pub unassigned: bool,
    pub offset: i64,
    pub limit: i64,
}

pub struct PortalIdentityRepository {
    pool: PgPool,
}

impl PortalIdentityRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// The identity with its app grants.
    pub async fn find_by_id(&self, id: &PortalIdentityId) -> Result<Option<PortalIdentity>> {
        let row = sqlx::query_as!(
            IdentityRow,
            "SELECT pi.id AS \"id: PortalIdentityId\", \
                    pi.client_id AS \"client_id: ClientId\", pi.email, pi.name, \
                    pi.password_hash, pi.status AS \"status: Stored<IdentityStatus>\", \
                    pi.source AS \"source: Stored<IdentitySource>\", pi.last_login_at, \
                    pi.invited_at, pi.invite_expires_at, pi.created_at, pi.updated_at \
                    FROM portal_identities pi WHERE pi.id = $1",
            id as &PortalIdentityId
        )
        .fetch_optional(&self.pool)
        .await?;
        self.one(row).await
    }

    /// The identity for (client, email); the email is matched lower-cased.
    pub async fn find_by_client_and_email(
        &self,
        client_id: &ClientId,
        email: &str,
    ) -> Result<Option<PortalIdentity>> {
        let row = sqlx::query_as!(
            IdentityRow,
            "SELECT pi.id AS \"id: PortalIdentityId\", \
                    pi.client_id AS \"client_id: ClientId\", pi.email, pi.name, \
                    pi.password_hash, pi.status AS \"status: Stored<IdentityStatus>\", \
                    pi.source AS \"source: Stored<IdentitySource>\", pi.last_login_at, \
                    pi.invited_at, pi.invite_expires_at, pi.created_at, pi.updated_at \
                    FROM portal_identities pi \
             WHERE pi.client_id = $1 AND pi.email = $2",
            client_id as &ClientId,
            super::entity::normalize_email(email)
        )
        .fetch_optional(&self.pool)
        .await?;
        self.one(row).await
    }

    async fn one(&self, row: Option<IdentityRow>) -> Result<Option<PortalIdentity>> {
        let Some(row) = row else {
            return Ok(None);
        };
        let mut idents = vec![PortalIdentity::try_from(row)?];
        self.attach_apps(&mut idents).await?;
        Ok(idents.pop())
    }

    /// A client's identities, newest first, with the total match count.
    pub async fn search(&self, f: &IdentitySearch) -> Result<(Vec<PortalIdentity>, i64)> {
        let query = f.query.trim().to_lowercase();
        let pattern = (!query.is_empty()).then(|| format!("{}%", escape_like(&query)));
        // One statement shape; the optional filters switch off when NULL.
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) AS \"count!\" FROM portal_identities pi WHERE pi.client_id = $1 \
             AND ($2::text IS NULL OR pi.email LIKE $2 OR lower(pi.name) LIKE $2) \
             AND ($3::text IS NULL OR EXISTS (SELECT 1 FROM portal_identity_apps g \
                  WHERE g.identity_id = pi.id AND g.portal_app_id = $3)) \
             AND (NOT $4 OR NOT EXISTS (SELECT 1 FROM portal_identity_apps g WHERE g.identity_id = pi.id))",
            &f.client_id as &ClientId,
            pattern.as_ref(),
            &f.app_id as &Option<PortalAppId>,
            f.unassigned
        )
            .fetch_one(&self.pool);
        let page = sqlx::query_as!(
            IdentityRow,
            "SELECT pi.id AS \"id: PortalIdentityId\", \
                    pi.client_id AS \"client_id: ClientId\", pi.email, pi.name, \
                    pi.password_hash, pi.status AS \"status: Stored<IdentityStatus>\", \
                    pi.source AS \"source: Stored<IdentitySource>\", pi.last_login_at, \
                    pi.invited_at, pi.invite_expires_at, pi.created_at, pi.updated_at \
                    FROM portal_identities pi WHERE pi.client_id = $1 \
             AND ($2::text IS NULL OR pi.email LIKE $2 OR lower(pi.name) LIKE $2) \
             AND ($3::text IS NULL OR EXISTS (SELECT 1 FROM portal_identity_apps g \
                  WHERE g.identity_id = pi.id AND g.portal_app_id = $3)) \
             AND (NOT $4 OR NOT EXISTS (SELECT 1 FROM portal_identity_apps g WHERE g.identity_id = pi.id)) \
             ORDER BY pi.created_at DESC, pi.id DESC LIMIT $5 OFFSET $6",
            &f.client_id as &ClientId,
            pattern.as_ref(),
            &f.app_id as &Option<PortalAppId>,
            f.unassigned,
            f.limit,
            f.offset
        )
            .fetch_all(&self.pool);
        let (total, rows) = tokio::try_join!(count, page)?;
        let mut idents = rows
            .into_iter()
            .map(PortalIdentity::try_from)
            .collect::<Result<Vec<_>>>()?;
        self.attach_apps(&mut idents).await?;
        Ok((idents, total))
    }

    /// A client's identities holding no portal-app grant, oldest first.
    pub async fn find_unassigned(&self, client_id: &ClientId) -> Result<Vec<PortalIdentity>> {
        let rows = sqlx::query_as!(
            IdentityRow,
            "SELECT pi.id AS \"id: PortalIdentityId\", \
                    pi.client_id AS \"client_id: ClientId\", pi.email, pi.name, \
                    pi.password_hash, pi.status AS \"status: Stored<IdentityStatus>\", \
                    pi.source AS \"source: Stored<IdentitySource>\", pi.last_login_at, \
                    pi.invited_at, pi.invite_expires_at, pi.created_at, pi.updated_at \
                    FROM portal_identities pi \
             WHERE pi.client_id = $1 AND NOT EXISTS (SELECT 1 FROM portal_identity_apps g WHERE g.identity_id = pi.id) ORDER BY pi.created_at, pi.id",
            client_id as &ClientId
        )
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(PortalIdentity::try_from).collect()
    }

    /// How many of a client's identities hold no portal-app grant.
    pub async fn count_unassigned(&self, client_id: &ClientId) -> Result<i64> {
        Ok(sqlx::query_scalar!(
            "SELECT COUNT(*) AS \"count!\" FROM portal_identities pi WHERE pi.client_id = $1 AND NOT EXISTS (SELECT 1 FROM portal_identity_apps g WHERE g.identity_id = pi.id)",
            client_id as &ClientId
        )
            .fetch_one(&self.pool)
            .await?)
    }

    /// Load the app grants of a page of identities in one query.
    async fn attach_apps(&self, idents: &mut [PortalIdentity]) -> Result<()> {
        if idents.is_empty() {
            return Ok(());
        }
        let ids: Vec<PortalIdentityId> = idents.iter().map(|i| i.id.clone()).collect();
        let rows = sqlx::query_as!(
            GrantRow,
            "SELECT identity_id AS \"identity_id: PortalIdentityId\", \
                    portal_app_id AS \"portal_app_id: PortalAppId\", \
                    source AS \"source: Stored<IdentitySource>\", granted_at \
                    FROM portal_identity_apps \
             WHERE identity_id = ANY($1) ORDER BY granted_at",
            &ids as &[PortalIdentityId]
        )
        .fetch_all(&self.pool)
        .await?;
        let mut by_identity: HashMap<PortalIdentityId, Vec<AppGrant>> = HashMap::new();
        for r in rows {
            by_identity
                .entry(r.identity_id)
                .or_default()
                .push(AppGrant {
                    app_id: r.portal_app_id,
                    source: r.source.known().unwrap_or(IdentitySource::Invite),
                    granted_at: r.granted_at,
                });
        }
        for ident in idents.iter_mut() {
            ident.apps = by_identity.remove(&ident.id).unwrap_or_default();
        }
        Ok(())
    }

    /// Best-effort stamp of a successful login.
    pub async fn touch_last_login(&self, id: &PortalIdentityId) -> Result<()> {
        sqlx::query!(
            "UPDATE portal_identities SET last_login_at = NOW(), updated_at = NOW() WHERE id = $1",
            id as &PortalIdentityId
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Record the latest invite. `expires_at` `None` = an SSO invite.
    pub async fn mark_invited(
        &self,
        id: &PortalIdentityId,
        at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        sqlx::query!(
            "UPDATE portal_identities SET invited_at = $2, invite_expires_at = $3, \
             updated_at = NOW() WHERE id = $1",
            id as &PortalIdentityId,
            at,
            expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Write a freshly set password (the invite/reset confirm path).
    pub async fn set_password_hash(&self, id: &PortalIdentityId, hash: &str) -> Result<()> {
        let done = sqlx::query!(
            "UPDATE portal_identities SET password_hash = $2, updated_at = NOW() WHERE id = $1",
            id as &PortalIdentityId,
            hash
        )
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Err(PlatformError::internal("portal identity not found"));
        }
        Ok(())
    }
}

/// Neutralise LIKE metacharacters (backslash is Postgres' default escape).
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

impl Persist<PortalIdentity> for PortalIdentityRepository {
    /// Conflict on (client, email) updates the mutable fields, so re-ensuring
    /// keeps the original id/source/created_at. Grants then apply against the
    /// id that actually holds the row: revoked grants are deleted, every
    /// grant in `apps` is inserted if missing, nothing else is touched.
    async fn persist(&self, i: &PortalIdentity, tx: &mut DbTx<'_>) -> Result<()> {
        let name = (!i.name.is_empty()).then_some(i.name.as_str());
        let row_id: PortalIdentityId = sqlx::query_scalar!(
            "INSERT INTO portal_identities \
                 (id, client_id, email, name, password_hash, status, source, last_login_at, \
                  created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (client_id, email) DO UPDATE SET \
                 name = EXCLUDED.name, \
                 status = EXCLUDED.status, \
                 updated_at = EXCLUDED.updated_at \
             RETURNING id AS \"id: PortalIdentityId\"",
            &i.id as &PortalIdentityId,
            &i.client_id as &ClientId,
            &i.email,
            name,
            i.password_hash.as_ref(),
            i.status as IdentityStatus,
            i.source as IdentitySource,
            i.last_login_at,
            i.created_at,
            Utc::now()
        )
        .fetch_one(&mut **tx.inner)
        .await?;

        if !i.revoked_apps().is_empty() {
            sqlx::query!(
                "DELETE FROM portal_identity_apps \
                 WHERE identity_id = $1 AND portal_app_id = ANY($2)",
                &row_id as &PortalIdentityId,
                i.revoked_apps() as &[PortalAppId]
            )
            .execute(&mut **tx.inner)
            .await?;
        }
        if !i.apps.is_empty() {
            let app_ids: Vec<&PortalAppId> = i.apps.iter().map(|g| &g.app_id).collect();
            let sources: Vec<IdentitySource> = i.apps.iter().map(|g| g.source).collect();
            let granted: Vec<DateTime<Utc>> = i.apps.iter().map(|g| g.granted_at).collect();
            sqlx::query!(
            "INSERT INTO portal_identity_apps (identity_id, portal_app_id, source, granted_at) \
                 SELECT $1, a, s, g FROM UNNEST($2::text[], $3::text[], $4::timestamptz[]) \
                     AS t(a, s, g) \
                 ON CONFLICT DO NOTHING",
            &row_id as &PortalIdentityId,
            &app_ids as &[&PortalAppId],
            &sources as &[IdentitySource],
            &granted
        )
            .execute(&mut **tx.inner)
            .await?;
        }
        Ok(())
    }

    /// Offboarding is deleting the row; grants cascade.
    async fn delete(&self, i: &PortalIdentity, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM portal_identities WHERE id = $1",
            &i.id as &PortalIdentityId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}

// ── Portal apps ───────────────────────────────────────────────────────────

struct AppRow {
    id: PortalAppId,
    client_id: ClientId,
    code: String,
    name: String,
    description: Option<String>,
    active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<AppRow> for PortalApp {
    fn from(r: AppRow) -> Self {
        Self {
            id: r.id,
            client_id: r.client_id,
            code: r.code,
            name: r.name,
            description: r.description,
            active: r.active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

struct LinkedRow {
    portal_app_id: PortalAppId,
    id: String,
    client_id: String,
    client_name: String,
}

pub struct PortalAppRepository {
    pool: PgPool,
}

impl PortalAppRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn find_by_id(&self, id: &PortalAppId) -> Result<Option<PortalApp>> {
        let row = sqlx::query_as!(
            AppRow,
            "SELECT pa.id AS \"id: PortalAppId\", \
                    pa.client_id AS \"client_id: ClientId\", pa.code, pa.name, \
                    pa.description, pa.active, pa.created_at, pa.updated_at \
                    FROM portal_apps pa WHERE pa.id = $1",
            id as &PortalAppId
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(PortalApp::from))
    }

    /// The client's app with that (normalised) code.
    pub async fn find_by_client_and_code(
        &self,
        client_id: &ClientId,
        code: &str,
    ) -> Result<Option<PortalApp>> {
        let row = sqlx::query_as!(
            AppRow,
            "SELECT pa.id AS \"id: PortalAppId\", \
                    pa.client_id AS \"client_id: ClientId\", pa.code, pa.name, \
                    pa.description, pa.active, pa.created_at, pa.updated_at \
                    FROM portal_apps pa WHERE pa.client_id = $1 AND pa.code = $2",
            client_id as &ClientId,
            super::entity::normalize_app_code(code)
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(PortalApp::from))
    }

    /// The app the OAuth client (by its public `client_id`) is linked to.
    pub async fn find_by_oauth_client_id(
        &self,
        oauth_client_id: &str,
    ) -> Result<Option<PortalApp>> {
        let row = sqlx::query_as!(
            AppRow,
            "SELECT pa.id AS \"id: PortalAppId\", \
                    pa.client_id AS \"client_id: ClientId\", pa.code, pa.name, \
                    pa.description, pa.active, pa.created_at, pa.updated_at \
                    FROM portal_apps pa JOIN oauth_clients oc ON oc.portal_app_id = pa.id \
             WHERE oc.client_id = $1",
            oauth_client_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(PortalApp::from))
    }

    /// A client's apps by name; `None` lists every client's (anchor views).
    pub async fn find_by_client(&self, client_id: Option<&ClientId>) -> Result<Vec<PortalApp>> {
        let rows = sqlx::query_as!(
            AppRow,
            "SELECT pa.id AS \"id: PortalAppId\", \
                    pa.client_id AS \"client_id: ClientId\", pa.code, pa.name, \
                    pa.description, pa.active, pa.created_at, pa.updated_at \
                    FROM portal_apps pa WHERE ($1::text IS NULL OR pa.client_id = $1) ORDER BY pa.name",
            client_id as Option<&ClientId>
        )
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(PortalApp::from).collect())
    }

    /// The number of identities granted each app.
    pub async fn grant_counts(&self, app_ids: &[PortalAppId]) -> Result<HashMap<PortalAppId, i64>> {
        if app_ids.is_empty() {
            return Ok(HashMap::new());
        }
        // `portal_app_id!`: `= ANY($1)` never matches a NULL.
        let rows = sqlx::query!(
            "SELECT portal_app_id AS \"portal_app_id!: PortalAppId\", COUNT(*) AS \"c1!\" FROM portal_identity_apps \
             WHERE portal_app_id = ANY($1) GROUP BY portal_app_id",
            app_ids as &[PortalAppId]
        )
        .fetch_all(&self.pool)
        .await?.into_iter().map(|r| (r.portal_app_id, r.c1)).collect::<Vec<_>>();
        Ok(rows.into_iter().collect())
    }

    /// The OAuth clients linked to each app, by client name.
    pub async fn linked_oauth_clients(
        &self,
        app_ids: &[PortalAppId],
    ) -> Result<HashMap<PortalAppId, Vec<LinkedOAuthClient>>> {
        let mut out: HashMap<PortalAppId, Vec<LinkedOAuthClient>> = HashMap::new();
        if app_ids.is_empty() {
            return Ok(out);
        }
        // `portal_app_id!`: `= ANY($1)` never matches a NULL.
        let rows = sqlx::query_as!(
            LinkedRow,
            "SELECT portal_app_id AS \"portal_app_id!: PortalAppId\", id, client_id, \
                    client_name \
                    FROM oauth_clients \
             WHERE portal_app_id = ANY($1) ORDER BY client_name",
            app_ids as &[PortalAppId]
        )
        .fetch_all(&self.pool)
        .await?;
        for r in rows {
            out.entry(r.portal_app_id)
                .or_default()
                .push(LinkedOAuthClient {
                    id: r.id,
                    client_id: r.client_id,
                    client_name: r.client_name,
                });
        }
        Ok(out)
    }
}

impl Persist<PortalApp> for PortalAppRepository {
    async fn persist(&self, a: &PortalApp, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "INSERT INTO portal_apps \
                 (id, client_id, code, name, description, active, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (id) DO UPDATE SET \
                 name = EXCLUDED.name, \
                 description = EXCLUDED.description, \
                 active = EXCLUDED.active, \
                 updated_at = EXCLUDED.updated_at",
            &a.id as &PortalAppId,
            &a.client_id as &ClientId,
            &a.code,
            &a.name,
            a.description.as_ref(),
            a.active,
            a.created_at,
            Utc::now()
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }

    /// The app's grants go with it (FK cascade).
    async fn delete(&self, a: &PortalApp, tx: &mut DbTx<'_>) -> Result<()> {
        sqlx::query!(
            "DELETE FROM portal_apps WHERE id = $1",
            &a.id as &PortalAppId
        )
        .execute(&mut **tx.inner)
        .await?;
        Ok(())
    }
}

// ── Portal-flagged OAuth clients (reads) ──────────────────────────────────

struct PortalOAuthClientRow {
    id: OAuthClientId,
    client_id: String,
    client_name: String,
    active: bool,
    pkce_required: bool,
    // Read as stored: a legacy row may hold a blank here, which strict id
    // decoding would refuse and the portal plane treats as "not set".
    portal_client_id: Option<String>,
    portal_app_id: Option<String>,
}

/// The portal plane's reads of `oauth_clients` (the portal columns the
/// shared OAuth client repository does not carry everywhere). Writes stay in
/// `OAuthClientRepository`.
pub struct PortalOAuthClientReader {
    pool: PgPool,
}

impl PortalOAuthClientReader {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// The OAuth client by its public `client_id`.
    pub async fn find_by_client_id(&self, client_id: &str) -> Result<Option<PortalOAuthClient>> {
        let row = sqlx::query_as!(
            PortalOAuthClientRow,
            "SELECT id AS \"id: OAuthClientId\", client_id, client_name, active, \
                    pkce_required, portal_client_id, portal_app_id \
                    FROM oauth_clients WHERE client_id = $1",
            client_id
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(self.hydrate(vec![row]).await?.pop())
    }

    /// Portal-flagged OAuth clients owned by a tenant client, by name (Go
    /// `OAuthClientRepo.FindByPortalClient`).
    pub async fn find_by_portal_client(
        &self,
        client_id: &ClientId,
    ) -> Result<Vec<PortalOAuthClient>> {
        let rows = sqlx::query_as!(
            PortalOAuthClientRow,
            "SELECT id AS \"id: OAuthClientId\", client_id, client_name, active, \
                    pkce_required, portal_client_id, portal_app_id \
                    FROM oauth_clients WHERE portal_client_id = $1 ORDER BY client_name",
            client_id as &ClientId
        )
        .fetch_all(&self.pool)
        .await?;
        self.hydrate(rows).await
    }

    async fn hydrate(&self, rows: Vec<PortalOAuthClientRow>) -> Result<Vec<PortalOAuthClient>> {
        let ids: Vec<OAuthClientId> = rows.iter().map(|r| r.id.clone()).collect();
        let mut uris: HashMap<OAuthClientId, Vec<String>> = HashMap::new();
        if !ids.is_empty() {
            let pairs = sqlx::query!(
            "SELECT oauth_client_id AS \"oauth_client_id: OAuthClientId\", redirect_uri FROM oauth_client_redirect_uris \
                 WHERE oauth_client_id = ANY($1) ORDER BY redirect_uri",
            &ids as &[OAuthClientId]
        )
            .fetch_all(&self.pool)
            .await?.into_iter().map(|r| (r.oauth_client_id, r.redirect_uri)).collect::<Vec<_>>();
            for (id, uri) in pairs {
                uris.entry(id).or_default().push(uri);
            }
        }
        Ok(rows
            .into_iter()
            .map(|r| PortalOAuthClient {
                redirect_uris: uris.remove(&r.id).unwrap_or_default(),
                id: r.id,
                client_id: r.client_id,
                client_name: r.client_name,
                active: r.active,
                pkce_required: r.pkce_required,
                portal_client_id: r.portal_client_id.map(ClientId::from_wire),
                portal_app_id: r.portal_app_id.map(PortalAppId::from_wire),
            })
            .collect())
    }
}

// ── Portal login flows (infrastructure) ───────────────────────────────────

struct FlowRow {
    id: String,
    oauth_client_id: String,
    portal_client_id: ClientId,
    redirect_uri: String,
    scope: Option<String>,
    state: String,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl From<FlowRow> for LoginFlow {
    fn from(r: FlowRow) -> Self {
        Self {
            id: r.id,
            oauth_client_id: r.oauth_client_id,
            portal_client_id: r.portal_client_id,
            redirect_uri: r.redirect_uri,
            scope: r.scope,
            state: r.state,
            nonce: r.nonce,
            code_challenge: r.code_challenge,
            code_challenge_method: r.code_challenge_method,
            created_at: r.created_at,
            expires_at: r.expires_at,
        }
    }
}

/// `portal_login_flows`: parked `/portal/authorize` chains.
pub struct PortalFlowRepository {
    pool: PgPool,
}

impl PortalFlowRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Park a fresh flow.
    pub async fn park(&self, f: &LoginFlow) -> Result<()> {
        sqlx::query!(
            "INSERT INTO portal_login_flows \
                 (id, oauth_client_id, portal_client_id, redirect_uri, scope, state, \
                  nonce, code_challenge, code_challenge_method, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            &f.id,
            &f.oauth_client_id,
            &f.portal_client_id as &ClientId,
            &f.redirect_uri,
            f.scope.as_ref(),
            &f.state,
            f.nonce.as_ref(),
            f.code_challenge.as_ref(),
            f.code_challenge_method.as_ref(),
            f.created_at,
            f.expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// A live flow, NOT consumed (a failed password attempt must not burn it).
    pub async fn find_live(&self, id: &str) -> Result<Option<LoginFlow>> {
        let row = sqlx::query_as!(
            FlowRow,
            "SELECT id, oauth_client_id, \
                    portal_client_id AS \"portal_client_id: ClientId\", redirect_uri, \
                    scope, state, nonce, code_challenge, code_challenge_method, \
                    created_at, expires_at \
                    FROM portal_login_flows WHERE id = $1 AND expires_at > NOW()",
            id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(LoginFlow::from))
    }

    /// Atomically delete and return the live flow (single use).
    pub async fn consume(&self, id: &str) -> Result<Option<LoginFlow>> {
        let row = sqlx::query_as!(
            FlowRow,
            "DELETE FROM portal_login_flows WHERE id = $1 AND expires_at > NOW() \
             RETURNING id, oauth_client_id, portal_client_id AS \"portal_client_id: ClientId\", redirect_uri, scope, state, nonce, code_challenge, code_challenge_method, created_at, expires_at",
            id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(LoginFlow::from))
    }

    /// Remove expired flows (janitor).
    pub async fn purge_expired(&self) -> Result<u64> {
        let done = sqlx::query!("DELETE FROM portal_login_flows WHERE expires_at <= NOW()")
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }
}

// ── Reset / invite tokens keyed by portal identities (infrastructure) ────

/// One `iam_password_reset_tokens` row whose subject is a portal identity.
#[derive(Debug, Clone)]
pub struct PortalResetToken {
    pub id: PasswordResetTokenId,
    pub identity_id: PortalIdentityId,
    pub expires_at: DateTime<Utc>,
    pub redirect_uri: Option<String>,
}

/// The table holds principal and portal-identity subjects in one column, so
/// the subject is read as stored and only a `ptu_` one becomes a token here.
struct ResetTokenRow {
    id: PasswordResetTokenId,
    principal_id: String,
    expires_at: DateTime<Utc>,
    redirect_uri: Option<String>,
}

impl PortalResetToken {
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

/// The portal slice of `iam_password_reset_tokens`: tokens whose subject is a
/// `ptu_…` identity (Go shares the table and the confirm endpoint).
pub struct PortalResetTokenRepository {
    pool: PgPool,
}

impl PortalResetTokenRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Invalidate every outstanding token of the subject.
    pub async fn delete_for_subject(&self, subject_id: &PortalIdentityId) -> Result<()> {
        sqlx::query!(
            "DELETE FROM iam_password_reset_tokens WHERE principal_id = $1",
            subject_id as &PortalIdentityId
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Store a fresh token (`purpose` is Go's `reset` or `invite`).
    pub async fn issue(
        &self,
        subject_id: &PortalIdentityId,
        token_hash: &str,
        expires_at: DateTime<Utc>,
        purpose: &str,
        redirect_uri: Option<&str>,
    ) -> Result<()> {
        sqlx::query!(
            "INSERT INTO iam_password_reset_tokens \
                 (id, principal_id, token_hash, expires_at, created_at, purpose, redirect_uri) \
             VALUES ($1, $2, $3, $4, NOW(), $5, $6)",
            PasswordResetTokenId::generate() as PasswordResetTokenId,
            subject_id as &PortalIdentityId,
            token_hash,
            expires_at,
            purpose,
            redirect_uri
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The token with that hash, when its subject is a portal identity (a
    /// principal's token is the shared reset handlers' to answer).
    pub async fn find_by_hash(&self, token_hash: &str) -> Result<Option<PortalResetToken>> {
        let row = sqlx::query_as!(
            ResetTokenRow,
            "SELECT id AS \"id: PasswordResetTokenId\", principal_id, expires_at, \
                    redirect_uri \
                    FROM iam_password_reset_tokens \
             WHERE token_hash = $1",
            token_hash
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|r| {
            Some(PortalResetToken {
                identity_id: PortalIdentityId::parse(r.principal_id).ok()?,
                id: r.id,
                expires_at: r.expires_at,
                redirect_uri: r.redirect_uri,
            })
        }))
    }
}

// ── Portal OIDC login states (infrastructure) ─────────────────────────────

/// A portal-flagged `oauth_oidc_login_states` row: the IdP handshake of a
/// portal SSO login, carrying the flow's OAuth chain.
#[derive(Debug, Clone)]
pub struct PortalOidcState {
    pub state: String,
    pub identity_provider_id: IdentityProviderId,
    pub nonce: String,
    pub code_verifier: String,
    pub portal_client_id: Option<ClientId>,
    pub oauth_client_id: Option<String>,
    pub oauth_redirect_uri: Option<String>,
    pub oauth_scope: Option<String>,
    pub oauth_state: Option<String>,
    pub oauth_code_challenge: Option<String>,
    pub oauth_code_challenge_method: Option<String>,
    pub oauth_nonce: Option<String>,
}

pub struct PortalOidcStateRepository {
    pool: PgPool,
}

impl PortalOidcStateRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Store the handshake. The email-domain and mapping columns are empty:
    /// a portal login is provider-direct (Go `NewLoginState(state, "",
    /// idp.ID, "", …)`).
    pub async fn park(&self, s: &PortalOidcState, expires_at: DateTime<Utc>) -> Result<()> {
        sqlx::query!(
            "INSERT INTO oauth_oidc_login_states \
                 (state, email_domain, identity_provider_id, email_domain_mapping_id, nonce, \
                  code_verifier, oauth_client_id, oauth_redirect_uri, oauth_scope, oauth_state, \
                  oauth_code_challenge, oauth_code_challenge_method, oauth_nonce, \
                  created_at, expires_at, portal_client_id) \
             VALUES ($1, '', $2, '', $3, $4, $5, $6, $7, $8, $9, $10, $11, NOW(), $12, $13)",
            &s.state,
            &s.identity_provider_id as &IdentityProviderId,
            &s.nonce,
            &s.code_verifier,
            s.oauth_client_id.as_ref(),
            s.oauth_redirect_uri.as_ref(),
            s.oauth_scope.as_ref(),
            s.oauth_state.as_ref(),
            s.oauth_code_challenge.as_ref(),
            s.oauth_code_challenge_method.as_ref(),
            s.oauth_nonce.as_ref(),
            expires_at,
            &s.portal_client_id as &Option<ClientId>
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Atomically consume the state IF it is a live portal-plane handshake;
    /// an employee-plane state is left for the employee callback.
    pub async fn consume_portal(&self, state: &str) -> Result<Option<PortalOidcState>> {
        Ok(sqlx::query_as!(
            PortalOidcState,
            "DELETE FROM oauth_oidc_login_states \
             WHERE state = $1 AND portal_client_id IS NOT NULL AND portal_client_id <> '' \
               AND expires_at > NOW() \
             RETURNING state, identity_provider_id AS \"identity_provider_id: IdentityProviderId\", nonce, code_verifier, portal_client_id AS \"portal_client_id: ClientId\", oauth_client_id, oauth_redirect_uri, oauth_scope, oauth_state, oauth_code_challenge, oauth_code_challenge_method, oauth_nonce",
            state
        )
        .fetch_optional(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::escape_like;

    #[test]
    fn like_metacharacters_are_escaped() {
        assert_eq!(escape_like("under_score"), "under\\_score");
        assert_eq!(escape_like("50%"), "50\\%");
        assert_eq!(escape_like("a\\b"), "a\\\\b");
    }
}
