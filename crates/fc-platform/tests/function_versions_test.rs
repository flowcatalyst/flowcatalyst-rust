//! Artifacts, publishing and versions over HTTP against a real database
//! (Java `FunctionArtifactUploadApiTest`, `PublishPromoteRetireTest`,
//! `PublishSignaturesTest`, `FunctionCheckManifestApiTest` and the version
//! routes of `FunctionApiTest`): upload, publish, list, get and retire;
//! upload's checks in Java's order; `platform://` refs; the publish checks;
//! signatures off in dev mode and required with a signer policy; the next
//! version number under concurrency; the manifest check writing nothing.
//! Requires Docker.

#[path = "support/mod.rs"]
mod support;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use tower::ServiceExt;

use fc_function_signing::{SignatureVerifier, Signatures, SignaturesMode, TrustRoot};
use fc_platform::application::entity::Application;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::function::api::{functions_router, FunctionsState};
use fc_platform::function::artifact::{ArtifactBlobStore, FileArtifactBlobStore};
use fc_platform::function::operations::{FunctionOperations, PublishChecks, TriggerSync};
use fc_platform::function::settings_repository::FunctionSettingsRepository;
use fc_platform::function::{Digest, FunctionLimits};
use fc_platform::role::entity::{permissions, AuthRole};
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform::shared::authorization_service::{ApplicationAccessService, AuthorizationService};
use fc_platform::shared::middleware::{AppState, AuthLayer};
use fc_platform::Client;
use support::{read_json, TestApp};

use permissions::function::{
    FUNCTION_DOMAIN_MANAGE, FUNCTION_MANAGE, FUNCTION_POLICY_MANAGE, FUNCTION_PUBLISH,
    FUNCTION_VIEW,
};

const ALL: &[&str] = &[
    FUNCTION_VIEW,
    FUNCTION_MANAGE,
    FUNCTION_PUBLISH,
    FUNCTION_POLICY_MANAGE,
    FUNCTION_DOMAIN_MANAGE,
];

/// Java's golden Sigstore bundle, signed by the conformance beacon over
/// `artifact.txt`.
const BUNDLE: &str = include_str!("data/function/sigstore/happy-path-v0.3.sigstore.json");
const SIGNED_ARTIFACT: &[u8] = include_bytes!("data/function/sigstore/artifact.txt");
const ISSUER: &str = "https://token.actions.githubusercontent.com";
const SUBJECT: &str =
    "https://github.com/sigstore-conformance/extremely-dangerous-public-oidc-beacon/\
                       .github/workflows/extremely-dangerous-oidc-beacon.yml@refs/heads/main";

// ── Harness ─────────────────────────────────────────────────────────────────

struct Tmp(std::path::PathBuf);

