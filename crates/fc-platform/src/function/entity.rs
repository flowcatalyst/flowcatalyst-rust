//! The function registry's aggregates and read models (Java
//! `function/Function.java`, `FunctionVersion.java`, `FunctionHost.java`,
//! `ClientPolicy.java`, `FunctionDomain.java`, `FunctionRoute.java`,
//! `SecretValue.java`, and the per-key settings `FunctionSettingsRepository`
//! writes). Plain data and domain rules: no SQL, no driver types.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Duration, Utc};
use serde::{Serialize, Serializer};

use super::{
    java_is_blank, ClientCeilings, Digest, FunctionAddress, FunctionLimits, FunctionOwner,
    Hostname, Manifest, NonPositiveLimit, RoutePattern, Runtime, LIVE_ALIAS,
};
use crate::shared::tsid::{self, EntityType};
use crate::usecase::{HasId, UseCaseError};

// ── Function ────────────────────────────────────────────────────────────────

/// A function's lifecycle status (Java `FunctionStatus`), stored as its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionStatus {
    Active,
    Disabled,
}

crate::shared::enum_str::str_enum!(FunctionStatus, "function status", {
    Active => "ACTIVE",
    Disabled => "DISABLED",
});

/// A named pointer from a function to one of its versions (`fn_aliases`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionAlias {
    pub alias: String,
    pub version_id: String,
    pub updated_by: String,
    pub updated_at: DateTime<Utc>,
}

/// The function aggregate root (Java `Function`): one row per
/// `app.service.name` address plus its alias pointers. Nothing changes
/// `application_id`, `address`, `owner` or `runtime` after [`Function::create`];
/// the repository's upsert does not write them either.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    /// `fnc_…`
    pub id: String,
    pub application_id: String,
    pub address: FunctionAddress,
    pub owner: FunctionOwner,
    pub runtime: Runtime,
    /// `None` for absent or blank, as Java's constructor normalises it.
    pub description: Option<String>,
    pub status: FunctionStatus,
    pub aliases: Vec<FunctionAlias>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn normalise_description(description: Option<String>) -> Option<String> {
    description.filter(|d| !java_is_blank(d))
}

