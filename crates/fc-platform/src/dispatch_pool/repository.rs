//! DispatchPool Repository

use mongodb::{Collection, Database, bson::doc};
use futures::TryStreamExt;
use crate::DispatchPool;
use crate::shared::error::Result;

pub struct DispatchPoolRepository {
    collection: Collection<DispatchPool>,
}

impl DispatchPoolRepository {
    pub fn new(db: &Database) -> Self {
        Self {
            collection: db.collection("dispatch_pools"),
        }
    }

    pub async fn insert(&self, pool: &DispatchPool) -> Result<()> {
        self.collection.insert_one(pool).await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<DispatchPool>> {
        Ok(self.collection.find_one(doc! { "_id": id }).await?)
    }

    pub async fn find_by_code(&self, code: &str) -> Result<Option<DispatchPool>> {
        Ok(self.collection.find_one(doc! { "code": code }).await?)
    }

    pub async fn find_by_code_and_client(&self, code: &str, client_id: Option<&str>) -> Result<Option<DispatchPool>> {
        let filter = match client_id {
            Some(id) => doc! { "code": code, "$or": [{ "clientId": id }, { "clientId": null }] },
            None => doc! { "code": code, "clientId": null },
        };
        Ok(self.collection.find_one(filter).await?)
    }

    pub async fn find_active(&self) -> Result<Vec<DispatchPool>> {
        let cursor = self.collection
            .find(doc! { "status": "ACTIVE" })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn find_by_client(&self, client_id: Option<&str>) -> Result<Vec<DispatchPool>> {
        let filter = match client_id {
            Some(id) => doc! { "$or": [{ "clientId": id }, { "clientId": null }] },
            None => doc! { "clientId": null },
        };
        let cursor = self.collection.find(filter).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn update(&self, pool: &DispatchPool) -> Result<()> {
        self.collection
            .replace_one(doc! { "_id": &pool.id }, pool)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = self.collection.delete_one(doc! { "_id": id }).await?;
        Ok(result.deleted_count > 0)
    }
}
