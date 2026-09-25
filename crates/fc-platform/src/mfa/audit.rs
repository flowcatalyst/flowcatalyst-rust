//! The audit rows Go writes for 2FA state changes (`auditMFA`,
//! auth/login/twofactor.go:318-332; the admin reset,
//! principal/api/api.go:1506-1516): entity `PRINCIPAL`, the operation name,
//! the acting principal, no command body. Best-effort: a failure is logged.

use tracing::warn;

use crate::audit::entity::AuditLog;
use crate::audit::repository::AuditLogRepository;

pub const TOTP_ENROLLED: &str = "2FA_TOTP_ENROLLED";
pub const EMAIL_ENROLLED: &str = "2FA_EMAIL_ENROLLED";
pub const METHOD_REMOVED: &str = "2FA_METHOD_REMOVED";
pub const RECOVERY_REGENERATED: &str = "2FA_RECOVERY_REGENERATED";
pub const RESET_BY_ADMIN: &str = "2FA_RESET_BY_ADMIN";

/// Record `operation` on principal `principal_id`, performed by `actor_id`.
pub async fn record(
    audit_logs: &AuditLogRepository,
    principal_id: &str,
    operation: &str,
    actor_id: &str,
) {
    let mut log = AuditLog::new(
        "PRINCIPAL",
        principal_id,
        operation,
        None,
        Some(actor_id.to_string()),
    );
    log.id = crate::shared::tsid::generate(crate::EntityType::AuditLog);
    if let Err(e) = audit_logs.insert(&log).await {
        warn!(principal_id, operation, error = %e, "2FA audit row not written");
    }
}
