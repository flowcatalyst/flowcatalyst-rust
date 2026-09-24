//! Audit Service
//!
//! Provides centralized audit logging for all platform mutations.

use std::sync::Arc;
use tracing::{error, info};

use crate::AuditLog;
use crate::AuditLogRepository;
use crate::AuthContext;

/// Audit service for recording platform actions
#[derive(Clone)]
pub struct AuditService {
    repo: Arc<AuditLogRepository>,
}

impl AuditService {
    pub fn new(repo: Arc<AuditLogRepository>) -> Self {
        Self { repo }
    }

    /// Record that `auth`'s principal performed `operation` on an entity.
    ///
    /// Best-effort: a failed insert is logged and swallowed so the caller's
    /// operation is never failed by audit logging.
    pub async fn log(
        &self,
        auth: &AuthContext,
        entity_type: &str,
        entity_id: &str,
        operation: impl Into<String>,
    ) {
        let log = AuditLog::new(
            entity_type,
            entity_id,
            operation,
            None,
            Some(auth.principal_id.clone()),
        );

        info!(
            operation = %log.operation,
            entity_type = %log.entity_type,
            entity_id = ?log.entity_id,
            principal_id = ?log.principal_id,
            "Audit log recorded"
        );

        if let Err(e) = self.repo.insert(&log).await {
            error!(error = %e, "Failed to insert audit log");
        }
    }
}
