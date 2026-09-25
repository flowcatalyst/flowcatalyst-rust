//! fc-dev runs a function host beside the platform (plan H8; Java `fcdev
//! start`, `docs/spec/function-developer-surface.md` §1 in the Java repo).
//!
//! On by default; `--no-functions` or `FC_DEV_FUNCTIONS=false` turns it off.
//! When on, fc-dev:
//!
//! - points the platform's `FC_FN_POOL_URL` at the host's private listener,
//!   so the subscriptions and scheduled jobs promote wires reach it;
//! - provisions two OAuth `client_credentials` clients at start-up
//!   ([`bootstrap_identities`], Java's `FunctionDevBootstrap`): the host's
//!   own (`fcdev-fn-host`, role `platform:function-host`) and the `fn` CLI's
//!   (`fcdev-fn-cli`, roles `platform:function-publisher` and
//!   `platform:messaging-admin`), each a SERVICE principal with anchor
//!   scope, with a fresh secret every start;
//! - starts an in-process `fc-fnhost` ([`start_host`]: fc-fnhost-core's
//!   `FnHost` with the WASM runtime and both listeners) for pool `default`,
//!   authenticating as `fcdev-fn-host` against the local platform;
//! - writes the CLI's credentials to `<data dir>/fn-cli.json` (owner-only),
//!   so `fc-dev fn …` needs no flags, and removes it at shutdown;
//! - asks the host to reconcile right after every successful write under
//!   `/api/function*` ([`nudge_on_function_writes`]), so a publish is
//!   `READY` and a promote live in well under the 15 s poll. This is a
//!   dev-only shortcut; the host's own loop is unchanged.
//!
//! The host stops before the platform, so its last heartbeat (`DRAINING`)
//! still reaches it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::Router;
use rand::Rng;
use tracing::info;

use fc_fnhost_core::env::{EnvReader, HostEnv};
use fc_fnhost_core::host::{FnHost, Listener};
use fc_fnhost_core::listener::FnListener;
use fc_fnhost_core::loader::Loaders;
use fc_fnhost_core::wasm::{WasmLoader, WasmRuntime, WasmSettings};
use fc_platform::auth::oauth_entity::{GrantType, OAuthClient, OAuthClientType};
use fc_platform::repository::Repositories;
use fc_platform::service_account::entity::{AssignmentSource, RoleAssignment};
use fc_platform::shared::encryption_service::EncryptionService;
use fc_platform::{Principal, UserScope};

use crate::fn_cli::credentials::CliFile;

/// The one pool fc-dev's host serves: the manifest's default.
pub const POOL: &str = "default";
/// The host's fixed OAuth client id (Java `FunctionDevBootstrap.HOST_CLIENT_ID`).
pub const HOST_CLIENT_ID: &str = "fcdev-fn-host";
/// The `fn` CLI's fixed OAuth client id (Java `FunctionDevBootstrap.CLI_CLIENT_ID`).
pub const CLI_CLIENT_ID: &str = "fcdev-fn-cli";
/// The host id fc-dev's host reports, unless `FC_FN_HOST_ID` names one. A
/// stable id keeps restarts from leaving stale hosts in `fn_hosts`.
const HOST_ID: &str = "fc-dev";

/// The function host's start flags (Java `StartOptions`).
#[derive(clap::Args, Debug, Clone)]
pub struct FunctionArgs {
    /// Do not run a function host beside the platform (also
    /// `FC_DEV_FUNCTIONS=false`).
    #[arg(long)]
    pub no_functions: bool,

    /// Run a function host beside the platform.
    #[arg(
        long,
        env = "FC_DEV_FUNCTIONS",
        default_value_t = true,
        action = clap::ArgAction::Set,
        hide = true
    )]
    pub functions: bool,

    /// The function host's private listener port (`/functions/…`).
    #[arg(long, env = "FC_FN_PORT", default_value_t = 8090)]
    pub fn_port: u16,

    /// The function host's public listener port (claimed hostnames).
    #[arg(long, env = "FC_FN_PUBLIC_PORT", default_value_t = 8091)]
    pub fn_public_port: u16,

    /// The function host's `/health`, `/ready` and `/metrics` port.
    #[arg(long, env = "FC_FN_METRICS_PORT", default_value_t = 9091)]
    pub fn_metrics_port: u16,
}

impl FunctionArgs {
    pub fn enabled(&self) -> bool {
        self.functions && !self.no_functions
    }

    /// The URL the platform wires subscriptions and schedules to.
    pub fn host_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.fn_port)
    }

    pub fn public_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.fn_public_port)
    }
}

/// fc-dev's data directory: `<user cache dir>/flowcatalyst-dev`, beside
/// the embedded Postgres data and the JWT keys.
pub fn data_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("flowcatalyst-dev")
}

