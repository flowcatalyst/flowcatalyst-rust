//! The `GET /control/functions/desired-state` document (Java
//! `function/operations/DesiredState.java`, spec `function-api.md` §6.1). A
//! read, not a use case.
//!
//! **Deterministic bytes are the point.** A host polls every 15 s with
//! `If-None-Match`, and the `ETag` is the sha256 of the body, so one database
//! state must always serialise to the same bytes: `functions` is sorted by
//! `(address, version)`, `unload` likewise, `publicRoutes` by
//! `(hostname, pathPrefix)`; `config` and `secrets` are key-ordered maps; the
//! manifest is written in its own field order; and the document is serde's,
//! in field order, `null` members omitted (`Json.MAPPER`'s `NON_ABSENT`).
//! Nothing here depends on row or hash-map order. The ETag is opaque to the
//! hosts (they store and echo it), so it need not equal Java's; the golden
//! test (`tests/function_desired_state_golden_test.rs`) holds the document's
//! content to what Java's own `DesiredState` writes for the same rows.
//!
//! What a pool's document holds, per `ACTIVE` function:
//! - its `live` version, when that version's own manifest names the pool
//!   (`mode` warm when the manifest says so, else lazy);
//! - its newest `PUBLISHED` version, when newer than live (or there is no
//!   live) and in the pool: role `candidate`, always lazy;
//! - every version a named alias points at that is neither of those, when in
//!   the pool: role `alias`, always lazy.
//!
//! plus `unload`: every `(address, version)` a live host of the pool still
//! reports that the document no longer names; and `publicRoutes`: the routes
//! of every function whose live version is in the pool.
//!
//! A corrupt candidate or alias version is skipped with an ERROR (a host
//! never unloads anything for it). A corrupt **live** version fails the
//! whole build with `500 CORRUPT_ROW` when it could be in this pool: leaving
//! it out would make every host that has it loaded unload it, and a platform
//! fault must never unload a function (`function-host-reconciler.md` §1.2).
//!
//! The reads are batched: a fixed number of queries per build, however many
//! functions there are (Java reads settings and credentials per function).

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use indexmap::{IndexMap, IndexSet};
use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::entity::{Function, FunctionHost, FunctionStatus, FunctionVersion, SignerIdentity};
use super::host_repository::FunctionHostRepository;
use super::repository::FunctionRepository;
use super::route_repository::FunctionRouteRepository;
use super::settings_repository::FunctionSettingsRepository;
use super::version_repository::{CorruptVersion, FunctionVersionRepository};
use super::{java_is_blank, DnsLabel, EndpointAuth, FunctionOwner, JsonNode, Manifest, LIVE_ALIAS};
use crate::service_account::outbound_credentials::OutboundCredentialsResolver;
use crate::shared::error::PlatformError;

/// What one entry is to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Live,
    Candidate,
    Alias,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Live => "live",
            Role::Candidate => "candidate",
            Role::Alias => "alias",
        }
    }
}

/// One `functions` entry (Java `DesiredState.FunctionEntry`). `Debug` masks
/// the signing secret and the secret values, as Java's `toString`.
#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionEntry {
    pub address: String,
    pub function_id: String,
    pub version_id: String,
    pub version: i32,
    pub role: Role,
    /// `warm` or `lazy`: only a live entry is ever warm.
    pub mode: &'static str,
    pub digest: String,
    pub artifact_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_bundle: Option<String>,
    /// The stored, normalised manifest.
    pub manifest: JsonNode,
    /// `None` when the version was published with signatures off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<SignerIdentity>,
    /// Present only for a version with a `webhook` endpoint whose
    /// application has an active service account with a signing secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_signing_secret: Option<String>,
    /// Always carried, a platform-owned function's too.
    pub application_id: String,
    /// `None` for a platform-owned function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// The version's declared config keys that have a value.
    pub config: BTreeMap<String, String>,
    /// The version's declared secrets (and `db[].secretRef`s), decrypted.
    pub secrets: BTreeMap<String, String>,
    /// Every declared key with no value: config first, in manifest order,
    /// then secrets.
    pub missing_settings: Vec<String>,
    /// The named (non-`live`) aliases pointing at this version, sorted.
    pub aliases: Vec<String>,
}

