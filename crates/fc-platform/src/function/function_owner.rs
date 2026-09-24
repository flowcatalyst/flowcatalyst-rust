//! Java `function/FunctionOwner.java`.

use std::fmt;

use super::java_is_blank;

/// Who a function, domain or client policy belongs to: the platform itself
/// or one client. `fn_functions.client_id` and `fn_domains.client_id` are
/// NULL for the platform; `fn_client_policies.client_id` is a primary key,
/// so the platform's row uses the reserved key [`FunctionOwner::PLATFORM_KEY`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionOwner {
    Platform,
    Client(String),
}

/// A client id that was blank (but not absent). Java's
/// `IllegalArgumentException`: never coerced to [`FunctionOwner::Platform`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("clientId must not be blank")]
pub struct BlankClientId;

impl FunctionOwner {
    /// The `fn_client_policies.client_id` value for the platform. No TSID is
    /// ever spelled this way.
    pub const PLATFORM_KEY: &'static str = "PLATFORM";

    /// A specific client; a blank id is rejected.
    pub fn client(client_id: impl Into<String>) -> Result<FunctionOwner, BlankClientId> {
        let client_id = client_id.into();
        if java_is_blank(&client_id) {
            return Err(BlankClientId);
        }
        Ok(FunctionOwner::Client(client_id))
    }

    /// From a nullable `client_id` column: `None` is the platform.
    pub fn of_client_id(client_id: Option<&str>) -> Result<FunctionOwner, BlankClientId> {
        match client_id {
            None => Ok(FunctionOwner::Platform),
            Some(id) => Self::client(id),
        }
    }

    /// The nullable `client_id` column value.
    pub fn client_id_or_none(&self) -> Option<&str> {
        match self {
            FunctionOwner::Platform => None,
            FunctionOwner::Client(id) => Some(id),
        }
    }

    /// The `fn_client_policies.client_id` primary key, and the entity id an
    /// audit row names for a policy write.
    pub fn key(&self) -> &str {
        match self {
            FunctionOwner::Platform => Self::PLATFORM_KEY,
            FunctionOwner::Client(id) => id,
        }
    }

    /// The inverse of [`FunctionOwner::key`].
    pub fn from_key(key: &str) -> Result<FunctionOwner, BlankClientId> {
        if key == Self::PLATFORM_KEY {
            Ok(FunctionOwner::Platform)
        } else {
            Self::client(key)
        }
    }

    /// The wire spelling (the policy API's `{owner}` path segment): the
    /// literal `platform`, or the client id.
    pub fn to_wire(&self) -> &str {
        match self {
            FunctionOwner::Platform => "platform",
            FunctionOwner::Client(id) => id,
        }
    }

    /// The inverse of [`FunctionOwner::to_wire`].
    pub fn from_wire(wire: &str) -> Result<FunctionOwner, BlankClientId> {
        if wire == "platform" {
            Ok(FunctionOwner::Platform)
        } else {
            Self::client(wire)
        }
    }
}

impl fmt::Display for FunctionOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_wire())
    }
}

/// Java `FunctionOwnerTest`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn of_client_id_none_is_platform() {
        assert_eq!(
            FunctionOwner::of_client_id(None),
            Ok(FunctionOwner::Platform)
        );
    }

    #[test]
    fn of_client_id_non_blank_is_client() {
        assert_eq!(
            FunctionOwner::of_client_id(Some("clt_1")),
            Ok(FunctionOwner::Client("clt_1".into()))
        );
    }

    #[test]
    fn blank_is_rejected_rather_than_coerced_to_platform() {
        assert_eq!(FunctionOwner::of_client_id(Some("")), Err(BlankClientId));
        assert_eq!(FunctionOwner::of_client_id(Some("   ")), Err(BlankClientId));
        assert_eq!(FunctionOwner::client(""), Err(BlankClientId));
    }

    #[test]
    fn client_id_or_none_is_the_inverse_of_of_client_id() {
        assert_eq!(FunctionOwner::Platform.client_id_or_none(), None);
        let owner = FunctionOwner::Client("clt_2".into());
        assert_eq!(
            FunctionOwner::of_client_id(owner.client_id_or_none()),
            Ok(owner)
        );
    }

    #[test]
    fn key_and_wire_spellings() {
        assert_eq!(FunctionOwner::Platform.key(), "PLATFORM");
        assert_eq!(
            FunctionOwner::from_key("PLATFORM"),
            Ok(FunctionOwner::Platform)
        );
        assert_eq!(FunctionOwner::Platform.to_wire(), "platform");
        assert_eq!(
            FunctionOwner::from_wire("platform"),
            Ok(FunctionOwner::Platform)
        );
        let client = FunctionOwner::Client("clt_1".into());
        assert_eq!(client.key(), "clt_1");
        assert_eq!(client.to_wire(), "clt_1");
        assert_eq!(FunctionOwner::from_key("clt_1"), Ok(client.clone()));
        assert_eq!(FunctionOwner::from_wire("clt_1"), Ok(client));
        assert_eq!(FunctionOwner::from_wire(" "), Err(BlankClientId));
    }
}
