//! Persistence of an email-domain mapping moving to another identity
//! provider (Go `MoveMappingTx`): the mapping's provider column and, for a
//! move to an internal provider, the domain's federated users, in one
//! transaction. The principal writes go through `PrincipalRepository`.

use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;

use crate::principal::repository::PrincipalRepository;
use fc_platform_core::shared::error::Result;
use fc_platform_core::usecase::unit_of_work::HasId;

/// The move: the mapping (by id), its new provider, the users to reset.
#[derive(Debug, Clone)]
pub struct ProviderMove {
    pub mapping_id: String,
    pub identity_provider_id: String,
    /// Also link this primary client (an identity-provider claim of a
    /// mapping that had none).
    pub primary_client_id: Option<String>,
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
impl fc_platform_core::usecase::Persist<ProviderMove> for ProviderMoveRepository {
    async fn persist(
        &self,
        m: &ProviderMove,
        tx: &mut fc_platform_core::usecase::DbTx<'_>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE tnt_email_domain_mappings SET identity_provider_id = $2, \
             primary_client_id = COALESCE($3, primary_client_id), updated_at = NOW() \
             WHERE id = $1",
        )
        .bind(&m.mapping_id)
        .bind(&m.identity_provider_id)
        .bind(&m.primary_client_id)
        .execute(&mut **tx.inner)
        .await?;
        self.principal_repo
            .reset_to_internal_in_tx(&m.reset_user_ids, tx)
            .await
    }

    async fn delete(
        &self,
        _m: &ProviderMove,
        _tx: &mut fc_platform_core::usecase::DbTx<'_>,
    ) -> Result<()> {
        Err(fc_platform_core::shared::error::PlatformError::internal(
            "a provider move is not deleted",
        ))
    }
}
