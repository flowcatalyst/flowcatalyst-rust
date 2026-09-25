//! Create Service Account Use Case

use async_trait::async_trait;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::client_reach::{dedupe_client_ids, require_clients_exist};
use super::events::ServiceAccountCreated;
use crate::principal::entity::UserScope;
use crate::role::entity::roles;
use crate::service_account::entity::{AssignmentSource, RoleAssignment};
use crate::shared::encryption_service::{require_configured, EncryptionService};
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::{ClientRepository, ServiceAccountRepository};
use crate::{ServiceAccount, WebhookCredentials};

/// Generate a bearer token with fc_ prefix
fn generate_auth_token() -> String {
    let random_part: String = (0..32)
        .map(|_| {
            let idx = rand::rng().random_range(0..36);
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'a' + idx - 10) as char
            }
        })
        .collect();
    format!("fc_{}", random_part)
}

/// Generate a signing secret (URL-safe base64)
fn generate_signing_secret() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// The code rule, as Go's create has it
/// (flowcatalyst-go serviceaccount/operations/create_credentials.go:65-76):
/// trimmed and lower-cased; empty is `CODE_REQUIRED`; the `app:` namespace,
/// which belongs to application service accounts, is `RESERVED_CODE` (so
/// `app:` alone is reserved too); otherwise it must match
/// `^[a-z][a-z0-9-]*$` (shared/validate/validate.go:18), else
/// `INVALID_CODE_FORMAT`. Only a code chosen through the API
/// (`user_chosen`) is checked. Provisioning (an application's own account)
/// keeps `app:<applicationCode>` as it is, unchecked, as Go's provisioning
/// does (application/operations/provision_service_account.go:101).
fn normalise_code(raw: &str, user_chosen: bool) -> Result<String, UseCaseError> {
    if !user_chosen {
        let code = raw.trim();
        return if code.is_empty() {
            Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "code is required",
            ))
        } else {
            Ok(code.to_string())
        };
    }
    let code = raw.trim().to_lowercase();
    if code.is_empty() {
        return Err(UseCaseError::validation(
            "CODE_REQUIRED",
            "code is required",
        ));
    }
    if code.starts_with("app:") {
        return Err(UseCaseError::validation(
            "RESERVED_CODE",
            "codes starting with 'app:' are reserved for application service accounts",
        ));
    }
    let well_formed = code.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !well_formed {
        return Err(UseCaseError::validation(
            "INVALID_CODE_FORMAT",
            "code must start with a lowercase letter and contain only lowercase alphanumeric and hyphens",
        ));
    }
    Ok(code)
}

/// Command for creating a new service account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateServiceAccountCommand {
    /// Unique code (1-50 chars)
    pub code: String,

    /// Human-readable name (1-100 chars)
    pub name: String,

    /// Optional description (max 500 chars)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Requested scope, stored on the account as sent. As in Go it doesn't
    /// decide the token tier, which follows `client_ids` (none → ANCHOR, one
    /// → CLIENT, several → PARTNER).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<UserScope>,

    /// Client IDs this account can access
    #[serde(default)]
    pub client_ids: Vec<String>,

    /// Application ID, set only by application provisioning. The account is
    /// bound to it and granted it; otherwise it starts with no application
    /// access at all (owner ruling).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,

    /// Grant the account every application, present and future (Go's
    /// `allApplications`, serviceaccount/operations/create.go:21-24). Off by
    /// default: a new account starts with no application access. Can't be
    /// combined with `application_id` (`ALL_APPLICATIONS_WITH_APPLICATION_ID`).
    /// Only a caller that itself reaches every application may ask for it;
    /// the handler checks that.
    #[serde(default)]
    pub all_applications: bool,
}

impl crate::usecase::AuditMasked for CreateServiceAccountCommand {}

