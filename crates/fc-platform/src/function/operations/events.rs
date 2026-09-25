//! The function registry's domain events (Java
//! `function/operations/FunctionEvents.java`).
//!
//! Source `platform:function`. A function's events have subject
//! `platform.function.{functionId}` and message group
//! `platform:function:{functionId}`. A policy has no function: its event's
//! subject is `platform.function-policy.{owner key}` and its group
//! `platform:function-policy:{owner key}`. A domain's are
//! `platform.function-domain.{domainId}` and `platform:function-domain:{domainId}`.
//!
//! The persisted `data` is exactly Java's `data()` record: the payload
//! fields, camelCase, absent fields omitted. The envelope lives in the
//! `msg_events` columns, not in `data`, as in Java.
//!
//! No event carries a secret value, a config value or a signer list: keys
//! and counts only.

use serde::Serialize;

use crate::function::entity::{ClientPolicy, Function, FunctionDomain, FunctionVersion};
use crate::impl_domain_event;
use crate::usecase::{EventMetadata, ExecutionContext};

pub const SOURCE: &str = "platform:function";
const SPEC_VERSION: &str = "1.0";

pub const CREATED: &str = "platform:function:function:created";
pub const UPDATED: &str = "platform:function:function:updated";
pub const DELETED: &str = "platform:function:function:deleted";
pub const VERSION_PUBLISHED: &str = "platform:function:version:published";
pub const VERSION_READY: &str = "platform:function:version:ready";
pub const VERSION_RETIRED: &str = "platform:function:version:retired";
pub const ALIAS_CHANGED: &str = "platform:function:alias:changed";
pub const ALIAS_REMOVED: &str = "platform:function:alias:removed";
pub const POLICY_UPDATED: &str = "platform:function:policy:updated";
pub const CONFIG_UPDATED: &str = "platform:function:config:updated";
pub const SECRET_SET: &str = "platform:function:secret:set";
pub const SECRET_DELETED: &str = "platform:function:secret:deleted";
pub const DOMAIN_CLAIMED: &str = "platform:function:domain:claimed";
pub const DOMAIN_RELEASED: &str = "platform:function:domain:released";

fn metadata(ctx: &ExecutionContext, event_type: &str, aggregate: &str, id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.{aggregate}.{id}"),
        format!("platform:{aggregate}:{id}"),
    )
}

fn function_metadata(ctx: &ExecutionContext, event_type: &str, f: &Function) -> EventMetadata {
    metadata(ctx, event_type, "function", &f.id)
}

/// `{functionId, address, applicationId, clientId?, runtime}`; `runtime` is
/// the stored name (`JVM`, `WASM`), as Java's `runtime().name()`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
    pub application_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub runtime: String,
}
impl_domain_event!(FunctionCreated);

impl FunctionCreated {
    pub fn new(ctx: &ExecutionContext, f: &Function) -> Self {
        Self {
            metadata: function_metadata(ctx, CREATED, f),
            function_id: f.id.clone(),
            address: f.address.render(),
            application_id: f.application_id.clone(),
            client_id: f.owner.client_id_or_none().map(str::to_string),
            runtime: f.runtime.as_str().to_string(),
        }
    }
}

/// `{functionId, address, description?, status}`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
}
impl_domain_event!(FunctionUpdated);

impl FunctionUpdated {
    pub fn new(ctx: &ExecutionContext, f: &Function) -> Self {
        Self {
            metadata: function_metadata(ctx, UPDATED, f),
            function_id: f.id.clone(),
            address: f.address.render(),
            description: f.description.clone(),
            status: f.status.as_str().to_string(),
        }
    }
}

/// `{functionId, address}`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
}
impl_domain_event!(FunctionDeleted);

impl FunctionDeleted {
    pub fn new(ctx: &ExecutionContext, f: &Function) -> Self {
        Self {
            metadata: function_metadata(ctx, DELETED, f),
            function_id: f.id.clone(),
            address: f.address.render(),
        }
    }
}

