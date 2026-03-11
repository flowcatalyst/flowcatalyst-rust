//! PasswordResetToken Repository — PostgreSQL via SeaORM

use sea_orm::*;
use chrono::Utc;

use super::entity::PasswordResetToken;
use crate::entities::iam_password_reset_tokens;
use crate::shared::error::Result;

pub struct PasswordResetTokenRepository {
    db: DatabaseConnection,
}

impl PasswordResetTokenRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn create(&self, token: &PasswordResetToken) -> Result<()> {
        let model = iam_password_reset_tokens::ActiveModel {
            id: Set(token.id.clone()),
            principal_id: Set(token.principal_id.clone()),
            token_hash: Set(token.token_hash.clone()),
            expires_at: Set(token.expires_at.into()),
            created_at: Set(Utc::now().into()),
        };
        iam_password_reset_tokens::Entity::insert(model).exec(&self.db).await?;
        Ok(())
    }

    pub async fn find_by_token_hash(&self, hash: &str) -> Result<Option<PasswordResetToken>> {
        let result = iam_password_reset_tokens::Entity::find()
            .filter(iam_password_reset_tokens::Column::TokenHash.eq(hash))
            .one(&self.db)
            .await?;
        Ok(result.map(PasswordResetToken::from))
    }

    pub async fn delete_by_principal_id(&self, principal_id: &str) -> Result<()> {
        iam_password_reset_tokens::Entity::delete_many()
            .filter(iam_password_reset_tokens::Column::PrincipalId.eq(principal_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    pub async fn delete_expired(&self) -> Result<u64> {
        let result = iam_password_reset_tokens::Entity::delete_many()
            .filter(iam_password_reset_tokens::Column::ExpiresAt.lt(Utc::now()))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }

    pub async fn delete_by_id(&self, id: &str) -> Result<()> {
        iam_password_reset_tokens::Entity::delete_by_id(id).exec(&self.db).await?;
        Ok(())
    }
}