impl Function {
    /// A fresh, `ACTIVE` function with no aliases.
    pub fn create(
        application_id: impl Into<String>,
        address: FunctionAddress,
        owner: FunctionOwner,
        runtime: Runtime,
        description: Option<String>,
    ) -> Function {
        let now = Utc::now();
        Function {
            id: tsid::generate(EntityType::Function),
            application_id: application_id.into(),
            address,
            owner,
            runtime,
            description: normalise_description(description),
            status: FunctionStatus::Active,
            aliases: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// The only editable field besides status and aliases. A blank
    /// description clears it.
    pub fn describe(&mut self, description: String, now: DateTime<Utc>) {
        self.description = normalise_description(Some(description));
        self.updated_at = now;
    }

    /// `409 FUNCTION_ALREADY_DISABLED` when it already is.
    pub fn disable(&mut self, now: DateTime<Utc>) -> Result<(), UseCaseError> {
        if self.status == FunctionStatus::Disabled {
            return Err(UseCaseError::business_rule(
                "FUNCTION_ALREADY_DISABLED",
                "function is already disabled",
            ));
        }
        self.status = FunctionStatus::Disabled;
        self.updated_at = now;
        Ok(())
    }

    /// `409 FUNCTION_ALREADY_ACTIVE` when it already is.
    pub fn enable(&mut self, now: DateTime<Utc>) -> Result<(), UseCaseError> {
        if self.status == FunctionStatus::Active {
            return Err(UseCaseError::business_rule(
                "FUNCTION_ALREADY_ACTIVE",
                "function is already active",
            ));
        }
        self.status = FunctionStatus::Active;
        self.updated_at = now;
        Ok(())
    }

    /// The version `live` points at, if any.
    pub fn live_version_id(&self) -> Option<&str> {
        self.version_id_of(LIVE_ALIAS)
    }

    /// The version `alias` points at, if the function has that pointer.
    pub fn version_id_of(&self, alias: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|a| a.alias == alias)
            .map(|a| a.version_id.as_str())
    }

    /// Points `alias` at `version` (Java `Function.promote`); returns the
    /// version it pointed at before, `None` on a first promotion. Readiness
    /// (`VERSION_NOT_READY`) is the promote use case's own guard, not this
    /// one's. `ALIAS_UNCHANGED` compares against what this alias points at,
    /// never `live`'s.
    ///
    /// `400 ALIAS_INVALID`, `400 VERSION_NOT_OF_FUNCTION`, `409
    /// VERSION_RETIRED`, `409 FUNCTION_DISABLED`, `409 ALIAS_UNCHANGED`, in
    /// that order.
    pub fn promote(
        &mut self,
        alias: &str,
        version: &FunctionVersion,
        principal_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<String>, UseCaseError> {
        require_valid_alias_name(alias)?;
        if version.function_id != self.id {
            return Err(UseCaseError::validation(
                "VERSION_NOT_OF_FUNCTION",
                "version does not belong to this function",
            ));
        }
        if let VersionState::Retired(_) = version.state {
            return Err(UseCaseError::business_rule(
                "VERSION_RETIRED",
                "version is retired",
            ));
        }
        if self.status == FunctionStatus::Disabled {
            return Err(UseCaseError::business_rule(
                "FUNCTION_DISABLED",
                "function is disabled",
            ));
        }
        let previous = self.version_id_of(alias).map(str::to_string);
        if previous.as_deref() == Some(version.id.as_str()) {
            return Err(UseCaseError::business_rule(
                "ALIAS_UNCHANGED",
                "alias already points at this version",
            ));
        }
        let pointer = FunctionAlias {
            alias: alias.to_string(),
            version_id: version.id.clone(),
            updated_by: principal_id.to_string(),
            updated_at: now,
        };
        match self.aliases.iter_mut().find(|a| a.alias == alias) {
            Some(existing) => *existing = pointer,
            None => self.aliases.push(pointer),
        }
        self.updated_at = now;
        Ok(previous)
    }

    /// Deletes a named pointer (Java `Function.removeAlias`); returns the
    /// version it pointed at. `live` is never removable: promote another
    /// version instead (`409 ALIAS_PROTECTED`); an alias the function does
    /// not have is `404 Alias_NOT_FOUND`, never a no-op.
    pub fn remove_alias(
        &mut self,
        alias: &str,
        now: DateTime<Utc>,
    ) -> Result<String, UseCaseError> {
        if alias == LIVE_ALIAS {
            return Err(UseCaseError::business_rule(
                "ALIAS_PROTECTED",
                "promote another version; live cannot be removed",
            ));
        }
        let Some(at) = self.aliases.iter().position(|a| a.alias == alias) else {
            return Err(super::operations::access::resource_not_found(
                "Alias", alias,
            ));
        };
        let removed = self.aliases.remove(at);
        self.updated_at = now;
        Ok(removed.version_id)
    }

    pub fn is_live(&self, version_id: &str) -> bool {
        self.live_version_id() == Some(version_id)
    }
}

/// Every alias name, `live` included, is 1-63 characters of `a-z`, `0-9`
/// and `-`, not starting or ending with `-`: `fn_aliases`' own check
/// constraint (Java `Function.requireValidAliasName`, the rule's one home).
/// `400 ALIAS_INVALID` otherwise.
pub fn require_valid_alias_name(alias: &str) -> Result<(), UseCaseError> {
    let b = alias.as_bytes();
    let inner = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-';
    let edge = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    let valid = !b.is_empty()
        && b.len() <= 63
        && b.iter().all(inner)
        && edge(&b[0])
        && edge(&b[b.len() - 1]);
    if valid {
        Ok(())
    } else {
        Err(UseCaseError::validation(
            "ALIAS_INVALID",
            "alias must be 1-63 characters of a-z, 0-9 and '-', not starting or ending with '-'",
        ))
    }
}

impl HasId for Function {
    fn id(&self) -> &str {
        &self.id
    }
}

// ── Versions ────────────────────────────────────────────────────────────────

/// A version's lifecycle (Java `FunctionVersion.VersionState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionState {
    Published,
    Ready(DateTime<Utc>),
    Retired(DateTime<Utc>),
}

impl VersionState {
    /// The stored and wire name.
    pub fn name(&self) -> &'static str {
        match self {
            VersionState::Published => "PUBLISHED",
            VersionState::Ready(_) => "READY",
            VersionState::Retired(_) => "RETIRED",
        }
    }
}

