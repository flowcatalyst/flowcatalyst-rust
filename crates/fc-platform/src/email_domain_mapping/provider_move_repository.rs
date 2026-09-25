//! Persistence of an email-domain mapping moving to another identity
//! provider (Go `MoveMappingTx`): the mapping's provider column and, for a
//! move to an internal provider, the domain's federated users, in one
//! transaction. The principal writes go through `PrincipalRepository`.

use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use crate::shared::error::Result;
use crate::usecase::unit_of_work::HasId;
use crate::PrincipalRepository;

/// The move: the mapping (by id), its new provider, the users to reset.
#[derive(Debug, Clone)]
pub struct ProviderMove {
    pub mapping_id: String,
    pub identity_provider_id: String,
    pub reset_user_ids: Vec<String>,
}

impl HasId for ProviderMove {
    fn id(&self) -> &str {
        &self.mapping_id
    }
}

pub struct ProviderMoveRepository {
    _pool: PgPool,
    principal_repo: Arc<PrincipalRepository>,
}

impl ProviderMoveRepository {
    pub fn new(pool: &PgPool, principal_repo: Arc<PrincipalRepository>) -> Self {
        Self {
            _pool: pool.clone(),
            principal_repo,
        }
    }
}

#[async_trait]
impl crate::usecase::Persist<ProviderMove> for ProviderMoveRepository {
    async fn persist(&self, m: &ProviderMove, tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        sqlx::query(
            "UPDATE tnt_email_domain_mappings SET identity_provider_id = $2, updated_at = NOW() \
             WHERE id = $1",
        )
        .bind(&m.mapping_id)
        .bind(&m.identity_provider_id)
        .execute(&mut **tx.inner)
        .await?;
        self.principal_repo
            .reset_to_internal_in_tx(&m.reset_user_ids, tx)
            .await
    }

    async fn delete(&self, _m: &ProviderMove, _tx: &mut crate::usecase::DbTx<'_>) -> Result<()> {
        Err(crate::shared::error::PlatformError::internal(
            "a provider move is not deleted",
        ))
    }
}