/// `{functionId, address, alias, versionId, version, previousVersionId?}`:
/// `previousVersionId` is absent on a first promotion.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasChanged {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
    pub alias: String,
    pub version_id: String,
    pub version: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_version_id: Option<String>,
}
impl_domain_event!(AliasChanged);

impl AliasChanged {
    pub fn new(
        ctx: &ExecutionContext,
        f: &Function,
        alias: &str,
        v: &FunctionVersion,
        previous_version_id: Option<String>,
    ) -> Self {
        Self {
            metadata: function_metadata(ctx, ALIAS_CHANGED, f),
            function_id: f.id.clone(),
            address: f.address.render(),
            alias: alias.to_string(),
            version_id: v.id.clone(),
            version: v.version,
            previous_version_id,
        }
    }
}

/// `{functionId, address, alias, versionId, version}`: the version the
/// removed pointer named.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasRemoved {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
    pub alias: String,
    pub version_id: String,
    pub version: i32,
}
impl_domain_event!(AliasRemoved);

impl AliasRemoved {
    pub fn new(ctx: &ExecutionContext, f: &Function, alias: &str, v: &FunctionVersion) -> Self {
        Self {
            metadata: function_metadata(ctx, ALIAS_REMOVED, f),
            function_id: f.id.clone(),
            address: f.address.render(),
            alias: alias.to_string(),
            version_id: v.id.clone(),
            version: v.version,
        }
    }
}

/// `{functionId, address, versionId, version, digest, pool, signerIssuer?,
/// signerSubject?}`: the signer is absent when signatures are off.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionPublished {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
    pub version_id: String,
    pub version: i32,
    pub digest: String,
    pub pool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_subject: Option<String>,
}
impl_domain_event!(VersionPublished);

impl VersionPublished {
    pub fn new(ctx: &ExecutionContext, f: &Function, v: &FunctionVersion) -> Self {
        Self {
            metadata: function_metadata(ctx, VERSION_PUBLISHED, f),
            function_id: f.id.clone(),
            address: f.address.render(),
            version_id: v.id.clone(),
            version: v.version,
            digest: v.digest.value().to_string(),
            pool: v.manifest.pool.value().to_string(),
            signer_issuer: v.signer.as_ref().map(|s| s.issuer.clone()),
            signer_subject: v.signer.as_ref().map(|s| s.subject.clone()),
        }
    }
}

/// `{functionId, address, versionId, version, hostId}`: a host's heartbeat
/// marked the version `READY` (Java `FunctionEvents.VersionReady`), grouped
/// with the function's own events.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionReady {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
    pub version_id: String,
    pub version: i32,
    pub host_id: String,
}
impl_domain_event!(VersionReady);

impl VersionReady {
    pub fn new(ctx: &ExecutionContext, f: &Function, v: &FunctionVersion, host_id: &str) -> Self {
        Self {
            metadata: function_metadata(ctx, VERSION_READY, f),
            function_id: f.id.clone(),
            address: f.address.render(),
            version_id: v.id.clone(),
            version: v.version,
            host_id: host_id.to_string(),
        }
    }
}

/// `{functionId, address, versionId, version}`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionRetired {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
    pub version_id: String,
    pub version: i32,
}
impl_domain_event!(VersionRetired);

impl VersionRetired {
    pub fn new(ctx: &ExecutionContext, f: &Function, v: &FunctionVersion) -> Self {
        Self {
            metadata: function_metadata(ctx, VERSION_RETIRED, f),
            function_id: f.id.clone(),
            address: f.address.render(),
            version_id: v.id.clone(),
            version: v.version,
        }
    }
}

/// `{functionId, address, keys}`: the keys of the new config map, never
/// its values.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
    pub keys: Vec<String>,
}
impl_domain_event!(ConfigUpdated);

impl ConfigUpdated {
    pub fn new(ctx: &ExecutionContext, f: &Function, keys: Vec<String>) -> Self {
        Self {
            metadata: function_metadata(ctx, CONFIG_UPDATED, f),
            function_id: f.id.clone(),
            address: f.address.render(),
            keys,
        }
    }
}