/// The keyless signer a version's bundle was verified against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerIdentity {
    pub issuer: String,
    pub subject: String,
}

/// A published, immutable function version (Java `FunctionVersion`): its
/// artifact, digest, signer and manifest never change once published; only
/// its state moves (`PUBLISHED` to `READY` on a host's heartbeat, either to
/// `RETIRED`). Versions are retired, never deleted.
#[derive(Clone)]
pub struct FunctionVersion {
    /// `fnv_…`
    pub id: String,
    pub function_id: String,
    pub version: i32,
    pub artifact_ref: String,
    pub digest: Digest,
    pub signature_bundle: Option<String>,
    pub signature_bundle_ref: Option<String>,
    pub signer: Option<SignerIdentity>,
    pub manifest: Manifest,
    pub state: VersionState,
    pub published_by: String,
    pub published_at: DateTime<Utc>,
}

impl FunctionVersion {
    /// A fresh `PUBLISHED` version (Java `FunctionVersion.publish`).
    #[allow(clippy::too_many_arguments)]
    pub fn publish(
        function_id: &str,
        version: i32,
        artifact_ref: &str,
        digest: Digest,
        signature_bundle: Option<String>,
        signer: Option<SignerIdentity>,
        manifest: Manifest,
        published_by: &str,
        now: DateTime<Utc>,
    ) -> FunctionVersion {
        FunctionVersion {
            id: tsid::generate(EntityType::FunctionVersion),
            function_id: function_id.to_string(),
            version,
            artifact_ref: artifact_ref.to_string(),
            digest,
            signature_bundle,
            signature_bundle_ref: None,
            signer,
            manifest,
            state: VersionState::Published,
            published_by: published_by.to_string(),
            published_at: now,
        }
    }

    /// `PUBLISHED` or `READY` to `RETIRED(now)`; `409
    /// VERSION_ALREADY_RETIRED` when it already is.
    pub fn retire(&mut self, now: DateTime<Utc>) -> Result<(), UseCaseError> {
        if let VersionState::Retired(_) = self.state {
            return Err(UseCaseError::business_rule(
                "VERSION_ALREADY_RETIRED",
                "version is already retired",
            ));
        }
        self.state = VersionState::Retired(now);
        Ok(())
    }

    /// When the version became `READY`, if it is.
    pub fn ready_at(&self) -> Option<DateTime<Utc>> {
        match self.state {
            VersionState::Ready(at) => Some(at),
            _ => None,
        }
    }

    /// When the version was retired, if it was.
    pub fn retired_at(&self) -> Option<DateTime<Utc>> {
        match self.state {
            VersionState::Retired(at) => Some(at),
            _ => None,
        }
    }
}

impl HasId for FunctionVersion {
    fn id(&self) -> &str {
        &self.id
    }
}

