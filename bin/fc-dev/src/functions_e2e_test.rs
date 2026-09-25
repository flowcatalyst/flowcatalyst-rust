//! fc-dev's function loop, end to end (plan H8): an embedded Postgres, the
//! platform wired as `fc-dev` wires it, the in-process function host with
//! the identities fc-dev provisions, and the `fc-dev fn` CLI driving it
//! over HTTP with the `fn-cli.json` fc-dev writes.
//!
//! The function is `examples/function-hello-rust` (its committed
//! component, `crates/fc-fnhost-core/tests/fixtures/wasm/hello.wasm`, and
//! its own `manifest.json`):
//!
//! 1. `fn config set` creates the function from the manifest and sets its
//!    config; `fn secret set` its secret (from a file, never an argument).
//! 2. `fn publish` uploads the component and publishes version 1.
//! 3. `fn deploy` of the same component gets version 1 back (a republish
//!    of the same digest and manifest is a `200` no-op), waits for `READY`
//!    (the host registers the candidate) and promotes it live; a second
//!    `fn deploy` is a no-op (`changed: false`).
//! 4. The promote wired the subscription to the host's URL, and
//!    `fn invoke` reaches the function, unversioned and versioned.
//!
//! Needs no Docker: the Postgres is the one bundled into fc-dev, in a
//! temporary data directory on a free port. It sets process environment
//! (the platform reads its function settings from it once), which no
//! other test in this binary reads.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use postgresql_embedded::PostgreSQL;
use serde_json::Value;

use fc_platform::api::middleware::{AppState, AuthLayer};
use fc_platform::repository::{Repositories, RoleRepository};
use fc_platform::shared::server_setup::{
    build_platform_routes, init_auth_services, AuthInitConfig, PlatformRoutesConfig,
};
use fc_platform::usecase::PgUnitOfWork;

use super::*;
use crate::fn_cli::{self, Io};

const ADDRESS: &str = "shop.fulfilment.book-shipment";

fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// `fc-dev fn <args>`, with no process environment: credentials come only
/// from the flags and the credentials file.
async fn fc_dev_fn(args: &[&str]) -> (i32, String, String) {
    let parsed = fn_cli::parse(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
    let (mut out, mut err, mut stdin) = (Vec::new(), Vec::new(), std::io::empty());
    let env = |_: &str| None;
    let code = fn_cli::run_with(
        &parsed,
        &env,
        &mut Io {
            out: &mut out,
            err: &mut err,
            stdin: &mut stdin,
        },
    )
    .await;
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// `fc-dev init --yes --code shop …` against `database_url`.
async fn fc_dev_init(database_url: &str, root: &Path) {
    use clap::Parser;

    #[derive(Parser)]
    struct Init {
        #[command(flatten)]
        args: crate::init::InitArgs,
    }

    let mut init = Init::try_parse_from([
        "init",
        "--yes",
        "--root",
        root.to_str().unwrap(),
        "--admin-email",
        "admin@shop.test",
        "--admin-password",
        "Correct-Horse-Battery-9",
        "--code",
        "shop",
        "--name",
        "Shop",
        "--database-url",
        database_url,
    ])
    .unwrap()
    .args;
    init.embedded_db = false;
    crate::init::run(init).await.expect("fc-dev init");
}

/// The platform as `main` builds it, on `api_port`.
async fn start_platform(database_url: &str, api_port: u16, slot: HostSlot) -> Repositories {
    let pool = fc_platform::shared::database::create_pool(database_url)
        .await
        .expect("pool");
    fc_platform::shared::database::run_migrations(
        &pool,
        fc_platform::shared::database::MigrationProfile::Embedded,
    )
    .await
    .expect("migrations");
    fc_platform::shared::database::seed_builtin_roles(&pool)
        .await
        .expect("built-in roles");
    fc_platform::shared::database::seed_platform_application(&pool)
        .await
        .expect("platform application");
    fc_platform::service::RoleSyncService::new(Arc::new(RoleRepository::new(&pool)))
        .sync_code_defined_roles()
        .await
        .expect("role sync");

    let repos = Repositories::new(&pool);
    let unit_of_work = Arc::new(PgUnitOfWork::new(pool.clone()));
    let platform_url = format!("http://localhost:{api_port}");
    let auth_services = init_auth_services(
        &repos,
        AuthInitConfig {
            issuer: platform_url.clone(),
            private_key_path: None,
            public_key_path: None,
            previous_public_key: None,
            access_token_expiry_secs: 3600,
            session_token_expiry_secs: 86400,
            refresh_token_expiry_secs: 86400,
        },
    )
    .expect("auth services");
    let platform_application_id = repos
        .application_repo
        .find_by_code("platform")
        .await
        .unwrap()
        .expect("platform application")
        .id;
    let routes = build_platform_routes(
        &repos,
        &auth_services,
        &unit_of_work,
        PlatformRoutesConfig {
            rate_limit_store: Arc::new(fc_platform::shared::rate_limit_store::NoopRateLimitStore),
            rate_limit_policies: Arc::new(
                fc_platform::shared::rate_limit_store::RateLimitPolicies::from_env(),
            ),
            session_cookie_secure: false,
            session_cookie_same_site: PlatformRoutesConfig::DEFAULT_SAME_SITE.to_string(),
            session_token_expiry_secs: PlatformRoutesConfig::DEFAULT_SESSION_EXPIRY_SECS,
            static_dir: None,
            oidc_login_external_base_url: None,
            well_known_external_base_url: platform_url.clone(),
            password_reset_external_base_url: platform_url,
        },
        platform_application_id,
    );
    let (router, _openapi) = routes.build();
    let router = router.layer(AuthLayer::new(AppState {
        auth_service: auth_services.auth.clone(),
        authz_service: auth_services.authz.clone(),
    }));
    let router = nudge_on_function_writes(router, slot);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", api_port))
        .await
        .expect("the API port");
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    repos
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fc_dev_publishes_deploys_and_invokes_a_function_on_its_own_host() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    std::env::set_var(
        "FLOWCATALYST_APP_KEY",
        "MpU3dI07kjZmZGROrElYfDXQgab30e3wr0KTnxQbePg=",
    );
    // Pinned so a developer's own settings cannot leak in.
    std::env::set_var(
        "FC_FN_ARTIFACT_STORE",
        format!("file://{}", data_dir.join("fn-artifacts").display()),
    );
    std::env::set_var("FC_FN_SIGNATURES", "off");
    std::env::set_var("FLOWCATALYST_DEV_MODE", "true");
    std::env::remove_var("FC_FN_POOL_URL");

    let api_port = free_port();
    let args = FunctionArgs {
        no_functions: false,
        functions: true,
        fn_port: free_port(),
        fn_public_port: free_port(),
        fn_metrics_port: free_port(),
    };
    apply_platform_defaults(&args, &data_dir);
    assert_eq!(
        std::env::var("FC_FN_POOL_URL").unwrap(),
        format!("http://127.0.0.1:{}", args.fn_port)
    );

    let mut postgres = PostgreSQL::default();
    postgres.setup().await.expect("embedded Postgres setup");
    postgres.start().await.expect("embedded Postgres start");
    postgres.create_database("flowcatalyst").await.unwrap();
    let database_url = postgres.settings().url("flowcatalyst");

    let slot = HostSlot::default();
    let repos = start_platform(&database_url, api_port, slot.clone()).await;
    let platform_url = format!("http://localhost:{api_port}");

    // Start-up provisioning is idempotent: a second run rotates the secrets.
    let first = bootstrap_identities(&repos).await.expect("identities");
    let identities = bootstrap_identities(&repos)
        .await
        .expect("identities again");
    assert_ne!(first.cli.client_secret, identities.cli.client_secret);
    assert_eq!(identities.host.client_id, HOST_CLIENT_ID);

    let host = start_host(
        &args,
        &platform_url,
        &identities.host,
        &data_dir.join("fn-cache"),
    )
    .await
    .expect("the function host starts");
    slot.set(host);
    let cli_file_path = data_dir.join("fn-cli.json");
    write_cli_file(&cli_file_path, &cli_file(&args, api_port, &identities.cli)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cli_file_path)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let creds = cli_file_path.to_str().unwrap().to_string();

    // The function's application, as a developer makes it (`fc-dev init`,
    // whose service account carries the signing secret a function with
    // subscriptions needs), and the event type its subscription names.
    fc_dev_init(&database_url, tmp.path()).await;
    sqlx::query(
        "INSERT INTO msg_event_types (id, code, name, status, source, client_scoped, \
         application, subdomain, aggregate, created_at, updated_at) \
         VALUES ($1, 'shop:orders:order:placed', 'Order placed', 'CURRENT', 'API', false, \
         'shop', 'orders', 'order', NOW(), NOW())",
    )
    .bind(fc_platform::shared::tsid::generate_untyped())
    .execute(&repos.pool)
    .await
    .unwrap();

    let manifest = repo_path("examples/function-hello-rust/manifest.json");
    let manifest = manifest.to_str().unwrap();
    let wasm = repo_path("crates/fc-fnhost-core/tests/fixtures/wasm/hello.wasm");
    let wasm = wasm.to_str().unwrap();
    let secret_file = tmp.path().join("carrier-key");
    std::fs::write(&secret_file, "k-carrier-1\n").unwrap();

    // ── 1. settings; the first creates the function ──────────────────────
    let (code, out, err) = fc_dev_fn(&[
        "--credentials-file",
        &creds,
        "config",
        "set",
        ADDRESS,
        "CARRIER_API_URL=https://api.carrier.example",
        "CARRIER_ACCOUNT=acct-7",
        "--manifest",
        manifest,
    ])
    .await;
    assert_eq!(code, 0, "{out}{err}");
    let (code, out, err) = fc_dev_fn(&[
        "--credentials-file",
        &creds,
        "secret",
        "set",
        ADDRESS,
        "CARRIER_API_KEY",
        "--from-file",
        secret_file.to_str().unwrap(),
    ])
    .await;
    assert_eq!(code, 0, "{out}{err}");
    assert!(!out.contains("k-carrier-1") && !err.contains("k-carrier-1"));

    // ── 2. publish ───────────────────────────────────────────────────────
    let (code, out, err) = fc_dev_fn(&[
        "--credentials-file",
        &creds,
        "publish",
        wasm,
        ADDRESS,
        "--manifest",
        manifest,
    ])
    .await;
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.starts_with(&format!("published {ADDRESS} version 1 (sha256:")),
        "{out}"
    );

    // ── 3. deploy: the same bytes promote version 1; twice is a no-op ────
    let started = Instant::now();
    let (code, out, err) = fc_dev_fn(&[
        "--credentials-file",
        &creds,
        "deploy",
        wasm,
        ADDRESS,
        "--manifest",
        manifest,
        "--wait",
        "90s",
    ])
    .await;
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        out.trim(),
        format!("{ADDRESS}: version 1 deployed and live")
    );
    // Nudged, the host registers the candidate well inside its 15 s poll.
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );
    let (code, out, err) = fc_dev_fn(&[
        "--credentials-file",
        &creds,
        "--output",
        "json",
        "deploy",
        wasm,
        ADDRESS,
        "--manifest",
        manifest,
    ])
    .await;
    assert_eq!(code, 0, "{out}{err}");
    let again: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(again["version"], 1, "{again}");

    // ── 4. wired to fc-dev's host, and invocable through it ──────────────
    let (target,): (String,) =
        sqlx::query_as("SELECT target FROM msg_subscriptions WHERE source = 'FUNCTION'")
            .fetch_one(&repos.pool)
            .await
            .unwrap();
    assert_eq!(
        target,
        format!(
            "http://127.0.0.1:{}/functions/{ADDRESS}/events/order-placed",
            args.fn_port
        )
    );

    let deadline = Instant::now() + Duration::from_secs(120);
    let (code, out, err) = loop {
        let result = fc_dev_fn(&[
            "--credentials-file",
            &creds,
            "--output",
            "json",
            "invoke",
            ADDRESS,
            "--path",
            "/healthz",
        ])
        .await;
        if result.0 == 0 || Instant::now() > deadline {
            break result;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    assert_eq!(code, 0, "{out}{err}");
    let answer: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(answer["status"], 200, "{answer}");
    assert_eq!(answer["body"], r#"{"ok":true}"#, "{answer}");

    // A versioned call carries the CLI's bearer token, and the host accepts
    // it as a platform token (no 401). It is still refused (403
    // PERMISSION_REQUIRED): the host reads permissions from the `scope`
    // claim, as Java's tokens carry them, but the Rust platform's `scope`
    // is the principal's tier (`ANCHOR`). A platform/host mismatch outside
    // fc-dev; see docs/function-runner-plan.md, H8.
    let (_, out, err) = fc_dev_fn(&[
        "--credentials-file",
        &creds,
        "invoke",
        &format!("{ADDRESS}:1"),
        "--path",
        "/healthz",
    ])
    .await;
    assert!(out.starts_with("HTTP "), "{out}{err}");
    assert!(!out.starts_with("HTTP 401"), "{out}{err}");

    // A two-part address is a usage error.
    let (code, _, err) = fc_dev_fn(&["--credentials-file", &creds, "invoke", "shop.book"]).await;
    assert_eq!(code, 2, "{err}");

    tokio::time::timeout(Duration::from_secs(30), slot.close())
        .await
        .expect("the host shuts down");
    assert!(!slot.is_running());
    postgres.stop().await.unwrap();
}
