//! The host's reading of `GET /control/functions/desired-state` (Java
//! `fnhost/reconcile/DesiredDocument.java`), field for field and just as
//! lenient: unknown keys are ignored, and one unreadable entry never takes
//! the rest of the document down. An entry that cannot be read but whose
//! address and version can is kept as [`UnreadableEntry`]: the reconciler
//! reports it `FAILED` and never unloads its address because of it.

use std::collections::BTreeMap;
use std::fmt;

use fc_function_abi::FunctionAddress;
use fc_function_model::{JsonNode, Manifest};
use serde_json::Value;

use crate::digest::{Digest, SignerIdentity};
use crate::java;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Live,
    Candidate,
    Alias,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Warm,
    Lazy,
}

/// An entry's stored manifest, read as Java's `DesiredDocument` reads it:
/// with [`Manifest::read_stored`], the platform's own reader (tolerant of
/// unknown keys and malformed entries, failing only when `runtime` or
/// `entrypoint` is unreadable). A missing `manifest` reads as Jackson's
/// missing node: not an object.
fn read_manifest(node: Option<&Value>) -> Result<Manifest, String> {
    let root = node.map_or(JsonNode::Null, JsonNode::from);
    Manifest::read_stored(&root).map_err(|e| e.to_string())
}

/// One readable entry of `functions`.
#[derive(Clone, PartialEq)]
pub struct Entry {
    pub address: FunctionAddress,
    pub function_id: String,
    pub version_id: String,
    pub version: i32,
    pub role: Role,
    pub mode: Mode,
    pub digest: Digest,
    pub artifact_ref: String,
    pub signature_bundle: Option<String>,
    /// `None` when the version was published with signatures off.
    pub signer: Option<SignerIdentity>,
    pub manifest: Manifest,
    pub webhook_signing_secret: Option<String>,
    pub application_id: Option<String>,
    pub client_id: Option<String>,
    pub config: BTreeMap<String, String>,
    pub secrets: BTreeMap<String, String>,
    pub missing_settings: Vec<String>,
    pub aliases: Vec<String>,
}

/// Masks the signing secret and secret values, as Java's `toString`.
impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("address", &self.address.render())
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
            .field("runtime", &self.manifest.runtime.wire_value())
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

/// One entry of `unload`: close and drop this exact (address, version).
/// Parsed for completeness; the reconciler derives what to unload itself
/// and never acts on it (owner decision 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnloadRef {
    pub address: FunctionAddress,
    pub version: i32,
}

/// One entry of `publicRoutes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicRouteRef {
    pub hostname: String,
    pub path_prefix: String,
    pub address: FunctionAddress,
    pub alias_prefixes: Vec<String>,
}

/// An entry the host could not read, but whose address and version it could.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadableEntry {
    pub address: FunctionAddress,
    pub version: i32,
    /// `UNREADABLE:<why>`, reported verbatim as the heartbeat error.
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DesiredDocument {
    pub functions: Vec<Entry>,
    pub unload: Vec<UnloadRef>,
    pub unreadable: Vec<UnreadableEntry>,
    pub public_routes: Vec<PublicRouteRef>,
}

impl DesiredDocument {
    /// Parses the raw body. Only a body that is not a JSON object at all is
    /// an error (the control-plane client maps that to UNAVAILABLE).
    pub fn parse(body: &str) -> Result<Self, String> {
        let root: Value = serde_json::from_str(body)
            .map_err(|e| format!("desired-state document is not JSON: {e}"))?;
        if !root.is_object() {
            return Err("desired-state document is not a JSON object".into());
        }
        let mut document = DesiredDocument::default();
        for node in java::elements(root.get("functions")) {
            match parse_entry(node) {
                Ok(entry) => document.functions.push(entry),
                Err(why) => {
                    let address = node
                        .get("address")
                        .and_then(|a| a.as_str())
                        .and_then(|a| FunctionAddress::parse(a).ok());
                    let version = java::as_java_int(node.get("version"));
                    tracing::warn!(
                        address = address.as_ref().map(FunctionAddress::render),
                        err = %why,
                        "dropping unreadable desired-state entry"
                    );
                    if let (Some(address), Some(version)) = (address, version) {
                        document.unreadable.push(UnreadableEntry {
                            address,
                            version,
                            reason: format!("UNREADABLE:{why}"),
                        });
                    }
                }
            }
        }
        for node in java::elements(root.get("unload")) {
            match parse_unload(node) {
                Ok(unload) => document.unload.push(unload),
                Err(why) => tracing::warn!(err = %why, "dropping unreadable unload entry"),
            }
        }
        for node in java::elements(root.get("publicRoutes")) {
            match parse_public_route(node) {
                Ok(route) => document.public_routes.push(route),
                Err(why) => tracing::warn!(err = %why, "dropping unreadable publicRoutes entry"),
            }
        }
        Ok(document)
    }