impl fmt::Debug for FunctionEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FunctionEntry")
            .field("address", &self.address)
            .field("function_id", &self.function_id)
            .field("version_id", &self.version_id)
            .field("version", &self.version)
            .field("role", &self.role)
            .field("mode", &self.mode)
            .field("digest", &self.digest)
            .field("artifact_ref", &self.artifact_ref)
            .field(
                "signature_bundle",
                &self
                    .signature_bundle
                    .as_ref()
                    .map(|b| format!("{} chars", b.len())),
            )
            .field("signer", &self.signer)
            .field(
                "webhook_signing_secret",
                &self.webhook_signing_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("application_id", &self.application_id)
            .field("client_id", &self.client_id)
            .field("config", &self.config)
            .field("secrets", &self.secrets.keys().collect::<Vec<_>>())
            .field("missing_settings", &self.missing_settings)
            .field("aliases", &self.aliases)
            .finish()
    }
}

/// One `unload` entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct UnloadEntry {
    pub address: String,
    pub version: i32,
}

/// One `publicRoutes` entry: `{hostname, pathPrefix, address, aliasPrefixes}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicRouteEntry {
    pub hostname: String,
    pub path_prefix: String,
    pub address: String,
    pub alias_prefixes: Vec<String>,
}

/// The whole document (Java `DesiredState.Document`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub pool: String,
    pub functions: Vec<FunctionEntry>,
    pub unload: Vec<UnloadEntry>,
    pub public_routes: Vec<PublicRouteEntry>,
}

impl Document {
    /// The response body: serialised once, so the `ETag` hashes exactly
    /// the bytes a 200 returns.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a desired-state document always serialises")
    }
}

/// `"<sha256 hex of body>"`, quoted: a strong validator.
pub fn etag(body: &[u8]) -> String {
    format!("\"{}\"", hex::encode(Sha256::digest(body)))
}

/// Java `ETags.matches`: `If-None-Match` against this response's own strong
/// `ETag`. A weak validator (`W/"…"`) and a comma list are compared by
/// stripping `W/` and splitting on commas; `*` matches; an absent or blank
/// header never does.
pub fn etag_matches(header: Option<&str>, etag: &str) -> bool {
    let Some(header) = header else {
        return false;
    };
    let trimmed = header.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed == "*" {
        return true;
    }
    trimmed.split(',').any(|candidate| {
        let value = candidate.trim();
        let value = value.strip_prefix("W/").map(str::trim).unwrap_or(value);
        value == etag
    })
}

/// A corrupt live version that could be in the requested pool (Java
/// `CorruptFunctionVersionException`, the house `500 CORRUPT_ROW`).
pub fn corrupt_row(version_id: &str, cause: &str) -> PlatformError {
    PlatformError::Coded {
        status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        code: "CORRUPT_ROW".to_string(),
        message: format!("function version {version_id} has a corrupt row: {cause}"),
        details: Default::default(),
    }
}

/// Builds [`Document`]s. One per process, shared by the control routes.
pub struct DesiredStateBuilder {
    pub functions: Arc<FunctionRepository>,
    pub versions: Arc<FunctionVersionRepository>,
    pub hosts: Arc<FunctionHostRepository>,
    pub settings: Arc<FunctionSettingsRepository>,
    pub routes: Arc<FunctionRouteRepository>,
    /// The resolver the deliveries use, read fresh (uncached) here so a
    /// rotated signing secret reaches the next poll.
    pub credentials: Arc<OutboundCredentialsResolver>,
}

/// One entry before its settings and signing secret are resolved.
struct Selected<'a> {
    function: &'a Function,
    version: &'a FunctionVersion,
    role: Role,
}

