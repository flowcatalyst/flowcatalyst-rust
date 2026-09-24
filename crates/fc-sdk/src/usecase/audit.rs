//! Audit rows never store passwords or secrets.
//!
//! Every audit log the SDK writes to the outbox — through the unit of work,
//! [`AuditLogPayload`](crate::outbox::AuditLogPayload) or
//! [`CreateAuditLogDto`](crate::outbox::CreateAuditLogDto) — has its command
//! document redacted first by the platform's one rule (owner spec
//! `docs/spec/audit-redaction.md` in the Java repo; implementation in
//! [`fc_common::audit_redaction`]): a key that, lower-cased with `_` and `-`
//! removed, ends with `password`, `passwordhash`, `secret`, `secretref`,
//! `passphrase` or `token`, or equals `apikey`, `privatekey`,
//! `authorization` or `cookie`, has its value replaced by `"***"` (null and
//! booleans kept); objects and arrays are walked.
//!
//! A command can also declare **masked fields** the name rule would keep, by
//! implementing [`AuditMasked`]. The unit of work accepts any `Serialize`
//! command, so to have those declarations applied, pass the command wrapped
//! in [`Audited`]:
//!
//! ```ignore
//! use fc_sdk::usecase::{AuditMasked, Audited};
//!
//! impl AuditMasked for SetApiKeyCommand {
//!     fn audit_masked_fields(&self) -> &'static [&'static str] {
//!         &["value"]
//!     }
//! }
//!
//! uow.commit(&config, event, &Audited(&command)).await
//! ```
//!
//! The audit row's `operation` is still the command's own type name.

use serde::{Serialize, Serializer};

pub use fc_common::audit_redaction::{
    is_secret_key, redact, redact_document, redacted_command_json, AuditMasked, MASK,
};

/// A command whose [`AuditMasked`] fields the audit row should mask.
///
/// Serialises as the command's JSON with those fields (and every
/// secret-named key) already redacted. The audit `operation` recorded for
/// `Audited(&cmd)` is `cmd`'s type name, not `Audited`.
pub struct Audited<'a, C: ?Sized>(pub &'a C);

impl<C: Serialize + AuditMasked + ?Sized> Serialize for Audited<'_, C> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        redacted_command_json(self.0)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

/// The audit `operation` for a command type: the last path segment of its
/// type name, looking through a generic wrapper such as [`Audited`] to the
/// command inside.
pub(crate) fn command_name<C: ?Sized>() -> String {
    let full = std::any::type_name::<C>().trim_end_matches('>');
    let innermost = full.rsplit('<').next().unwrap_or(full);
    innermost
        .rsplit("::")
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown")
        .trim_start_matches('&')
        .to_string()
}

/// The command document an audit row stores: serialised, then redacted by
/// the name rule. A command wrapped in [`Audited`] arrives with its declared
/// fields already masked; the rule is idempotent on those.
pub(crate) fn audit_operation_json<C: Serialize + ?Sized>(
    command: &C,
) -> Option<serde_json::Value> {
    serde_json::to_value(command)
        .ok()
        .map(|json| redact(&json, &[]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SetApiKeyCommand {
        property: &'static str,
        value: &'static str,
        client_secret: &'static str,
    }

    impl AuditMasked for SetApiKeyCommand {
        fn audit_masked_fields(&self) -> &'static [&'static str] {
            &["value"]
        }
    }

    const CMD: SetApiKeyCommand = SetApiKeyCommand {
        property: "stripe",
        value: "sk_live_123",
        client_secret: "cs",
    };

    #[test]
    fn audited_serialises_with_declared_and_named_fields_masked() {
        assert_eq!(
            serde_json::to_value(Audited(&CMD)).unwrap(),
            json!({"property": "stripe", "value": "***", "clientSecret": "***"})
        );
    }

    #[test]
    fn a_bare_command_gets_the_name_rule_only() {
        assert_eq!(
            audit_operation_json(&CMD).unwrap(),
            json!({"property": "stripe", "value": "sk_live_123", "clientSecret": "***"})
        );
    }

    #[test]
    fn command_name_looks_through_audited() {
        assert_eq!(command_name::<SetApiKeyCommand>(), "SetApiKeyCommand");
        assert_eq!(
            command_name::<Audited<'_, SetApiKeyCommand>>(),
            "SetApiKeyCommand"
        );
        assert_eq!(command_name::<&SetApiKeyCommand>(), "SetApiKeyCommand");
        assert_eq!(command_name::<serde_json::Value>(), "Value");
    }
}