    /// The current document's live entry for `address`.
    pub fn live_entry(&self, address: &FunctionAddress) -> Option<&Entry> {
        self.functions
            .iter()
            .find(|e| e.role == Role::Live && &e.address == address)
    }

    /// The entry for `address` at exactly `version`, live or candidate.
    pub fn entry_for(&self, address: &FunctionAddress, version: i32) -> Option<&Entry> {
        self.functions
            .iter()
            .find(|e| &e.address == address && e.version == version)
    }

    /// The entry for `address` whose `aliases` names `alias`; a candidate
    /// never qualifies.
    pub fn entry_for_alias(&self, address: &FunctionAddress, alias: &str) -> Option<&Entry> {
        self.functions
            .iter()
            .filter(|e| &e.address == address && e.role != Role::Candidate)
            .find(|e| e.aliases.iter().any(|a| a == alias))
    }
}

fn require_text(node: &Value, field: &str) -> Result<String, String> {
    match node.get(field) {
        Some(Value::String(s)) if !java::is_blank(s) => Ok(s.clone()),
        _ => Err(format!(
            "{field} is required and must be a non-blank string"
        )),
    }
}

fn optional_text(node: &Value, field: &str) -> Option<String> {
    match node.get(field) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn require_int(node: &Value, field: &str) -> Result<i32, String> {
    java::as_java_int(node.get(field))
        .ok_or_else(|| format!("{field} is required and must be an integer"))
}

/// Java `FunctionAddress.parse`'s `UseCaseException`, as `getMessage()`
/// renders it.
fn parse_address(raw: &str) -> Result<FunctionAddress, String> {
    fc_function_model::parse_address(raw).map_err(|e| e.to_string())
}

fn parse_entry(node: &Value) -> Result<Entry, String> {
    let address = parse_address(&require_text(node, "address")?)?;
    let function_id = require_text(node, "functionId")?;
    let version_id = require_text(node, "versionId")?;
    let version = require_int(node, "version")?;
    let role = match require_text(node, "role")?.as_str() {
        "live" => Role::Live,
        "candidate" => Role::Candidate,
        "alias" => Role::Alias,
        other => return Err(format!("unrecognised role: {other}")),
    };
    let mode = match require_text(node, "mode")?.as_str() {
        "warm" => Mode::Warm,
        "lazy" => Mode::Lazy,
        other => return Err(format!("unrecognised mode: {other}")),
    };
    let digest = Digest::parse(&require_text(node, "digest")?).map_err(|e| e.to_string())?;
    let artifact_ref = require_text(node, "artifactRef")?;
    let signature_bundle = optional_text(node, "signatureBundle");
    let signer = match node.get("signer") {
        None | Some(Value::Null) => None,
        Some(signer) => Some(SignerIdentity::new(
            require_text(signer, "issuer")?,
            require_text(signer, "subject")?,
        )),
    };
    let manifest = read_manifest(node.get("manifest"))?;
    Ok(Entry {
        address,
        function_id,
        version_id,
        version,
        role,
        mode,
        digest,
        artifact_ref,
        signature_bundle,
        signer,
        manifest,
        webhook_signing_secret: optional_text(node, "webhookSigningSecret"),
        application_id: optional_text(node, "applicationId"),
        client_id: optional_text(node, "clientId"),
        config: string_map(node.get("config")),
        secrets: string_map(node.get("secrets")),
        missing_settings: string_list(node.get("missingSettings")),
        aliases: string_list(node.get("aliases")),
    })
}

/// Not an object, or any value not a string, drops the whole map: a value
/// the caller cannot trust in part is not trusted at all.
fn string_map(node: Option<&Value>) -> BTreeMap<String, String> {
    let Some(Value::Object(map)) = node else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (key, value) in map {
        match value {
            Value::String(s) => {
                out.insert(key.clone(), s.clone());
            }
            _ => return BTreeMap::new(),
        }
    }
    out
}

/// Not an array reads as empty; a non-string element is dropped.
fn string_list(node: Option<&Value>) -> Vec<String> {
    match node {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_unload(node: &Value) -> Result<UnloadRef, String> {
    Ok(UnloadRef {
        address: parse_address(&require_text(node, "address")?)?,
        version: require_int(node, "version")?,
    })
}

fn parse_public_route(node: &Value) -> Result<PublicRouteRef, String> {
    Ok(PublicRouteRef {
        hostname: require_text(node, "hostname")?,
        path_prefix: require_text(node, "pathPrefix")?,
        address: parse_address(&require_text(node, "address")?)?,
        alias_prefixes: string_list(node.get("aliasPrefixes")),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn entry_json(address: &str, version: i32, role: &str, mode: &str) -> Value {
        json!({
            "address": address,
            "functionId": "fnc_1",
            "versionId": format!("fnv_{address}_{version}"),
            "version": version,
            "role": role,
            "mode": mode,
            "digest": format!("sha256:{}", "a".repeat(64)),
            "artifactRef": "file:///tmp/fn.wasm",
            "manifest": {"runtime": "wasm", "entrypoint": "handle"},
            "applicationId": "app_1"
        })
    }

    #[test]
    fn a_full_entry_reads_every_field() {
        let mut e = entry_json("app.svc.fn", 3, "live", "warm");
        e["signatureBundle"] = json!("{}");
        e["signer"] = json!({"issuer": "https://issuer", "subject": "sub"});
        e["webhookSigningSecret"] = json!("whsec");
        e["clientId"] = json!("clt_1");
        e["config"] = json!({"b": "2", "a": "1"});
        e["secrets"] = json!({"k": "v"});
        e["missingSettings"] = json!(["X", 5]);
        e["aliases"] = json!(["qa"]);
        e["futureField"] = json!(true);
        e["manifest"]["endpoints"] = json!([{"path": "/hook", "auth": "WEBHOOK"}]);
        let body = json!({"functions": [e], "unload": [{"address": "app.svc.old", "version": 1}],
            "publicRoutes": [{"hostname": "api.example.com", "pathPrefix": "/", "address": "app.svc.fn", "aliasPrefixes": ["qa"]}],
            "unknownTop": 1});
        let doc = DesiredDocument::parse(&body.to_string()).unwrap();
        let entry = &doc.functions[0];
        assert_eq!(entry.address.render(), "app.svc.fn");
        assert_eq!(entry.version, 3);
        assert_eq!(entry.role, Role::Live);
        assert_eq!(entry.mode, Mode::Warm);
        assert_eq!(
            entry.signer,
            Some(SignerIdentity::new("https://issuer", "sub"))
        );
        assert_eq!(entry.config.len(), 2);
        assert_eq!(entry.missing_settings, ["X"]);
        assert_eq!(entry.manifest.runtime, fc_function_model::Runtime::Wasm);
        assert!(entry.manifest.has_webhook_endpoint());
        assert_eq!(doc.unload[0].version, 1);
        assert_eq!(doc.public_routes[0].alias_prefixes, ["qa"]);
        let debug = format!("{entry:?}");
        assert!(!debug.contains("whsec") && !debug.contains("\"v\""));
    }

    #[test]
    fn an_unreadable_entry_is_reported_and_the_rest_applies() {
        let mut bad_digest = entry_json("app.svc.bad", 2, "live", "warm");
        bad_digest["digest"] = json!("sha256:XYZ");
        let mut bad_role = entry_json("app.svc.role", 1, "promoted", "warm");
        bad_role["role"] = json!("promoted");
        let no_address = json!({"version": 1});
        let mut bad_manifest = entry_json("app.svc.man", 4, "live", "lazy");
        bad_manifest["manifest"] = json!({"runtime": "python", "entrypoint": "x"});
        let good = entry_json("app.svc.good", 1, "live", "lazy");
        let body = json!({"functions": [bad_digest, bad_role, no_address, bad_manifest, good]});
        let doc = DesiredDocument::parse(&body.to_string()).unwrap();
        assert_eq!(doc.functions.len(), 1);
        assert_eq!(doc.functions[0].address.render(), "app.svc.good");
        let reasons: Vec<_> = doc
            .unreadable
            .iter()
            .map(|u| (u.address.render(), u.reason.clone()))
            .collect();
        assert_eq!(
            reasons,
            [
                (
                    "app.svc.bad".to_owned(),
                    format!("UNREADABLE:{}", crate::digest::DIGEST_INVALID_MESSAGE)
                ),
                (
                    "app.svc.role".to_owned(),
                    "UNREADABLE:unrecognised role: promoted".to_owned()
                ),
                (
                    "app.svc.man".to_owned(),
                    "UNREADABLE:manifest runtime is unreadable".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn lenient_maps_and_lists() {
        let mut e = entry_json("app.svc.fn", 1, "candidate", "lazy");
        e["config"] = json!({"a": "1", "b": 2});
        e["aliases"] = json!("not-a-list");
        let doc = DesiredDocument::parse(&json!({"functions": [e]}).to_string()).unwrap();
        assert!(doc.functions[0].config.is_empty());
        assert!(doc.functions[0].aliases.is_empty());
    }

    #[test]
    fn a_non_object_document_is_an_error() {
        assert!(DesiredDocument::parse("[]").is_err());
        assert!(DesiredDocument::parse("nope").is_err());
        assert_eq!(
            DesiredDocument::parse("{}").unwrap(),
            DesiredDocument::default()
        );
    }
}
