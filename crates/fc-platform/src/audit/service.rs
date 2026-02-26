//! Audit Service
//!
//! Provides centralized audit logging for all platform mutations.
//! Uses the same schema as Java for cross-platform compatibility.

use std::sync::Arc;
use tracing::{info, error};

use crate::AuditLog;
use crate::AuditLogRepository;
use crate::AuthContext;
use crate::shared::error::Result;

/// Audit service for recording platform actions
#[derive(Clone)]
pub struct AuditService {
    repo: Arc<AuditLogRepository>,
}

impl AuditService {
    pub fn new(repo: Arc<AuditLogRepository>) -> Self {
        Self { repo }
    }

    /// Log a create action
    pub async fn log_create(
        &self,
        auth: &AuthContext,
        entity_type: &str,
        entity_id: &str,
        operation: impl Into<String>,
    ) -> Result<()> {
        let log = self.build_log(auth, entity_type, Some(entity_id), operation);
        self.insert(log).await
    }

    /// Log an update action
    pub async fn log_update(
        &self,
        auth: &AuthContext,
        entity_type: &str,
        entity_id: &str,
        operation: impl Into<String>,
    ) -> Result<()> {
        let log = self.build_log(auth, entity_type, Some(entity_id), operation);
        self.insert(log).await
    }

    /// Log a delete action
    pub async fn log_delete(
        &self,
        auth: &AuthContext,
        entity_type: &str,
        entity_id: &str,
        operation: impl Into<String>,
    ) -> Result<()> {
        let log = self.build_log(auth, entity_type, Some(entity_id), operation);
        self.insert(log).await
    }

    /// Log an archive action
    pub async fn log_archive(
        &self,
        auth: &AuthContext,
        entity_type: &str,
        entity_id: &str,
        operation: impl Into<String>,
    ) -> Result<()> {
        let log = self.build_log(auth, entity_type, Some(entity_id), operation);
        self.insert(log).await
    }

    /// Log a role assignment
    pub async fn log_role_assigned(
        &self,
        auth: &AuthContext,
        principal_id: &str,
        _role: &str,
        _client_id: Option<&str>,
    ) -> Result<()> {
        let log = self.build_log(auth, "Principal", Some(principal_id), "AssignRoleCommand");
        self.insert(log).await
    }

    /// Log a role removal
    pub async fn log_role_unassigned(
        &self,
        auth: &AuthContext,
        principal_id: &str,
        _role: &str,
    ) -> Result<()> {
        let log = self.build_log(auth, "Principal", Some(principal_id), "UnassignRoleCommand");
        self.insert(log).await
    }

    /// Log client access granted
    pub async fn log_client_access_granted(
        &self,
        auth: &AuthContext,
        principal_id: &str,
        _client_id: &str,
    ) -> Result<()> {
        let log = self.build_log(auth, "Principal", Some(principal_id), "GrantClientAccessCommand");
        self.insert(log).await
    }

    /// Log client access revoked
    pub async fn log_client_access_revoked(
        &self,
        auth: &AuthContext,
        principal_id: &str,
        _client_id: &str,
    ) -> Result<()> {
        let log = self.build_log(auth, "Principal", Some(principal_id), "RevokeClientAccessCommand");
        self.insert(log).await
    }

    /// Log a login attempt
    pub async fn log_login(
        &self,
        _email: &str,
        success: bool,
        _ip_address: Option<&str>,
    ) -> Result<()> {
        let operation = if success { "LoginCommand" } else { "FailedLoginCommand" };
        let log = AuditLog::new("Session", None, operation, None, None);
        self.insert(log).await
    }

    /// Log a logout
    pub async fn log_logout(&self, auth: &AuthContext) -> Result<()> {
        let log = self.build_log(auth, "Session", None, "LogoutCommand");
        self.insert(log).await
    }

    /// Build an audit log from auth context (matches Java schema)
    fn build_log(
        &self,
        auth: &AuthContext,
        entity_type: &str,
        entity_id: Option<&str>,
        operation: impl Into<String>,
    ) -> AuditLog {
        AuditLog::new(
            entity_type,
            entity_id.map(String::from),
            operation,
            None,
            Some(auth.principal_id.clone()),
        )
    }

    /// Insert an audit log
    async fn insert(&self, log: AuditLog) -> Result<()> {
        info!(
            operation = %log.operation,
            entity_type = %log.entity_type,
            entity_id = ?log.entity_id,
            principal_id = ?log.principal_id,
            "Audit log recorded"
        );

        if let Err(e) = self.repo.insert(&log).await {
            error!(error = %e, "Failed to insert audit log");
            // Don't fail the operation if audit logging fails
            // but log the error for monitoring
        }

        Ok(())
    }
}
