//! What an application's connections sync writes, as one aggregate (Go
//! `connection/operations/sync.go`): the connections to save, each with its
//! source, and the ids to delete, for one application and client scope.

use super::entity::Connection;
use crate::usecase::unit_of_work::HasId;

/// Where a connection came from (Go `connection.Source`).
pub const SOURCE_API: &str = "API";
pub const SOURCE_CODE: &str = "CODE";
pub const SOURCE_UI: &str = "UI";

#[derive(Debug, Clone)]
pub struct ConnectionSyncPlan {
    pub application_code: String,
    pub client_id: Option<String>,
    /// Connections to upsert, with their stored source.
    pub saves: Vec<(Connection, String)>,
    /// Connection ids to delete.
    pub deletes: Vec<String>,
}

impl HasId for ConnectionSyncPlan {
    fn id(&self) -> &str {
        &self.application_code
    }
}
