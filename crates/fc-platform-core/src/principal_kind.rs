//! The kinds of principal an [`AuthContext`](crate::shared::authorization_service::AuthContext)
//! carries: its type (user or service account) and its client tier. They
//! are here, beside the authorization context, rather than with the
//! `Principal` aggregate (fc-platform-iam), which re-exports them.

use serde::{Deserialize, Serialize};

/// Principal type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum PrincipalType {
    /// Human user
    #[default]
    User,
    /// Machine service account
    Service,
}

crate::shared::enum_str::str_enum!(PrincipalType, "principal type", {
    User => "USER",
    Service => "SERVICE",
});

/// User scope determines client access level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum UserScope {
    /// Platform admin - access to all clients
    Anchor,
    /// Partner user - access to multiple assigned clients
    Partner,
    /// Client user - access to single home client
    #[default]
    Client,
}

impl UserScope {
    /// Check if this scope has access to all clients
    pub fn is_anchor(&self) -> bool {
        matches!(self, Self::Anchor)
    }

    /// Check if this scope can access a specific client
    pub fn can_access_client(
        &self,
        client_id: &str,
        home_client_id: Option<&str>,
        assigned_clients: &[String],
    ) -> bool {
        match self {
            Self::Anchor => true,
            Self::Partner => assigned_clients.iter().any(|c| c == client_id),
            Self::Client => home_client_id == Some(client_id),
        }
    }
}

crate::shared::enum_str::str_enum!(UserScope, "user scope", {
    Anchor => "ANCHOR",
    Partner => "PARTNER",
    Client => "CLIENT",
});