/// Masks the signature bundle to its length, as Java's `toString` does.
impl fmt::Debug for FunctionVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FunctionVersion")
            .field("id", &self.id)
            .field("function_id", &self.function_id)
            .field("version", &self.version)
            .field("artifact_ref", &self.artifact_ref)
            .field("digest", &self.digest)
            .field(
                "signature_bundle",
                &self
                    .signature_bundle
                    .as_ref()
                    .map(|b| format!("{} chars", b.len())),
            )
            .field("signature_bundle_ref", &self.signature_bundle_ref)
            .field("signer", &self.signer)
            .field("state", &self.state)
            .field("published_by", &self.published_by)
            .field("published_at", &self.published_at)
            .finish_non_exhaustive()
    }
}

// ── Hosts (read here; written by the host control plane in P6) ──────────────

/// A host's own state (Java `FunctionHost.HostState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostState {
    Active,
    Draining,
}

crate::shared::enum_str::str_enum!(HostState, "function host state", {
    Active => "ACTIVE",
    Draining => "DRAINING",
});

/// One version on a host (Java `FunctionHost.LoadState`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    Registered,
    Loaded,
    Failed(String),
}

impl LoadState {
    pub fn name(&self) -> &'static str {
        match self {
            LoadState::Registered => "REGISTERED",
            LoadState::Loaded => "LOADED",
            LoadState::Failed(_) => "FAILED",
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            LoadState::Failed(error) => Some(error),
            _ => None,
        }
    }
}

/// One function version a host has fetched, verified or loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedVersion {
    pub address: FunctionAddress,
    pub version: i32,
    pub state: LoadState,
}

/// A running function host (Java `FunctionHost`), as its heartbeats left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionHost {
    pub id: String,
    pub pool: String,
    pub state: HostState,
    pub loaded: Vec<LoadedVersion>,
    pub started_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
}

impl FunctionHost {
    /// Three missed 15 s beats (Java `FunctionHost.LIVE_WINDOW`).
    pub fn live_window() -> Duration {
        Duration::seconds(45)
    }
}

// ── Client policies ─────────────────────────────────────────────────────────

/// One allowed keyless signer, scoped to the runtimes it may publish for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerRule {
    pub issuer: String,
    pub subject: String,
    /// A set: no duplicates, in [`Runtime::ALL`] order.
    pub runtimes: Vec<Runtime>,
}

impl SignerRule {
    pub fn new(
        issuer: impl Into<String>,
        subject: impl Into<String>,
        runtimes: impl IntoIterator<Item = Runtime>,
    ) -> SignerRule {
        let wanted: Vec<Runtime> = runtimes.into_iter().collect();
        SignerRule {
            issuer: issuer.into(),
            subject: subject.into(),
            runtimes: Runtime::ALL
                .iter()
                .copied()
                .filter(|r| wanted.contains(r))
                .collect(),
        }
    }
}

/// A client's (or the platform's) allowed signers and resource ceilings
/// (Java `ClientPolicy`). Natural key: the owner's
/// [`FunctionOwner::key`], which is also the audited entity id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientPolicy {
    pub owner: FunctionOwner,
    pub signers: Vec<SignerRule>,
    /// `None`: the platform default applies.
    pub max_duration_ms: Option<i32>,
    pub max_concurrency: Option<i32>,
    pub max_wasm_memory_mb: Option<i32>,
    pub max_db_pool_size: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClientPolicy {
    /// Whether some rule permits `signer` to publish for `runtime` (Java
    /// `ClientPolicy.permits`): an equal issuer, an equal subject and the
    /// runtime in that rule's set. No patterns, no case folding, no
    /// trimming; an empty signer list permits nothing.
    pub fn permits(&self, signer: &SignerIdentity, runtime: Runtime) -> bool {
        self.signers.iter().any(|rule| {
            rule.issuer == signer.issuer
                && rule.subject == signer.subject
                && rule.runtimes.contains(&runtime)
        })
    }

    /// Each ceiling resolved against the platform default: the column when
    /// set, else the default.
    pub fn ceilings(&self, defaults: &FunctionLimits) -> Result<ClientCeilings, NonPositiveLimit> {
        ClientCeilings::new(
            self.max_duration_ms.unwrap_or(defaults.max_duration_ms()),
            self.max_concurrency.unwrap_or(defaults.max_concurrency()),
            self.max_wasm_memory_mb.unwrap_or(defaults.wasm_memory_mb()),
            self.max_db_pool_size.unwrap_or(defaults.db_pool_size()),
        )
    }
}

