//! BFF Audit Logs API — **temporary**.
//!
//! Owner spec `docs/spec/audit-redaction.md` in the Java repo, "Temporary:
//! redact existing rows from the dashboard" (2026-09-24; to be removed once
//! the rows written before source-side redaction have been swept, together
//! with the dashboard's "Audit logs" card).
//!
//! | Method | Path | Gate |
//! |---|---|---|
//! | POST | `/bff/audit-logs/redact-existing` | anchor + `platform:admin:audit-log:view` |

use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

use crate::audit::operations::{RedactExistingAuditLogsCommand, RedactExistingAuditLogsUseCase};
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::middleware::Authenticated;
use fc_platform_core::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

#[derive(Clone)]
pub struct BffAuditLogsState {
    pub redact_existing_use_case: Arc<RedactExistingAuditLogsUseCase<PgUnitOfWork>>,
}

/// `{scanned, redacted}`: candidate rows read, rows rewritten.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RedactExistingAuditLogsResponse {
    pub scanned: u64,
    pub redacted: u64,
}

/// Redact passwords and secrets from existing audit rows (temporary).
///
/// Same gate as `/bff/roles/sync-platform` (anchor-only), plus the audit-log
/// read permission: this reads and rewrites `aud_logs`.
#[utoipa::path(
    post,
    path = "/redact-existing",
    tag = "bff-audit-logs",
    operation_id = "postBffAuditLogsRedactExisting",
    responses(
        (status = 200, description = "Existing audit rows redacted", body = RedactExistingAuditLogsResponse),
        (status = 403, description = "Not anchor, or no audit-log read permission")
    ),
    security(("bearer_auth" = []))
)]
pub async fn redact_existing_audit_logs(
    State(state): State<BffAuditLogsState>,
    auth: Authenticated,
) -> Result<Json<RedactExistingAuditLogsResponse>, PlatformError> {
    checks::require_anchor(&auth.0)?;
    checks::can_read_audit_logs(&auth.0)?;

    let ctx = ExecutionContext::from_auth(&auth.0);
    let event = state
        .redact_existing_use_case
        .run(RedactExistingAuditLogsCommand::default(), ctx)
        .await
        .into_result()?;

    Ok(Json(RedactExistingAuditLogsResponse {
        scanned: event.scanned,
        redacted: event.redacted,
    }))
}