/// Where fc-dev writes the `fn` CLI's credentials.
pub fn cli_file_path() -> PathBuf {
    data_dir().join("fn-cli.json")
}

/// The platform's function settings, each a default an operator's own
/// value overrides (Java `StartCommand.java:391-413`): dev mode on,
/// uploaded artifacts kept under `data_dir`, signatures off (which the
/// platform allows only in dev mode) and, with the host on, the pool URL
/// pointing at it. Call before the platform's routes are built: it reads
/// them once.
pub fn apply_platform_defaults(args: &FunctionArgs, data_dir: &Path) {
    set_default("FLOWCATALYST_DEV_MODE", "true");
    set_default(
        "FC_FN_ARTIFACT_STORE",
        &file_uri(&data_dir.join("fn-artifacts")),
    );
    set_default("FC_FN_SIGNATURES", "off");
    if args.enabled() {
        set_default("FC_FN_POOL_URL", &args.host_url());
    }
}

fn set_default(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        std::env::set_var(key, value);
    }
}

/// `dir` as a `file://` URI, percent-encoded as Java's `Path#toUri`
/// (macOS's cache path can hold a space). A Windows path becomes
/// `file:///C:/…`.
fn file_uri(dir: &Path) -> String {
    let path = dir
        .to_string_lossy()
        .replace('\\', "/")
        .replace('%', "%25")
        .replace(' ', "%20");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

/// One client's freshly minted credentials. The plaintext secret is never
/// stored, only its verify-only hash.
#[derive(Clone)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct DevIdentities {
    pub host: Credentials,
    pub cli: Credentials,
}

/// Upserts the host's and the CLI's clients, principals and roles, and
/// returns fresh secrets for both. Meant for every start, not just the
/// first.
///
/// Direct repository writes, no unit of work and no events, as Java's
/// `FunctionDevBootstrap` and fc-dev's MCP bootstrap: this is dev-only
/// start-up provisioning of fc-dev's own infrastructure, and a secret
/// rotated on every start would otherwise emit an event per start.
pub async fn bootstrap_identities(repos: &Repositories) -> Result<DevIdentities> {
    let encryption = EncryptionService::from_env()
        .context("FLOWCATALYST_APP_KEY is not set: cannot store the function clients' secrets")?;
    let host = upsert_client(
        repos,
        &encryption,
        HOST_CLIENT_ID,
        "fcdev function host",
        &["platform:function-host"],
    )
    .await?;
    let cli = upsert_client(
        repos,
        &encryption,
        CLI_CLIENT_ID,
        "fcdev fn CLI",
        &["platform:function-publisher", "platform:messaging-admin"],
    )
    .await?;
    Ok(DevIdentities { host, cli })
}

async fn upsert_client(
    repos: &Repositories,
    encryption: &EncryptionService,
    client_id: &str,
    name: &str,
    roles: &[&str],
) -> Result<Credentials> {
    let secret = generate_secret();
    let secret_ref = encryption.hash_secret(&secret);

    let existing = repos
        .oauth_client_repo
        .find_by_client_id(client_id)
        .await
        .map_err(|e| anyhow!("looking up OAuth client {client_id}: {e:?}"))?;
    let principal = match existing
        .as_ref()
        .and_then(|c| c.service_account_principal_id.as_deref())
    {
        Some(id) => repos
            .principal_repo
            .find_by_id(id)
            .await
            .map_err(|e| anyhow!("looking up the principal of {client_id}: {e:?}"))?,
        None => None,
    };

    let principal = match principal {
        Some(mut principal) => {
            let mut changed = false;
            for role in roles {
                if !principal.roles.iter().any(|r| r.role == *role) {
                    principal.roles.push(RoleAssignment::with_source(
                        *role,
                        AssignmentSource::Bootstrap,
                    ));
                    changed = true;
                }
            }
            if !principal.active || !principal.all_applications || !principal.scope.is_anchor() {
                principal.active = true;
                principal.all_applications = true;
                principal.scope = UserScope::Anchor;
                changed = true;
            }
            if changed {
                repos
                    .principal_repo
                    .update(&principal)
                    .await
                    .map_err(|e| anyhow!("updating the principal of {client_id}: {e:?}"))?;
            }
            principal
        }
        None => {
            // No service-account aggregate behind it, as Java's
            // FunctionDevBootstrap: the principal and its client are all
            // a token needs.
            let mut principal = Principal::new_service("", name, UserScope::Anchor);
            principal.service_account_id = None;
            principal.all_applications = true;
            principal.roles = roles
                .iter()
                .map(|r| RoleAssignment::with_source(*r, AssignmentSource::Bootstrap))
                .collect();
            repos
                .principal_repo
                .insert(&principal)
                .await
                .map_err(|e| anyhow!("inserting the principal of {client_id}: {e:?}"))?;
            principal
        }
    };

    match existing {
        Some(mut client) => {
            client.set_secret_ref(secret_ref);
            client.active = true;
            client.client_type = OAuthClientType::Confidential;
            client.service_account_principal_id = Some(principal.id.clone());
            if !client.grant_types.contains(&GrantType::ClientCredentials) {
                client.grant_types.push(GrantType::ClientCredentials);
            }
            repos
                .oauth_client_repo
                .update(&client)
                .await
                .map_err(|e| anyhow!("updating OAuth client {client_id}: {e:?}"))?;
        }
        None => {
            let client = OAuthClient::confidential(client_id, name)
                .with_service_account(principal.id.clone())
                .with_secret_ref(secret_ref);
            repos
                .oauth_client_repo
                .insert(&client)
                .await
                .map_err(|e| anyhow!("inserting OAuth client {client_id}: {e:?}"))?;
        }
    }

    Ok(Credentials {
        client_id: client_id.to_string(),
        client_secret: secret,
    })
}

