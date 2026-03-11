//! LoginAttempt Repository — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use super::entity::LoginAttempt;
use crate::entities::iam_login_attempts;
use crate::shared::error::Result;

pub struct LoginAttemptRepository {
    db: DatabaseConnection,
}

impl LoginAttemptRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn create(&self, attempt: &LoginAttempt) -> Result<()> {
        let model = iam_login_attempts::ActiveModel {
            id: Set(attempt.id.clone()),
            attempt_type: Set(attempt.attempt_type.as_str().to_string()),
            outcome: Set(attempt.outcome.as_str().to_string()),
            failure_reason: Set(attempt.failure_reason.clone()),
            identifier: Set(attempt.identifier.clone()),
            principal_id: Set(attempt.principal_id.clone()),
            ip_address: Set(attempt.ip_address.clone()),
            user_agent: Set(attempt.user_agent.clone()),
            attempted_at: Set(attempt.attempted_at.into()),
        };
        iam_login_attempts::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_paged(
        &self,
        attempt_type: Option<&str>,
        outcome: Option<&str>,
        identifier: Option<&str>,
        principal_id: Option<&str>,
        date_from: Option<&str>,
        date_to: Option<&str>,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<LoginAttempt>, u64)> {
        let mut q = iam_login_attempts::Entity::find();

        if let Some(at) = attempt_type {
            q = q.filter(iam_login_attempts::Column::AttemptType.eq(at));
        }
        if let Some(o) = outcome {
            q = q.filter(iam_login_attempts::Column::Outcome.eq(o));
        }
        if let Some(ident) = identifier {
            q = q.filter(iam_login_attempts::Column::Identifier.eq(ident));
        }
        if let Some(pid) = principal_id {
            q = q.filter(iam_login_attempts::Column::PrincipalId.eq(pid));
        }
        if let Some(from) = date_from {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(from) {
                q = q.filter(iam_login_attempts::Column::AttemptedAt.gte(dt));
            }
        }
        if let Some(to) = date_to {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(to) {
                q = q.filter(iam_login_attempts::Column::AttemptedAt.lte(dt));
            }
        }

        let total = q.clone().count(&self.db).await?;

        let results = q
            .order_by_desc(iam_login_attempts::Column::AttemptedAt)
            .offset(page * page_size)
            .limit(page_size)
            .all(&self.db)
            .await?;

        Ok((results.into_iter().map(LoginAttempt::from).collect(), total))
    }
}