impl HasId for ClientPolicy {
    fn id(&self) -> &str {
        self.owner.key()
    }
}

// ── Domains and routes ──────────────────────────────────────────────────────

/// A claimed zone (Java `FunctionDomain`): verified by being made, so usable
/// by its owner at once. It covers its own hostname and every hostname
/// under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDomain {
    /// `fnd_…`
    pub id: String,
    pub owner: FunctionOwner,
    pub hostname: Hostname,
    pub created_at: DateTime<Utc>,
}

impl FunctionDomain {
    pub fn claim(owner: FunctionOwner, hostname: Hostname, now: DateTime<Utc>) -> FunctionDomain {
        FunctionDomain {
            id: tsid::generate(EntityType::FunctionDomain),
            owner,
            hostname,
            created_at: now,
        }
    }
}

impl HasId for FunctionDomain {
    fn id(&self) -> &str {
        &self.id
    }
}

/// One public route of a function (Java `FunctionRoute`), materialised
/// wholesale from the live manifest's `public[]` at promote (P5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionRoute {
    /// `fnr_…`
    pub id: String,
    pub function_id: String,
    pub hostname: Hostname,
    pub path_prefix: RoutePattern,
    /// Opt-in alias prefixes; empty for an exact-hostname route.
    pub alias_prefixes: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl FunctionRoute {
    /// A fresh route row (Java `FunctionRoute.of`), materialised at promote.
    pub fn of(
        function_id: &str,
        hostname: Hostname,
        path_prefix: RoutePattern,
        alias_prefixes: Vec<String>,
        now: DateTime<Utc>,
    ) -> FunctionRoute {
        FunctionRoute {
            id: tsid::generate(EntityType::FunctionRoute),
            function_id: function_id.to_string(),
            hostname,
            path_prefix,
            alias_prefixes,
            created_at: now,
        }
    }
}

// ── Trigger objects (written at promote) ──────────────────────────────

/// What a `fn_trigger_objects` row links a function to (Java
/// `TriggerObjectKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriggerObjectKind {
    Pool,
    Subscription,
    ScheduledJob,
}

crate::shared::enum_str::str_enum!(TriggerObjectKind, "trigger object kind", {
    Pool => "POOL",
    Subscription => "SUBSCRIPTION",
    ScheduledJob => "SCHEDULED_JOB",
});

/// One `fn_trigger_objects` row (Java `TriggerObject`): an object a
/// function's live manifest created at promote, keyed by
/// `(function, kind, trigger key)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerObject {
    pub function_id: String,
    pub kind: TriggerObjectKind,
    pub object_id: String,
    pub trigger_key: String,
    pub created_at: DateTime<Utc>,
}

/// One `fn_trigger_objects` row, with whether the object it names still
/// exists in its own table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerObjectLink {
    pub kind: TriggerObjectKind,
    pub trigger_key: String,
    pub object_id: String,
    pub present: bool,
}

// ── Config and secrets ──────────────────────────────────────────────────────

/// A secret's plaintext, in flight only (Java `SecretValue`): between the
/// HTTP body and the repository's encryption call. Every way of formatting
/// it — `Debug`, `Display`, `Serialize` — writes `***`, so a command that
/// carries one can reach neither an audit row nor a log in the clear.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub const MASKED: &'static str = "***";

    pub fn new(value: impl Into<String>) -> SecretValue {
        SecretValue(value.into())
    }

    /// The plaintext. Only validation and the repository's encryption call
    /// read it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue[***]")
    }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(Self::MASKED)
    }
}

impl Serialize for SecretValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(Self::MASKED)
    }
}