/// `{functionId, address, key}`: the key only, never the value.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretSet {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
    pub key: String,
}
impl_domain_event!(SecretSet);

impl SecretSet {
    pub fn new(ctx: &ExecutionContext, f: &Function, key: &str) -> Self {
        Self {
            metadata: function_metadata(ctx, SECRET_SET, f),
            function_id: f.id.clone(),
            address: f.address.render(),
            key: key.to_string(),
        }
    }
}

/// `{functionId, address, key}`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub function_id: String,
    pub address: String,
    pub key: String,
}
impl_domain_event!(SecretDeleted);

impl SecretDeleted {
    pub fn new(ctx: &ExecutionContext, f: &Function, key: &str) -> Self {
        Self {
            metadata: function_metadata(ctx, SECRET_DELETED, f),
            function_id: f.id.clone(),
            address: f.address.render(),
            key: key.to_string(),
        }
    }
}

/// `{owner, signerCount}`: the owner's wire spelling (`platform` or the
/// client id) and how many signers, never the signers themselves.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub owner: String,
    pub signer_count: usize,
}
impl_domain_event!(PolicyUpdated);

impl PolicyUpdated {
    pub fn new(ctx: &ExecutionContext, p: &ClientPolicy) -> Self {
        Self {
            metadata: metadata(ctx, POLICY_UPDATED, "function-policy", p.owner.key()),
            owner: p.owner.to_wire().to_string(),
            signer_count: p.signers.len(),
        }
    }
}

/// `{domainId, hostname, owner}`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainClaimed {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub domain_id: String,
    pub hostname: String,
    pub owner: String,
}
impl_domain_event!(DomainClaimed);

impl DomainClaimed {
    pub fn new(ctx: &ExecutionContext, d: &FunctionDomain) -> Self {
        Self {
            metadata: metadata(ctx, DOMAIN_CLAIMED, "function-domain", &d.id),
            domain_id: d.id.clone(),
            hostname: d.hostname.value().to_string(),
            owner: d.owner.to_wire().to_string(),
        }
    }
}

/// `{domainId, hostname, owner}`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainReleased {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub domain_id: String,
    pub hostname: String,
    pub owner: String,
}
impl_domain_event!(DomainReleased);

