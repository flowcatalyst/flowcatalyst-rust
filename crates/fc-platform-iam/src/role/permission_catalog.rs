//! The persistent permission catalogue (`iam_permissions`): permission
//! definitions that exist independently of any role (Go `role.Permission` +
//! `PermissionRepo`). The domain entity only; SQL lives in
//! `permission_repository.rs`.

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::PermissionId;

/// A permission definition. `code` is the canonical four-segment string
/// `application:context:aggregate:action`; the segments are stored alongside
/// it (`subdomain` holds the application segment, as in Go).
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogPermission {
    pub id: PermissionId,
    pub code: String,
    pub subdomain: String,
    pub context: String,
    pub aggregate: String,
    pub action: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl CatalogPermission {
    /// A new definition for a four-segment `code`; `None` when the code is
    /// not four segments.
    pub fn new(code: &str, description: Option<String>) -> Option<Self> {
        let parts: Vec<&str> = code.split(':').collect();
        if parts.len() != 4 {
            return None;
        }
        let now = Utc::now();
        Some(Self {
            id: PermissionId::generate(),
            code: code.to_string(),
            subdomain: parts[0].to_string(),
            context: parts[1].to_string(),
            aggregate: parts[2].to_string(),
            action: parts[3].to_string(),
            description,
            created_at: now,
            updated_at: now,
        })
    }

    /// Go's `permissionFromRow` category: `subdomain:context:aggregate`.
    pub fn category(&self) -> String {
        format!("{}:{}:{}", self.subdomain, self.context, self.aggregate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_definition_needs_four_segments() {
        assert!(CatalogPermission::new("a:b:c", None).is_none());
        let p = CatalogPermission::new("shop:orders:order:ship", Some("Ship".into())).unwrap();
        assert_eq!(p.subdomain, "shop");
        assert_eq!(p.action, "ship");
        assert_eq!(p.category(), "shop:orders:order");
        assert!(p.id.as_str().starts_with("prm_"));
    }
}