impl Tmp {
    fn new() -> Tmp {
        Tmp(std::env::temp_dir().join(format!(
            "fc-fn-versions-{}",
            fc_platform::shared::tsid::generate_untyped()
        )))
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The function routes with the given signature policy, store and limits,
/// behind the same auth layer as the full router.
fn router(
    app: &TestApp,
    signatures: Signatures,
    artifacts: Option<Arc<dyn ArtifactBlobStore>>,
    limits: FunctionLimits,
) -> Router {
    let settings = Arc::new(FunctionSettingsRepository::new(&app.pool, None));
    let state = FunctionsState {
        functions: app.repos.function_repo.clone(),
        versions: app.repos.function_version_repo.clone(),
        hosts: app.repos.function_host_repo.clone(),
        settings: settings.clone(),
        policies: app.repos.function_policy_repo.clone(),
        domains: app.repos.function_domain_repo.clone(),
        routes: app.repos.function_route_repo.clone(),
        trigger_objects: app.repos.function_trigger_object_repo.clone(),
        app_access: Arc::new(ApplicationAccessService::new(
            app.repos.principal_repo.clone(),
            app.repos.application_repo.clone(),
        )),
        limits,
        ops: FunctionOperations {
            functions: app.repos.function_repo.clone(),
            versions: app.repos.function_version_repo.clone(),
            applications: app.repos.application_repo.clone(),
            clients: app.repos.client_repo.clone(),
            settings: settings.clone(),
            policies: app.repos.function_policy_repo.clone(),
            domains: app.repos.function_domain_repo.clone(),
            routes: app.repos.function_route_repo.clone(),
            trigger_sync: TriggerSync::from_repositories(
                &app.repos,
                settings.clone(),
                fc_platform::function::PoolUrlTemplate::parse("http://fn-{pool}:8080").unwrap(),
            ),
            limits,
            signatures,
            artifacts,
            publish_checks: PublishChecks {
                event_types: app.repos.event_type_repo.clone(),
                service_accounts: app.repos.service_account_repo.clone(),
                versions: app.repos.function_version_repo.clone(),
                functions: app.repos.function_repo.clone(),
                domains: app.repos.function_domain_repo.clone(),
                routes: app.repos.function_route_repo.clone(),
                hosts: app.repos.function_host_repo.clone(),
                limits,
            },
            unit_of_work: app.unit_of_work.clone(),
        },
    };
    let (router, _) = functions_router(state).split_for_parts();
    router.layer(AuthLayer::new(AppState {
        auth_service: app.auth_service.clone(),
        authz_service: Arc::new(AuthorizationService::new(app.repos.role_repo.clone())),
    }))
}

/// Signatures off, as fc-dev runs: `FC_FN_SIGNATURES=off` with
/// `FLOWCATALYST_DEV_MODE=true`.
fn dev_mode() -> Signatures {
    Signatures::resolve(SignaturesMode::Off, true, "").expect("off is allowed in dev mode")
}

fn required() -> Signatures {
    Signatures::Required(Arc::new(SignatureVerifier::new(
        TrustRoot::sigstore_public_good(),
    )))
}

fn file_store(tmp: &Tmp) -> Arc<FileArtifactBlobStore> {
    Arc::new(FileArtifactBlobStore::new(tmp.0.join("store")).unwrap())
}

async fn token(app: &TestApp, scope: UserScope, clients: &[&str], perms: &[&str]) -> String {
    let n = fc_platform::shared::tsid::generate_untyped().to_lowercase();
    let role = AuthRole::new("platform", format!("fnv-test-{n}"), "Function version test")
        .with_permissions(perms.iter().map(|p| p.to_string()));
    app.repos.role_repo.insert(&role).await.expect("role");
    let mut principal = Principal::new_user(format!("fnv-{n}@flowcatalyst.test"), scope);
    if scope == UserScope::Client {
        principal = principal.with_client_id(clients[0]);
    }
    principal.roles = vec![RoleAssignment::new(role.name.clone())];
    principal.all_applications = true;
    app.repos
        .principal_repo
        .insert(&principal)
        .await
        .expect("principal");
    app.auth_service
        .generate_access_token(&principal)
        .expect("token")
}

async fn anchor(app: &TestApp) -> String {
    token(app, UserScope::Anchor, &[], ALL).await
}

async fn application(app: &TestApp, code: &str) -> Application {
    let a = Application::new(code, code.to_uppercase());
    app.repos.application_repo.insert(&a).await.expect("app");
    a
}

async fn client(app: &TestApp, identifier: &str) -> Client {
    let c = Client::new(identifier.to_uppercase(), identifier);
    app.repos.client_repo.insert(&c).await.expect("client");
    c
}

async fn send(
    r: &Router,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    read_json(r.clone().oneshot(req.body(body).unwrap()).await.unwrap()).await
}

async fn get(r: &Router, path: &str, token: &str) -> (StatusCode, Value) {
    send(r, Method::GET, path, token, None).await
}

async fn post(r: &Router, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
    send(r, Method::POST, path, token, Some(body)).await
}

/// `PUT …/artifacts/{digest}` with a raw body; `length` overrides the
/// declared `Content-Length`.
async fn upload(
    r: &Router,
    address: &str,
    digest: &str,
    token: &str,
    bytes: Vec<u8>,
    length: Option<u64>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/functions/{address}/artifacts/{digest}"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/octet-stream");
    if let Some(n) = length {
        req = req.header("content-length", n.to_string());
    }
    read_json(
        r.clone()
            .oneshot(req.body(Body::from(bytes)).unwrap())
            .await
            .unwrap(),
    )
    .await
}

fn digest_of(bytes: &[u8]) -> String {
    Digest::from_sha256(&Sha256::digest(bytes).into())
        .value()
        .to_string()
}

fn hex_of(bytes: &[u8]) -> String {
    digest_of(bytes)["sha256:".len()..].to_string()
}

#[track_caller]
fn assert_error(got: &(StatusCode, Value), status: StatusCode, code: &str) {
    assert_eq!(got.0, status, "body: {}", got.1);
    assert_eq!(got.1["error"], code, "body: {}", got.1);
    assert!(got.1["message"].is_string(), "body: {}", got.1);
}

async fn create_function(
    r: &Router,
    token: &str,
    app_code: &str,
    name: &str,
    client_id: Option<&str>,
) -> Value {
    let mut body =
        json!({"applicationCode": app_code, "serviceName": "svc", "name": name, "runtime": "wasm"});
    if let Some(c) = client_id {
        body["clientId"] = json!(c);
    }
    let (status, out) = post(r, "/api/functions", token, body).await;
    assert_eq!(status, StatusCode::CREATED, "body: {out}");
    out
}

fn manifest() -> Value {
    json!({"runtime": "wasm", "entrypoint": "handle", "config": ["GREETING"]})
}

fn publish_body(artifact_ref: &str, digest: &str) -> Value {
    json!({"artifactRef": artifact_ref, "digest": digest, "manifest": manifest()})
}

async fn version_count(app: &TestApp, function_id: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM fn_versions WHERE function_id = $1")
        .bind(function_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    n
}

async fn set_alias(app: &TestApp, function_id: &str, alias: &str, version_id: &str) {
    sqlx::query(
        "INSERT INTO fn_aliases (function_id, alias, version_id, updated_by) VALUES ($1, $2, $3, 'prn_t') \
         ON CONFLICT (function_id, alias) DO UPDATE SET version_id = EXCLUDED.version_id",
    )
    .bind(function_id)
    .bind(alias)
    .bind(version_id)
    .execute(&app.pool)
    .await
    .expect("alias");
}

// ── Upload → publish → list → get → retire ──────────────────────────────────

/// U1 and the version lifecycle (Java `PublishPromoteRetireTest`,
/// `FunctionApiTest`'s version routes).
#[tokio::test]
#[ignore = "requires Docker"]
async fn upload_publish_list_get_and_retire() {
    let app = TestApp::setup().await;
    application(&app, "billing").await;
    let tmp = Tmp::new();
    let store = file_store(&tmp);
    let r = router(
        &app,
        dev_mode(),
        Some(store.clone()),
        FunctionLimits::defaults(),
    );
    let t = anchor(&app).await;
    let f = create_function(&r, &t, "billing", "create", None).await;
    let fid = f["id"].as_str().unwrap().to_string();
    let path = "/api/functions/billing.svc.create";

    // Upload: 200 with the platform:// ref; idempotent.
    let bytes = b"\0asm-component-bytes".to_vec();
    let digest = digest_of(&bytes);
    let up = upload(&r, "billing.svc.create", &digest, &t, bytes.clone(), None).await;
    assert_eq!(up.0, StatusCode::OK, "{}", up.1);
    let platform_ref = format!("platform://{fid}/{}", hex_of(&bytes));
    assert_eq!(
        up.1,
        json!({"artifactRef": platform_ref, "digest": digest, "bytes": bytes.len()})
    );
    let again = upload(&r, "billing.svc.create", &digest, &t, bytes.clone(), None).await;
    assert_eq!(again, up, "an upload is idempotent");
    let stored = std::fs::read(tmp.0.join("store").join(&fid).join(hex_of(&bytes))).unwrap();
    assert_eq!(stored, bytes, "the blob's bytes equal the upload");
    // No event and no audit row for an upload: it is infrastructure.
    assert_eq!(app.audit_count_for(&fid).await, 1, "only the create");

    // Publish with the returned ref: 201, version 1, no signer.
    let (status, published) = post(
        &r,
        &format!("{path}/versions"),
        &t,
        publish_body(&platform_ref, &digest),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
    assert!(published["id"].as_str().unwrap().starts_with("fnv_"));
    assert_eq!(published["version"], 1);
    assert_eq!(published["state"], "PUBLISHED");
    assert_eq!(published["digest"], digest);
    assert!(published.get("signer").is_none(), "{published}");
    assert_eq!(
        app.event_count_by_type("platform:function:version:published")
            .await,
        1
    );
    assert_eq!(app.audit_count_for(&fid).await, 2, "create + publish");

    // The same digest and manifest again: a no-op, 200 with the existing
    // version, and nothing written.
    let (status, again) = post(
        &r,
        &format!("{path}/versions"),
        &t,
        publish_body(&platform_ref, &digest),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(
        again, published,
        "the existing version, as the publish answered it"
    );
    assert_eq!(
        app.event_count_by_type("platform:function:version:published")
            .await,
        1,
        "no event for a no-op"
    );
    assert_eq!(
        app.audit_count_for(&fid).await,
        2,
        "no audit row for a no-op"
    );
    assert_eq!(version_count(&app, &fid).await, 1);

    // The same digest under another manifest: still 409, naming the version.
    let mut other = publish_body(&platform_ref, &digest);
    other["manifest"]["warm"] = json!(true);
    let dup = post(&r, &format!("{path}/versions"), &t, other).await;
    assert_error(&dup, StatusCode::CONFLICT, "VERSION_DIGEST_EXISTS");
    assert_eq!(dup.1["details"]["version"], 1);
    assert_eq!(
        dup.1["message"],
        "digest is already published as version 1 for this function"
    );

    // A second version by reference.
    let d2 = digest_of(b"v2");
    let (status, v2) = post(
        &r,
        &format!("{path}/versions"),
        &t,
        publish_body("oci://registry.example/fn@x", &d2),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{v2}");
    assert_eq!(v2["version"], 2);

    // List: newest first, the list shape (no manifest).
    let (status, list) = get(&r, &format!("{path}/versions"), &t).await;
    assert_eq!(status, StatusCode::OK);
    let list = list.as_array().unwrap();
    assert_eq!(
        list.iter()
            .map(|v| v["version"].clone())
            .collect::<Vec<_>>(),
        vec![json!(2), json!(1)]
    );
    let v1 = &list[1];
    assert_eq!(v1["artifactRef"], platform_ref);
    assert_eq!(v1["pool"], "default");
    assert_eq!(v1["warm"], false);
    assert_eq!(v1["live"], false);
    assert_eq!(v1["state"], "PUBLISHED");
    assert!(v1["publishedBy"].as_str().unwrap().starts_with("prn_"));
    assert!(v1["publishedAt"].as_str().unwrap().ends_with('Z'));
    for absent in ["manifest", "signer", "readyAt", "retiredAt"] {
        assert!(v1.get(absent).is_none(), "{absent}: {v1}");
    }

    // Get: adds the normalised manifest.
    let (status, one) = get(&r, &format!("{path}/versions/1"), &t).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["manifest"]["runtime"], "wasm");
    assert_eq!(one["manifest"]["pool"], "default");
    assert_eq!(one["manifest"]["config"], json!(["GREETING"]));
    assert_error(
        &get(&r, &format!("{path}/versions/0"), &t).await,
        StatusCode::BAD_REQUEST,
        "VERSION_INVALID",
    );
    assert_error(
        &get(&r, &format!("{path}/versions/9"), &t).await,
        StatusCode::NOT_FOUND,
        "FUNCTION_VERSION_NOT_FOUND",
    );

    // Status and config read the real versions now.
    let (_, status_body) = get(&r, &format!("{path}/status"), &t).await;
    assert_eq!(
        status_body["versions"],
        json!([{"version": 2, "state": "PUBLISHED"}, {"version": 1, "state": "PUBLISHED"}])
    );
    let (_, config) = get(&r, &format!("{path}/config"), &t).await;
    assert_eq!(config["declared"], json!(["GREETING"]));
    assert_eq!(config["missing"], json!(["GREETING"]));
    assert_eq!(
        config["declaredBy"],
        json!([{"version": 2, "keys": ["GREETING"]}])
    );

    // Retire 1: 200, the list shape, RETIRED with retiredAt; then again a
    // no-op: 200 with the same version, no second event.
    let (status, retired) = post(&r, &format!("{path}/versions/1/retire"), &t, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{retired}");
    assert_eq!(retired["state"], "RETIRED");
    assert!(retired["retiredAt"].is_string());
    assert!(retired.get("manifest").is_none());
    assert_eq!(
        app.event_count_by_type("platform:function:version:retired")
            .await,
        1
    );
    let (status, again) = post(&r, &format!("{path}/versions/1/retire"), &t, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, retired);
    assert_eq!(
        app.event_count_by_type("platform:function:version:retired")
            .await,
        1,
        "no event for a no-op"
    );

    // The live version never retires; a named alias blocks it too.
    let v2_id = v2["id"].as_str().unwrap();
    set_alias(&app, &fid, "live", v2_id).await;
    let live = post(&r, &format!("{path}/versions/2/retire"), &t, json!({})).await;
    assert_error(&live, StatusCode::CONFLICT, "VERSION_IS_LIVE");
    assert_eq!(live.1["message"], "promote another version first");
    let (_, v3) = post(
        &r,
        &format!("{path}/versions"),
        &t,
        publish_body("oci://r/a", &digest_of(b"v3")),
    )
    .await;
    set_alias(&app, &fid, "qa", v3["id"].as_str().unwrap()).await;
    set_alias(&app, &fid, "beta", v3["id"].as_str().unwrap()).await;
    let aliased = post(&r, &format!("{path}/versions/3/retire"), &t, json!({})).await;
    assert_error(&aliased, StatusCode::CONFLICT, "VERSION_ALIASED");
    assert_eq!(
        aliased.1["message"],
        "aliases beta, qa point at this version; move or remove them first"
    );
    let (_, list) = get(&r, &format!("{path}/versions"), &t).await;
    assert_eq!(list[1]["live"], true, "{list}");

    // Deleting the function deletes its blobs, after the commit.
    let (status, _) = send(&r, Method::DELETE, path, &t, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!tmp.0.join("store").join(&fid).exists());
}

// ── Upload's checks, in Java's order ────────────────────────────────────────

/// U2, U3, U5, U5b, U7: store, permission, reach, digest shape, declared
/// length, then the body.
#[tokio::test]
#[ignore = "requires Docker"]
async fn upload_checks_run_in_javas_order() {
    let app = TestApp::setup().await;
    application(&app, "billing").await;
    let acme = client(&app, "acme").await;
    let bravo = client(&app, "bravo").await;
    let tmp = Tmp::new();
    let store = file_store(&tmp);
    let r = router(
        &app,
        dev_mode(),
        Some(store.clone()),
        FunctionLimits::defaults(),
    );
    let no_store = router(&app, dev_mode(), None, FunctionLimits::defaults());
    let t = anchor(&app).await;
    let f = create_function(&r, &t, "billing", "up", Some(&acme.id)).await;
    let fid = f["id"].as_str().unwrap();
    let bytes = b"artifact".to_vec();
    let digest = digest_of(&bytes);
    let viewer = token(&app, UserScope::Anchor, &[], &[FUNCTION_VIEW]).await;
    let other_client = token(&app, UserScope::Client, &[&bravo.id], ALL).await;

    // No store: 503, even for a caller without the permission.
    let got = upload(
        &no_store,
        "billing.svc.up",
        &digest,
        &viewer,
        bytes.clone(),
        None,
    )
    .await;
    assert_error(
        &got,
        StatusCode::SERVICE_UNAVAILABLE,
        "ARTIFACT_STORE_NOT_CONFIGURED",
    );
    // Then the permission, before reach.
    let got = upload(
        &r,
        "billing.svc.nope",
        &digest,
        &viewer,
        bytes.clone(),
        None,
    )
    .await;
    assert_error(&got, StatusCode::FORBIDDEN, "PERMISSION_REQUIRED");
    // Then reach: another client's function is 404, as if absent.
    let got = upload(
        &r,
        "billing.svc.up",
        &digest,
        &other_client,
        bytes.clone(),
        None,
    )
    .await;
    assert_error(&got, StatusCode::NOT_FOUND, "FUNCTION_NOT_FOUND");
    // Then the digest's shape, then the declared length.
    let got = upload(&r, "billing.svc.up", "sha256:ABC", &t, bytes.clone(), None).await;
    assert_error(&got, StatusCode::BAD_REQUEST, "DIGEST_INVALID");
    let got = upload(
        &r,
        "billing.svc.up",
        &digest,
        &t,
        bytes.clone(),
        Some(256 * 1024 * 1024 + 1),
    )
    .await;
    assert_error(&got, StatusCode::PAYLOAD_TOO_LARGE, "ARTIFACT_TOO_LARGE");
    // The body: empty, then a mismatch, which stores nothing.
    let got = upload(&r, "billing.svc.up", &digest, &t, Vec::new(), None).await;
    assert_error(&got, StatusCode::UNPROCESSABLE_ENTITY, "ARTIFACT_EMPTY");
    let got = upload(&r, "billing.svc.up", &digest, &t, b"other".to_vec(), None).await;
    assert_error(&got, StatusCode::UNPROCESSABLE_ENTITY, "DIGEST_MISMATCH");
    let d = Digest::parse(&digest).unwrap();
    assert!(!store.exists(fid, &d).await.unwrap(), "nothing stored");
    // A 3 MB upload streams through, well past the JSON routes' body limit.
    let big = vec![7u8; 3 * 1024 * 1024];
    let got = upload(
        &r,
        "billing.svc.up",
        &digest_of(&big),
        &t,
        big.clone(),
        None,
    )
    .await;
    assert_eq!(got.0, StatusCode::OK, "{}", got.1);
    assert_eq!(got.1["bytes"], big.len());
}

// ── Publish: refs, reach, state, manifest ───────────────────────────────────

/// U6/U7 and Java's `PublishVersion` clauses before the signature.
#[tokio::test]
#[ignore = "requires Docker"]
async fn publish_refuses_bad_refs_unreachable_disabled_and_bad_manifests() {
    let app = TestApp::setup().await;
    application(&app, "billing").await;
    let acme = client(&app, "acme").await;
    let bravo = client(&app, "bravo").await;
    let tmp = Tmp::new();
    let store = file_store(&tmp);
    let r = router(
        &app,
        dev_mode(),
        Some(store.clone()),
        FunctionLimits::defaults(),
    );
    let no_store = router(&app, dev_mode(), None, FunctionLimits::defaults());
    let t = anchor(&app).await;
    let f = create_function(&r, &t, "billing", "pub", Some(&acme.id)).await;
    let fid = f["id"].as_str().unwrap().to_string();
    let other = create_function(&r, &t, "billing", "other", None).await;
    let path = "/api/functions/billing.svc.pub/versions";
    let bytes = b"component".to_vec();
    let digest = digest_of(&bytes);
    let own_ref = format!("platform://{fid}/{}", hex_of(&bytes));

    // Another function's id, or another digest's hex: mismatch.
    let theirs = format!(
        "platform://{}/{}",
        other["id"].as_str().unwrap(),
        hex_of(&bytes)
    );
    assert_error(
        &post(&r, path, &t, publish_body(&theirs, &digest)).await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "ARTIFACT_REF_MISMATCH",
    );
    let wrong_hex = format!("platform://{fid}/{}", "0".repeat(64));
    assert_error(
        &post(&r, path, &t, publish_body(&wrong_hex, &digest)).await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "ARTIFACT_REF_MISMATCH",
    );
    // Right, but never uploaded; and no store at all.
    assert_error(
        &post(&r, path, &t, publish_body(&own_ref, &digest)).await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "ARTIFACT_NOT_UPLOADED",
    );
    assert_error(
        &post(&no_store, path, &t, publish_body(&own_ref, &digest)).await,
        StatusCode::SERVICE_UNAVAILABLE,
        "ARTIFACT_STORE_NOT_CONFIGURED",
    );
    assert_eq!(version_count(&app, &fid).await, 0, "no version row");
    // An oci:// publish is unaffected by the missing store.
    let (status, _) = post(
        &no_store,
        path,
        &t,
        publish_body("oci://r/a", &digest_of(b"o")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Validation before any load.
    let missing = post(
        &r,
        path,
        &t,
        json!({"digest": digest, "manifest": manifest()}),
    )
    .await;
    assert_error(&missing, StatusCode::BAD_REQUEST, "ARTIFACT_REF_REQUIRED");
    assert_error(
        &post(&r, path, &t, publish_body("https://x/a.wasm", &digest)).await,
        StatusCode::BAD_REQUEST,
        "ARTIFACT_REF_INVALID",
    );
    assert_error(
        &post(&r, path, &t, publish_body("s3://bucket/key", "sha256:nope")).await,
        StatusCode::BAD_REQUEST,
        "DIGEST_INVALID",
    );

    // The manifest: absent, and another runtime's.
    let absent = post(
        &r,
        path,
        &t,
        json!({"artifactRef": "oci://r/b", "digest": digest}),
    )
    .await;
    assert_error(&absent, StatusCode::BAD_REQUEST, "MANIFEST_REQUIRED");
    let jvm = post(
        &r,
        path,
        &t,
        json!({"artifactRef": "oci://r/b", "digest": digest,
               "manifest": {"runtime": "jvm", "entrypoint": "com.acme.Fn"}}),
    )
    .await;
    assert_error(&jvm, StatusCode::BAD_REQUEST, "RUNTIME_MISMATCH");

    // Out of reach: 404, never 403; without the permission: 403.
    let other_client = token(&app, UserScope::Client, &[&bravo.id], ALL).await;
    assert_error(
        &post(&r, path, &other_client, publish_body("oci://r/c", &digest)).await,
        StatusCode::NOT_FOUND,
        "FUNCTION_NOT_FOUND",
    );
    let viewer = token(&app, UserScope::Anchor, &[], &[FUNCTION_VIEW]).await;
    assert_error(
        &post(&r, path, &viewer, publish_body("oci://r/c", &digest)).await,
        StatusCode::FORBIDDEN,
        "PERMISSION_REQUIRED",
    );

    // A disabled function takes no versions.
    let (status, _) = send(
        &r,
        Method::PUT,
        "/api/functions/billing.svc.pub",
        &t,
        Some(json!({"status": "DISABLED"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let disabled = post(&r, path, &t, publish_body("oci://r/d", &digest_of(b"d"))).await;
    assert_error(&disabled, StatusCode::CONFLICT, "FUNCTION_DISABLED");
    assert_eq!(disabled.1["message"], "function is disabled");
}

// ── Publish checks ──────────────────────────────────────────────────────────

/// Java `FunctionTriggerSync.checkPublish`, each code, and the manifest
/// check route returning all of them while writing nothing.
#[tokio::test]
#[ignore = "requires Docker"]
async fn publish_checks_and_the_manifest_check() {
    let app = TestApp::setup().await;
    let billing = application(&app, "billing").await;
    let acme = client(&app, "acme").await;
    let limits = FunctionLimits::new(30000, 32, 64, 4, 1).unwrap();
    let r = router(&app, dev_mode(), None, limits);
    let t = anchor(&app).await;
    let f = create_function(&r, &t, "billing", "chk", None).await;
    let fid = f["id"].as_str().unwrap().to_string();
    let path = "/api/functions/billing.svc.chk";
    let publish =
        |m: Value, d: &str| json!({"artifactRef": "oci://r/x", "digest": d, "manifest": m});
    let webhook = json!([{"path": "/events/*", "auth": "webhook"}]);

    // A subscription to an unknown event type, and no signing secret.
    let subscribing = json!({"runtime": "wasm", "entrypoint": "handle", "endpoints": webhook,
        "subscriptions": [{"eventType": "billing:invoices:invoice:created", "path": "/events/x"}]});
    let got = post(
        &r,
        &format!("{path}/versions"),
        &t,
        publish(subscribing.clone(), &digest_of(b"1")),
    )
    .await;
    assert_error(&got, StatusCode::BAD_REQUEST, "EVENT_TYPE_NOT_FOUND");
    assert_eq!(
        got.1["message"],
        "event type 'billing:invoices:invoice:created' not found"
    );

    // The check route lists every problem, in Java's order, and writes nothing.
    let schedules = json!({"runtime": "wasm", "entrypoint": "handle", "endpoints": webhook,
        "subscriptions": [{"eventType": "billing:invoices:invoice:created", "path": "/events/x"}],
        "schedules": [{"cron": "0 * * *", "timezone": "Mars/Olympus", "path": "/events/tick"}]});
    let (status, check) = post(
        &r,
        &format!("{path}/manifest/check"),
        &t,
        json!({"manifest": schedules}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{check}");
    assert_eq!(check["valid"], false);
    let codes: Vec<&str> = check["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        vec![
            "EVENT_TYPE_NOT_FOUND",
            "CRON_INVALID",
            "TIMEZONE_INVALID",
            "APPLICATION_SIGNING_SECRET_REQUIRED"
        ]
    );
    assert_eq!(
        check["errors"][1]["message"],
        "cron expression '0 * * *' invalid: cron expression must have 5 or 6 \
         whitespace-separated fields ([sec] min hour dom mon dow), got 4: '0 * * *'"
    );
    assert_eq!(check["errors"][0]["details"], json!({}));
    assert!(
        check.get("plan").is_none(),
        "no plan for an invalid manifest"
    );
    // A manifest problem carries its pointer.
    let (_, bad) = post(
        &r,
        &format!("{path}/manifest/check"),
        &t,
        json!({"manifest": {"runtime": "wasm", "entrypoint": "handle", "bogus": 1}}),
    )
    .await;
    assert_eq!(bad["valid"], false);
    assert_eq!(bad["errors"][0]["code"], "MANIFEST_UNKNOWN_FIELD");
    assert_eq!(bad["errors"][0]["details"]["pointer"], "/bogus");
    let (_, good) = post(
        &r,
        &format!("{path}/manifest/check"),
        &t,
        json!({"manifest": manifest()}),
    )
    .await;
    // Valid: the plan for promoting it to live as the next version.
    let pool = format!("fn-{}", fid["fnc_".len()..].to_lowercase());
    assert_eq!(
        good,
        json!({"valid": true, "errors": [], "plan": {"alias": "live", "toVersion": 1,
            "settingsMissing": ["GREETING"], "httpOnly": false,
            "pool": {"action": "create", "key": pool, "changedFields": []},
            "subscriptions": [], "schedules": [],
            "publicRoutes": {"action": "unchanged", "added": [], "removed": []},
            "conflicts": [],
            "warnings": [{"code": "POOL_HAS_NO_LIVE_HOSTS",
                "message": "no host in pool 'default' has sent a heartbeat recently; the version stays PUBLISHED until one loads it"}]}})
    );
    assert_eq!(version_count(&app, &fid).await, 0);

    // A pool whose every live host reports its runtimes, none this one, is
    // refused; one host that says nothing makes it a warning instead.
    sqlx::query(
        "INSERT INTO fn_hosts (id, pool, state, loaded, runtimes) \
         VALUES ('jvm-host', 'default', 'ACTIVE', '[]', '[\"jvm\"]')",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let (_, refused) = post(
        &r,
        &format!("{path}/manifest/check"),
        &t,
        json!({"manifest": manifest()}),
    )
    .await;
    assert_eq!(refused["valid"], false, "{refused}");
    assert_eq!(refused["errors"][0]["code"], "POOL_RUNTIME_UNSUPPORTED");
    assert_eq!(
        refused["errors"][0]["message"],
        "no live host in pool 'default' can load runtime 'wasm'"
    );
    sqlx::query(
        "INSERT INTO fn_hosts (id, pool, state, loaded) VALUES ('silent-host', 'default', 'ACTIVE', '[]')",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let (_, unknown) = post(
        &r,
        &format!("{path}/manifest/check"),
        &t,
        json!({"manifest": manifest()}),
    )
    .await;
    assert_eq!(unknown["valid"], true, "{unknown}");
    assert_eq!(
        unknown["plan"]["warnings"][0]["code"],
        "POOL_RUNTIME_UNKNOWN"
    );
    sqlx::query("DELETE FROM fn_hosts WHERE id IN ('jvm-host', 'silent-host')")
        .execute(&app.pool)
        .await
        .unwrap();

    // Resolve both: the event type exists, the application signs.
    sqlx::query(
        "INSERT INTO msg_event_types (id, code, name, status, source, client_scoped, application, \
         subdomain, aggregate, created_at, updated_at) VALUES ('evt_t1', \
         'billing:invoices:invoice:created', 'Created', 'CURRENT', 'API', false, 'billing', \
         'invoices', 'invoice', NOW(), NOW())",
    )
    .execute(&app.pool)
    .await
    .expect("event type");
    sqlx::query(
        "INSERT INTO iam_service_accounts (id, code, name, application_id, active, \
         wh_signing_secret_ref) VALUES ('sac_t1', 'billing-sa', 'SA', $1, true, 'encrypted:x')",
    )
    .bind(&billing.id)
    .execute(&app.pool)
    .await
    .expect("service account");
    let (status, v1) = post(
        &r,
        &format!("{path}/versions"),
        &t,
        publish(subscribing, &digest_of(b"2")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{v1}");

    // An archived event type is refused as well.
    sqlx::query("UPDATE msg_event_types SET status = 'ARCHIVED' WHERE id = 'evt_t1'")
        .execute(&app.pool)
        .await
        .unwrap();
    let (_, check) = post(
        &r,
        &format!("{path}/manifest/check"),
        &t,
        json!({"manifest": {"runtime": "wasm", "entrypoint": "handle", "endpoints": webhook,
            "subscriptions": [{"eventType": "billing:invoices:invoice:created", "path": "/events/x"}]}}),
    )
    .await;
    assert_eq!(
        check["errors"][0]["message"],
        "event type 'billing:invoices:invoice:created' is archived"
    );

    // Warm capacity (limit 1): another function's live warm version fills it.
    let other = create_function(&r, &t, "billing", "warm", None).await;
    let warm = json!({"runtime": "wasm", "entrypoint": "handle", "warm": true});
    let (status, w) = post(
        &r,
        "/api/functions/billing.svc.warm/versions",
        &t,
        publish(warm.clone(), &digest_of(b"w1")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{w}");
    set_alias(
        &app,
        other["id"].as_str().unwrap(),
        "live",
        w["id"].as_str().unwrap(),
    )
    .await;
    // The warm function itself may republish: its own live is excluded.
    let (status, _) = post(
        &r,
        "/api/functions/billing.svc.warm/versions",
        &t,
        publish(warm.clone(), &digest_of(b"w2")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let full = post(
        &r,
        &format!("{path}/versions"),
        &t,
        publish(warm, &digest_of(b"w3")),
    )
    .await;
    assert_error(&full, StatusCode::BAD_REQUEST, "WARM_CAPACITY_EXCEEDED");
    assert_eq!(
        full.1["message"],
        "pool 'default' is at its warm-function limit (1)"
    );

    // Public routes: an unclaimed hostname, another owner's, and a taken route.
    let public = json!({"runtime": "wasm", "entrypoint": "handle",
        "public": [{"hostname": "api.acme.com", "pathPrefix": "/"}]});
    let unclaimed = post(
        &r,
        &format!("{path}/versions"),
        &t,
        publish(public.clone(), &digest_of(b"p1")),
    )
    .await;
    assert_error(
        &unclaimed,
        StatusCode::BAD_REQUEST,
        "PUBLIC_HOSTNAME_NOT_CLAIMED",
    );
    assert_eq!(
        unclaimed.1["message"],
        "hostname 'api.acme.com' is not under a domain claimed by this function's owner"
    );
    let (status, _) = post(
        &r,
        "/api/function-domains",
        &t,
        json!({"hostname": "acme.com", "clientId": acme.id}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let theirs = post(
        &r,
        &format!("{path}/versions"),
        &t,
        publish(public.clone(), &digest_of(b"p2")),
    )
    .await;
    assert_error(
        &theirs,
        StatusCode::BAD_REQUEST,
        "PUBLIC_HOSTNAME_NOT_CLAIMED",
    );
    sqlx::query("DELETE FROM fn_domains")
        .execute(&app.pool)
        .await
        .unwrap();
    let (status, _) = post(
        &r,
        "/api/function-domains",
        &t,
        json!({"hostname": "acme.com"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    sqlx::query(
        "INSERT INTO fn_routes (id, function_id, hostname, path_prefix, alias_prefixes) \
         VALUES ('fnr_t1', $1, 'api.acme.com', '/', '{}')",
    )
    .bind(other["id"].as_str().unwrap())
    .execute(&app.pool)
    .await
    .expect("route");
    let taken = post(
        &r,
        &format!("{path}/versions"),
        &t,
        publish(public, &digest_of(b"p3")),
    )
    .await;
    assert_error(&taken, StatusCode::CONFLICT, "PUBLIC_ROUTE_TAKEN");
    assert_eq!(
        taken.1["message"],
        "route 'api.acme.com/' is already taken by function 'billing.svc.warm'"
    );
}

// ── Signatures ──────────────────────────────────────────────────────────────

/// Java `PublishSignaturesTest` over HTTP: required by default, the owner's
/// policy decides the signer, and the signer is stored and shown.
#[tokio::test]
#[ignore = "requires Docker"]
async fn required_signatures_and_the_signer_policy() {
    let app = TestApp::setup().await;
    application(&app, "billing").await;
    let r = router(&app, required(), None, FunctionLimits::defaults());
    let t = anchor(&app).await;
    create_function(&r, &t, "billing", "signed", None).await;
    let path = "/api/functions/billing.svc.signed/versions";
    let digest = digest_of(SIGNED_ARTIFACT);
    let signed = |bundle: Option<&str>, d: &str| {
        let mut b = publish_body("oci://r/signed", d);
        if let Some(bundle) = bundle {
            b["signatureBundle"] = json!(bundle);
        }
        b
    };

    assert_error(
        &post(&r, path, &t, signed(None, &digest)).await,
        StatusCode::BAD_REQUEST,
        "SIGNATURE_REQUIRED",
    );
    let rejected = post(&r, path, &t, signed(Some(BUNDLE), &digest_of(b"not it"))).await;
    assert_error(&rejected, StatusCode::BAD_REQUEST, "SIGNATURE_REJECTED");
    assert_eq!(rejected.1["details"]["reason"], "DIGEST_MISMATCH");
    // No policy row permits nothing.
    let refused = post(&r, path, &t, signed(Some(BUNDLE), &digest)).await;
    assert_error(&refused, StatusCode::FORBIDDEN, "SIGNER_NOT_PERMITTED");
    assert_eq!(
        refused.1["message"],
        format!("signer not permitted to publish: issuer='{ISSUER}' subject='{SUBJECT}'")
    );
    // A policy for another runtime still refuses; the exact signer passes.
    let policy = |runtimes: &[&str]| json!({"signers": [{"issuer": ISSUER, "subject": SUBJECT, "runtimes": runtimes}]});
    let (status, _) = send(
        &r,
        Method::PUT,
        "/api/function-policies/platform",
        &t,
        Some(policy(&["jvm"])),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_error(
        &post(&r, path, &t, signed(Some(BUNDLE), &digest)).await,
        StatusCode::FORBIDDEN,
        "SIGNER_NOT_PERMITTED",
    );
    let (status, _) = send(
        &r,
        Method::PUT,
        "/api/function-policies/platform",
        &t,
        Some(policy(&["wasm"])),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, published) = post(&r, path, &t, signed(Some(BUNDLE), &digest)).await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
    assert_eq!(
        published["signer"],
        json!({"issuer": ISSUER, "subject": SUBJECT})
    );
    let (_, v) = get(&r, &format!("{path}/1"), &t).await;
    assert_eq!(v["signer"], json!({"issuer": ISSUER, "subject": SUBJECT}));
    let (bundle,): (Option<String>,) =
        sqlx::query_as("SELECT signature_bundle FROM fn_versions WHERE id = $1")
            .bind(published["id"].as_str().unwrap())
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        bundle.as_deref(),
        Some(BUNDLE),
        "the bundle is stored as sent"
    );
}

/// Signatures off (dev mode only): a bundle-less publish has no signer, and
/// a sent bundle is stored unverified.
#[tokio::test]
#[ignore = "requires Docker"]
async fn signatures_off_in_dev_mode() {
    assert!(
        Signatures::resolve(SignaturesMode::Off, false, "").is_err(),
        "off is refused outside dev mode"
    );
    let app = TestApp::setup().await;
    application(&app, "billing").await;
    let r = router(&app, dev_mode(), None, FunctionLimits::defaults());
    let t = anchor(&app).await;
    create_function(&r, &t, "billing", "dev", None).await;
    let path = "/api/functions/billing.svc.dev/versions";
    let (status, a) = post(&r, path, &t, publish_body("oci://r/a", &digest_of(b"a"))).await;
    assert_eq!(status, StatusCode::CREATED, "{a}");
    assert!(a.get("signer").is_none());
    let mut body = publish_body("oci://r/b", &digest_of(b"b"));
    body["signatureBundle"] = json!("not even a bundle");
    let (status, b) = post(&r, path, &t, body).await;
    assert_eq!(status, StatusCode::CREATED, "{b}");
    let (bundle, issuer): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT signature_bundle, signer_issuer FROM fn_versions WHERE id = $1")
            .bind(b["id"].as_str().unwrap())
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(bundle.as_deref(), Some("not even a bundle"));
    assert_eq!(issuer, None);
}

// ── The version number ──────────────────────────────────────────────────────

/// Java P11: concurrent publishes of different digests all succeed, with
/// consecutive numbers, never a unique-violation 500.
#[tokio::test]
#[ignore = "requires Docker"]
async fn concurrent_publishes_serialise_to_consecutive_versions() {
    let app = TestApp::setup().await;
    application(&app, "billing").await;
    let r = router(&app, dev_mode(), None, FunctionLimits::defaults());
    let t = anchor(&app).await;
    let f = create_function(&r, &t, "billing", "race", None).await;
    let publishes = (0..8).map(|i| {
        let (r, t) = (r.clone(), t.clone());
        async move {
            post(
                &r,
                "/api/functions/billing.svc.race/versions",
                &t,
                publish_body("oci://r/race", &digest_of(format!("race-{i}").as_bytes())),
            )
            .await
        }
    });
    let results = futures::future::join_all(publishes).await;
    let mut numbers: Vec<i64> = results
        .iter()
        .map(|(status, body)| {
            assert_eq!(*status, StatusCode::CREATED, "{body}");
            body["version"].as_i64().unwrap()
        })
        .collect();
    numbers.sort();
    assert_eq!(numbers, (1..=8).collect::<Vec<i64>>());
    assert_eq!(version_count(&app, f["id"].as_str().unwrap()).await, 8);
    assert_eq!(
        app.event_count_by_type("platform:function:version:published")
            .await,
        8
    );
}