impl DesiredStateBuilder {
    /// One consistent read of `pool`'s document. `now` drives the live
    /// window of the hosts whose reports feed `unload`.
    pub async fn build(
        &self,
        pool: &DnsLabel,
        now: DateTime<Utc>,
    ) -> Result<Document, PlatformError> {
        let active = self.functions.list_active().await?;
        let live_ids: Vec<String> = active
            .iter()
            .filter_map(|f| f.live_version_id().map(str::to_string))
            .collect();
        let active_ids: Vec<String> = active.iter().map(|f| f.id.clone()).collect();
        let (live_lookup, candidate_lookup) = tokio::try_join!(
            self.versions.find_batch_by_ids(&live_ids),
            self.versions.newest_published_by_functions(&active_ids),
        )?;

        // A named alias whose version is neither live nor the candidate: its
        // own `alias` entry (spec `function-zones-and-aliases.md` §5).
        let mut alias_only: IndexMap<&str, IndexSet<&str>> = IndexMap::new();
        for f in &active {
            let live_id = f.live_version_id();
            let candidate_id = candidate_lookup.versions.get(&f.id).map(|v| v.id.as_str());
            let mut ids = IndexSet::new();
            for a in &f.aliases {
                if a.alias == LIVE_ALIAS {
                    continue;
                }
                let id = Some(a.version_id.as_str());
                if id == live_id || id == candidate_id {
                    continue;
                }
                ids.insert(a.version_id.as_str());
            }
            if !ids.is_empty() {
                alias_only.insert(f.id.as_str(), ids);
            }
        }
        let alias_only_ids: Vec<String> = alias_only
            .values()
            .flatten()
            .map(|id| id.to_string())
            .collect::<IndexSet<_>>()
            .into_iter()
            .collect();
        let alias_lookup = self.versions.find_batch_by_ids(&alias_only_ids).await?;

        let others: Vec<&CorruptVersion> = candidate_lookup
            .corrupt
            .iter()
            .chain(alias_lookup.corrupt.iter())
            .collect();
        report_and_guard_corrupt(&live_lookup.corrupt, &others, pool)?;

        let mut selected: Vec<Selected<'_>> = Vec::new();
        let mut live_in_pool: Vec<&Function> = Vec::new();
        for f in &active {
            let live = f
                .live_version_id()
                .and_then(|id| live_lookup.versions.get(id));
            if let Some(live) = live {
                if &live.manifest.pool == pool {
                    selected.push(Selected {
                        function: f,
                        version: live,
                        role: Role::Live,
                    });
                    live_in_pool.push(f);
                }
            }
            if let Some(candidate) = candidate_lookup.versions.get(&f.id) {
                let newer = live.is_none_or(|l| candidate.version > l.version);
                if newer && &candidate.manifest.pool == pool {
                    selected.push(Selected {
                        function: f,
                        version: candidate,
                        role: Role::Candidate,
                    });
                }
            }
            for id in alias_only.get(f.id.as_str()).into_iter().flatten() {
                // Absent: corrupt (already logged) or gone.
                if let Some(aliased) = alias_lookup.versions.get(*id) {
                    if &aliased.manifest.pool == pool {
                        selected.push(Selected {
                            function: f,
                            version: aliased,
                            role: Role::Alias,
                        });
                    }
                }
            }
        }

        let mut entries = self.entries(&selected).await?;
        entries.sort_by(|a, b| (&a.address, a.version).cmp(&(&b.address, b.version)));

        let desired: HashSet<UnloadEntry> = entries
            .iter()
            .map(|e| UnloadEntry {
                address: e.address.clone(),
                version: e.version,
            })
            .collect();
        let live_hosts = self
            .hosts
            .list_live(pool.value(), now - FunctionHost::live_window())
            .await?;
        let mut unload: Vec<UnloadEntry> = live_hosts
            .iter()
            .flat_map(|h| h.loaded.iter())
            .map(|lv| UnloadEntry {
                address: lv.address.render(),
                version: lv.version,
            })
            .filter(|key| !desired.contains(key))
            .collect::<IndexSet<_>>()
            .into_iter()
            .collect();
        unload.sort();

        let public_routes = self.public_routes(&live_in_pool).await?;
        Ok(Document {
            pool: pool.value().to_string(),
            functions: entries,
            unload,
            public_routes,
        })
    }

