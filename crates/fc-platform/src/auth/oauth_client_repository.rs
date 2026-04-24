//! OAuth Client Repository — PostgreSQL via SQLx
//!
//! Includes an in-memory TTL cache for `find_by_client_id` (the hot path
//! during OAuth authorize/token flows), matching the TS oidc-provider
//! adapter caching pattern.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use async_trait::async_trait;
use sqlx::PgPool;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::auth::oauth_entity::{OAuthClient, OAuthClientType, GrantType};
use crate::shared::error::Result;

// ── Row structs ─────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct OAuthClientRow {
    id: String,
    client_id: String,
    client_name: String,
    client_type: String,
    client_secret_ref: Option<String>,
    default_scopes: Option<String>,
    pkce_required: bool,
    service_account_principal_id: Option<String>,
    active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<OAuthClientRow> for OAuthClient {
    fn from(r: OAuthClientRow) -> Self {
        let default_scopes: Vec<String> = r.default_scopes
            .map(|s| s.split(',').filter(|v| !v.is_empty()).map(String::from).collect())
            .unwrap_or_default();

        Self {
            id: r.id,
            client_id: r.client_id,
            client_name: r.client_name,
            client_type: OAuthClientType::from_str(&r.client_type),
            client_secret_ref: r.client_secret_ref,
            redirect_uris: vec![],   // loaded separately
            grant_types: vec![],     // loaded separately
            default_scopes,
            pkce_required: r.pkce_required,
            application_ids: vec![], // loaded separately
            allowed_origins: vec![], // loaded separately
            service_account_principal_id: r.service_account_principal_id,
            active: r.active,
            created_at: r.created_at,
            updated_at: r.updated_at,
            created_by: None,
        }
    }
}

struct CacheEntry {
    client: OAuthClient,
    inserted_at: Instant,
}

pub struct OAuthClientRepository {
    pool: PgPool,
    /// In-memory cache keyed by client_id (public OAuth identifier)
    cache_by_client_id: RwLock<HashMap<String, CacheEntry>>,
    cache_ttl: Duration,
}