/// A function's whole config map (`fn_config`), replaced wholesale by
/// `PUT …/config`. Keys are in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionConfig {
    pub function_id: String,
    pub values: BTreeMap<String, String>,
    pub updated_by: String,
    pub updated_at: DateTime<Utc>,
}

impl HasId for FunctionConfig {
    fn id(&self) -> &str {
        &self.function_id
    }
}

/// One secret of a function (`fn_secrets`). The repository encrypts
/// `value` on write; nothing ever reads a value back out through the API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSecret {
    pub function_id: String,
    pub key: String,
    pub value: SecretValue,
    pub updated_by: String,
    pub updated_at: DateTime<Utc>,
}

impl HasId for FunctionSecret {
    fn id(&self) -> &str {
        &self.function_id
    }
}

/// A secret's metadata: never its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretInfo {
    pub key: String,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
}

/// Java `FunctionTest` (the transitions this workstream uses).
#[cfg(test)]
mod tests {
    use super::*;

    fn function() -> Function {
        Function::create(
            "app_1",
            FunctionAddress::parse("billing.invoices.create").unwrap(),
            FunctionOwner::Platform,
            Runtime::Wasm,
            Some("  ".into()),
        )
    }

    #[test]
    fn create_is_active_with_no_aliases_and_a_blank_description_absent() {
        let f = function();
        assert!(f.id.starts_with("fnc_"));
        assert_eq!(f.status, FunctionStatus::Active);
        assert!(f.aliases.is_empty());
        assert_eq!(f.description, None);
        assert_eq!(f.live_version_id(), None);
    }

    #[test]
    fn disable_and_enable_refuse_a_no_op() {
        let mut f = function();
        let err = f.enable(Utc::now()).unwrap_err();
        assert_eq!(err.code(), "FUNCTION_ALREADY_ACTIVE");
        assert_eq!(err.http_status_code(), 409);
        f.disable(Utc::now()).unwrap();
        assert_eq!(f.status, FunctionStatus::Disabled);
        let err = f.disable(Utc::now()).unwrap_err();
        assert_eq!(err.code(), "FUNCTION_ALREADY_DISABLED");
        f.enable(Utc::now()).unwrap();
        assert_eq!(f.status, FunctionStatus::Active);
    }

    #[test]
    fn describe_blank_clears() {
        let mut f = function();
        f.describe("hello".into(), Utc::now());
        assert_eq!(f.description.as_deref(), Some("hello"));
        f.describe("".into(), Utc::now());
        assert_eq!(f.description, None);
    }

    #[test]
    fn live_is_the_live_alias() {
        let mut f = function();
        f.aliases.push(FunctionAlias {
            alias: "qa".into(),
            version_id: "fnv_2".into(),
            updated_by: "p".into(),
            updated_at: Utc::now(),
        });
        assert_eq!(f.live_version_id(), None);
        f.aliases.push(FunctionAlias {
            alias: "live".into(),
            version_id: "fnv_1".into(),
            updated_by: "p".into(),
            updated_at: Utc::now(),
        });
        assert_eq!(f.live_version_id(), Some("fnv_1"));
        assert!(f.is_live("fnv_1"));
        assert!(!f.is_live("fnv_2"));
    }

    #[test]
    fn secret_value_never_formats_its_plaintext() {
        let v = SecretValue::new("hunter2");
        assert_eq!(format!("{v:?}"), "SecretValue[***]");
        assert_eq!(v.to_string(), "***");
        assert_eq!(serde_json::to_value(&v).unwrap(), serde_json::json!("***"));
        assert_eq!(v.expose(), "hunter2");
    }