/// Result returned from create service account use case.
/// Contains the event plus one-time secrets that need to be returned to caller.
/// The secrets are never serialized, so this serializes exactly as the event.
#[derive(Serialize)]
pub struct CreateServiceAccountResult {
    #[serde(flatten)]
    pub event: ServiceAccountCreated,
    #[serde(skip_serializing)]
    pub auth_token: String,
    #[serde(skip_serializing)]
    pub signing_secret: String,
}

crate::impl_domain_event!(CreateServiceAccountResult => event);

/// Use case for creating a new service account.
pub struct CreateServiceAccountUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
    /// Encrypts the generated credential before it is stored. `None` when no
    /// key is configured; the use case then fails rather than store plaintext.
    encryption: Option<Arc<EncryptionService>>,
}

impl<U: UnitOfWork> CreateServiceAccountUseCase<U> {
    pub fn new(
        service_account_repo: Arc<ServiceAccountRepository>,
        client_repo: Arc<ClientRepository>,
        unit_of_work: Arc<U>,
        encryption: Option<Arc<EncryptionService>>,
    ) -> Self {
        Self {
            service_account_repo,
            client_repo,
            unit_of_work,
            encryption,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateServiceAccountUseCase<U> {
    type Command = CreateServiceAccountCommand;
    type Event = CreateServiceAccountResult;

    async fn validate(&self, command: &CreateServiceAccountCommand) -> Result<(), UseCaseError> {
        // Go create_credentials.go:80-84.
        if command.all_applications
            && command
                .application_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty())
        {
            return Err(UseCaseError::validation(
                "ALL_APPLICATIONS_WITH_APPLICATION_ID",
                "allApplications cannot be combined with applicationId",
            ));
        }
        let code = normalise_code(&command.code, command.application_id.is_none())?;
        if code.len() > 50 {
            return Err(UseCaseError::validation(
                "INVALID_CODE",
                "Code must be 1-50 characters",
            ));
        }

        let name = command.name.trim();
        if name.is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "name is required",
            ));
        }
        if name.len() > 100 {
            return Err(UseCaseError::validation(
                "INVALID_NAME",
                "Name must be 1-100 characters",
            ));
        }

        if let Some(ref desc) = command.description {
            if desc.len() > 500 {
                return Err(UseCaseError::validation(
                    "INVALID_DESCRIPTION",
                    "Description must be max 500 characters",
                ));
            }
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateServiceAccountCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateServiceAccountCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<CreateServiceAccountResult> {
        let code = match normalise_code(&command.code, command.application_id.is_none()) {
            Ok(code) => code,
            Err(e) => return UseCaseResult::failure(e),
        };
        let code = code.as_str();
        let name = command.name.trim();

        // Business rule: code must be unique (Go: 409 CODE_EXISTS,
        // create_credentials.go:91-98).
        let existing = match self.service_account_repo.find_by_code(code).await {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!("Service account with code '{}' already exists", code),
            ));
        }

        let client_ids = dedupe_client_ids(command.client_ids.clone());
        if let Err(e) = require_clients_exist(&self.client_repo, &client_ids).await {
            return UseCaseResult::failure(e);
        }

        // Generate credentials. The caller gets the plaintext once in the
        // result; only the `encrypted:` form is stored.
        let auth_token = generate_auth_token();
        let signing_secret = generate_signing_secret();
        let sealed = require_configured(self.encryption.as_deref()).and_then(|enc| {
            Ok((
                enc.encrypt_ref(&auth_token)?,
                enc.encrypt_ref(&signing_secret)?,
            ))
        });
        let (auth_token_ref, signing_secret_ref) = match sealed {
            Ok(refs) => refs,
            Err(e) => return UseCaseResult::failure(e.into()),
        };

