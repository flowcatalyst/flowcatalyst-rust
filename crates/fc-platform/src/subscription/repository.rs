//! Subscription Repository — PostgreSQL via SQLx

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::entity::{ConfigEntry, EventTypeBinding, Subscription};
use crate::dispatch_job::entity::parse_dispatch_mode;
use crate::shared::enum_str::decode;
use crate::shared::error::{PlatformError, Result};
use crate::usecase::unit_of_work::HasId;

// ── Row types ────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct SubscriptionRow {
    id: String,
    code: String,
    application_code: Option<String>,
    name: String,
    description: Option<String>,
    client_id: Option<String>,
    client_identifier: Option<String>,
    client_scoped: bool,
    connection_id: Option<String>,
    target: String,
    queue: Option<String>,
    source: String,
    status: String,
    max_age_seconds: i32,
    dispatch_pool_id: Option<String>,
    dispatch_pool_code: Option<String>,
    delay_seconds: i32,
    sequence: i32,
    mode: String,
    timeout_seconds: i32,
    max_retries: i32,
    service_account_id: Option<String>,
    data_only: bool,
    created_by: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<SubscriptionRow> for Subscription {
    type Error = PlatformError;
    fn try_from(r: SubscriptionRow) -> Result<Self> {
        let source = decode(&r.source, "msg_subscriptions", "source", &r.id)?;
        let status = decode(&r.status, "msg_subscriptions", "status", &r.id)?;
        let mode = parse_dispatch_mode(Some(&r.mode));
        Ok(Self {
            id: r.id,
            code: r.code,
            application_code: r.application_code,
            name: r.name,
            description: r.description,
            client_id: r.client_id,
            client_identifier: r.client_identifier,
            client_scoped: r.client_scoped,
            event_types: vec![], // loaded separately
            connection_id: r.connection_id,
            endpoint: r.target,
            queue: r.queue,
            custom_config: vec![], // loaded separately
            source,
            status,
            max_age_seconds: r.max_age_seconds,
            dispatch_pool_id: r.dispatch_pool_id,
            dispatch_pool_code: r.dispatch_pool_code,
            delay_seconds: r.delay_seconds,
            sequence: r.sequence,
            mode,
            timeout_seconds: r.timeout_seconds,
            max_retries: r.max_retries,
            service_account_id: r.service_account_id,
            data_only: r.data_only,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct SubscriptionEventTypeRow {
    subscription_id: String,
    event_type_id: Option<String>,
    event_type_code: String,
    spec_version: Option<String>,
}

#[derive(sqlx::FromRow)]
struct SubscriptionCustomConfigRow {
    subscription_id: String,
    config_key: String,
    config_value: String,
}

// ── Repository ───────────────────────────────────────────────────────────────

pub struct SubscriptionRepository {
    pool: PgPool,
}

impl SubscriptionRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    async fn load_event_types(&self, subscription_id: &str) -> Result<Vec<EventTypeBinding>> {
        let rows = sqlx::query_as::<_, SubscriptionEventTypeRow>(
            "SELECT subscription_id, event_type_id, event_type_code, spec_version
             FROM msg_subscription_event_types WHERE subscription_id = $1",
        )
        .bind(subscription_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| EventTypeBinding {
                event_type_id: r.event_type_id,
                event_type_code: r.event_type_code,
                spec_version: r.spec_version,
                filter: None,
            })
            .collect())
    }

    async fn load_custom_config(&self, subscription_id: &str) -> Result<Vec<ConfigEntry>> {
        let rows = sqlx::query_as::<_, SubscriptionCustomConfigRow>(
            "SELECT subscription_id, config_key, config_value
             FROM msg_subscription_custom_configs WHERE subscription_id = $1",
        )
        .bind(subscription_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ConfigEntry {
                key: r.config_key,
                value: r.config_value,
            })
            .collect())
    }

    async fn hydrate(&self, mut sub: Subscription) -> Result<Subscription> {
        sub.event_types = self.load_event_types(&sub.id).await?;
        sub.custom_config = self.load_custom_config(&sub.id).await?;
        Ok(sub)
    }

    /// Batch-hydrate event types and custom config for multiple subscriptions (avoids N+1)
    async fn hydrate_all(&self, rows: Vec<SubscriptionRow>) -> Result<Vec<Subscription>> {
        if rows.is_empty() {
            return Ok(vec![]);
        }

        let ids: Vec<String> = rows.iter().map(|m| m.id.clone()).collect();

        // Batch-load event type bindings
        let all_et = sqlx::query_as::<_, SubscriptionEventTypeRow>(
            "SELECT subscription_id, event_type_id, event_type_code, spec_version
             FROM msg_subscription_event_types WHERE subscription_id = ANY($1)",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;
        let mut et_map: std::collections::HashMap<String, Vec<EventTypeBinding>> =
            std::collections::HashMap::new();
        for r in all_et {
            et_map
                .entry(r.subscription_id.clone())
                .or_default()
                .push(EventTypeBinding {
                    event_type_id: r.event_type_id,
                    event_type_code: r.event_type_code,
                    spec_version: r.spec_version,
                    filter: None,
                });
        }

        // Batch-load custom configs
        let all_cfg = sqlx::query_as::<_, SubscriptionCustomConfigRow>(
            "SELECT subscription_id, config_key, config_value
             FROM msg_subscription_custom_configs WHERE subscription_id = ANY($1)",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;
        let mut cfg_map: std::collections::HashMap<String, Vec<ConfigEntry>> =
            std::collections::HashMap::new();
        for r in all_cfg {
            cfg_map
                .entry(r.subscription_id.clone())
                .or_default()
                .push(ConfigEntry {
                    key: r.config_key,
                    value: r.config_value,
                });
        }

        rows.into_iter()
            .map(|r| {
                let id = r.id.clone();
                let mut sub = Subscription::try_from(r)?;
                if let Some(ets) = et_map.remove(&id) {
                    sub.event_types = ets;
                }
                if let Some(cfgs) = cfg_map.remove(&id) {
                    sub.custom_config = cfgs;
                }
                Ok(sub)
            })
            .collect()
    }

    pub async fn insert(&self, sub: &Subscription) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO msg_subscriptions
                (id, code, application_code, name, description, client_id, client_identifier,
                 client_scoped, connection_id, target, queue, source, status, max_age_seconds,
                 dispatch_pool_id, dispatch_pool_code, delay_seconds, sequence, mode,
                 timeout_seconds, max_retries, service_account_id, data_only, created_at, updated_at,
                 created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26)"
        )
        .bind(&sub.id)
        .bind(&sub.code)
        .bind(&sub.application_code)
        .bind(&sub.name)
        .bind(&sub.description)
        .bind(&sub.client_id)
        .bind(&sub.client_identifier)
        .bind(sub.client_scoped)
        .bind(&sub.connection_id)
        .bind(&sub.endpoint)
        .bind(&sub.queue)
        .bind(sub.source.as_str())
        .bind(sub.status.as_str())
        .bind(sub.max_age_seconds)
        .bind(&sub.dispatch_pool_id)
        .bind(&sub.dispatch_pool_code)
        .bind(sub.delay_seconds)
        .bind(sub.sequence)
        .bind(sub.mode.as_str())
        .bind(sub.timeout_seconds)
        .bind(sub.max_retries)
        .bind(&sub.service_account_id)
        .bind(sub.data_only)
        .bind(now)
        .bind(now)
        .bind(&sub.created_by)
        .execute(&self.pool)
        .await?;
        self.save_event_types(&sub.id, &sub.event_types).await?;
        self.save_custom_config(&sub.id, &sub.custom_config).await?;
        Ok(())
    }

    async fn save_event_types(
        &self,
        subscription_id: &str,
        event_types: &[EventTypeBinding],
    ) -> Result<()> {
        // Delete existing then re-insert via UNNEST
        sqlx::query("DELETE FROM msg_subscription_event_types WHERE subscription_id = $1")
            .bind(subscription_id)
            .execute(&self.pool)
            .await?;

        if !event_types.is_empty() {
            let mut sub_ids: Vec<String> = Vec::with_capacity(event_types.len());
            let mut et_ids: Vec<Option<String>> = Vec::with_capacity(event_types.len());
            let mut et_codes: Vec<String> = Vec::with_capacity(event_types.len());
            let mut spec_versions: Vec<Option<String>> = Vec::with_capacity(event_types.len());
            for et in event_types {
                sub_ids.push(subscription_id.to_string());
                et_ids.push(et.event_type_id.clone());
                et_codes.push(et.event_type_code.clone());
                spec_versions.push(et.spec_version.clone());
            }
            sqlx::query(
                "INSERT INTO msg_subscription_event_types
                    (subscription_id, event_type_id, event_type_code, spec_version)
                 SELECT * FROM UNNEST($1::varchar[], $2::varchar[], $3::varchar[], $4::varchar[])",
            )
            .bind(&sub_ids)
            .bind(&et_ids as &[Option<String>])
            .bind(&et_codes)
            .bind(&spec_versions as &[Option<String>])
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn save_custom_config(
        &self,
        subscription_id: &str,
        config: &[ConfigEntry],
    ) -> Result<()> {
        sqlx::query("DELETE FROM msg_subscription_custom_configs WHERE subscription_id = $1")
            .bind(subscription_id)
            .execute(&self.pool)
            .await?;

        if !config.is_empty() {
            let mut sub_ids: Vec<String> = Vec::with_capacity(config.len());
            let mut keys: Vec<String> = Vec::with_capacity(config.len());
            let mut values: Vec<String> = Vec::with_capacity(config.len());
            for entry in config {
                sub_ids.push(subscription_id.to_string());
                keys.push(entry.key.clone());
                values.push(entry.value.clone());
            }
            sqlx::query(
                "INSERT INTO msg_subscription_custom_configs
                    (subscription_id, config_key, config_value)
                 SELECT * FROM UNNEST($1::varchar[], $2::varchar[], $3::varchar[])",
            )
            .bind(&sub_ids)
            .bind(&keys)
            .bind(&values)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Subscription>> {
        let row =
            sqlx::query_as::<_, SubscriptionRow>("SELECT * FROM msg_subscriptions WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        match row {
            Some(r) => Ok(Some(self.hydrate(Subscription::try_from(r)?).await?)),
            None => Ok(None),
        }
    }

    /// Every subscription named by `ids`, hydrated in one pass; an id with
    /// no row is simply absent.
    pub async fn find_by_ids(&self, ids: &[String]) -> Result<Vec<Subscription>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM msg_subscriptions WHERE id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    pub async fn find_all(&self) -> Result<Vec<Subscription>> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM msg_subscriptions ORDER BY code ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    pub async fn find_by_client(&self, client_id: Option<&str>) -> Result<Vec<Subscription>> {
        let rows = if let Some(cid) = client_id {
            sqlx::query_as::<_, SubscriptionRow>(
                "SELECT * FROM msg_subscriptions WHERE client_id = $1 OR client_scoped = false",
            )
            .bind(cid)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, SubscriptionRow>(
                "SELECT * FROM msg_subscriptions WHERE client_scoped = false",
            )
            .fetch_all(&self.pool)
            .await?
        };
        self.hydrate_all(rows).await
    }

    pub async fn find_active_for_event_type(
        &self,
        event_type_code: &str,
        client_id: Option<&str>,
    ) -> Result<Vec<Subscription>> {
        // Find subscription IDs that have a matching event type binding
        let sub_ids: Vec<String> = sqlx::query_scalar(
            "SELECT subscription_id FROM msg_subscription_event_types WHERE event_type_code = $1",
        )
        .bind(event_type_code)
        .fetch_all(&self.pool)
        .await?;

        if sub_ids.is_empty() {
            return Ok(vec![]);
        }

        let rows = if let Some(cid) = client_id {
            sqlx::query_as::<_, SubscriptionRow>(
                "SELECT * FROM msg_subscriptions
                 WHERE id = ANY($1) AND status = 'ACTIVE'
                   AND (client_id = $2 OR client_scoped = false)",
            )
            .bind(&sub_ids)
            .bind(cid)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, SubscriptionRow>(
                "SELECT * FROM msg_subscriptions
                 WHERE id = ANY($1) AND status = 'ACTIVE'",
            )
            .bind(&sub_ids)
            .fetch_all(&self.pool)
            .await?
        };
        self.hydrate_all(rows).await
    }

    pub async fn update(&self, sub: &Subscription) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE msg_subscriptions SET
                code = $2, application_code = $3, name = $4, description = $5,
                client_id = $6, client_identifier = $7, client_scoped = $8,
                connection_id = $9, target = $10, queue = $11, source = $12, status = $13,
                max_age_seconds = $14, dispatch_pool_id = $15, dispatch_pool_code = $16,
                delay_seconds = $17, sequence = $18, mode = $19, timeout_seconds = $20,
                max_retries = $21, service_account_id = $22, data_only = $23, updated_at = $24
             WHERE id = $1",
        )
        .bind(&sub.id)
        .bind(&sub.code)
        .bind(&sub.application_code)
        .bind(&sub.name)
        .bind(&sub.description)
        .bind(&sub.client_id)
        .bind(&sub.client_identifier)
        .bind(sub.client_scoped)
        .bind(&sub.connection_id)
        .bind(&sub.endpoint)
        .bind(&sub.queue)
        .bind(sub.source.as_str())
        .bind(sub.status.as_str())
        .bind(sub.max_age_seconds)
        .bind(&sub.dispatch_pool_id)
        .bind(&sub.dispatch_pool_code)
        .bind(sub.delay_seconds)
        .bind(sub.sequence)
        .bind(sub.mode.as_str())
        .bind(sub.timeout_seconds)
        .bind(sub.max_retries)
        .bind(&sub.service_account_id)
        .bind(sub.data_only)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.save_event_types(&sub.id, &sub.event_types).await?;
        self.save_custom_config(&sub.id, &sub.custom_config).await?;
        Ok(())
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<Subscription>> {
        let row =
            sqlx::query_as::<_, SubscriptionRow>("SELECT * FROM msg_subscriptions WHERE code = $1")
                .bind(code)
                .fetch_optional(&self.pool)
                .await?;
        match row {
            Some(r) => Ok(Some(self.hydrate(Subscription::try_from(r)?).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_code_and_client(
        &self,
        code: &str,
        client_id: Option<&str>,
    ) -> Result<Option<Subscription>> {
        let row = if let Some(cid) = client_id {
            sqlx::query_as::<_, SubscriptionRow>(
                "SELECT * FROM msg_subscriptions WHERE code = $1 AND client_id = $2",
            )
            .bind(code)
            .bind(cid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, SubscriptionRow>(
                "SELECT * FROM msg_subscriptions WHERE code = $1 AND client_id IS NULL",
            )
            .bind(code)
            .fetch_optional(&self.pool)
            .await?
        };
        match row {
            Some(r) => Ok(Some(self.hydrate(Subscription::try_from(r)?).await?)),
            None => Ok(None),
        }
    }

    /// The subscription keyed by (code, application, client), where `None`
    /// is a real value of the key, never a wildcard (Go `FindByCode`: the
    /// `(application_code, client_id, code)` uniqueness of migration 050).
    pub async fn find_by_code_in_scope(
        &self,
        code: &str,
        application_code: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<Option<Subscription>> {
        let row = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM msg_subscriptions WHERE code = $1 \
             AND application_code IS NOT DISTINCT FROM $2 \
             AND client_id IS NOT DISTINCT FROM $3",
        )
        .bind(code)
        .bind(application_code)
        .bind(client_id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok(Some(self.hydrate(Subscription::try_from(r)?).await?)),
            None => Ok(None),
        }
    }

    /// Go `FindWithFilters`: every subscription matching the given
    /// filters (none = all, any status), by code.
    pub async fn find_with_filters(
        &self,
        status: Option<&str>,
        client_id: Option<&str>,
    ) -> Result<Vec<Subscription>> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM msg_subscriptions \
             WHERE ($1::text IS NULL OR status = $1) \
               AND ($2::text IS NULL OR client_id = $2) \
             ORDER BY code",
        )
        .bind(status)
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    /// Go `FindByApplicationAndClient`: the subscriptions of one
    /// application under one client, where no client matches only the
    /// client-less rows — the row set one (application, client) sync may
    /// touch.
    pub async fn find_by_application_and_client(
        &self,
        application_code: &str,
        client_id: Option<&str>,
    ) -> Result<Vec<Subscription>> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM msg_subscriptions \
             WHERE application_code = $1 AND client_id IS NOT DISTINCT FROM $2 \
             ORDER BY code",
        )
        .bind(application_code)
        .bind(client_id)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    /// Check if any subscriptions reference a given connection ID
    pub async fn exists_by_connection_id(&self, connection_id: &str) -> Result<bool> {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM msg_subscriptions WHERE connection_id = $1")
                .bind(connection_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0 > 0)
    }

    pub async fn find_by_application_code(
        &self,
        application_code: &str,
    ) -> Result<Vec<Subscription>> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM msg_subscriptions WHERE application_code = $1 ORDER BY code ASC",
        )
        .bind(application_code)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    pub async fn find_by_connection_id(&self, connection_id: &str) -> Result<Vec<Subscription>> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM msg_subscriptions WHERE connection_id = $1 ORDER BY code ASC",
        )
        .bind(connection_id)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    pub async fn find_by_status(&self, status: &str) -> Result<Vec<Subscription>> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM msg_subscriptions WHERE status = $1 ORDER BY code ASC",
        )
        .bind(status)
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    pub async fn find_active(&self) -> Result<Vec<Subscription>> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM msg_subscriptions WHERE status = 'ACTIVE' ORDER BY code ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        self.hydrate_all(rows).await
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        sqlx::query("DELETE FROM msg_subscription_event_types WHERE subscription_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM msg_subscription_custom_configs WHERE subscription_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        let result = sqlx::query("DELETE FROM msg_subscriptions WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

// ── Persist<Subscription> ────────────────────────────────────────────────────

impl HasId for Subscription {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait]
impl crate::usecase::Persist<Subscription> for SubscriptionRepository {
    async fn persist(&self, s: &Subscription, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        let now = Utc::now();

        // 1. Upsert main row
        sqlx::query(
            "INSERT INTO msg_subscriptions (id, code, application_code, name, description, client_id, client_identifier, client_scoped, connection_id, target, queue, source, status, max_age_seconds, dispatch_pool_id, dispatch_pool_code, delay_seconds, sequence, mode, timeout_seconds, max_retries, service_account_id, data_only, created_at, updated_at, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26)
             ON CONFLICT (id) DO UPDATE SET
                code = EXCLUDED.code,
                application_code = EXCLUDED.application_code,
                name = EXCLUDED.name,
                description = EXCLUDED.description,
                client_id = EXCLUDED.client_id,
                client_identifier = EXCLUDED.client_identifier,
                client_scoped = EXCLUDED.client_scoped,
                connection_id = EXCLUDED.connection_id,
                target = EXCLUDED.target,
                queue = EXCLUDED.queue,
                source = EXCLUDED.source,
                status = EXCLUDED.status,
                max_age_seconds = EXCLUDED.max_age_seconds,
                dispatch_pool_id = EXCLUDED.dispatch_pool_id,
                dispatch_pool_code = EXCLUDED.dispatch_pool_code,
                delay_seconds = EXCLUDED.delay_seconds,
                sequence = EXCLUDED.sequence,
                mode = EXCLUDED.mode,
                timeout_seconds = EXCLUDED.timeout_seconds,
                max_retries = EXCLUDED.max_retries,
                service_account_id = EXCLUDED.service_account_id,
                data_only = EXCLUDED.data_only,
                updated_at = EXCLUDED.updated_at,
                created_by = COALESCE(msg_subscriptions.created_by, EXCLUDED.created_by)"
        )
        .bind(&s.id)
        .bind(&s.code)
        .bind(&s.application_code)
        .bind(&s.name)
        .bind(&s.description)
        .bind(&s.client_id)
        .bind(&s.client_identifier)
        .bind(s.client_scoped)
        .bind(&s.connection_id)
        .bind(&s.endpoint)
        .bind(&s.queue)
        .bind(s.source.as_str())
        .bind(s.status.as_str())
        .bind(s.max_age_seconds)
        .bind(&s.dispatch_pool_id)
        .bind(&s.dispatch_pool_code)
        .bind(s.delay_seconds)
        .bind(s.sequence)
        .bind(s.mode.as_str())
        .bind(s.timeout_seconds)
        .bind(s.max_retries)
        .bind(&s.service_account_id)
        .bind(s.data_only)
        .bind(now)
        .bind(now)
        .bind(&s.created_by)
        .execute(&mut **tx.inner).await?;

        sqlx::query("DELETE FROM msg_subscription_event_types WHERE subscription_id = $1")
            .bind(&s.id)
            .execute(&mut **tx.inner)
            .await?;
        for et in &s.event_types {
            sqlx::query(
                "INSERT INTO msg_subscription_event_types (subscription_id, event_type_id, event_type_code, spec_version)
                 VALUES ($1, $2, $3, $4)"
            )
            .bind(&s.id)
            .bind(&et.event_type_id)
            .bind(&et.event_type_code)
            .bind(&et.spec_version)
            .execute(&mut **tx.inner).await?;
        }

        sqlx::query("DELETE FROM msg_subscription_custom_configs WHERE subscription_id = $1")
            .bind(&s.id)
            .execute(&mut **tx.inner)
            .await?;
        for entry in &s.custom_config {
            sqlx::query(
                "INSERT INTO msg_subscription_custom_configs (subscription_id, config_key, config_value)
                 VALUES ($1, $2, $3)"
            )
            .bind(&s.id)
            .bind(&entry.key)
            .bind(&entry.value)
            .execute(&mut **tx.inner).await?;
        }

        Ok(())
    }

    async fn delete(&self, s: &Subscription, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query("DELETE FROM msg_subscription_event_types WHERE subscription_id = $1")
            .bind(&s.id)
            .execute(&mut **tx.inner)
            .await?;
        sqlx::query("DELETE FROM msg_subscription_custom_configs WHERE subscription_id = $1")
            .bind(&s.id)
            .execute(&mut **tx.inner)
            .await?;
        sqlx::query("DELETE FROM msg_subscriptions WHERE id = $1")
            .bind(&s.id)
            .execute(&mut **tx.inner)
            .await?;
        Ok(())
    }
}

impl SubscriptionRepository {
    /// The codes of the subscriptions using each of `connection_ids`, as
    /// `(connection_id, code)` by code (Go `FindCodesByConnectionID`, batched).
    pub async fn codes_by_connection_ids(
        &self,
        connection_ids: &[String],
    ) -> crate::shared::error::Result<Vec<(String, String)>> {
        if connection_ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as(
            "SELECT connection_id, code FROM msg_subscriptions \
             WHERE connection_id = ANY($1) ORDER BY code",
        )
        .bind(connection_ids)
        .fetch_all(&self.pool)
        .await?)
    }
}