    #[test]
    fn policy_ceilings_fall_back_to_the_defaults() {
        let defaults = FunctionLimits::defaults();
        let p = ClientPolicy {
            owner: FunctionOwner::Platform,
            signers: vec![],
            max_duration_ms: Some(9000),
            max_concurrency: None,
            max_wasm_memory_mb: None,
            max_db_pool_size: Some(2),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let c = p.ceilings(&defaults).unwrap();
        assert_eq!(c.max_duration_ms(), 9000);
        assert_eq!(c.max_concurrency(), defaults.max_concurrency());
        assert_eq!(c.wasm_memory_mb(), defaults.wasm_memory_mb());
        assert_eq!(c.db_pool_size(), 2);
        assert_eq!(p.id(), "PLATFORM");
    }

    fn version() -> FunctionVersion {
        let defaults = FunctionLimits::defaults();
        let manifest = Manifest::parse_strict(
            Some(
                &crate::function::JsonNode::parse(r#"{"runtime":"wasm","entrypoint":"handle"}"#)
                    .unwrap(),
            ),
            Runtime::Wasm,
            &defaults,
            &ClientCeilings::of(&defaults),
        )
        .unwrap();
        FunctionVersion::publish(
            "fnc_1",
            1,
            "oci://r/a",
            Digest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap(),
            Some("{}".into()),
            None,
            manifest,
            "prn_1",
            Utc::now(),
        )
    }

    #[test]
    fn publish_is_published_and_retire_refuses_a_retired_version() {
        let mut v = version();
        assert!(v.id.starts_with("fnv_"));
        assert_eq!(v.state, VersionState::Published);
        assert_eq!((v.ready_at(), v.retired_at()), (None, None));
        assert_eq!(v.signature_bundle_ref, None);
        let now = Utc::now();
        v.retire(now).unwrap();
        assert_eq!(v.state, VersionState::Retired(now));
        assert_eq!(v.retired_at(), Some(now));
        let err = v.retire(Utc::now()).unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code(), err.message()),
            (409, "VERSION_ALREADY_RETIRED", "version is already retired")
        );
        // A READY version retires too.
        let mut ready = version();
        ready.state = VersionState::Ready(now);
        ready.retire(now).unwrap();
        assert_eq!(ready.retired_at(), Some(now));
    }

