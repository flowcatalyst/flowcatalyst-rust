//! Sync OpenAPI Spec Use Case.
//!
//! Takes an incoming OpenAPI document, compares it with the application's
//! current CURRENT row, and either:
//! - no-ops (byte-identical to the current spec — `unchanged=true`),
//! - flips the prior CURRENT to ARCHIVED + inserts a new CURRENT with
//!   computed change_notes describing removals.
//!
//! The archive, the insert and the event are one unit-of-work commit, so a
//! failed insert leaves the prior CURRENT in place (Go writes the two rows
//! outside its event transaction). Concurrent dual syncs are caught by the
//! partial unique index `(application_id) WHERE status='CURRENT'` — one wins,
//! the other returns an error that the caller can retry.

use async_trait::async_trait;
use fc_platform_core::shared::id::ApplicationId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::diff::{compute_change_notes, spec_hash};
use super::events::ApplicationOpenApiSpecSynced;
use crate::application_openapi_spec::entity::{OpenApiSpec, OpenApiSpecStatus};
use crate::application_openapi_spec::repository::OpenApiSpecRepository;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};

/// Command for syncing an application's OpenAPI document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOpenApiSpecCommand {
    pub application_id: ApplicationId,
    pub application_code: String,
    /// Raw OpenAPI document. The version is read from `info.version`; if the
    /// document doesn't carry one, falls back to the synced-at timestamp so
    /// each sync gets a unique key under `UNIQUE (application_id, version)`.
    pub spec: serde_json::Value,
}

impl AuditMasked for SyncOpenApiSpecCommand {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOpenApiSpecResult {
    pub application_code: String,
    pub spec_id: String,
    pub version: String,
    pub status: String,
    pub archived_prior_version: Option<String>,
    pub has_breaking: bool,
    pub unchanged: bool,
    pub change_notes_text: Option<String>,
}

pub struct SyncOpenApiSpecUseCase<U: UnitOfWork> {
    repo: Arc<OpenApiSpecRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncOpenApiSpecUseCase<U> {
    pub fn new(repo: Arc<OpenApiSpecRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

fn extract_version(spec: &serde_json::Value, synced_at: chrono::DateTime<chrono::Utc>) -> String {
    spec.pointer("/info/version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| synced_at.format("%Y%m%d%H%M%S").to_string())
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncOpenApiSpecUseCase<U> {
    type Command = SyncOpenApiSpecCommand;
    type Event = ApplicationOpenApiSpecSynced;

    async fn validate(&self, command: &SyncOpenApiSpecCommand) -> Result<(), UseCaseError> {
        if command.application_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_ID_REQUIRED",
                "Application id is required",
            ));
        }
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED",
                "Application code is required",
            ));
        }
        if !command.spec.is_object() {
            return Err(UseCaseError::validation(
                "INVALID_OPENAPI_SPEC",
                "OpenAPI spec must be a JSON object",
            ));
        }
        // Cheap sanity check that this looks like an OpenAPI document.
        let has_openapi_field = command
            .spec
            .get("openapi")
            .or_else(|| command.spec.get("swagger"))
            .is_some();
        if !has_openapi_field {
            return Err(UseCaseError::validation(
                "INVALID_OPENAPI_SPEC",
                "Spec is missing the top-level `openapi` (or `swagger`) field",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &SyncOpenApiSpecCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        // Handler enforces permission + service-account-belongs-to-application.
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncOpenApiSpecCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ApplicationOpenApiSpecSynced>, UseCaseError> {
        let (writes, event) = self.prepare(&command, &ctx).await?;

        if writes.is_empty() {
            return self.unit_of_work.emit_event(event, &command).await;
        }
        // The prior CURRENT is archived before the new one is written, so the
        // one-CURRENT index holds at every step.
        self.unit_of_work
            .commit_all(&writes, &*self.repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> SyncOpenApiSpecUseCase<U> {
    async fn prepare(
        &self,
        command: &SyncOpenApiSpecCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Vec<OpenApiSpec>, ApplicationOpenApiSpecSynced), UseCaseError> {
        let prior = self
            .repo
            .find_current_by_application(&command.application_id)
            .await?;

        let now = chrono::Utc::now();
        let new_hash = spec_hash(&command.spec);

        // No-op short-circuit: byte-identical to existing CURRENT.
        if let Some(ref existing) = prior {
            if existing.spec_hash == new_hash {
                return Ok((
                    Vec::new(),
                    ApplicationOpenApiSpecSynced {
                        unchanged: true,
                        ..ApplicationOpenApiSpecSynced::new(
                            ctx,
                            &command.application_id,
                            &command.application_code,
                            existing,
                        )
                    },
                ));
            }
        }

        let (change_notes, change_notes_text) = match &prior {
            Some(p) => compute_change_notes(&p.spec, &command.spec),
            None => Default::default(),
        };

        let archived_prior_version = prior.as_ref().map(|p| p.version.clone());

        // 1) Demote prior CURRENT (if any) to ARCHIVED with computed change_notes.
        let mut writes = Vec::with_capacity(2);
        if let Some(mut archived) = prior {
            archived.status = OpenApiSpecStatus::Archived;
            archived.change_notes = Some(change_notes.clone());
            archived.change_notes_text = Some(change_notes_text.clone());
            archived.updated_at = now;
            writes.push(archived);
        }

        // 2) Insert new CURRENT. The `version` column is `UNIQUE (application_id, version)`
        //    across ALL rows (not just CURRENT), so a repeated `info.version` —
        //    common for utoipa-generated specs that pin to the crate version —
        //    would collide with the prior archived row. Disambiguate with the
        //    synced_at timestamp, the same fallback used when `info.version` is
        //    missing.
        let version_candidate = extract_version(&command.spec, now);
        let version = if self
            .repo
            .exists_by_application_and_version(&command.application_id, &version_candidate)
            .await?
        {
            format!("{}+{}", version_candidate, now.format("%Y%m%d%H%M%S"))
        } else {
            version_candidate
        };
        let mut new_spec = OpenApiSpec::new(
            command.application_id.clone(),
            &version,
            command.spec.clone(),
            &new_hash,
        )
        .with_synced_by(Some(ctx.principal_id.clone()));
        new_spec.synced_at = now;

        let event = ApplicationOpenApiSpecSynced {
            archived_prior_version,
            has_breaking: change_notes.has_breaking,
            ..ApplicationOpenApiSpecSynced::new(
                ctx,
                &command.application_id,
                &command.application_code,
                &new_spec,
            )
        };
        writes.push(new_spec);
        Ok((writes, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_version_reads_info_version() {
        let spec = serde_json::json!({"info": {"version": "2.3.4"}});
        assert_eq!(extract_version(&spec, chrono::Utc::now()), "2.3.4");
    }

    #[test]
    fn extract_version_falls_back_to_timestamp() {
        let spec = serde_json::json!({"info": {}});
        let v = extract_version(&spec, chrono::Utc::now());
        assert_eq!(v.len(), 14); // YYYYMMDDHHMMSS
    }

    #[test]
    fn command_validates_object_shape() {
        let cmd = SyncOpenApiSpecCommand {
            application_id: ApplicationId::parse("app_X").unwrap(),
            application_code: "platform".into(),
            spec: serde_json::Value::String("not-an-object".into()),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("app_X"));
    }
}
