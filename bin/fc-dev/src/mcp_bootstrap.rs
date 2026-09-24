//! Auto-provision an OAuth client + service-account principal so that
//! `./fc-dev mcp` works with zero env vars on first start.
//!
//! Runs once per fc-dev startup, after migrations + role seeding, before
//! HTTP serving. Gated on `FC_DEV_MODE` so it can never touch a production
//! deployment's IAM tables.
//!
//! ## What it does
//!
//!   1. Looks for an OAuth client with `client_id = "flowcatalyst-mcp-local"`.
//!   2. If absent:
//!      - Creates a Service principal with the `platform:super-admin` role
//!        (full ADMIN_ALL). Acceptable here because this client only ever
//!        authenticates against a local fc-dev instance, and the user opted
//!        into full admin specifically because of that.
//!      - Generates a fresh client_secret (UUIDv4), encrypts it with
//!        `EncryptionService::from_env()` (uses `FLOWCATALYST_APP_KEY`),
//!        and stores the encrypted form as `client_secret_ref`.
//!      - Persists the principal + OAuth client.
//!      - Writes the **plaintext** client_id + client_secret to
//!        `~/.cache/flowcatalyst-dev/mcp-credentials.json` with mode 0600.
//!   3. If the client already exists, leaves it alone. The credentials file
//!      may or may not still be present — `fc-dev mcp` handles that.
//!
//! The bootstrap is intentionally driven by the *server* side (this binary)
//! rather than by `fc-dev mcp`, so the MCP subcommand never needs database
//! access. The credentials file is the only handoff.

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{info, warn};
use uuid::Uuid;

use fc_common::tsid::{self, EntityType};
use fc_platform::auth::oauth_entity::{GrantType, OAuthClient};
use fc_platform::repository::Repositories;
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform::shared::encryption_service::EncryptionService;
use fc_platform::{Principal, UserScope};

/// Stable identifiers — the local MCP client always uses these names.
/// `client_id` is part of the OAuth public surface; `principal_name` is
/// what shows up in audit logs.
const CLIENT_ID: &str = "flowcatalyst-mcp-local";
const CLIENT_NAME: &str = "FlowCatalyst MCP (local dev)";
const PRINCIPAL_NAME: &str = "fc-mcp local";
const SUPER_ADMIN_ROLE: &str = "platform:super-admin";