    /// Java `FunctionTest`'s promote and remove-alias rules, each code.
    #[test]
    fn promote_points_an_alias_and_refuses_in_javas_order() {
        let mut f = function();
        let mut v = version();
        v.function_id = f.id.clone();
        v.state = VersionState::Ready(Utc::now());
        let now = Utc::now();

        let err = f.promote("Live", &v, "prn_1", now).unwrap_err();
        assert_eq!((err.http_status_code(), err.code()), (400, "ALIAS_INVALID"));

        let mut foreign = v.clone();
        foreign.function_id = "fnc_other".into();
        let err = f.promote("live", &foreign, "prn_1", now).unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code(), err.message()),
            (
                400,
                "VERSION_NOT_OF_FUNCTION",
                "version does not belong to this function"
            )
        );

        assert_eq!(f.promote("live", &v, "prn_1", now).unwrap(), None);
        assert_eq!(f.live_version_id(), Some(v.id.as_str()));
        assert_eq!(f.aliases[0].updated_by, "prn_1");

        let err = f.promote("live", &v, "prn_2", now).unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code(), err.message()),
            (
                409,
                "ALIAS_UNCHANGED",
                "alias already points at this version"
            )
        );
        // Another alias may name the same version as live.
        assert_eq!(f.promote("qa", &v, "prn_1", now).unwrap(), None);

        let mut v2 = v.clone();
        v2.id = "fnv_2".into();
        assert_eq!(
            f.promote("live", &v2, "prn_2", now).unwrap(),
            Some(v.id.clone())
        );
        assert_eq!(f.aliases.len(), 2);
        assert_eq!(f.version_id_of("qa"), Some(v.id.as_str()));

        let mut retired = v2.clone();
        retired.id = "fnv_3".into();
        retired.state = VersionState::Retired(now);
        let err = f.promote("live", &retired, "prn_1", now).unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code(), err.message()),
            (409, "VERSION_RETIRED", "version is retired")
        );

        let mut v3 = v2.clone();
        v3.id = "fnv_4".into();
        f.disable(now).unwrap();
        let err = f.promote("live", &v3, "prn_1", now).unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code(), err.message()),
            (409, "FUNCTION_DISABLED", "function is disabled")
        );
    }

    #[test]
    fn remove_alias_protects_live_and_refuses_an_unknown_alias() {
        let mut f = function();
        let mut v = version();
        v.function_id = f.id.clone();
        v.state = VersionState::Ready(Utc::now());
        f.promote("live", &v, "prn_1", Utc::now()).unwrap();
        f.promote("qa", &v, "prn_1", Utc::now()).unwrap();

        let err = f.remove_alias("live", Utc::now()).unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code(), err.message()),
            (
                409,
                "ALIAS_PROTECTED",
                "promote another version; live cannot be removed"
            )
        );
        let err = f.remove_alias("nope", Utc::now()).unwrap_err();
        assert_eq!(
            (err.http_status_code(), err.code(), err.message()),
            (404, "Alias_NOT_FOUND", "Alias not found: nope")
        );
        assert_eq!(f.remove_alias("qa", Utc::now()).unwrap(), v.id);
        assert_eq!(f.version_id_of("qa"), None);
        assert_eq!(f.live_version_id(), Some(v.id.as_str()));
    }

    /// `fn_aliases`' check constraint: 1-63 of `a-z0-9-`, no `-` at an end.
    #[test]
    fn alias_names_follow_the_check_constraint() {
        for ok in ["live", "qa", "a", "0", "a-b", "v2", &"a".repeat(63)] {
            assert!(require_valid_alias_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-a",
            "a-",
            "A",
            "a_b",
            "a.b",
            "é",
            &"a".repeat(64),
            " live",
        ] {
            let err = require_valid_alias_name(bad).unwrap_err();
            assert_eq!(err.code(), "ALIAS_INVALID", "{bad:?}");
            assert_eq!(
                err.message(),
                "alias must be 1-63 characters of a-z, 0-9 and '-', not starting or ending with '-'"
            );
        }
    }

    #[test]
    fn a_version_debug_masks_the_bundle_to_its_length() {
        let text = format!("{:?}", version());
        assert!(text.contains("\"2 chars\""), "{text}");
        assert!(!text.contains("{}"), "{text}");
    }

    #[test]
    fn permits_matches_issuer_subject_and_runtime_exactly() {
        let signer = SignerIdentity {
            issuer: "https://issuer".into(),
            subject: "repo:acme/fn".into(),
        };
        let policy = |signers: Vec<SignerRule>| ClientPolicy {
            owner: FunctionOwner::Platform,
            signers,
            max_duration_ms: None,
            max_concurrency: None,
            max_wasm_memory_mb: None,
            max_db_pool_size: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert!(!policy(vec![]).permits(&signer, Runtime::Wasm));
        let exact = policy(vec![SignerRule::new(
            "https://issuer",
            "repo:acme/fn",
            [Runtime::Wasm],
        )]);
        assert!(exact.permits(&signer, Runtime::Wasm));
        assert!(
            !exact.permits(&signer, Runtime::Jvm),
            "runtime must be in the rule"
        );
        for (issuer, subject) in [
            ("https://issuer/", "repo:acme/fn"),
            ("https://ISSUER", "repo:acme/fn"),
            ("https://issuer", "repo:acme/fn "),
            ("https://issuer", "repo:acme/*"),
        ] {
            let p = policy(vec![SignerRule::new(issuer, subject, [Runtime::Wasm])]);
            assert!(!p.permits(&signer, Runtime::Wasm), "{issuer} {subject}");
        }
    }

    #[test]
    fn signer_runtimes_are_a_set_in_declaration_order() {
        let rule = SignerRule::new("i", "s", [Runtime::Wasm, Runtime::Jvm, Runtime::Wasm]);
        assert_eq!(rule.runtimes, vec![Runtime::Jvm, Runtime::Wasm]);
    }
}
