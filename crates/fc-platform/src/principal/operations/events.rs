//! Principal Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::impl_domain_event;
use crate::principal::entity::UserScope;

/// Event emitted when a new user is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    pub email: String,
    pub email_domain: String,
    pub name: String,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub is_anchor_user: bool,
}

impl_domain_event!(UserCreated);

impl UserCreated {
    const EVENT_TYPE: &'static str = "platform:iam:user:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    /// Create a new UserCreated event (legacy constructor).
    ///
    /// Prefer using `UserCreated::builder()` for better readability.
    pub fn new(
        ctx: &ExecutionContext,
        principal_id: &str,
        email: &str,
        name: &str,
        scope: UserScope,
        client_id: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.user.{}", principal_id);
        let message_group = format!("platform:user:{}", principal_id);
        let email_domain = extract_email_domain(email);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            principal_id: principal_id.to_string(),
            email: email.to_string(),
            email_domain,
            name: name.to_string(),
            scope: format!("{:?}", scope).to_uppercase(),
            client_id: client_id.map(String::from),
            is_anchor_user: false,
        }
    }

    /// Create a builder for UserCreated event.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let event = UserCreated::builder()
    ///     .from(&ctx)
    ///     .principal_id(&user.id)
    ///     .email(&email)
    ///     .name(&user.name)
    ///     .scope(UserScope::Client)
    ///     .client_id(Some(&client_id))
    ///     .is_anchor_user(false)
    ///     .build();
    /// ```
    pub fn builder() -> UserCreatedBuilder {
        UserCreatedBuilder::new()
    }
}

/// Builder for UserCreated event.
#[derive(Default)]
pub struct UserCreatedBuilder {
    ctx: Option<ExecutionContext>,
    principal_id: Option<String>,
    email: Option<String>,
    name: Option<String>,
    scope: Option<UserScope>,
    client_id: Option<String>,
    is_anchor_user: bool,
}

impl UserCreatedBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Copy tracing metadata from ExecutionContext.
    pub fn from(mut self, ctx: &ExecutionContext) -> Self {
        self.ctx = Some(ctx.clone());
        self
    }

    pub fn principal_id(mut self, id: impl Into<String>) -> Self {
        self.principal_id = Some(id.into());
        self
    }

    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn scope(mut self, scope: UserScope) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn client_id(mut self, client_id: Option<impl Into<String>>) -> Self {
        self.client_id = client_id.map(|s| s.into());
        self
    }

    pub fn is_anchor_user(mut self, is_anchor: bool) -> Self {
        self.is_anchor_user = is_anchor;
        self
    }

    /// Build the UserCreated event.
    ///
    /// # Panics
    ///
    /// Panics if required fields are missing.
    pub fn build(self) -> UserCreated {
        let ctx = self.ctx.expect("ExecutionContext is required (use .from(ctx))");
        let principal_id = self.principal_id.expect("principal_id is required");
        let email = self.email.expect("email is required");
        let name = self.name.unwrap_or_else(|| email.split('@').next().unwrap_or("").to_string());
        let scope = self.scope.expect("scope is required");

        let subject = format!("platform.user.{}", principal_id);
        let message_group = format!("platform:user:{}", principal_id);
        let email_domain = extract_email_domain(&email);

        UserCreated {
            metadata: EventMetadata::builder()
                .from(&ctx)
                .event_type(UserCreated::EVENT_TYPE)
                .spec_version(UserCreated::SPEC_VERSION)
                .source(UserCreated::SOURCE)
                .subject(subject)
                .message_group(message_group)
                .build(),
            principal_id,
            email,
            email_domain,
            name,
            scope: format!("{:?}", scope).to_uppercase(),
            client_id: self.client_id,
            is_anchor_user: self.is_anchor_user,
        }
    }
}

/// Extract the domain part from an email address.
fn extract_email_domain(email: &str) -> String {
    email
        .split('@')
        .nth(1)
        .map(|s| s.to_lowercase())
        .unwrap_or_default()
}

/// Event emitted when a user is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl_domain_event!(UserUpdated);

impl UserUpdated {
    const EVENT_TYPE: &'static str = "platform:iam:user:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        principal_id: &str,
        name: Option<&str>,
        email: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.user.{}", principal_id);
        let message_group = format!("platform:user:{}", principal_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            principal_id: principal_id.to_string(),
            name: name.map(String::from),
            email: email.map(String::from),
        }
    }
}

