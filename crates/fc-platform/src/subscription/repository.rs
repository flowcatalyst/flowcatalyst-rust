//! Subscription Repository — PostgreSQL via SeaORM

use async_trait::async_trait;
use sea_orm::*;
use sea_orm::sea_query::OnConflict;
use chrono::Utc;

use super::entity::{Subscription, EventTypeBinding, ConfigEntry};
use crate::entities::{msg_subscriptions, msg_subscription_event_types, msg_subscription_custom_configs};
use crate::shared::error::Result;
use crate::usecase::unit_of_work::{HasId, PgPersist};

pub struct SubscriptionRepository {
    db: DatabaseConnection,
}

impl SubscriptionRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    async fn load_event_types(&self, subscription_id: &str) -> Result<Vec<EventTypeBinding>> {
        let rows = msg_subscription_event_types::Entity::find()
            .filter(msg_subscription_event_types::Column::SubscriptionId.eq(subscription_id))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(|r| EventTypeBinding {
            event_type_id: r.event_type_id,
            event_type_code: r.event_type_code,
            spec_version: r.spec_version,
            filter: None,
        }).collect())
    }

    async fn load_custom_config(&self, subscription_id: &str) -> Result<Vec<ConfigEntry>> {
        let rows = msg_subscription_custom_configs::Entity::find()
            .filter(msg_subscription_custom_configs::Column::SubscriptionId.eq(subscription_id))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(|r| ConfigEntry { key: r.config_key, value: r.config_value }).collect())
    }

    async fn hydrate(&self, mut sub: Subscription) -> Result<Subscription> {
        sub.event_types = self.load_event_types(&sub.id).await?;
        sub.custom_config = self.load_custom_config(&sub.id).await?;
        Ok(sub)
    }

    pub async fn insert(&self, sub: &Subscription) -> Result<()> {
        let model = msg_subscriptions::ActiveModel {
            id: Set(sub.id.clone()),
            code: Set(sub.code.clone()),
            application_code: Set(sub.application_code.clone()),
            name: Set(sub.name.clone()),
            description: Set(sub.description.clone()),
            client_id: Set(sub.client_id.clone()),
            client_identifier: Set(sub.client_identifier.clone()),
            client_scoped: Set(sub.client_scoped),
            connection_id: Set(if sub.connection_id.is_empty() { None } else { Some(sub.connection_id.clone()) }),
            target: Set(String::new()),
            queue: Set(sub.queue.clone()),
            source: Set(sub.source.as_str().to_string()),
            status: Set(sub.status.as_str().to_string()),
            max_age_seconds: Set(sub.max_age_seconds),
            dispatch_pool_id: Set(sub.dispatch_pool_id.clone()),
            dispatch_pool_code: Set(sub.dispatch_pool_code.clone()),
            delay_seconds: Set(sub.delay_seconds),
            sequence: Set(sub.sequence),
            mode: Set(sub.mode.as_str().to_string()),
            timeout_seconds: Set(sub.timeout_seconds),
            max_retries: Set(sub.max_retries),
            service_account_id: Set(sub.service_account_id.clone()),
            data_only: Set(sub.data_only),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        msg_subscriptions::Entity::insert(model).exec(&self.db).await?;
        self.save_event_types(&sub.id, &sub.event_types).await?;
        self.save_custom_config(&sub.id, &sub.custom_config).await?;
        Ok(())
    }

    async fn save_event_types(&self, subscription_id: &str, event_types: &[EventTypeBinding]) -> Result<()> {
        // Delete existing then re-insert
        msg_subscription_event_types::Entity::delete_many()
            .filter(msg_subscription_event_types::Column::SubscriptionId.eq(subscription_id))
            .exec(&self.db)
            .await?;
        for et in event_types {
            let model = msg_subscription_event_types::ActiveModel {
                id: NotSet,
                subscription_id: Set(subscription_id.to_string()),
                event_type_id: Set(et.event_type_id.clone()),
                event_type_code: Set(et.event_type_code.clone()),
                spec_version: Set(et.spec_version.clone()),
            };
            msg_subscription_event_types::Entity::insert(model).exec(&self.db).await?;
        }
        Ok(())
    }

    async fn save_custom_config(&self, subscription_id: &str, config: &[ConfigEntry]) -> Result<()> {
        msg_subscription_custom_configs::Entity::delete_many()
            .filter(msg_subscription_custom_configs::Column::SubscriptionId.eq(subscription_id))
            .exec(&self.db)
            .await?;
        for entry in config {
            let model = msg_subscription_custom_configs::ActiveModel {
                id: NotSet,
                subscription_id: Set(subscription_id.to_string()),
                config_key: Set(entry.key.clone()),
                config_value: Set(entry.value.clone()),
            };
            msg_subscription_custom_configs::Entity::insert(model).exec(&self.db).await?;
        }
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Subscription>> {
        let result = msg_subscriptions::Entity::find_by_id(id).one(&self.db).await?;
        match result {
            Some(m) => Ok(Some(self.hydrate(Subscription::from(m)).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_all(&self) -> Result<Vec<Subscription>> {
        let rows = msg_subscriptions::Entity::find()
            .order_by_asc(msg_subscriptions::Column::Code)
            .all(&self.db)
            .await?;
        let mut results = Vec::with_capacity(rows.len());
        for m in rows {
            results.push(self.hydrate(Subscription::from(m)).await?);
        }
        Ok(results)
    }

    pub async fn find_by_client(&self, client_id: Option<&str>) -> Result<Vec<Subscription>> {
        let rows = if let Some(cid) = client_id {
            msg_subscriptions::Entity::find()
                .filter(
                    Condition::any()
                        .add(msg_subscriptions::Column::ClientId.eq(cid))
                        .add(msg_subscriptions::Column::ClientScoped.eq(false)),
                )
                .all(&self.db)
                .await?
        } else {
            msg_subscriptions::Entity::find()
                .filter(msg_subscriptions::Column::ClientScoped.eq(false))
                .all(&self.db)
                .await?
        };
        let mut results = Vec::with_capacity(rows.len());
        for m in rows {
            results.push(self.hydrate(Subscription::from(m)).await?);
        }
        Ok(results)
    }

    pub async fn find_active_for_event_type(&self, event_type_code: &str, client_id: Option<&str>) -> Result<Vec<Subscription>> {
        // Find subscription IDs that have a matching event type binding
        let et_rows = msg_subscription_event_types::Entity::find()
            .filter(msg_subscription_event_types::Column::EventTypeCode.eq(event_type_code))
            .all(&self.db)
            .await?;
        let sub_ids: Vec<String> = et_rows.into_iter().map(|r| r.subscription_id).collect();
        if sub_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut q = msg_subscriptions::Entity::find()
            .filter(msg_subscriptions::Column::Id.is_in(sub_ids))
            .filter(msg_subscriptions::Column::Status.eq("ACTIVE"));
        if let Some(cid) = client_id {
            q = q.filter(
                Condition::any()
                    .add(msg_subscriptions::Column::ClientId.eq(cid))
                    .add(msg_subscriptions::Column::ClientScoped.eq(false)),
            );
        }
        let rows = q.all(&self.db).await?;
        let mut results = Vec::with_capacity(rows.len());
        for m in rows {
            results.push(self.hydrate(Subscription::from(m)).await?);
        }
        Ok(results)
    }

    pub async fn update(&self, sub: &Subscription) -> Result<()> {
        let model = msg_subscriptions::ActiveModel {
            id: Set(sub.id.clone()),
            code: Set(sub.code.clone()),
            application_code: Set(sub.application_code.clone()),
            name: Set(sub.name.clone()),
            description: Set(sub.description.clone()),
            client_id: Set(sub.client_id.clone()),
            client_identifier: Set(sub.client_identifier.clone()),
            client_scoped: Set(sub.client_scoped),
            connection_id: Set(if sub.connection_id.is_empty() { None } else { Some(sub.connection_id.clone()) }),
            target: Set(String::new()),
            queue: Set(sub.queue.clone()),
            source: Set(sub.source.as_str().to_string()),
            status: Set(sub.status.as_str().to_string()),
            max_age_seconds: Set(sub.max_age_seconds),
            dispatch_pool_id: Set(sub.dispatch_pool_id.clone()),
            dispatch_pool_code: Set(sub.dispatch_pool_code.clone()),
            delay_seconds: Set(sub.delay_seconds),
            sequence: Set(sub.sequence),
            mode: Set(sub.mode.as_str().to_string()),
            timeout_seconds: Set(sub.timeout_seconds),
            max_retries: Set(sub.max_retries),
            service_account_id: Set(sub.service_account_id.clone()),
            data_only: Set(sub.data_only),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };
        msg_subscriptions::Entity::update(model).exec(&self.db).await?;
        self.save_event_types(&sub.id, &sub.event_types).await?;
        self.save_custom_config(&sub.id, &sub.custom_config).await?;
        Ok(())
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<Subscription>> {
        let result = msg_subscriptions::Entity::find()
            .filter(msg_subscriptions::Column::Code.eq(code))
            .one(&self.db)
            .await?;
        match result {
            Some(m) => Ok(Some(self.hydrate(Subscription::from(m)).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_code_and_client(&self, code: &str, client_id: Option<&str>) -> Result<Option<Subscription>> {
        let mut q = msg_subscriptions::Entity::find()
            .filter(msg_subscriptions::Column::Code.eq(code));
        if let Some(cid) = client_id {
            q = q.filter(msg_subscriptions::Column::ClientId.eq(cid));
        } else {
            q = q.filter(msg_subscriptions::Column::ClientId.is_null());
        }
        let result = q.one(&self.db).await?;
        match result {
            Some(m) => Ok(Some(self.hydrate(Subscription::from(m)).await?)),
            None => Ok(None),
        }
    }

    /// Check if any subscriptions reference a given connection ID
    pub async fn exists_by_connection_id(&self, connection_id: &str) -> Result<bool> {
        let count = msg_subscriptions::Entity::find()
            .filter(msg_subscriptions::Column::ConnectionId.eq(connection_id))
            .count(&self.db)
            .await?;
        Ok(count > 0)
    }

    pub async fn find_by_application_code(&self, application_code: &str) -> Result<Vec<Subscription>> {
        let rows = msg_subscriptions::Entity::find()
            .filter(msg_subscriptions::Column::ApplicationCode.eq(application_code))
            .order_by_asc(msg_subscriptions::Column::Code)
            .all(&self.db)
            .await?;
        let mut results = Vec::with_capacity(rows.len());
        for m in rows {
            results.push(self.hydrate(Subscription::from(m)).await?);
        }
        Ok(results)
    }

    pub async fn find_by_connection_id(&self, connection_id: &str) -> Result<Vec<Subscription>> {
        let rows = msg_subscriptions::Entity::find()
            .filter(msg_subscriptions::Column::ConnectionId.eq(connection_id))
            .order_by_asc(msg_subscriptions::Column::Code)
            .all(&self.db)
            .await?;
        let mut results = Vec::with_capacity(rows.len());
        for m in rows {
            results.push(self.hydrate(Subscription::from(m)).await?);
        }
        Ok(results)
    }

    pub async fn find_by_status(&self, status: &str) -> Result<Vec<Subscription>> {
        let rows = msg_subscriptions::Entity::find()
            .filter(msg_subscriptions::Column::Status.eq(status))
            .order_by_asc(msg_subscriptions::Column::Code)
            .all(&self.db)
            .await?;
        let mut results = Vec::with_capacity(rows.len());
        for m in rows {
            results.push(self.hydrate(Subscription::from(m)).await?);
        }
        Ok(results)
    }

    pub async fn find_active(&self) -> Result<Vec<Subscription>> {
        let rows = msg_subscriptions::Entity::find()
            .filter(msg_subscriptions::Column::Status.eq("ACTIVE"))
            .order_by_asc(msg_subscriptions::Column::Code)
            .all(&self.db)
            .await?;
        let mut results = Vec::with_capacity(rows.len());
        for m in rows {
            results.push(self.hydrate(Subscription::from(m)).await?);
        }
        Ok(results)
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        msg_subscription_event_types::Entity::delete_many()
            .filter(msg_subscription_event_types::Column::SubscriptionId.eq(id))
            .exec(&self.db)
            .await?;
        msg_subscription_custom_configs::Entity::delete_many()
            .filter(msg_subscription_custom_configs::Column::SubscriptionId.eq(id))
            .exec(&self.db)
            .await?;
        let result = msg_subscriptions::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(result.rows_affected > 0)
    }
}

// ── PgPersist implementation ──────────────────────────────────────────────────

impl HasId for Subscription {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for Subscription {
    async fn pg_upsert(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        let model = msg_subscriptions::ActiveModel {
            id: Set(self.id.clone()),
            code: Set(self.code.clone()),
            application_code: Set(self.application_code.clone()),
            name: Set(self.name.clone()),
            description: Set(self.description.clone()),
            client_id: Set(self.client_id.clone()),
            client_identifier: Set(self.client_identifier.clone()),
            client_scoped: Set(self.client_scoped),
            connection_id: Set(if self.connection_id.is_empty() { None } else { Some(self.connection_id.clone()) }),
            target: Set(String::new()),
            queue: Set(self.queue.clone()),
            source: Set(self.source.as_str().to_string()),
            status: Set(self.status.as_str().to_string()),
            max_age_seconds: Set(self.max_age_seconds),
            dispatch_pool_id: Set(self.dispatch_pool_id.clone()),
            dispatch_pool_code: Set(self.dispatch_pool_code.clone()),
            delay_seconds: Set(self.delay_seconds),
            sequence: Set(self.sequence),
            mode: Set(self.mode.as_str().to_string()),
            timeout_seconds: Set(self.timeout_seconds),
            max_retries: Set(self.max_retries),
            service_account_id: Set(self.service_account_id.clone()),
            data_only: Set(self.data_only),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        msg_subscriptions::Entity::insert(model)
            .on_conflict(
                OnConflict::column(msg_subscriptions::Column::Id)
                    .update_columns([
                        msg_subscriptions::Column::Code,
                        msg_subscriptions::Column::ApplicationCode,
                        msg_subscriptions::Column::Name,
                        msg_subscriptions::Column::Description,
                        msg_subscriptions::Column::ClientId,
                        msg_subscriptions::Column::ClientIdentifier,
                        msg_subscriptions::Column::ClientScoped,
                        msg_subscriptions::Column::ConnectionId,
                        msg_subscriptions::Column::Target,
                        msg_subscriptions::Column::Queue,
                        msg_subscriptions::Column::Source,
                        msg_subscriptions::Column::Status,
                        msg_subscriptions::Column::MaxAgeSeconds,
                        msg_subscriptions::Column::DispatchPoolId,
                        msg_subscriptions::Column::DispatchPoolCode,
                        msg_subscriptions::Column::DelaySeconds,
                        msg_subscriptions::Column::Sequence,
                        msg_subscriptions::Column::Mode,
                        msg_subscriptions::Column::TimeoutSeconds,
                        msg_subscriptions::Column::MaxRetries,
                        msg_subscriptions::Column::ServiceAccountId,
                        msg_subscriptions::Column::DataOnly,
                        msg_subscriptions::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;

        // Sync event type bindings
        msg_subscription_event_types::Entity::delete_many()
            .filter(msg_subscription_event_types::Column::SubscriptionId.eq(&self.id))
            .exec(txn)
            .await?;
        for et in &self.event_types {
            let et_model = msg_subscription_event_types::ActiveModel {
                id: NotSet,
                subscription_id: Set(self.id.clone()),
                event_type_id: Set(et.event_type_id.clone()),
                event_type_code: Set(et.event_type_code.clone()),
                spec_version: Set(et.spec_version.clone()),
            };
            msg_subscription_event_types::Entity::insert(et_model).exec(txn).await?;
        }

        // Sync custom config
        msg_subscription_custom_configs::Entity::delete_many()
            .filter(msg_subscription_custom_configs::Column::SubscriptionId.eq(&self.id))
            .exec(txn)
            .await?;
        for entry in &self.custom_config {
            let cfg_model = msg_subscription_custom_configs::ActiveModel {
                id: NotSet,
                subscription_id: Set(self.id.clone()),
                config_key: Set(entry.key.clone()),
                config_value: Set(entry.value.clone()),
            };
            msg_subscription_custom_configs::Entity::insert(cfg_model).exec(txn).await?;
        }

        Ok(())
    }

    async fn pg_delete(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        msg_subscription_event_types::Entity::delete_many()
            .filter(msg_subscription_event_types::Column::SubscriptionId.eq(&self.id))
            .exec(txn)
            .await?;
        msg_subscription_custom_configs::Entity::delete_many()
            .filter(msg_subscription_custom_configs::Column::SubscriptionId.eq(&self.id))
            .exec(txn)
            .await?;
        msg_subscriptions::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}