        // Create the service account entity
        let mut service_account = ServiceAccount::new(code, name, UserScope::Anchor);
        service_account.description = command.description.clone();
        // As Go does (create_credentials.go:102, 125): the requested scope is
        // stored as sent and the principal's tier follows the client links.
        //
        // Known Go behaviour, kept on purpose and flagged to the owner as a
        // risk: a CLIENT (or PARTNER) scope requested with no clients stores
        // that scope but yields an ANCHOR principal, which reaches every
        // client. Only an unrecognised scope value is refused (X-06).
        service_account.requested_scope = command.scope.map(|s| s.as_str().to_string());
        service_account.link_clients(client_ids);
        service_account.application_id = command.application_id.clone();
        // No application access unless the account is made for one
        // application, which it then reaches alone (Go: all_applications
        // false plus a single access row), or asks for all of them (Go
        // create_credentials.go:118).
        service_account.all_applications = command.all_applications;
        service_account.accessible_application_ids =
            command.application_id.clone().into_iter().collect();
        // An application's own service account is granted the seeded
        // least-privilege `platform:application-service` role, marked
        // PROVISIONED, as Go's provisioning does
        // (application/operations/provision_service_account.go:159-165).
        // Without it the account's token carries no permissions and every
        // SDK sync call is a 403 until an admin assigns the role.
        if command.application_id.is_some() {
            service_account.roles = vec![RoleAssignment::with_source(
                roles::application_service().name,
                AssignmentSource::Provisioned,
            )];
        }
        service_account.webhook_credentials = WebhookCredentials::bearer_token(&auth_token_ref);
        service_account.webhook_credentials.signing_secret = Some(signing_secret_ref);

        // Create domain event
        let event = ServiceAccountCreated::new(
            &ctx,
            &service_account.id,
            &service_account.code,
            &service_account.name,
        );

        // Create result with one-time secrets
        let result = CreateServiceAccountResult {
            event: event.clone(),
            auth_token,
            signing_secret,
        };

        // Atomic commit through UnitOfWork. `.map()` is defined inside the
        // usecase module, so it can translate the committed event into our
        // wrapper result (which carries the one-time secrets) without
        // bypassing the seal.
        self.unit_of_work
            .commit(
                &service_account,
                &*self.service_account_repo,
                event,
                &command,
            )
            .await
            .map(|_| result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::unit_of_work::HasId;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateServiceAccountCommand {
            code: "my-service".to_string(),
            name: "My Service Account".to_string(),
            description: Some("Handles order processing".to_string()),
            scope: Some(UserScope::Client),
            client_ids: vec!["client-123".to_string()],
            application_id: None,
            all_applications: false,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("my-service"));
        assert!(json.contains("My Service Account"));
        assert!(json.contains(r#""scope":"CLIENT""#));
    }

    #[test]
    fn test_service_account_has_id() {
        let sa = ServiceAccount::new("test", "Test", UserScope::Client);
        assert!(!sa.id().is_empty());
    }

    #[test]
    fn code_rule_matches_go() {
        // Trimmed and lower-cased.
        assert_eq!(
            normalise_code(" SACreate-Happy ", true).unwrap(),
            "sacreate-happy"
        );
        for (raw, code) in [
            ("", "CODE_REQUIRED"),
            ("   ", "CODE_REQUIRED"),
            ("1abc", "INVALID_CODE_FORMAT"),
            ("-abc", "INVALID_CODE_FORMAT"),
            ("a_b", "INVALID_CODE_FORMAT"),
            ("a b", "INVALID_CODE_FORMAT"),
            ("app:", "RESERVED_CODE"),
            ("app:orders", "RESERVED_CODE"),
            ("APP:1abc", "RESERVED_CODE"),
        ] {
            assert_eq!(
                normalise_code(raw, true).unwrap_err().code(),
                code,
                "{raw:?}"
            );
        }
        // Provisioning keeps its app: code unchecked.
        assert_eq!(normalise_code("app:My_App", false).unwrap(), "app:My_App");
    }

    #[test]
    fn test_generate_auth_token() {
        let token = generate_auth_token();
        assert!(token.starts_with("fc_"));
        assert_eq!(token.len(), 35); // "fc_" + 32 chars
    }

    #[test]
    fn test_generate_signing_secret() {
        let secret = generate_signing_secret();
        assert!(!secret.is_empty());
        // URL-safe base64 of 32 bytes should be ~43 chars
        assert!(secret.len() > 40);
    }
}
