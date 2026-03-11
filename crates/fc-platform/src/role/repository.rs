//! Role Repository
//!
//! PostgreSQL persistence for AuthRole entities using SeaORM.
//! Permissions are stored in the iam_role_permissions junction table.

use async_trait::async_trait;
use sea_orm::*;
use sea_orm::sea_query::OnConflict;
use chrono::Utc;

use super::entity::{AuthRole, RoleSource};
use crate::entities::{iam_roles, iam_role_permissions};
use crate::shared::error::Result;
use crate::usecase::unit_of_work::{HasId, PgPersist};

pub struct RoleRepository {
    db: DatabaseConnection,
}

impl RoleRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    pub async fn insert(&self, role: &AuthRole) -> Result<()> {
        let model = iam_roles::ActiveModel {
            id: Set(role.id.clone()),
            application_id: Set(role.application_id.clone()),
            application_code: Set(Some(role.application_code.clone())),
            name: Set(role.name.clone()),
            display_name: Set(role.display_name.clone()),
            description: Set(role.description.clone()),
            source: Set(role.source.as_str().to_string()),
            client_managed: Set(role.client_managed),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };

        iam_roles::Entity::insert(model)
            .exec(&self.db)
            .await?;

        // Insert permissions into junction table
        self.insert_permissions(&role.id, &role.permissions).await?;

        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<AuthRole>> {
        let result = iam_roles::Entity::find_by_id(id)
            .one(&self.db)
            .await?;

        match result {
            Some(model) => {
                let mut role = AuthRole::from(model);
                role.permissions = self.load_permissions(&role.id).await?;
                Ok(Some(role))
            }
            None => Ok(None),
        }
    }

    /// Find role by name (formerly find_by_code)
    pub async fn find_by_name(&self, name: &str) -> Result<Option<AuthRole>> {
        let result = iam_roles::Entity::find()
            .filter(iam_roles::Column::Name.eq(name))
            .one(&self.db)
            .await?;

        match result {
            Some(model) => {
                let mut role = AuthRole::from(model);
                role.permissions = self.load_permissions(&role.id).await?;
                Ok(Some(role))
            }
            None => Ok(None),
        }
    }

    /// Backward-compatible alias for find_by_name
    pub async fn find_by_code(&self, code: &str) -> Result<Option<AuthRole>> {
        self.find_by_name(code).await
    }

    pub async fn find_all(&self) -> Result<Vec<AuthRole>> {
        let results = iam_roles::Entity::find()
            .all(&self.db)
            .await?;

        self.hydrate_roles(results).await
    }

    pub async fn find_by_application(&self, application_code: &str) -> Result<Vec<AuthRole>> {
        let results = iam_roles::Entity::find()
            .filter(iam_roles::Column::ApplicationCode.eq(application_code))
            .all(&self.db)
            .await?;

        self.hydrate_roles(results).await
    }

    pub async fn find_by_application_id(&self, application_id: &str) -> Result<Vec<AuthRole>> {
        let results = iam_roles::Entity::find()
            .filter(iam_roles::Column::ApplicationId.eq(application_id))
            .all(&self.db)
            .await?;

        self.hydrate_roles(results).await
    }

    pub async fn find_by_source(&self, source: RoleSource) -> Result<Vec<AuthRole>> {
        let results = iam_roles::Entity::find()
            .filter(iam_roles::Column::Source.eq(source.as_str()))
            .all(&self.db)
            .await?;

        self.hydrate_roles(results).await
    }

    pub async fn find_client_managed(&self) -> Result<Vec<AuthRole>> {
        let results = iam_roles::Entity::find()
            .filter(iam_roles::Column::ClientManaged.eq(true))
            .all(&self.db)
            .await?;

        self.hydrate_roles(results).await
    }

    pub async fn find_by_codes(&self, codes: &[String]) -> Result<Vec<AuthRole>> {
        if codes.is_empty() {
            return Ok(vec![]);
        }
        let results = iam_roles::Entity::find()
            .filter(iam_roles::Column::Name.is_in(codes.to_vec()))
            .all(&self.db)
            .await?;

        self.hydrate_roles(results).await
    }

    /// Search roles by name or display_name (case-insensitive partial match)
    pub async fn search(&self, term: &str) -> Result<Vec<AuthRole>> {
        let pattern = format!("%{}%", term);
        let results = iam_roles::Entity::find()
            .filter(
                Condition::any()
                    .add(iam_roles::Column::Name.like(&pattern))
                    .add(iam_roles::Column::DisplayName.like(&pattern))
            )
            .all(&self.db)
            .await?;

        self.hydrate_roles(results).await
    }

    pub async fn find_with_permission(&self, permission: &str) -> Result<Vec<AuthRole>> {
        // Find role_ids that have this permission
        let role_ids: Vec<String> = iam_role_permissions::Entity::find()
            .filter(iam_role_permissions::Column::Permission.eq(permission))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|rp| rp.role_id)
            .collect();

        if role_ids.is_empty() {
            return Ok(vec![]);
        }

        let results = iam_roles::Entity::find()
            .filter(iam_roles::Column::Id.is_in(role_ids))
            .all(&self.db)
            .await?;

