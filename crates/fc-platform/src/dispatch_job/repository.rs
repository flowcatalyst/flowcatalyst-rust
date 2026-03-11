//! DispatchJob Repository — PostgreSQL via SeaORM

use sea_orm::*;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::OnConflict;
use chrono::{DateTime, Utc};
use crate::{DispatchJob, DispatchJobRead, DispatchStatus};
use crate::entities::{msg_dispatch_jobs, msg_dispatch_jobs_read};
use crate::shared::error::Result;

pub struct DispatchJobRepository {
    db: DatabaseConnection,
}

impl DispatchJobRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    fn to_active_model(job: &DispatchJob) -> msg_dispatch_jobs::ActiveModel {
        let metadata_json = serde_json::to_value(&job.metadata).unwrap_or_default();

        msg_dispatch_jobs::ActiveModel {
            id: Set(job.id.clone()),
            external_id: Set(job.external_id.clone()),
            source: Set(job.source.clone()),
            kind: Set(job.kind.as_str().to_string()),
            code: Set(job.code.clone()),
            subject: Set(job.subject.clone()),
            event_id: Set(job.event_id.clone()),
            correlation_id: Set(job.correlation_id.clone()),
            metadata: Set(metadata_json),
            target_url: Set(job.target_url.clone()),
            protocol: Set(job.protocol.as_str().to_string()),
            payload: Set(job.payload.clone()),
            payload_content_type: Set(Some(job.payload_content_type.clone())),
            data_only: Set(job.data_only),
            service_account_id: Set(job.service_account_id.clone()),
            client_id: Set(job.client_id.clone()),
            subscription_id: Set(job.subscription_id.clone()),
            mode: Set(job.mode.as_str().to_string()),
            dispatch_pool_id: Set(job.dispatch_pool_id.clone()),
            message_group: Set(job.message_group.clone()),
            sequence: Set(job.sequence),
            timeout_seconds: Set(job.timeout_seconds as i32),
            schema_id: Set(job.schema_id.clone()),
            status: Set(job.status.as_str().to_string()),
            max_retries: Set(job.max_retries as i32),
            retry_strategy: Set(job.retry_strategy.as_str().to_string()),
            scheduled_for: Set(job.scheduled_for.map(Into::into)),
            expires_at: Set(job.expires_at.map(Into::into)),
            attempt_count: Set(job.attempt_count as i32),
            last_attempt_at: Set(job.last_attempt_at.map(Into::into)),
            completed_at: Set(job.completed_at.map(Into::into)),
            duration_millis: Set(job.duration_millis),
            last_error: Set(job.last_error.clone()),
            idempotency_key: Set(job.idempotency_key.clone()),
            created_at: Set(job.created_at.into()),
            updated_at: Set(job.updated_at.into()),
        }
    }

    pub async fn insert(&self, job: &DispatchJob) -> Result<()> {
        let model = Self::to_active_model(job);
        msg_dispatch_jobs::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<DispatchJob>> {
        let result = msg_dispatch_jobs::Entity::find_by_id(id)
            .one(&self.db)
            .await?;
        Ok(result.map(DispatchJob::from))
    }

    pub async fn find_by_event_id(&self, event_id: &str) -> Result<Vec<DispatchJob>> {
        let rows = msg_dispatch_jobs::Entity::find()
            .filter(msg_dispatch_jobs::Column::EventId.eq(event_id))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(DispatchJob::from).collect())
    }

    pub async fn find_by_subscription_id(&self, subscription_id: &str, limit: i64) -> Result<Vec<DispatchJob>> {
        let mut query = msg_dispatch_jobs::Entity::find()
            .filter(msg_dispatch_jobs::Column::SubscriptionId.eq(subscription_id));
        if limit > 0 {
            query = query.limit(limit as u64);
        }
        let rows = query.all(&self.db).await?;
        Ok(rows.into_iter().map(DispatchJob::from).collect())
    }

    pub async fn find_by_status(&self, status: DispatchStatus, limit: i64) -> Result<Vec<DispatchJob>> {
        let mut query = msg_dispatch_jobs::Entity::find()
            .filter(msg_dispatch_jobs::Column::Status.eq(status.as_str()));
        if limit > 0 {
            query = query.limit(limit as u64);
        }
        let rows = query.all(&self.db).await?;
        Ok(rows.into_iter().map(DispatchJob::from).collect())
    }

    pub async fn find_pending_for_dispatch(&self, limit: i64) -> Result<Vec<DispatchJob>> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let mut query = msg_dispatch_jobs::Entity::find()
            .filter(msg_dispatch_jobs::Column::Status.eq("PENDING"))
            .filter(
                Condition::any()
                    .add(msg_dispatch_jobs::Column::ScheduledFor.is_null())
                    .add(msg_dispatch_jobs::Column::ScheduledFor.lte(now))
            );
        if limit > 0 {
            query = query.limit(limit as u64);
        }
        let rows = query.all(&self.db).await?;
        Ok(rows.into_iter().map(DispatchJob::from).collect())
    }

    pub async fn find_stale_in_progress(&self, stale_threshold: DateTime<Utc>, limit: i64) -> Result<Vec<DispatchJob>> {
        let threshold: chrono::DateTime<chrono::FixedOffset> = stale_threshold.into();
        let mut query = msg_dispatch_jobs::Entity::find()
            .filter(msg_dispatch_jobs::Column::Status.eq("IN_PROGRESS"))
            .filter(msg_dispatch_jobs::Column::UpdatedAt.lt(threshold));
        if limit > 0 {
            query = query.limit(limit as u64);
        }
        let rows = query.all(&self.db).await?;
        Ok(rows.into_iter().map(DispatchJob::from).collect())
    }

    pub async fn find_by_client(&self, client_id: &str, limit: i64) -> Result<Vec<DispatchJob>> {
        let mut query = msg_dispatch_jobs::Entity::find()
            .filter(msg_dispatch_jobs::Column::ClientId.eq(client_id));
        if limit > 0 {
            query = query.limit(limit as u64);
        }
        let rows = query.all(&self.db).await?;
        Ok(rows.into_iter().map(DispatchJob::from).collect())
    }

    pub async fn find_by_correlation_id(&self, correlation_id: &str) -> Result<Vec<DispatchJob>> {
        let rows = msg_dispatch_jobs::Entity::find()
            .filter(msg_dispatch_jobs::Column::CorrelationId.eq(correlation_id))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(DispatchJob::from).collect())
    }

    pub async fn update(&self, job: &DispatchJob) -> Result<()> {
        let model = Self::to_active_model(job);
        msg_dispatch_jobs::Entity::update(model).exec(&self.db).await?;
        Ok(())
    }

    /// Bulk insert multiple dispatch jobs
    pub async fn insert_many(&self, jobs: &[DispatchJob]) -> Result<()> {
        if jobs.is_empty() {
            return Ok(());
        }
        let models: Vec<msg_dispatch_jobs::ActiveModel> = jobs.iter().map(Self::to_active_model).collect();
        msg_dispatch_jobs::Entity::insert_many(models).exec(&self.db).await?;
        Ok(())
    }

    pub async fn update_status(&self, id: &str, status: DispatchStatus) -> Result<bool> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let result = msg_dispatch_jobs::Entity::update_many()
            .col_expr(msg_dispatch_jobs::Column::Status, Expr::value(status.as_str()))
            .col_expr(msg_dispatch_jobs::Column::UpdatedAt, Expr::value(now))
            .filter(msg_dispatch_jobs::Column::Id.eq(id))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    // Read projection methods
    pub async fn find_read_by_id(&self, id: &str) -> Result<Option<DispatchJobRead>> {
        let result = msg_dispatch_jobs_read::Entity::find_by_id(id)
            .one(&self.db)
            .await?;
        Ok(result.map(DispatchJobRead::from))
    }

    pub async fn insert_read_projection(&self, projection: &DispatchJobRead) -> Result<()> {
        let model = Self::read_to_active_model(projection);
        msg_dispatch_jobs_read::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn update_read_projection(&self, projection: &DispatchJobRead) -> Result<()> {
        let model = Self::read_to_active_model(projection);
        let on_conflict = OnConflict::column(msg_dispatch_jobs_read::Column::Id)
            .update_columns([
                msg_dispatch_jobs_read::Column::Status,
                msg_dispatch_jobs_read::Column::AttemptCount,
                msg_dispatch_jobs_read::Column::LastError,
                msg_dispatch_jobs_read::Column::UpdatedAt,
                msg_dispatch_jobs_read::Column::CompletedAt,
                msg_dispatch_jobs_read::Column::LastAttemptAt,
                msg_dispatch_jobs_read::Column::DurationMillis,
                msg_dispatch_jobs_read::Column::IsCompleted,
                msg_dispatch_jobs_read::Column::IsTerminal,
                msg_dispatch_jobs_read::Column::ProjectedAt,
            ])
            .to_owned();
        msg_dispatch_jobs_read::Entity::insert(model)
            .on_conflict(on_conflict)
            .exec(&self.db)
            .await?;
        Ok(())
    }

    fn read_to_active_model(p: &DispatchJobRead) -> msg_dispatch_jobs_read::ActiveModel {
        msg_dispatch_jobs_read::ActiveModel {
            id: Set(p.id.clone()),
            external_id: Set(p.external_id.clone()),
            source: Set(p.source.clone()),
            kind: Set(p.kind.as_str().to_string()),
            code: Set(p.code.clone()),
            subject: Set(p.subject.clone()),
            event_id: Set(p.event_id.clone()),
            correlation_id: Set(p.correlation_id.clone()),
            target_url: Set(p.target_url.clone()),
            protocol: Set(p.protocol.as_str().to_string()),
            client_id: Set(p.client_id.clone()),
            subscription_id: Set(p.subscription_id.clone()),
            service_account_id: Set(p.service_account_id.clone()),
            dispatch_pool_id: Set(p.dispatch_pool_id.clone()),
            message_group: Set(p.message_group.clone()),
            mode: Set(p.mode.as_str().to_string()),
            sequence: Set(p.sequence),
            status: Set(p.status.as_str().to_string()),
            attempt_count: Set(p.attempt_count as i32),
            max_retries: Set(p.max_retries as i32),
            last_error: Set(p.last_error.clone()),
            timeout_seconds: Set(p.timeout_seconds as i32),
            retry_strategy: Set(p.retry_strategy.as_str().to_string()),
            application: Set(p.application.clone()),
            subdomain: Set(p.subdomain.clone()),
            aggregate: Set(p.aggregate.clone()),
            created_at: Set(p.created_at.into()),
            updated_at: Set(p.updated_at.into()),
            scheduled_for: Set(p.scheduled_for.map(Into::into)),
            expires_at: Set(p.expires_at.map(Into::into)),
            completed_at: Set(p.completed_at.map(Into::into)),
            last_attempt_at: Set(p.last_attempt_at.map(Into::into)),
            duration_millis: Set(p.duration_millis),
            idempotency_key: Set(p.idempotency_key.clone()),
            is_completed: Set(Some(p.is_completed)),
            is_terminal: Set(Some(p.is_terminal)),
            projected_at: Set(p.projected_at.map(Into::into)),
        }
    }

    /// Count jobs by status
    pub async fn count_by_status(&self, status: DispatchStatus) -> Result<u64> {
        let count = msg_dispatch_jobs::Entity::find()
            .filter(msg_dispatch_jobs::Column::Status.eq(status.as_str()))
            .count(&self.db)
            .await?;
        Ok(count)
    }

    /// Count all jobs
    pub async fn count_all(&self) -> Result<u64> {
        let count = msg_dispatch_jobs::Entity::find()
            .count(&self.db)
            .await?;
        Ok(count)
    }

    /// Find recent dispatch jobs with pagination (for debug/admin)
    pub async fn find_recent_paged(&self, page: u32, size: u32) -> Result<Vec<DispatchJob>> {
        let rows = msg_dispatch_jobs::Entity::find()
            .order_by_desc(msg_dispatch_jobs::Column::CreatedAt)
            .offset(page as u64 * size as u64)
            .limit(size as u64)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(DispatchJob::from).collect())
    }
}