/// Event emitted when a user is activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserActivated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
}

impl_domain_event!(UserActivated);

impl UserActivated {
    const EVENT_TYPE: &'static str = "platform:iam:user:activated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, principal_id: &str) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.user.{}", principal_id);
        let message_group = format!("platform:user:{}", principal_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            principal_id: principal_id.to_string(),
        }
    }
}

/// Event emitted when a user is deactivated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDeactivated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl_domain_event!(UserDeactivated);

impl UserDeactivated {
    const EVENT_TYPE: &'static str = "platform:iam:user:deactivated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, principal_id: &str, reason: Option<&str>) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.user.{}", principal_id);
        let message_group = format!("platform:user:{}", principal_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            principal_id: principal_id.to_string(),
            reason: reason.map(String::from),
        }
    }
}

/// Event emitted when a user is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
}

impl_domain_event!(UserDeleted);

impl UserDeleted {
    const EVENT_TYPE: &'static str = "platform:iam:user:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, principal_id: &str) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.user.{}", principal_id);
        let message_group = format!("platform:user:{}", principal_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            principal_id: principal_id.to_string(),
        }
    }
}

/// Event emitted when roles are assigned to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolesAssigned {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub principal_id: String,
    pub roles: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl_domain_event!(RolesAssigned);

impl RolesAssigned {
    const EVENT_TYPE: &'static str = "platform:iam:user:roles-assigned";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        principal_id: &str,
        roles: Vec<String>,
        added: Vec<String>,
        removed: Vec<String>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.user.{}", principal_id);
        let message_group = format!("platform:user:{}", principal_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            principal_id: principal_id.to_string(),
            roles,
            added,
            removed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::DomainEvent;

    #[test]
    fn test_user_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = UserCreated::new(
            &ctx,
            "user-1",
            "user@example.com",
            "Test User",
            UserScope::Client,
            Some("client-1"),
        );

        assert_eq!(event.event_type(), "platform:iam:user:created");
        assert_eq!(event.principal_id, "user-1");
        assert_eq!(event.email, "user@example.com");
        assert_eq!(event.email_domain, "example.com");
        assert_eq!(event.scope, "CLIENT");
    }

    #[test]
    fn test_user_created_builder() {
        let ctx = ExecutionContext::create("admin-123");
        let event = UserCreated::builder()
            .from(&ctx)
            .principal_id("user-1")
            .email("john.doe@acme.org")
            .name("John Doe")
            .scope(UserScope::Client)
            .client_id(Some("client-1"))
            .is_anchor_user(false)
            .build();

        assert_eq!(event.event_type(), "platform:iam:user:created");
        assert_eq!(event.principal_id, "user-1");
        assert_eq!(event.email, "john.doe@acme.org");
        assert_eq!(event.email_domain, "acme.org");
        assert_eq!(event.name, "John Doe");
        assert_eq!(event.scope, "CLIENT");
        assert!(!event.is_anchor_user);
        // Verify tracing context was copied
        assert_eq!(event.execution_id(), ctx.execution_id);
        assert_eq!(event.correlation_id(), ctx.correlation_id);
    }

    #[test]
    fn test_user_created_builder_derives_name_from_email() {
        let ctx = ExecutionContext::create("admin-123");
        let event = UserCreated::builder()
            .from(&ctx)
            .principal_id("user-1")
            .email("john.doe@acme.org")
            .scope(UserScope::Anchor)
            // name not set - should derive from email
            .build();

        assert_eq!(event.name, "john.doe");
    }

    #[test]
    fn test_email_domain_extraction() {
        assert_eq!(extract_email_domain("user@example.com"), "example.com");
        assert_eq!(extract_email_domain("user@SUB.Example.COM"), "sub.example.com");
        assert_eq!(extract_email_domain("invalid-email"), "");
        assert_eq!(extract_email_domain(""), "");
    }

    #[test]
    fn test_user_deactivated_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = UserDeactivated::new(&ctx, "user-1", Some("Policy violation"));

        assert_eq!(event.event_type(), "platform:iam:user:deactivated");
        assert_eq!(event.reason, Some("Policy violation".to_string()));
    }
}