impl DomainReleased {
    pub fn new(ctx: &ExecutionContext, d: &FunctionDomain) -> Self {
        Self {
            metadata: metadata(ctx, DOMAIN_RELEASED, "function-domain", &d.id),
            domain_id: d.id.clone(),
            hostname: d.hostname.value().to_string(),
            owner: d.owner.to_wire().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::function::entity::SignerRule;
    use crate::function::{FunctionAddress, FunctionOwner, Hostname, Runtime};
    use crate::usecase::DomainEvent;
    use chrono::Utc;
    use serde_json::json;

    fn ctx() -> ExecutionContext {
        ExecutionContext::create("prn_1")
    }

    fn function(owner: FunctionOwner) -> Function {
        let mut f = Function::create(
            "app_1",
            FunctionAddress::parse("billing.invoices.create").unwrap(),
            owner,
            Runtime::Wasm,
            None,
        );
        f.id = "fnc_1".into();
        f
    }

    fn assert_envelope(meta: &EventMetadata, event_type: &str, subject: &str, group: &str) {
        assert_eq!(meta.event_type, event_type);
        assert_eq!(meta.source, "platform:function");
        assert_eq!(meta.spec_version, "1.0");
        assert_eq!(meta.subject, subject);
        assert_eq!(meta.message_group, group);
        assert_eq!(meta.principal_id, "prn_1");
    }

    #[test]
    fn function_events_are_grouped_by_the_function() {
        let f = function(FunctionOwner::Platform);
        let created = FunctionCreated::new(&ctx(), &f);
        assert_envelope(
            created.metadata(),
            "platform:function:function:created",
            "platform.function.fnc_1",
            "platform:function:fnc_1",
        );
        // data is Java's Data record: no envelope, clientId absent for the
        // platform, runtime by its stored name.
        assert_eq!(
            serde_json::to_value(&created).unwrap(),
            json!({"functionId": "fnc_1", "address": "billing.invoices.create",
                   "applicationId": "app_1", "runtime": "WASM"})
        );
        let client = function(FunctionOwner::Client("clt_1".into()));
        assert_eq!(
            serde_json::to_value(FunctionCreated::new(&ctx(), &client)).unwrap()["clientId"],
            "clt_1"
        );

        let updated = FunctionUpdated::new(&ctx(), &f);
        assert_envelope(
            updated.metadata(),
            "platform:function:function:updated",
            "platform.function.fnc_1",
            "platform:function:fnc_1",
        );
        assert_eq!(
            serde_json::to_value(&updated).unwrap(),
            json!({"functionId": "fnc_1", "address": "billing.invoices.create", "status": "ACTIVE"})
        );

        let deleted = FunctionDeleted::new(&ctx(), &f);
        assert_eq!(
            deleted.metadata().event_type,
            "platform:function:function:deleted"
        );
        assert_eq!(
            serde_json::to_value(&deleted).unwrap(),
            json!({"functionId": "fnc_1", "address": "billing.invoices.create"})
        );

        let config = ConfigUpdated::new(&ctx(), &f, vec!["A".into(), "B".into()]);
        assert_eq!(
            config.metadata().event_type,
            "platform:function:config:updated"
        );
        assert_eq!(config.metadata().subject, "platform.function.fnc_1");
        assert_eq!(
            serde_json::to_value(&config).unwrap()["keys"],
            json!(["A", "B"])
        );

        let set = SecretSet::new(&ctx(), &f, "API_KEY");
        assert_eq!(set.metadata().event_type, "platform:function:secret:set");
        assert_eq!(
            serde_json::to_value(&set).unwrap(),
            json!({"functionId": "fnc_1", "address": "billing.invoices.create", "key": "API_KEY"})
        );
        let deleted = SecretDeleted::new(&ctx(), &f, "API_KEY");
        assert_eq!(
            deleted.metadata().event_type,
            "platform:function:secret:deleted"
        );
        assert_eq!(deleted.metadata().message_group, "platform:function:fnc_1");
    }

    #[test]
    fn a_policy_event_is_grouped_by_its_owner_key_and_counts_signers() {
        let p = ClientPolicy {
            owner: FunctionOwner::Platform,
            signers: vec![
                SignerRule::new("i", "a", [Runtime::Jvm]),
                SignerRule::new("i", "b", [Runtime::Wasm]),
            ],
            max_duration_ms: None,
            max_concurrency: None,
            max_wasm_memory_mb: None,
            max_db_pool_size: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let event = PolicyUpdated::new(&ctx(), &p);
        assert_envelope(
            event.metadata(),
            "platform:function:policy:updated",
            "platform.function-policy.PLATFORM",
            "platform:function-policy:PLATFORM",
        );
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"owner": "platform", "signerCount": 2})
        );
    }

    #[test]
    fn domain_events_are_grouped_by_the_domain() {
        let mut d = FunctionDomain::claim(
            FunctionOwner::Client("clt_1".into()),
            Hostname::parse("acme.com").unwrap(),
            Utc::now(),
        );
        d.id = "fnd_1".into();
        let claimed = DomainClaimed::new(&ctx(), &d);
        assert_envelope(
            claimed.metadata(),
            "platform:function:domain:claimed",
            "platform.function-domain.fnd_1",
            "platform:function-domain:fnd_1",
        );
        assert_eq!(
            serde_json::to_value(&claimed).unwrap(),
            json!({"domainId": "fnd_1", "hostname": "acme.com", "owner": "clt_1"})
        );
        let released = DomainReleased::new(&ctx(), &d);
        assert_envelope(
            released.metadata(),
            "platform:function:domain:released",
            "platform.function-domain.fnd_1",
            "platform:function-domain:fnd_1",
        );
    }
}
