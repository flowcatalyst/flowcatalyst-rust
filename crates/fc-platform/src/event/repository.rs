//! Event Repository — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use super::entity::{Event, EventRead, ContextData};
use crate::entities::{msg_events, msg_events_read};
use crate::shared::error::Result;

pub struct EventRepository {
    db: DatabaseConnection,
}

impl EventRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, event: &Event) -> Result<()> {
        let context_json = serde_json::to_value(&event.context_data).ok().map(sea_orm::JsonValue::from);
        let model = msg_events::ActiveModel {
            id: Set(event.id.clone()),
            spec_version: Set(Some(event.spec_version.clone())),
            event_type: Set(event.event_type.clone()),
            source: Set(event.source.clone()),
            subject: Set(event.subject.clone()),
            time: Set(event.time.into()),
            data: Set(Some(sea_orm::JsonValue::from(event.data.clone()))),
            correlation_id: Set(event.correlation_id.clone()),
            causation_id: Set(event.causation_id.clone()),
            deduplication_id: Set(event.deduplication_id.clone()),
            message_group: Set(event.message_group.clone()),
            client_id: Set(event.client_id.clone()),
            context_data: Set(context_json),
            created_at: Set(Utc::now().into()),
        };
        msg_events::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Event>> {
        let result = msg_events::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(Event::from))
    }

    pub async fn find_by_type(&self, event_type: &str, limit: u64) -> Result<Vec<Event>> {
        let results = msg_events::Entity::find()
            .filter(msg_events::Column::EventType.eq(event_type))
            .order_by_desc(msg_events::Column::Time)
            .limit(limit)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Event::from).collect())
    }

    pub async fn find_by_client(&self, client_id: &str, limit: u64) -> Result<Vec<Event>> {
        let results = msg_events::Entity::find()
            .filter(msg_events::Column::ClientId.eq(client_id))
            .order_by_desc(msg_events::Column::Time)
            .limit(limit)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Event::from).collect())
    }

    pub async fn find_by_correlation_id(&self, correlation_id: &str) -> Result<Vec<Event>> {
        let results = msg_events::Entity::find()
            .filter(msg_events::Column::CorrelationId.eq(correlation_id))
            .order_by_desc(msg_events::Column::Time)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Event::from).collect())
    }

    pub async fn find_by_deduplication_id(&self, deduplication_id: &str) -> Result<Option<Event>> {
        let result = msg_events::Entity::find()
            .filter(msg_events::Column::DeduplicationId.eq(deduplication_id))
            .one(&self.db)
            .await?;
        Ok(result.map(Event::from))
    }

    pub async fn insert_many(&self, events: &[Event]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        for event in events {
            self.insert(event).await?;
        }
        Ok(())
    }

    pub async fn find_recent_paged(&self, page: u64, size: u64) -> Result<Vec<Event>> {
        let results = msg_events::Entity::find()
            .order_by_desc(msg_events::Column::CreatedAt)
            .offset(page * size)
            .limit(size)
            .all(&self.db)
            .await?;
        Ok(results.into_iter().map(Event::from).collect())
    }

    pub async fn count_all(&self) -> Result<u64> {
        let count = msg_events::Entity::find().count(&self.db).await?;
        Ok(count)
    }

    // Read projection methods
    pub async fn find_read_by_id(&self, id: &str) -> Result<Option<EventRead>> {
        let result = msg_events_read::Entity::find_by_id(id).one(&self.db).await?;
        Ok(result.map(EventRead::from))
    }

    pub async fn insert_read_projection(&self, projection: &EventRead) -> Result<()> {
        let model = msg_events_read::ActiveModel {
            id: Set(projection.id.clone()),
            spec_version: Set(None),
            event_type: Set(projection.event_type.clone()),
            source: Set(projection.source.clone()),
            subject: Set(projection.subject.clone()),
            time: Set(projection.time.into()),
            data: Set(None),
            correlation_id: Set(projection.correlation_id.clone()),
            causation_id: Set(None),
            deduplication_id: Set(None),
            message_group: Set(projection.message_group.clone()),
            client_id: Set(projection.client_id.clone()),
            application: Set(projection.application.clone()),
            subdomain: Set(projection.subdomain.clone()),
            aggregate: Set(projection.aggregate.clone()),
            projected_at: Set(Utc::now().into()),
        };
        msg_events_read::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn update_read_projection(&self, projection: &EventRead) -> Result<()> {
        let model = msg_events_read::ActiveModel {
            id: Set(projection.id.clone()),
            spec_version: NotSet,
            event_type: Set(projection.event_type.clone()),
            source: Set(projection.source.clone()),
            subject: Set(projection.subject.clone()),
            time: Set(projection.time.into()),
            data: NotSet,
            correlation_id: Set(projection.correlation_id.clone()),
            causation_id: NotSet,
            deduplication_id: NotSet,
            message_group: Set(projection.message_group.clone()),
            client_id: Set(projection.client_id.clone()),
            application: Set(projection.application.clone()),
            subdomain: Set(projection.subdomain.clone()),
            aggregate: Set(projection.aggregate.clone()),
            projected_at: Set(Utc::now().into()),
        };
        msg_events_read::Entity::update(model).exec(&self.db).await?;
        Ok(())
    }
}