        self.hydrate_roles(results).await
    }

    pub async fn exists(&self, id: &str) -> Result<bool> {
        let count = iam_roles::Entity::find_by_id(id)
            .count(&self.db)
            .await?;
        Ok(count > 0)
    }

    /// Check if a role with the given name exists
    pub async fn exists_by_name(&self, name: &str) -> Result<bool> {
        self.exists_by_code(name).await
    }

    pub async fn exists_by_code(&self, code: &str) -> Result<bool> {
        let count = iam_roles::Entity::find()
            .filter(iam_roles::Column::Name.eq(code))
            .count(&self.db)
            .await?;
        Ok(count > 0)
    }

    pub async fn update(&self, role: &AuthRole) -> Result<()> {
        let model = iam_roles::ActiveModel {
            id: Set(role.id.clone()),
            application_id: Set(role.application_id.clone()),
            application_code: Set(Some(role.application_code.clone())),
            name: Set(role.name.clone()),
            display_name: Set(role.display_name.clone()),
            description: Set(role.description.clone()),
            source: Set(role.source.as_str().to_string()),
            client_managed: Set(role.client_managed),
            created_at: NotSet,
            updated_at: Set(Utc::now().into()),
        };

        iam_roles::Entity::update(model)
            .exec(&self.db)
            .await?;

        // Sync permissions: delete all then re-insert
        iam_role_permissions::Entity::delete_many()
            .filter(iam_role_permissions::Column::RoleId.eq(&role.id))
            .exec(&self.db)
            .await?;

        self.insert_permissions(&role.id, &role.permissions).await?;

        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        // Permissions cascade due to ON DELETE CASCADE
        let result = iam_roles::Entity::delete_by_id(id)
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    // ── Helpers ──────────────────────────────────────────────

    /// Load permissions for a role from the junction table
    async fn load_permissions(&self, role_id: &str) -> Result<std::collections::HashSet<String>> {
        let perms = iam_role_permissions::Entity::find()
            .filter(iam_role_permissions::Column::RoleId.eq(role_id))
            .all(&self.db)
            .await?;

        Ok(perms.into_iter().map(|p| p.permission).collect())
    }

    /// Insert permissions into the junction table
    async fn insert_permissions(&self, role_id: &str, permissions: &std::collections::HashSet<String>) -> Result<()> {
        if permissions.is_empty() {
            return Ok(());
        }

        let models: Vec<iam_role_permissions::ActiveModel> = permissions
            .iter()
            .map(|perm| iam_role_permissions::ActiveModel {
                role_id: Set(role_id.to_string()),
                permission: Set(perm.clone()),
            })
            .collect();

        iam_role_permissions::Entity::insert_many(models)
            .exec(&self.db)
            .await?;

        Ok(())
    }

    pub(crate) async fn insert_permissions_txn(
        role_id: &str,
        permissions: &std::collections::HashSet<String>,
        txn: &sea_orm::DatabaseTransaction,
    ) -> Result<()> {
        if permissions.is_empty() { return Ok(()); }
        let models: Vec<iam_role_permissions::ActiveModel> = permissions
            .iter()
            .map(|perm| iam_role_permissions::ActiveModel {
                role_id: Set(role_id.to_string()),
                permission: Set(perm.clone()),
            })
            .collect();
        iam_role_permissions::Entity::insert_many(models).exec(txn).await?;
        Ok(())
    }

    /// Convert a list of DB models to domain entities with permissions loaded
    async fn hydrate_roles(&self, models: Vec<iam_roles::Model>) -> Result<Vec<AuthRole>> {
        if models.is_empty() {
            return Ok(vec![]);
        }

        // Batch-load all permissions for these roles
        let role_ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
        let all_perms = iam_role_permissions::Entity::find()
            .filter(iam_role_permissions::Column::RoleId.is_in(role_ids))
            .all(&self.db)
            .await?;

        // Group permissions by role_id
        let mut perm_map: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for rp in all_perms {
            perm_map
                .entry(rp.role_id)
                .or_default()
                .insert(rp.permission);
        }

        // Build domain entities
        let roles = models
            .into_iter()
            .map(|m| {
                let id = m.id.clone();
                let mut role = AuthRole::from(m);
                if let Some(perms) = perm_map.remove(&id) {
                    role.permissions = perms;
                }
                role
            })
            .collect();

        Ok(roles)
    }
}

// ── PgPersist implementation ──────────────────────────────────────────────────

impl HasId for AuthRole {
    fn id(&self) -> &str { &self.id }
}

#[async_trait]
impl PgPersist for AuthRole {
    async fn pg_upsert(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        let model = iam_roles::ActiveModel {
            id: Set(self.id.clone()),
            application_id: Set(self.application_id.clone()),
            application_code: Set(Some(self.application_code.clone())),
            name: Set(self.name.clone()),
            display_name: Set(self.display_name.clone()),
            description: Set(self.description.clone()),
            source: Set(self.source.as_str().to_string()),
            client_managed: Set(self.client_managed),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
        };
        iam_roles::Entity::insert(model)
            .on_conflict(
                OnConflict::column(iam_roles::Column::Id)
                    .update_columns([
                        iam_roles::Column::ApplicationId,
                        iam_roles::Column::ApplicationCode,
                        iam_roles::Column::Name,
                        iam_roles::Column::DisplayName,
                        iam_roles::Column::Description,
                        iam_roles::Column::Source,
                        iam_roles::Column::ClientManaged,
                        iam_roles::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(txn)
            .await?;

        // Sync permissions
        iam_role_permissions::Entity::delete_many()
            .filter(iam_role_permissions::Column::RoleId.eq(&self.id))
            .exec(txn)
            .await?;
        RoleRepository::insert_permissions_txn(&self.id, &self.permissions, txn).await?;
        Ok(())
    }

    async fn pg_delete(&self, txn: &sea_orm::DatabaseTransaction) -> Result<()> {
        iam_roles::Entity::delete_by_id(&self.id).exec(txn).await?;
        Ok(())
    }
}