impl OAuthClientRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            pool: pool.clone(),
            cache_by_client_id: RwLock::new(HashMap::new()),
            cache_ttl: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Invalidate cache entries for a client
    async fn invalidate_cache(&self, client: &OAuthClient) {
        self.cache_by_client_id.write().await.remove(&client.client_id);
    }

    // ── Junction table helpers ───────────────────────────────────

    async fn load_redirect_uris(&self, oauth_client_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT redirect_uri FROM oauth_client_redirect_uris WHERE oauth_client_id = $1"
        )
        .bind(oauth_client_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn load_grant_types(&self, oauth_client_id: &str) -> Result<Vec<GrantType>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT grant_type FROM oauth_client_grant_types WHERE oauth_client_id = $1"
        )
        .bind(oauth_client_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter_map(|s| GrantType::from_str(&s)).collect())
    }

    async fn load_application_ids(&self, oauth_client_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT application_id FROM oauth_client_application_ids WHERE oauth_client_id = $1"
        )
        .bind(oauth_client_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn load_allowed_origins(&self, oauth_client_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT allowed_origin FROM oauth_client_allowed_origins WHERE oauth_client_id = $1"
        )
        .bind(oauth_client_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    async fn hydrate(&self, mut client: OAuthClient) -> Result<OAuthClient> {
        let (uris, grants, app_ids, origins) = tokio::try_join!(
            self.load_redirect_uris(&client.id),
            self.load_grant_types(&client.id),
            self.load_application_ids(&client.id),
            self.load_allowed_origins(&client.id),
        )?;
        client.redirect_uris = uris;
        client.grant_types = grants;
        client.application_ids = app_ids;
        client.allowed_origins = origins;
        Ok(client)
    }

    /// Batch-hydrate junction tables for multiple clients (avoids N+1)
    async fn hydrate_all(&self, mut clients: Vec<OAuthClient>) -> Result<Vec<OAuthClient>> {
        if clients.is_empty() {
            return Ok(clients);
        }

        let ids: Vec<&str> = clients.iter().map(|c| c.id.as_str()).collect();

        // Batch-load all junction tables concurrently
        #[derive(sqlx::FromRow)]
        struct UriRow { oauth_client_id: String, redirect_uri: String }
        #[derive(sqlx::FromRow)]
        struct GrantRow { oauth_client_id: String, grant_type: String }
        #[derive(sqlx::FromRow)]
        struct AppRow { oauth_client_id: String, application_id: String }
        #[derive(sqlx::FromRow)]
        struct OriginRow { oauth_client_id: String, allowed_origin: String }

        let (uri_rows, grant_rows, app_rows, origin_rows) = tokio::try_join!(
            sqlx::query_as::<_, UriRow>(
                "SELECT oauth_client_id, redirect_uri FROM oauth_client_redirect_uris WHERE oauth_client_id = ANY($1)"
            ).bind(&ids).fetch_all(&self.pool),
            sqlx::query_as::<_, GrantRow>(
                "SELECT oauth_client_id, grant_type FROM oauth_client_grant_types WHERE oauth_client_id = ANY($1)"
            ).bind(&ids).fetch_all(&self.pool),
            sqlx::query_as::<_, AppRow>(
                "SELECT oauth_client_id, application_id FROM oauth_client_application_ids WHERE oauth_client_id = ANY($1)"
            ).bind(&ids).fetch_all(&self.pool),
            sqlx::query_as::<_, OriginRow>(
                "SELECT oauth_client_id, allowed_origin FROM oauth_client_allowed_origins WHERE oauth_client_id = ANY($1)"
            ).bind(&ids).fetch_all(&self.pool),
        )?;

        // Group by parent ID
        let mut uri_map: HashMap<String, Vec<String>> = HashMap::new();
        for r in uri_rows { uri_map.entry(r.oauth_client_id).or_default().push(r.redirect_uri); }

        let mut grant_map: HashMap<String, Vec<GrantType>> = HashMap::new();
        for r in grant_rows {
            if let Some(gt) = GrantType::from_str(&r.grant_type) {
                grant_map.entry(r.oauth_client_id).or_default().push(gt);
            }
        }

        let mut app_map: HashMap<String, Vec<String>> = HashMap::new();
        for r in app_rows { app_map.entry(r.oauth_client_id).or_default().push(r.application_id); }

        let mut origin_map: HashMap<String, Vec<String>> = HashMap::new();
        for r in origin_rows { origin_map.entry(r.oauth_client_id).or_default().push(r.allowed_origin); }

        for client in &mut clients {
            if let Some(v) = uri_map.remove(&client.id) { client.redirect_uris = v; }
            if let Some(v) = grant_map.remove(&client.id) { client.grant_types = v; }
            if let Some(v) = app_map.remove(&client.id) { client.application_ids = v; }
            if let Some(v) = origin_map.remove(&client.id) { client.allowed_origins = v; }
        }

        Ok(clients)
    }

    async fn save_redirect_uris(&self, oauth_client_id: &str, uris: &[String]) -> Result<()> {
        sqlx::query("DELETE FROM oauth_client_redirect_uris WHERE oauth_client_id = $1")
            .bind(oauth_client_id)
            .execute(&self.pool)
            .await?;
        for uri in uris {
            sqlx::query(
                "INSERT INTO oauth_client_redirect_uris (oauth_client_id, redirect_uri) VALUES ($1, $2)"
            )
            .bind(oauth_client_id)
            .bind(uri)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn save_grant_types(&self, oauth_client_id: &str, grant_types: &[GrantType]) -> Result<()> {
        sqlx::query("DELETE FROM oauth_client_grant_types WHERE oauth_client_id = $1")
            .bind(oauth_client_id)
            .execute(&self.pool)
            .await?;
        for gt in grant_types {
            sqlx::query(
                "INSERT INTO oauth_client_grant_types (oauth_client_id, grant_type) VALUES ($1, $2)"
            )
            .bind(oauth_client_id)
            .bind(gt.as_str())
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn save_allowed_origins(&self, oauth_client_id: &str, origins: &[String]) -> Result<()> {
        sqlx::query("DELETE FROM oauth_client_allowed_origins WHERE oauth_client_id = $1")
            .bind(oauth_client_id)
            .execute(&self.pool)
            .await?;
        for origin in origins {
            sqlx::query(
                "INSERT INTO oauth_client_allowed_origins (oauth_client_id, allowed_origin) VALUES ($1, $2)"
            )
            .bind(oauth_client_id)
            .bind(origin)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn save_application_ids(&self, oauth_client_id: &str, app_ids: &[String]) -> Result<()> {
        sqlx::query("DELETE FROM oauth_client_application_ids WHERE oauth_client_id = $1")
            .bind(oauth_client_id)
            .execute(&self.pool)
            .await?;
        for app_id in app_ids {
            sqlx::query(
                "INSERT INTO oauth_client_application_ids (oauth_client_id, application_id) VALUES ($1, $2)"
            )
            .bind(oauth_client_id)
            .bind(app_id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    // ── CRUD ─────────────────────────────────────────────────────

    pub async fn insert(&self, client: &OAuthClient) -> Result<()> {
        let scopes = if client.default_scopes.is_empty() {
            None
        } else {
            Some(client.default_scopes.join(","))
        };

        sqlx::query(
            r#"INSERT INTO oauth_clients
                (id, client_id, client_name, client_type, client_secret_ref,
                 default_scopes, pkce_required, service_account_principal_id, active,
                 created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())"#
        )
        .bind(&client.id)
        .bind(&client.client_id)
        .bind(&client.client_name)
        .bind(client.client_type.as_str())
        .bind(&client.client_secret_ref)
        .bind(&scopes)
        .bind(client.pkce_required)
        .bind(&client.service_account_principal_id)
        .bind(client.active)
        .execute(&self.pool)
        .await?;

        self.save_redirect_uris(&client.id, &client.redirect_uris).await?;
        self.save_grant_types(&client.id, &client.grant_types).await?;
        self.save_application_ids(&client.id, &client.application_ids).await?;
        self.save_allowed_origins(&client.id, &client.allowed_origins).await?;
        self.invalidate_cache(client).await;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<OAuthClient>> {
        let row = sqlx::query_as::<_, OAuthClientRow>(
            "SELECT * FROM oauth_clients WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok(Some(self.hydrate(OAuthClient::from(r)).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_client_id(&self, client_id: &str) -> Result<Option<OAuthClient>> {
        // Check cache first (hot path for OAuth authorize/token flows)
        {
            let cache = self.cache_by_client_id.read().await;
            if let Some(entry) = cache.get(client_id) {
                if entry.inserted_at.elapsed() < self.cache_ttl {
                    return Ok(Some(entry.client.clone()));
                }
            }
        }

        let row = sqlx::query_as::<_, OAuthClientRow>(
            "SELECT * FROM oauth_clients WHERE client_id = $1"
        )
        .bind(client_id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => {
                let client = self.hydrate(OAuthClient::from(r)).await?;
                // Populate cache
                self.cache_by_client_id.write().await.insert(
                    client_id.to_string(),
                    CacheEntry { client: client.clone(), inserted_at: Instant::now() },
                );
                Ok(Some(client))
            }
            None => Ok(None),
        }
    }

    pub async fn find_active(&self) -> Result<Vec<OAuthClient>> {
        let rows = sqlx::query_as::<_, OAuthClientRow>(
            "SELECT * FROM oauth_clients WHERE active = true"
        )
        .fetch_all(&self.pool)
        .await?;
        let clients: Vec<OAuthClient> = rows.into_iter().map(OAuthClient::from).collect();
        self.hydrate_all(clients).await
    }

    pub async fn find_all(&self) -> Result<Vec<OAuthClient>> {
        let rows = sqlx::query_as::<_, OAuthClientRow>(
            "SELECT * FROM oauth_clients"
        )
        .fetch_all(&self.pool)
        .await?;
        let clients: Vec<OAuthClient> = rows.into_iter().map(OAuthClient::from).collect();
        self.hydrate_all(clients).await
    }

    pub async fn find_by_application(&self, application_id: &str) -> Result<Vec<OAuthClient>> {
        let client_ids = sqlx::query_scalar::<_, String>(
            "SELECT oauth_client_id FROM oauth_client_application_ids WHERE application_id = $1"
        )
        .bind(application_id)
        .fetch_all(&self.pool)
        .await?;

        if client_ids.is_empty() {
            return Ok(vec![]);
        }

        let rows = sqlx::query_as::<_, OAuthClientRow>(
            "SELECT * FROM oauth_clients WHERE id = ANY($1)"
        )
        .bind(&client_ids)
        .fetch_all(&self.pool)
        .await?;
        let clients: Vec<OAuthClient> = rows.into_iter().map(OAuthClient::from).collect();
        self.hydrate_all(clients).await
    }

    pub async fn exists_by_client_id(&self, client_id: &str) -> Result<bool> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM oauth_clients WHERE client_id = $1)"
        )
        .bind(client_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    pub async fn update(&self, client: &OAuthClient) -> Result<()> {
        let scopes = if client.default_scopes.is_empty() {
            None
        } else {
            Some(client.default_scopes.join(","))
        };

        sqlx::query(
            r#"UPDATE oauth_clients SET
                client_id = $2, client_name = $3, client_type = $4,
                client_secret_ref = $5, default_scopes = $6,
                pkce_required = $7, service_account_principal_id = $8,
                active = $9, updated_at = NOW()
            WHERE id = $1"#
        )
        .bind(&client.id)
        .bind(&client.client_id)
        .bind(&client.client_name)
        .bind(client.client_type.as_str())
        .bind(&client.client_secret_ref)
        .bind(&scopes)
        .bind(client.pkce_required)
        .bind(&client.service_account_principal_id)
        .bind(client.active)
        .execute(&self.pool)
        .await?;

        self.save_redirect_uris(&client.id, &client.redirect_uris).await?;
        self.save_grant_types(&client.id, &client.grant_types).await?;
        self.save_application_ids(&client.id, &client.application_ids).await?;
        self.save_allowed_origins(&client.id, &client.allowed_origins).await?;
        self.invalidate_cache(client).await;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        // Evict from cache before delete
        {
            let cache = self.cache_by_client_id.read().await;
            // Find the client_id key to evict (reverse lookup)
            let client_id_key: Option<String> = cache.iter()
                .find(|(_, entry)| entry.client.id == id)
                .map(|(k, _)| k.clone());
            drop(cache);
            if let Some(key) = client_id_key {
                self.cache_by_client_id.write().await.remove(&key);
            }
        }

        // Junction tables have ON DELETE CASCADE, but delete explicitly for safety
        sqlx::query("DELETE FROM oauth_client_redirect_uris WHERE oauth_client_id = $1")
            .bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM oauth_client_grant_types WHERE oauth_client_id = $1")
            .bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM oauth_client_application_ids WHERE oauth_client_id = $1")
            .bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM oauth_client_allowed_origins WHERE oauth_client_id = $1")
            .bind(id).execute(&self.pool).await?;

        let result = sqlx::query("DELETE FROM oauth_clients WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ── Persist<OAuthClient> ────────────────────────────────────────────────────

impl crate::usecase::HasId for OAuthClient {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl crate::usecase::Persist<OAuthClient> for OAuthClientRepository {
    async fn persist(&self, c: &OAuthClient, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let scopes = if c.default_scopes.is_empty() {
            None
        } else {
            Some(c.default_scopes.join(","))
        };

        sqlx::query(
            r#"INSERT INTO oauth_clients
                (id, client_id, client_name, client_type, client_secret_ref,
                 default_scopes, pkce_required, service_account_principal_id, active,
                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE SET
                client_id = EXCLUDED.client_id,
                client_name = EXCLUDED.client_name,
                client_type = EXCLUDED.client_type,
                client_secret_ref = EXCLUDED.client_secret_ref,
                default_scopes = EXCLUDED.default_scopes,
                pkce_required = EXCLUDED.pkce_required,
                service_account_principal_id = EXCLUDED.service_account_principal_id,
                active = EXCLUDED.active,
                updated_at = EXCLUDED.updated_at"#
        )
        .bind(&c.id)
        .bind(&c.client_id)
        .bind(&c.client_name)
        .bind(c.client_type.as_str())
        .bind(&c.client_secret_ref)
        .bind(&scopes)
        .bind(c.pkce_required)
        .bind(&c.service_account_principal_id)
        .bind(c.active)
        .bind(c.created_at)
        .bind(c.updated_at)
        .execute(&mut **tx.inner)
        .await?;

        // Sync junction tables: delete-then-reinsert all in the same tx
        sqlx::query("DELETE FROM oauth_client_redirect_uris WHERE oauth_client_id = $1")
            .bind(&c.id).execute(&mut **tx.inner).await?;
        for uri in &c.redirect_uris {
            sqlx::query(
                "INSERT INTO oauth_client_redirect_uris (oauth_client_id, redirect_uri) VALUES ($1, $2)"
            )
            .bind(&c.id).bind(uri).execute(&mut **tx.inner).await?;
        }

        sqlx::query("DELETE FROM oauth_client_grant_types WHERE oauth_client_id = $1")
            .bind(&c.id).execute(&mut **tx.inner).await?;
        for gt in &c.grant_types {
            sqlx::query(
                "INSERT INTO oauth_client_grant_types (oauth_client_id, grant_type) VALUES ($1, $2)"
            )
            .bind(&c.id).bind(gt.as_str()).execute(&mut **tx.inner).await?;
        }

        sqlx::query("DELETE FROM oauth_client_application_ids WHERE oauth_client_id = $1")
            .bind(&c.id).execute(&mut **tx.inner).await?;
        for app_id in &c.application_ids {
            sqlx::query(
                "INSERT INTO oauth_client_application_ids (oauth_client_id, application_id) VALUES ($1, $2)"
            )
            .bind(&c.id).bind(app_id).execute(&mut **tx.inner).await?;
        }

        sqlx::query("DELETE FROM oauth_client_allowed_origins WHERE oauth_client_id = $1")
            .bind(&c.id).execute(&mut **tx.inner).await?;
        for origin in &c.allowed_origins {
            sqlx::query(
                "INSERT INTO oauth_client_allowed_origins (oauth_client_id, allowed_origin) VALUES ($1, $2)"
            )
            .bind(&c.id).bind(origin).execute(&mut **tx.inner).await?;
        }

        self.invalidate_cache(c).await;
        Ok(())
    }

    async fn delete(&self, c: &OAuthClient, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM oauth_client_redirect_uris WHERE oauth_client_id = $1")
            .bind(&c.id).execute(&mut **tx.inner).await?;
        sqlx::query("DELETE FROM oauth_client_grant_types WHERE oauth_client_id = $1")
            .bind(&c.id).execute(&mut **tx.inner).await?;
        sqlx::query("DELETE FROM oauth_client_application_ids WHERE oauth_client_id = $1")
            .bind(&c.id).execute(&mut **tx.inner).await?;
        sqlx::query("DELETE FROM oauth_client_allowed_origins WHERE oauth_client_id = $1")
            .bind(&c.id).execute(&mut **tx.inner).await?;
        sqlx::query("DELETE FROM oauth_clients WHERE id = $1")
            .bind(&c.id).execute(&mut **tx.inner).await?;
        self.invalidate_cache(c).await;
        Ok(())
    }
}