/// Provision (idempotently) the MCP-local OAuth client and its
/// underlying service principal. Returns Ok even if everything was already
/// in place; never blocks fc-dev startup on its own errors.
pub async fn run(repos: &Repositories) -> Result<()> {
    // Hard gate: production deployments must never auto-provision an
    // anchor-admin OAuth client into their IAM tables.
    if std::env::var("FC_DEV_MODE").as_deref() != Ok("true") {
        return Ok(());
    }

    // Existing client? Nothing to do for the DB side; the credentials
    // file is fc-dev mcp's responsibility to find.
    if repos
        .oauth_client_repo
        .find_by_client_id(CLIENT_ID)
        .await
        .with_context(|| "looking up MCP OAuth client")?
        .is_some()
    {
        // Don't try to recreate the credentials file — we'd need to read
        // back the plaintext secret, which we no longer have (only the
        // encrypted form is in DB). If the user blew it away, they'll
        // need to recreate via `fc-dev reset-mcp` (not yet implemented)
        // or by wiping the data dir.
        info!(
            client_id = %CLIENT_ID,
            "MCP OAuth client already provisioned; leaving DB + credentials file as-is",
        );
        if !credentials_path().map(|p| p.exists()).unwrap_or(false) {
            warn!(
                "MCP OAuth client exists in DB but credentials file is missing — \
                 `fc-dev mcp` will not work until the file is restored or fc-dev's \
                 data dir is reset (`--reset-db`)",
            );
        }
        return Ok(());
    }

    // Need to provision. Encryption service is required to round-trip the
    // secret through DB storage. `FLOWCATALYST_APP_KEY` is always set in
    // fc-dev's `main` so this should be reachable.
    let encryption = EncryptionService::from_env()
        .context("FLOWCATALYST_APP_KEY not set — cannot encrypt MCP client secret")?;

    // 1. Service principal with super-admin role.
    //
    // `Principal::service_account_id` is a VARCHAR(17) column expecting a
    // TSID, NOT the OAuth `client_id`. We generate a proper TSID here and
    // keep `CLIENT_ID` (the human-readable OAuth public identifier) only
    // on the OAuthClient row.
    let service_account_id = tsid::generate(EntityType::ServiceAccount);
    let mut principal = Principal::new_service(service_account_id, PRINCIPAL_NAME);
    principal.scope = UserScope::Anchor; // matches the role's reach
    principal.roles = vec![RoleAssignment::new(SUPER_ADMIN_ROLE)];

    // The `{:?}` formatter on the source error shows the full sqlx error
    // message (chain). Plain `Context::context` would mask it as just
    // "inserting MCP service principal", which we hit before.
    repos
        .principal_repo
        .insert(&principal)
        .await
        .map_err(|e| anyhow::anyhow!("inserting MCP service principal: {e:?}"))?;

    // 2. Fresh plaintext secret. UUIDv4 has 122 bits of entropy — plenty
    // for a local-only credential.
    let secret_plaintext = Uuid::new_v4().simple().to_string();
    let secret_encrypted = encryption
        .encrypt(&secret_plaintext)
        .map_err(|e| anyhow::anyhow!("encrypting MCP client secret: {e}"))?;

    // 3. OAuth client.
    let mut oauth_client = OAuthClient::confidential(CLIENT_ID, CLIENT_NAME)
        .with_service_account(principal.id.clone())
        .with_secret_ref(secret_encrypted);
    // `confidential()` defaults to ClientCredentials only — explicit for clarity.
    if !oauth_client
        .grant_types
        .contains(&GrantType::ClientCredentials)
    {
        oauth_client.grant_types.push(GrantType::ClientCredentials);
    }

    repos
        .oauth_client_repo
        .insert(&oauth_client)
        .await
        .map_err(|e| anyhow::anyhow!("inserting MCP OAuth client: {e:?}"))?;

    // 4. Persist plaintext creds for `fc-dev mcp` to find.
    write_credentials_file(CLIENT_ID, &secret_plaintext)?;

    info!(
        client_id = %CLIENT_ID,
        principal_id = %principal.id,
        path = %credentials_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<none>".to_string()),
        "Provisioned MCP OAuth client + credentials file for `fc-dev mcp`",
    );
    Ok(())
}

/// `~/.cache/flowcatalyst-dev/mcp-credentials.json` — shared between
/// fc-dev (writer) and fc-dev mcp (reader). Returns `None` only on
/// platforms where `dirs::cache_dir()` is unreachable (very rare).
pub fn credentials_path() -> Option<PathBuf> {
    Some(
        dirs::cache_dir()?
            .join("flowcatalyst-dev")
            .join("mcp-credentials.json"),
    )
}

fn write_credentials_file(client_id: &str, client_secret: &str) -> Result<()> {
    let path =
        credentials_path().context("dirs::cache_dir() returned None — cannot persist creds")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let payload = serde_json::json!({
        "client_id": client_id,
        "client_secret": client_secret,
        // base_url is constant for the local install; written here so
        // fc-dev mcp doesn't need its own knowledge of the API port.
        "base_url": format!(
            "http://localhost:{}",
            std::env::var("FC_API_PORT").unwrap_or_else(|_| "8080".to_string()),
        ),
    });
    let bytes = serde_json::to_vec_pretty(&payload)?;

    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;

    // chmod 0600 on Unix — the file holds a long-lived (for this install)
    // OAuth client_secret. Windows ACLs default to user-only for files
    // under %LOCALAPPDATA% so no extra hardening needed there.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }

    Ok(())
}
