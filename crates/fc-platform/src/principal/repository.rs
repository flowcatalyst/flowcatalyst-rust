//! Principal Repository
//!
//! PostgreSQL persistence for Principal entities using SeaORM.
//! Roles are loaded from iam_principal_roles junction table.
//! Assigned clients are loaded from iam_client_access_grants.

use async_trait::async_trait;
use sea_orm::*;
use sea_orm::sea_query::OnConflict;
use chrono::Utc;

use super::entity::{Principal, UserScope};
use crate::service_account::entity::RoleAssignment;
use crate::entities::{iam_principals, iam_principal_roles, iam_client_access_grants};
use crate::shared::error::Result;
use crate::usecase::unit_of_work::{HasId, PgPersist};

pub struct PrincipalRepository {
    db: DatabaseConnection,
}

impl PrincipalRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, principal: &Principal) -> Result<()> {
        // Extract email domain from email
        let email_domain = principal.user_identity.as_ref()
            .map(|i| i.email.split('@').nth(1).unwrap_or("").to_string());

        let model = iam_principals::ActiveModel {
            id: Set(principal.id.clone()),
            principal_type: Set(principal.principal_type.as_str().to_string()),
            scope: Set(Some(principal.scope.as_str().to_string())),
            client_id: Set(principal.client_id.clone()),
            application_id: Set(principal.application_id.clone()),
            name: Set(principal.name.clone()),
            active: Set(principal.active),
            email: Set(principal.user_identity.as_ref().map(|i| i.email.clone())),
            email_domain: Set(email_domain),
            idp_type: Set(principal.user_identity.as_ref().and_then(|i| i.provider.clone())
                .or_else(|| if principal.is_user() { Some("INTERNAL".to_string()) } else { None })),
            external_idp_id: Set(principal.external_identity.as_ref().map(|e| e.external_id.clone())),
            password_hash: Set(principal.user_identity.as_ref().and_then(|i| i.password_hash.clone())),
            last_login_at: Set(principal.user_identity.as_ref()
                .and_then(|i| i.last_login_at.map(|dt| dt.into()))),
            service_account_id: Set(principal.service_account_id.clone()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };

        iam_principals::Entity::insert(model)
            .exec(&self.db)
            .await?;

        // Insert roles into junction table
        self.insert_roles(&principal.id, &principal.roles).await?;

        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Principal>> {
        let result = iam_principals::Entity::find_by_id(id)
            .one(&self.db)
            .await?;

        match result {
            Some(model) => Ok(Some(self.hydrate_principal(model).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<Principal>> {
        let result = iam_principals::Entity::find()
            .filter(iam_principals::Column::PrincipalType.eq("USER"))
            .filter(iam_principals::Column::Email.eq(email))
            .one(&self.db)
            .await?;

        match result {
            Some(model) => Ok(Some(self.hydrate_principal(model).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_service_account(&self, service_account_id: &str) -> Result<Option<Principal>> {
        let result = iam_principals::Entity::find()
            .filter(iam_principals::Column::PrincipalType.eq("SERVICE"))
            .filter(iam_principals::Column::ServiceAccountId.eq(service_account_id))
            .one(&self.db)
            .await?;

        match result {
            Some(model) => Ok(Some(self.hydrate_principal(model).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_all(&self) -> Result<Vec<Principal>> {
        let results = iam_principals::Entity::find()
            .all(&self.db)
            .await?;

        self.hydrate_principals(results).await
    }

    pub async fn find_active(&self) -> Result<Vec<Principal>> {
        let results = iam_principals::Entity::find()
            .filter(iam_principals::Column::Active.eq(true))
            .all(&self.db)
            .await?;

        self.hydrate_principals(results).await
    }

    pub async fn find_users(&self) -> Result<Vec<Principal>> {
        let results = iam_principals::Entity::find()
            .filter(iam_principals::Column::PrincipalType.eq("USER"))
            .filter(iam_principals::Column::Active.eq(true))
            .all(&self.db)
            .await?;

        self.hydrate_principals(results).await
    }

    pub async fn find_services(&self) -> Result<Vec<Principal>> {
        let results = iam_principals::Entity::find()
            .filter(iam_principals::Column::PrincipalType.eq("SERVICE"))
            .filter(iam_principals::Column::Active.eq(true))
            .all(&self.db)
            .await?;

        self.hydrate_principals(results).await
    }

    pub async fn find_by_client(&self, client_id: &str) -> Result<Vec<Principal>> {
        // Find principals that either have this client_id OR have a grant for it
        let grant_principal_ids: Vec<String> = iam_client_access_grants::Entity::find()
            .filter(iam_client_access_grants::Column::ClientId.eq(client_id))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|g| g.principal_id)
            .collect();

        let results = iam_principals::Entity::find()
            .filter(iam_principals::Column::Active.eq(true))
            .filter(
                Condition::any()
                    .add(iam_principals::Column::ClientId.eq(client_id))
                    .add(iam_principals::Column::Id.is_in(grant_principal_ids))
            )
            .all(&self.db)
            .await?;

        self.hydrate_principals(results).await
    }

    pub async fn find_by_scope(&self, scope: UserScope) -> Result<Vec<Principal>> {
        let results = iam_principals::Entity::find()
            .filter(iam_principals::Column::Scope.eq(scope.as_str()))
            .filter(iam_principals::Column::Active.eq(true))
            .all(&self.db)
            .await?;

        self.hydrate_principals(results).await
    }

    pub async fn find_anchors(&self) -> Result<Vec<Principal>> {
        let results = iam_principals::Entity::find()
            .filter(iam_principals::Column::Scope.eq("ANCHOR"))
            .filter(iam_principals::Column::Active.eq(true))
            .all(&self.db)
            .await?;

        self.hydrate_principals(results).await
    }

    pub async fn find_by_application(&self, application_id: &str) -> Result<Vec<Principal>> {
        let results = iam_principals::Entity::find()
            .filter(iam_principals::Column::ApplicationId.eq(application_id))
            .all(&self.db)
            .await?;

        self.hydrate_principals(results).await
    }

    pub async fn find_with_role(&self, role: &str) -> Result<Vec<Principal>> {
        // Find principal_ids that have this role
        let principal_ids: Vec<String> = iam_principal_roles::Entity::find()
            .filter(iam_principal_roles::Column::RoleName.eq(role))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|pr| pr.principal_id)
            .collect();

        if principal_ids.is_empty() {
            return Ok(vec![]);
        }

        let results = iam_principals::Entity::find()
            .filter(iam_principals::Column::Id.is_in(principal_ids))
            .filter(iam_principals::Column::Active.eq(true))
            .all(&self.db)
            .await?;

        self.hydrate_principals(results).await
    }

    pub async fn update(&self, principal: &Principal) -> Result<()> {
        let email_domain = principal.user_identity.as_ref()
            .map(|i| i.email.split('@').nth(1).unwrap_or("").to_string());

        let model = iam_principals::ActiveModel {
            id: Set(principal.id.clone()),
            principal_type: Set(principal.principal_type.as_str().to_string()),
            scope: Set(Some(principal.scope.as_str().to_string())),
            client_id: Set(principal.client_id.clone()),
            application_id: Set(principal.application_id.clone()),
            name: Set(principal.name.clone()),
            active: Set(principal.active),
            email: Set(principal.user_identity.as_ref().map(|i| i.email.clone())),
            email_domain: Set(email_domain),
            idp_type: Set(principal.user_identity.as_ref().and_then(|i| i.provider.clone())
                .or_else(|| if principal.is_user() { Some("INTERNAL".to_string()) } else { None })),
            external_idp_id: Set(principal.external_identity.as_ref().map(|e| e.external_id.clone())),
            password_hash: Set(principal.user_identity.as_ref().and_then(|i| i.password_hash.clone())),
            last_login_at: Set(principal.user_identity.as_ref()
                .and_then(|i| i.last_login_at.map(|dt| dt.into()))),
            service_account_id: Set(principal.service_account_id.clone()),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };

        iam_principals::Entity::update(model)
            .exec(&self.db)
            .await?;

        // Sync roles: delete all then re-insert
        iam_principal_roles::Entity::delete_many()
            .filter(iam_principal_roles::Column::PrincipalId.eq(&principal.id))
            .exec(&self.db)
            .await?;

        self.insert_roles(&principal.id, &principal.roles).await?;

        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = iam_principals::Entity::delete_by_id(id)
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    /// Search principals by name or email (case-insensitive partial match)
    pub async fn search(&self, term: &str) -> Result<Vec<Principal>> {
        let pattern = format!("%{}%", term);
        let results = iam_principals::Entity::find()
            .filter(
                Condition::any()
                    .add(iam_principals::Column::Name.like(&pattern))
                    .add(iam_principals::Column::Email.like(&pattern))
            )
            .all(&self.db)
            .await?;

        self.hydrate_principals(results).await
    }

    /// Batch-lookup principal names by IDs. Returns a map of id -> name.
    pub async fn find_names_by_ids(&self, ids: &[String]) -> Result<std::collections::HashMap<String, String>> {
        if ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        use sea_orm::QuerySelect;
        let results: Vec<(String, String)> = iam_principals::Entity::find()
            .select_only()
            .column(iam_principals::Column::Id)
            .column(iam_principals::Column::Name)
            .filter(iam_principals::Column::Id.is_in(ids.to_vec()))
            .into_tuple()
            .all(&self.db)
            .await?;
        Ok(results.into_iter().collect())
    }

    /// Count principals with email ending in the given domain
    pub async fn count_by_email_domain(&self, domain: &str) -> Result<i64> {
        let count = iam_principals::Entity::find()
            .filter(iam_principals::Column::PrincipalType.eq("USER"))
            .filter(iam_principals::Column::EmailDomain.eq(domain.to_lowercase()))
            .count(&self.db)
            .await?;
        Ok(count as i64)
    }

    // ── Helpers ──────────────────────────────────────────────

    pub(crate) async fn insert_roles_txn(
        principal_id: &str,
        roles: &[RoleAssignment],
        txn: &sea_orm::DatabaseTransaction,
    ) -> Result<()> {
        if roles.is_empty() { return Ok(()); }
        let models: Vec<iam_principal_roles::ActiveModel> = roles
            .iter()
            .map(|r| iam_principal_roles::ActiveModel {
                principal_id: Set(principal_id.to_string()),
                role_name: Set(r.role.clone()),
                assignment_source: Set(r.assignment_source.clone()),
                assigned_at: Set(r.assigned_at.into()),
            })
            .collect();
        iam_principal_roles::Entity::insert_many(models).exec(txn).await?;
        Ok(())
    }

    /// Insert roles into the junction table
    async fn insert_roles(&self, principal_id: &str, roles: &[RoleAssignment]) -> Result<()> {
        if roles.is_empty() {
            return Ok(());
        }

        let models: Vec<iam_principal_roles::ActiveModel> = roles
            .iter()
            .map(|r| iam_principal_roles::ActiveModel {
                principal_id: Set(principal_id.to_string()),
                role_name: Set(r.role.clone()),
                assignment_source: Set(r.assignment_source.clone()),
                assigned_at: Set(r.assigned_at.into()),
            })
            .collect();

        iam_principal_roles::Entity::insert_many(models)
            .exec(&self.db)
            .await?;

        Ok(())
    }

    /// Load roles for a principal from the junction table
    async fn load_roles(&self, principal_id: &str) -> Result<Vec<RoleAssignment>> {
        let role_models = iam_principal_roles::Entity::find()
            .filter(iam_principal_roles::Column::PrincipalId.eq(principal_id))
            .all(&self.db)
            .await?;

        Ok(role_models.into_iter().map(|m| RoleAssignment {
            role: m.role_name,
            client_id: None,
            assignment_source: m.assignment_source,
            assigned_at: m.assigned_at.naive_utc().and_utc(),
            assigned_by: None,
        }).collect())
    }

    /// Load assigned client IDs from iam_client_access_grants
    async fn load_assigned_clients(&self, principal_id: &str) -> Result<Vec<String>> {
        let grants = iam_client_access_grants::Entity::find()
            .filter(iam_client_access_grants::Column::PrincipalId.eq(principal_id))
            .all(&self.db)
            .await?;

        Ok(grants.into_iter().map(|g| g.client_id).collect())
    }

    /// Hydrate a single principal with roles and client grants
    async fn hydrate_principal(&self, model: iam_principals::Model) -> Result<Principal> {
        let id = model.id.clone();
        let mut principal = Principal::from(model);
        principal.roles = self.load_roles(&id).await?;
        principal.assigned_clients = self.load_assigned_clients(&id).await?;
        Ok(principal)
    }

    /// Hydrate multiple principals with roles and client grants (batch)
    async fn hydrate_principals(&self, models: Vec<iam_principals::Model>) -> Result<Vec<Principal>> {
        if models.is_empty() {
            return Ok(vec![]);
        }

        let principal_ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();

        // Batch-load roles
        let all_roles = iam_principal_roles::Entity::find()
            .filter(iam_principal_roles::Column::PrincipalId.is_in(principal_ids.clone()))
            .all(&self.db)
            .await?;

        let mut role_map: std::collections::HashMap<String, Vec<RoleAssignment>> =
            std::collections::HashMap::new();
        for r in all_roles {
            role_map.entry(r.principal_id.clone()).or_default().push(RoleAssignment {
                role: r.role_name,
                client_id: None,
                assignment_source: r.assignment_source,
                assigned_at: r.assigned_at.naive_utc().and_utc(),
                assigned_by: None,
            });
        }

        // Batch-load client access grants
        let all_grants = iam_client_access_grants::Entity::find()
            .filter(iam_client_access_grants::Column::PrincipalId.is_in(principal_ids))
            .all(&self.db)
            .await?;

        let mut grant_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for g in all_grants {
            grant_map.entry(g.principal_id).or_default().push(g.client_id);
        }

        // Build domain entities
        let principals = models
            .into_iter()
            .map(|m| {
                let id = m.id.clone();
                let mut principal = Principal::from(m);
                if let Some(roles) = role_map.remove(&id) {
                    principal.roles = roles;
                }
                if let Some(clients) = grant_map.remove(&id) {
                    principal.assigned_clients = clients;
                }
                principal
            })
            .collect();

        Ok(principals)
    }
}

// ── PgPersist implementation ──────────────────────────────────────────────────

impl HasId for Principal {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for Principal {
    async fn pg_upsert(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        let email_domain = self.user_identity.as_ref()
            .map(|i| i.email.split('@').nth(1).unwrap_or("").to_string());

        let model = iam_principals::ActiveModel {
            id: Set(self.id.clone()),
            principal_type: Set(self.principal_type.as_str().to_string()),
            scope: Set(Some(self.scope.as_str().to_string())),
            client_id: Set(self.client_id.clone()),
            application_id: Set(self.application_id.clone()),
            name: Set(self.name.clone()),
            active: Set(self.active),
            email: Set(self.user_identity.as_ref().map(|i| i.email.clone())),
            email_domain: Set(email_domain),
            idp_type: Set(self.user_identity.as_ref().and_then(|i| i.provider.clone())
                .or_else(|| if self.is_user() { Some("INTERNAL".to_string()) } else { None })),
            external_idp_id: Set(self.external_identity.as_ref().map(|e| e.external_id.clone())),
            password_hash: Set(self.user_identity.as_ref().and_then(|i| i.password_hash.clone())),
            last_login_at: Set(self.user_identity.as_ref()
                .and_then(|i| i.last_login_at.map(|dt| dt.into()))),
            service_account_id: Set(self.service_account_id.clone()),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        iam_principals::Entity::insert(model)
            .on_conflict(
                OnConflict::column(iam_principals::Column::Id)
                    .update_columns([
                        iam_principals::Column::PrincipalType,
                        iam_principals::Column::Scope,
                        iam_principals::Column::ClientId,
                        iam_principals::Column::ApplicationId,
                        iam_principals::Column::Name,
                        iam_principals::Column::Active,
                        iam_principals::Column::Email,
                        iam_principals::Column::EmailDomain,
                        iam_principals::Column::IdpType,
                        iam_principals::Column::ExternalIdpId,
                        iam_principals::Column::PasswordHash,
                        iam_principals::Column::LastLoginAt,
                        iam_principals::Column::ServiceAccountId,
                        iam_principals::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;

        // Sync roles
        iam_principal_roles::Entity::delete_many()
            .filter(iam_principal_roles::Column::PrincipalId.eq(&self.id))
            .exec(txn)
            .await?;
        PrincipalRepository::insert_roles_txn(&self.id, &self.roles, txn).await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        iam_principal_roles::Entity::delete_many()
            .filter(iam_principal_roles::Column::PrincipalId.eq(&self.id))
            .exec(txn)
            .await?;
        iam_principals::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}