/// 32 random bytes, URL-safe base64.
fn generate_secret() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// The host's environment (Java `FnHostLauncher.Settings`): built from
/// fc-dev's own settings, never read from fc-dev's process environment
/// wholesale, because fc-dev and the host share variable names
/// (`FC_METRICS_PORT`, and `FC_FN_PORT`, which fc-dev reads as its own
/// `--fn-port`). The host's tuning variables pass through.
pub fn host_env_pairs(
    args: &FunctionArgs,
    platform_url: &str,
    credentials: &Credentials,
    cache_dir: &Path,
) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = [
        ("FC_FN_POOL", POOL.to_string()),
        ("FC_FN_PLATFORM_URL", platform_url.to_string()),
        ("FC_FN_CLIENT_ID", credentials.client_id.clone()),
        ("FC_FN_CLIENT_SECRET", credentials.client_secret.clone()),
        ("FC_FN_PORT", args.fn_port.to_string()),
        ("FC_FN_PUBLIC_PORT", args.fn_public_port.to_string()),
        ("FC_METRICS_PORT", args.fn_metrics_port.to_string()),
        ("FC_FN_CACHE_DIR", cache_dir.to_string_lossy().into_owned()),
        ("FC_FN_HOST_ID", HOST_ID.to_string()),
        (
            "FC_FN_SIGNATURES",
            std::env::var("FC_FN_SIGNATURES").unwrap_or_else(|_| "off".to_string()),
        ),
        (
            "FLOWCATALYST_DEV_MODE",
            std::env::var("FLOWCATALYST_DEV_MODE").unwrap_or_else(|_| "true".to_string()),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    for key in [
        "FC_FN_HOST_ID",
        "FC_FN_MAX_LOADED",
        "FC_FN_MAX_CONCURRENCY",
        "FC_FN_MAX_EXECUTING",
        "FC_FN_MAX_DB_POOLS",
        "FC_FN_TRUSTED_PROXIES",
        "FC_FN_TRUST_ROOT",
        "FC_DRAIN_TIMEOUT_SECONDS",
    ] {
        if let Ok(value) = std::env::var(key) {
            pairs.retain(|(k, _)| k != key);
            pairs.push((key.to_string(), value));
        }
    }
    pairs
}

/// Starts the in-process host: the WASM runtime, the private and public
/// listeners and the observability listener, then the first reconcile
/// against `platform_url` (which must already be accepting connections).
pub async fn start_host(
    args: &FunctionArgs,
    platform_url: &str,
    credentials: &Credentials,
    cache_dir: &Path,
) -> Result<FnHost> {
    let env = HostEnv::load(&EnvReader::from_pairs(host_env_pairs(
        args,
        platform_url,
        credentials,
        cache_dir,
    )))
    .map_err(|e| anyhow!("{e}"))?;
    let wasm = WasmRuntime::new(WasmSettings::from_env(&env))
        .map_err(|e| anyhow!("cannot start the WASM runtime: {e}"))?;
    let loaders = Loaders::none().with("wasm", Arc::new(WasmLoader::new(wasm)));
    let listener: Arc<dyn Listener> = Arc::new(FnListener::from_env(&env));
    let mut host = FnHost::new(env, loaders, Some(listener))
        .map_err(|e| anyhow!("cannot create the function cache directory: {e}"))?;
    if let Err(e) = host.start().await {
        host.close().await;
        return Err(anyhow!("the function host did not start: {e}"));
    }
    info!(
        pool = POOL,
        port = host.port(),
        public_port = host.public_port(),
        metrics_port = host.metrics_port(),
        "Function host started"
    );
    Ok(host)
}

/// The running host, shared between the nudge middleware and shutdown.
/// Empty until the host has started, and again once it is closed.
#[derive(Clone, Default)]
pub struct HostSlot(Arc<Mutex<Option<FnHost>>>);

impl HostSlot {
    pub fn set(&self, host: FnHost) {
        *self.0.lock().expect("host slot") = Some(host);
    }

    pub fn is_running(&self) -> bool {
        self.0.lock().expect("host slot").is_some()
    }

    /// Asks the host's reconcile loop for a cycle now (coalesced).
    pub fn nudge(&self) {
        if let Some(host) = self.0.lock().expect("host slot").as_ref() {
            host.trigger_reconcile();
        }
    }

    /// Drains and stops the host. Idempotent.
    pub async fn close(&self) {
        let host = self.0.lock().expect("host slot").take();
        if let Some(mut host) = host {
            host.close().await;
        }
    }
}

/// After a successful write under `/api/function*` (publish, promote,
/// config, secrets, delete, …), asks the host to reconcile at once instead
/// of at its next 15 s poll.
pub fn nudge_on_function_writes(router: Router, slot: HostSlot) -> Router {
    router.layer(axum::middleware::from_fn(
        move |req: Request, next: Next| {
            let slot = slot.clone();
            async move {
                let write = req.method() != Method::GET
                    && req.method() != Method::HEAD
                    && req.uri().path().starts_with("/api/function");
                let response = next.run(req).await;
                if write && response.status().is_success() {
                    slot.nudge();
                }
                response
            }
        },
    ))
}

/// Writes `fn-cli.json` owner-only (Java `OwnerOnlyFile`).
pub fn write_cli_file(path: &Path, file: &CliFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(file)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    std::io::Write::write_all(&mut out, &bytes)?;
    Ok(())
}

/// The `fn-cli.json` fc-dev writes for its own API port and host.
pub fn cli_file(args: &FunctionArgs, api_port: u16, cli: &Credentials) -> CliFile {
    CliFile {
        platform_url: Some(format!("http://localhost:{api_port}")),
        client_id: Some(cli.client_id.clone()),
        client_secret: Some(cli.client_secret.clone()),
        host_url: Some(args.host_url()),
        public_url: Some(args.public_url()),
    }
}

/// A free loopback port (bound, then released). Only for tests.
#[cfg(test)]
pub fn free_port() -> u16 {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free port")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> FunctionArgs {
        FunctionArgs {
            no_functions: false,
            functions: true,
            fn_port: 8090,
            fn_public_port: 8091,
            fn_metrics_port: 9091,
        }
    }

    #[test]
    fn functions_are_on_unless_either_switch_turns_them_off() {
        assert!(args().enabled());
        assert!(!FunctionArgs {
            no_functions: true,
            ..args()
        }
        .enabled());
        assert!(!FunctionArgs {
            functions: false,
            ..args()
        }
        .enabled());
    }

    #[test]
    fn the_host_gets_its_own_ports_not_fc_devs() {
        let creds = Credentials {
            client_id: HOST_CLIENT_ID.into(),
            client_secret: "s".into(),
        };
        let pairs = host_env_pairs(
            &args(),
            "http://localhost:8080",
            &creds,
            Path::new("/tmp/fc"),
        );
        let get = |k: &str| {
            pairs
                .iter()
                .rev()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("FC_FN_PORT").as_deref(), Some("8090"));
        assert_eq!(get("FC_FN_PUBLIC_PORT").as_deref(), Some("8091"));
        assert_eq!(get("FC_METRICS_PORT").as_deref(), Some("9091"));
        assert_eq!(get("FC_FN_POOL").as_deref(), Some("default"));
        assert_eq!(get("FC_FN_CLIENT_ID").as_deref(), Some(HOST_CLIENT_ID));
        assert_eq!(get("FC_FN_CACHE_DIR").as_deref(), Some("/tmp/fc"));
        let env = HostEnv::load(&EnvReader::from_pairs(pairs.clone())).expect("a valid host env");
        assert_eq!(env.port, 8090);
        assert_eq!(env.metrics_port, 9091);
    }

    #[test]
    fn a_file_uri_is_percent_encoded() {
        assert_eq!(
            file_uri(Path::new(
                "/Users/a b/Library/Caches/flowcatalyst-dev/fn-artifacts"
            )),
            "file:///Users/a%20b/Library/Caches/flowcatalyst-dev/fn-artifacts"
        );
    }
}

#[cfg(all(test, feature = "embedded-db"))]
#[path = "functions_e2e_test.rs"]
mod e2e_test;