    /// Resolves each selected version's settings and signing secret: three
    /// batched reads, whatever the number of entries.
    async fn entries(
        &self,
        selected: &[Selected<'_>],
    ) -> Result<Vec<FunctionEntry>, PlatformError> {
        let function_ids: Vec<String> = selected
            .iter()
            .map(|s| s.function.id.clone())
            .collect::<IndexSet<_>>()
            .into_iter()
            .collect();
        let signing_applications: Vec<String> = selected
            .iter()
            .filter(|s| has_webhook_endpoint(&s.version.manifest))
            .map(|s| s.function.application_id.clone())
            .collect::<IndexSet<_>>()
            .into_iter()
            .collect();
        let secret_requests: Vec<(&str, Vec<String>)> = selected
            .iter()
            .map(|s| {
                (
                    s.function.id.as_str(),
                    declared_secret_keys(&s.version.manifest),
                )
            })
            .collect();
        let (config_maps, secrets, credentials) = tokio::try_join!(
            self.settings.config_maps(&function_ids),
            self.settings.decrypt_secrets_each(&secret_requests),
            self.credentials
                .for_applications_fresh(&signing_applications),
        )?;
        let no_config = BTreeMap::new();
        Ok(selected
            .iter()
            .zip(secrets)
            .zip(secret_requests.iter())
            .map(|((s, secrets), (_, declared_secrets))| {
                let (f, v) = (s.function, s.version);
                let all_config = config_maps.get(&f.id).unwrap_or(&no_config);
                let config: BTreeMap<String, String> = v
                    .manifest
                    .config
                    .iter()
                    .filter_map(|k| all_config.get(k).map(|val| (k.clone(), val.clone())))
                    .collect();
                let missing_settings: Vec<String> = v
                    .manifest
                    .config
                    .iter()
                    .filter(|k| !config.contains_key(*k))
                    .chain(
                        declared_secrets
                            .iter()
                            .filter(|k| !secrets.contains_key(*k)),
                    )
                    .cloned()
                    .collect();
                let webhook_signing_secret = if has_webhook_endpoint(&v.manifest) {
                    credentials
                        .get(&f.application_id)
                        .and_then(|c| c.signing_secret.clone())
                        .filter(|secret| !java_is_blank(secret))
                } else {
                    None
                };
                let mut aliases: Vec<String> = f
                    .aliases
                    .iter()
                    .filter(|a| a.alias != LIVE_ALIAS && a.version_id == v.id)
                    .map(|a| a.alias.clone())
                    .collect();
                aliases.sort();
                FunctionEntry {
                    address: f.address.render(),
                    function_id: f.id.clone(),
                    version_id: v.id.clone(),
                    version: v.version,
                    role: s.role,
                    mode: if s.role == Role::Live && v.manifest.warm {
                        "warm"
                    } else {
                        "lazy"
                    },
                    digest: v.digest.value().to_string(),
                    artifact_ref: v.artifact_ref.clone(),
                    signature_bundle: v.signature_bundle.clone(),
                    manifest: v.manifest.to_json(),
                    signer: v.signer.clone(),
                    webhook_signing_secret,
                    application_id: f.application_id.clone(),
                    client_id: match &f.owner {
                        FunctionOwner::Platform => None,
                        FunctionOwner::Client(id) => Some(id.clone()),
                    },
                    config,
                    secrets,
                    missing_settings,
                    aliases,
                }
            })
            .collect())
    }

    /// One entry per route of every function whose live version is in the
    /// pool, sorted `(hostname, pathPrefix)`.
    async fn public_routes(
        &self,
        live_in_pool: &[&Function],
    ) -> Result<Vec<PublicRouteEntry>, PlatformError> {
        let ids: Vec<String> = live_in_pool.iter().map(|f| f.id.clone()).collect();
        let by_function = self.routes.list_by_functions(&ids).await?;
        let mut out: Vec<PublicRouteEntry> = live_in_pool
            .iter()
            .flat_map(|f| {
                by_function
                    .get(&f.id)
                    .into_iter()
                    .flatten()
                    .map(|r| PublicRouteEntry {
                        hostname: r.hostname.value().to_string(),
                        path_prefix: r.path_prefix.value().to_string(),
                        address: f.address.render(),
                        alias_prefixes: r.alias_prefixes.clone(),
                    })
            })
            .collect();
        out.sort_by(|a, b| (&a.hostname, &a.path_prefix).cmp(&(&b.hostname, &b.path_prefix)));
        Ok(out)
    }

    /// Whether `f`'s live version, or its newest published candidate, is
    /// `v` in `pool` (Java `DesiredState.serves`): exactly the selection
    /// [`Self::build`] makes, so the events route can never disagree with
    /// what a host's own document told it. A corrupt version is absent
    /// here, so a host is denied rather than confirmed.
    pub async fn serves(
        &self,
        f: &Function,
        v: &FunctionVersion,
        pool: &str,
    ) -> Result<bool, PlatformError> {
        if f.status != FunctionStatus::Active {
            return Ok(false);
        }
        let live_ids: Vec<String> = f
            .live_version_id()
            .map(str::to_string)
            .into_iter()
            .collect();
        let function_ids = vec![f.id.clone()];
        let (live_lookup, candidate_lookup) = tokio::try_join!(
            self.versions.find_batch_by_ids(&live_ids),
            self.versions.newest_published_by_functions(&function_ids),
        )?;
        let live = f
            .live_version_id()
            .and_then(|id| live_lookup.versions.get(id));
        if let Some(live) = live {
            if live.id == v.id && live.manifest.pool.value() == pool {
                return Ok(true);
            }
        }
        Ok(candidate_lookup.versions.get(&f.id).is_some_and(|c| {
            c.id == v.id
                && live.is_none_or(|l| c.version > l.version)
                && c.manifest.pool.value() == pool
        }))
    }
}

/// `manifest.secrets` then every `db[].secretRef`, each once, in that order
/// (Java's `LinkedHashSet`): a `db` connection's DSN is itself a secret.
fn declared_secret_keys(manifest: &Manifest) -> Vec<String> {
    manifest
        .secrets
        .iter()
        .cloned()
        .chain(manifest.db.iter().map(|d| d.secret_ref.clone()))
        .collect::<IndexSet<_>>()
        .into_iter()
        .collect()
}

/// Only a version with a `webhook` endpoint gets the signing secret: the
/// one auth mode the host verifies a signature for.
fn has_webhook_endpoint(manifest: &Manifest) -> bool {
    manifest
        .endpoints
        .iter()
        .any(|e| e.auth == EndpointAuth::Webhook)
}

/// One ERROR per distinct corrupt version the build touched (ids only,
/// never manifest content); then `500 CORRUPT_ROW` for a corrupt live
/// version whose peeked pool is this one or cannot be read at all. A corrupt
/// candidate or alias is safe to skip: it is never served, so nothing is
/// unloaded for it.
fn report_and_guard_corrupt(
    live: &[CorruptVersion],
    others: &[&CorruptVersion],
    pool: &DnsLabel,
) -> Result<(), PlatformError> {
    let mut distinct: IndexMap<&str, &CorruptVersion> = IndexMap::new();
    for c in live.iter().chain(others.iter().copied()) {
        distinct.entry(c.version_id.as_str()).or_insert(c);
    }
    for c in distinct.values() {
        tracing::error!(
            function_id = %c.function_id,
            version_id = %c.version_id,
            "fn_versions row has an unreadable manifest; excluded from the desired-state document"
        );
    }
    for c in live {
        if c.pool.as_ref().is_none_or(|p| p == pool) {
            return Err(corrupt_row(&c.version_id, &c.cause));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ETAG: &str = "\"abc123\"";

    /// Java `ETagsTest`, row for row.
    #[test]
    fn if_none_match_is_matched_as_javas_etags() {
        for (label, header, expected) in [
            ("exact match", "\"abc123\"", true),
            ("weak validator strips W/", "W/\"abc123\"", true),
            ("list, second entry matches", "\"xyz999\", \"abc123\"", true),
            ("list, weak entry matches", "\"xyz999\", W/\"abc123\"", true),
            ("star matches anything", "*", true),
            ("surrounding whitespace tolerated", "  \"abc123\"  ", true),
            ("no entry matches", "\"xyz999\"", false),
            ("similar but not equal (case)", "\"ABC123\"", false),
        ] {
            assert_eq!(etag_matches(Some(header), ETAG), expected, "{label}");
        }
        assert!(!etag_matches(None, ETAG), "an absent header never matches");
        assert!(
            !etag_matches(Some("   "), ETAG),
            "a blank header never matches"
        );
    }

    fn document() -> Document {
        Document {
            pool: "edge".into(),
            functions: vec![entry()],
            unload: vec![UnloadEntry {
                address: "a.b.old".into(),
                version: 2,
            }],
            public_routes: vec![PublicRouteEntry {
                hostname: "a.example.com".into(),
                path_prefix: "/".into(),
                address: "a.b.c".into(),
                alias_prefixes: vec!["qa".into()],
            }],
        }
    }

    /// The ETag is opaque to the hosts: what matters is that the same state
    /// always gives the same one and any change gives another.
    #[test]
    fn the_same_document_gives_the_same_etag_and_any_change_another() {
        let tag = |d: &Document| etag(&d.to_bytes());
        let base = tag(&document());
        assert_eq!(tag(&document()), base, "the same state, the same ETag");

        type Change = fn(&mut Document);
        let changes: &[(&str, Change)] = &[
            ("pool", |d| d.pool = "other".into()),
            ("address", |d| d.functions[0].address = "a.b.d".into()),
            ("functionId", |d| {
                d.functions[0].function_id = "fnc_2".into()
            }),
            ("versionId", |d| d.functions[0].version_id = "fnv_2".into()),
            ("version", |d| d.functions[0].version = 2),
            ("role", |d| d.functions[0].role = Role::Candidate),
            ("mode", |d| d.functions[0].mode = "warm"),
            ("digest", |d| d.functions[0].digest = "sha256:01".into()),
            ("artifactRef", |d| {
                d.functions[0].artifact_ref = "oci://y".into()
            }),
            ("signatureBundle", |d| {
                d.functions[0].signature_bundle = None
            }),
            ("manifest", |d| {
                d.functions[0].manifest = JsonNode::parse(r#"{"warm":true}"#).unwrap()
            }),
            ("signer", |d| {
                d.functions[0].signer = Some(SignerIdentity::new("https://issuer", "sub"))
            }),
            ("webhookSigningSecret", |d| {
                d.functions[0].webhook_signing_secret = Some("other".into())
            }),
            ("applicationId", |d| {
                d.functions[0].application_id = "app_2".into()
            }),
            ("clientId", |d| {
                d.functions[0].client_id = Some("clt_1".into())
            }),
            ("config", |d| {
                d.functions[0]
                    .config
                    .insert("PLAIN".into(), "changed".into());
            }),
            ("secrets", |d| {
                d.functions[0]
                    .secrets
                    .insert("API_KEY".into(), "rotated".into());
            }),
            ("missingSettings", |d| {
                d.functions[0].missing_settings.push("X".into())
            }),
            ("aliases", |d| d.functions[0].aliases.push("qa".into())),
            ("unload", |d| d.unload[0].version = 3),
            ("publicRoutes", |d| {
                d.public_routes[0].alias_prefixes.clear()
            }),
        ];
        for (field, change) in changes {
            let mut d = document();
            change(&mut d);
            assert_ne!(tag(&d), base, "{field}");
        }
    }

    #[test]
    fn the_etag_is_the_quoted_sha256_of_the_body() {
        assert_eq!(
            etag(b"{}"),
            "\"44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a\""
        );
    }

    fn entry() -> FunctionEntry {
        FunctionEntry {
            address: "a.b.c".into(),
            function_id: "fnc_1".into(),
            version_id: "fnv_1".into(),
            version: 1,
            role: Role::Live,
            mode: "lazy",
            digest: "sha256:00".into(),
            artifact_ref: "oci://x".into(),
            signature_bundle: Some("BUNDLE_MARKER".into()),
            manifest: JsonNode::object(),
            signer: None,
            webhook_signing_secret: Some("SIGNING_MARKER".into()),
            application_id: "app_1".into(),
            client_id: None,
            config: BTreeMap::from([("PLAIN".into(), "shown".into())]),
            secrets: BTreeMap::from([("API_KEY".into(), "SECRET_MARKER".into())]),
            missing_settings: vec![],
            aliases: vec![],
        }
    }

    #[test]
    fn debug_masks_the_signing_secret_and_secret_values() {
        let text = format!("{:?}", entry());
        assert!(!text.contains("SIGNING_MARKER"), "{text}");
        assert!(!text.contains("SECRET_MARKER"), "{text}");
        assert!(!text.contains("BUNDLE_MARKER"), "{text}");
        assert!(text.contains("API_KEY") && text.contains("shown"), "{text}");
    }

    #[test]
    fn absent_members_are_omitted_and_empty_ones_kept() {
        let mut e = entry();
        e.signature_bundle = None;
        e.webhook_signing_secret = None;
        e.secrets.clear();
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({
                "address": "a.b.c", "functionId": "fnc_1", "versionId": "fnv_1", "version": 1,
                "role": "live", "mode": "lazy", "digest": "sha256:00", "artifactRef": "oci://x",
                "manifest": {}, "applicationId": "app_1", "config": {"PLAIN": "shown"},
                "secrets": {}, "missingSettings": [], "aliases": []
            })
        );
    }
}
