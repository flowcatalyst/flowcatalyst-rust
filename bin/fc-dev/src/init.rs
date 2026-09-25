//! `fc-dev init` — interactive bootstrap of a fresh local app.
//!
//! Replaces the per-SDK init commands (the TS `flowcatalyst init` and
//! Laravel `flowcatalyst:init` artisan command). fc-dev is the natural
//! home for this:
//!
//!   - It already owns direct DB access (runs migrations, seeds built-in
//!     roles, etc.). No HTTP login round-trip needed.
//!   - It IS localhost by definition — no remote-URL guard.
//!   - One implementation works for any SDK (the only output is a
//!     `.env` plus rows in the DB).
//!
//! What it does:
//!
//!   1. Ensure built-in roles + platform application + internal IDP +
//!      email-domain mapping for the admin's domain exist. Idempotent.
//!   2. Create an anchor admin (`platform:super-admin`) if no anchor
//!      user exists yet. Prompts for email + password unless flags are
//!      passed.
//!   3. Resolve (or create) a "Default Client" so every consumer app
//!      starts client-scoped — FlowCatalyst is multi-client by design.
//!   4. Create the Application (prompted code / name / type / desc).
//!   5. Mint the service account: a USER `Principal` for the SA, a
//!      `ServiceAccount` row, attach to the Application, then create a
//!      CONFIDENTIAL `OAuthClient` with `grant_types: ["client_credentials"]`
//!      linked to the SA. Same shape the platform's
//!      `POST /api/applications/{id}/provision-service-account` produces.
//!   6. Write `{root}/.env` with FLOWCATALYST_BASE_URL, _APP_CODE,
//!      _CLIENT_ID, _CLIENT_SECRET — in-place update for existing keys,
//!      appended under a comment for new ones.
//!
//! All writes go directly to the repositories (no UoW, no events). This
//! is platform-infrastructure bootstrap — exactly the exception class
//! `crate::shared::bootstrap_admin` is in (see CLAUDE.md's "Platform
//! Infrastructure Processing" section).

use anyhow::{Context, Result};
use chrono::Utc;
use std::fs;
use std::io::{stdin, stdout, BufRead, Write};
use std::path::PathBuf;
use tracing::info;

use fc_common::tsid::{self, EntityType};
use fc_platform::application::entity::{Application, ApplicationType};
use fc_platform::application::repository::ApplicationRepository;
use fc_platform::auth::oauth_client_repository::OAuthClientRepository;
use fc_platform::auth::oauth_entity::{GrantType, OAuthClient, OAuthClientType};
use fc_platform::auth::password_service::{Argon2Config, PasswordPolicy, PasswordService};
use fc_platform::client::entity::Client;
use fc_platform::client::repository::ClientRepository;
use fc_platform::email_domain_mapping::entity::{EmailDomainMapping, ScopeType};
use fc_platform::email_domain_mapping::repository::EmailDomainMappingRepository;
use fc_platform::identity_provider::entity::{IdentityProvider, IdentityProviderType};
use fc_platform::identity_provider::repository::IdentityProviderRepository;
use fc_platform::principal::entity::{Principal, UserScope};
use fc_platform::principal::repository::PrincipalRepository;
use fc_platform::service_account::entity::{ServiceAccount, WebhookCredentials};
use fc_platform::service_account::repository::ServiceAccountRepository;
use fc_platform::shared::encryption_service::EncryptionService;

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Target project root. The .env file is written to `{root}/.env`.
    /// Defaults to the current working directory.
    #[arg(long, default_value = ".")]
    pub root: PathBuf,

    /// Non-interactive — fail if any required value is missing from
    /// flags rather than prompting.
    #[arg(long)]
    pub yes: bool,

    // ── Admin user (only used if no anchor admin exists yet) ──────────
    /// Anchor admin email. Prompts if omitted and no admin exists yet.
    #[arg(long)]
    pub admin_email: Option<String>,

    /// Anchor admin password. Prompts if omitted.
    #[arg(long)]
    pub admin_password: Option<String>,

    // ── Application ───────────────────────────────────────────────────
    /// Application code (URL-safe slug, e.g. "orders").
    #[arg(long)]
    pub code: Option<String>,

    /// Application name (e.g. "Orders").
    #[arg(long)]
    pub name: Option<String>,

    /// Application type: APPLICATION or INTEGRATION. Defaults to APPLICATION.
    #[arg(long, value_name = "TYPE")]
    pub app_type: Option<String>,

    /// Application description (optional).
    #[arg(long)]
    pub description: Option<String>,

    /// Application's deployed base URL (optional, fills `defaultBaseUrl`).
    #[arg(long)]
    pub default_base_url: Option<String>,

    // ── Default Client ────────────────────────────────────────────────
    /// Identifier for the Default Client. Reused across init runs.
    #[arg(long, default_value = "default")]
    pub client_identifier: String,

    /// Display name for the Default Client.
    #[arg(long, default_value = "Default Client")]
    pub client_name: String,

    // ── Database ──────────────────────────────────────────────────────
    /// PostgreSQL database URL. Falls back to FC_DATABASE_URL.
    #[arg(
        long,
        env = "FC_DATABASE_URL",
        default_value = "postgresql://localhost:5432/flowcatalyst"
    )]
    pub database_url: String,

    /// Use the embedded PostgreSQL instance (the one fc-dev starts at
    /// :15432 in `~/.cache/flowcatalyst-dev/pgdata/`). Default true so
    /// running `fc-dev init` immediately after `fc-dev` Just Works.
    #[cfg(feature = "embedded-db")]
    #[arg(long, env = "FC_EMBEDDED_DB", default_value = "true")]
    pub embedded_db: bool,

    /// API base URL written into FLOWCATALYST_BASE_URL.
    #[arg(long, default_value = "http://localhost:8080")]
    pub api_base_url: String,
}

pub async fn run(args: InitArgs) -> Result<()> {
    // Embedded PG (if enabled) — same data dir as the start path.
    #[cfg(feature = "embedded-db")]
    let (db_url, mut _embedded) = if args.embedded_db {
        let emb = crate::embedded_pg::start(false).await?;
        let url = emb.url.clone();
        (url, Some(emb))
    } else {
        (args.database_url.clone(), None)
    };
    #[cfg(not(feature = "embedded-db"))]
    let db_url = args.database_url.clone();

    let pool = fc_platform::shared::database::create_pool(&db_url)
        .await
        .context("connect to database")?;

    // Migrations + system seeds are idempotent — safe to re-run.
    fc_platform::shared::database::run_migrations(
        &pool,
        fc_platform::shared::database::MigrationProfile::Production,
    )
    .await
    .context("run migrations")?;
    fc_platform::shared::database::seed_builtin_roles(&pool)
        .await
        .context("seed built-in roles")?;
    fc_platform::shared::database::seed_platform_application(&pool)
        .await
        .context("seed platform application")?;
    fc_platform::shared::default_processes::seed_default_processes(&pool)
        .await
        .context("seed default processes")?;

    let principal_repo = PrincipalRepository::new(&pool);
    let client_repo = ClientRepository::new(&pool);
    let application_repo = ApplicationRepository::new(&pool);
    let service_account_repo = ServiceAccountRepository::new(&pool);
    let oauth_client_repo = OAuthClientRepository::new(&pool);

    println!("fc-dev init\n");

    // ── 1. Admin user ────────────────────────────────────────────────
    let any_anchor: bool = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM iam_principals WHERE type = 'USER' AND scope = 'ANCHOR')",
    )
    .fetch_one(&pool)
    .await
    .context("check for existing anchor user")?;

    if !any_anchor {
        let admin_email = resolve(args.admin_email.clone(), "Admin email", None, args.yes)?;
        let admin_password =
            resolve_secret(args.admin_password.clone(), "Admin password", args.yes)?;
        create_admin(&pool, &principal_repo, &admin_email, &admin_password).await?;
        println!("  → admin {} created", admin_email);
    } else {
        println!("→ admin user already present, skipping creation");
    }

    // ── 2. Default Client ────────────────────────────────────────────
    let client_id = match client_repo
        .find_by_identifier(&args.client_identifier)
        .await
        .context("look up default client")?
    {
        Some(c) => {
            println!(
                "→ reusing default client \"{}\" (id={})",
                c.identifier, c.id
            );
            c.id
        }
        None => {
            let c = Client::new(args.client_name.clone(), args.client_identifier.clone());
            client_repo
                .insert(&c)
                .await
                .context("insert default client")?;
            println!(
                "  → default client \"{}\" created (id={})",
                c.identifier, c.id
            );
            c.id
        }
    };

    // ── 3. Application ───────────────────────────────────────────────
    let code = resolve(args.code.clone(), "Application code (slug)", None, args.yes)?;
    let name = resolve(args.name.clone(), "Application name", None, args.yes)?;
    let raw_type = resolve(
        args.app_type.clone(),
        "Application type [APPLICATION|INTEGRATION]",
        Some("APPLICATION".to_string()),
        args.yes,
    )?;
    let app_type: ApplicationType = raw_type
        .to_uppercase()
        .parse()
        .context("application type")?;
    let description = resolve(
        args.description.clone(),
        "Description (optional)",
        Some(String::new()),
        args.yes,
    )?;
    let app_base_url = resolve(
        args.default_base_url.clone(),
        "Application's deployed base URL (optional)",
        Some(String::new()),
        args.yes,
    )?;

    if let Some(existing) = application_repo
        .find_by_code(&code)
        .await
        .context("lookup application by code")?
    {
        return Err(anyhow::anyhow!(
            "an application with code \"{}\" already exists (id={}). \
             Either pick a different code or run `fc-dev fresh` to start over.",
            code,
            existing.id
        ));
    }

    let mut application = Application::new(&code, &name);
    application.application_type = app_type;
    if !description.is_empty() {
        application.description = Some(description);
    }
    if !app_base_url.is_empty() {
        application.default_base_url = Some(app_base_url);
    }
    application_repo
        .insert(&application)
        .await
        .context("insert application")?;
    let app_id = application.id.clone();
    println!("  → application \"{}\" created (id={})", code, app_id);

    // ── 4. Service Account + linked Principal ────────────────────────
    let sa_code = format!("app:{}", code);
    let sa_name = format!("{} Service Account", name);
    let sa_description = format!("Service account for application: {}", name);

    // An application's service account is ANCHOR with no client links, as
    // Go's fcdev init and application provisioning make it.
    let mut sa = ServiceAccount::new(&sa_code, &sa_name, UserScope::Anchor);
    sa.description = Some(sa_description.clone());
    sa.application_id = Some(app_id.clone());
    // Webhook credentials, as the platform's CreateServiceAccount mints
    // them: the application's deliveries are signed with them, and a
    // function of the application cannot declare subscriptions or
    // schedules without the signing secret (APPLICATION_SIGNING_SECRET_REQUIRED).
    sa.webhook_credentials = generate_webhook_credentials()?;

    // ServiceAccount.id is the principal id (CLAUDE.md note); insert a
    // matching Principal::new_service row first so the SA's FK is valid.
    // Bound to and granted its own application only, as provisioning does
    // (Go: all_applications false plus one access row).
    let mut sa_principal =
        Principal::new_service(sa.id.clone(), sa_name.clone(), UserScope::Anchor)
            .with_application_id(&app_id);
    sa_principal.accessible_application_ids = vec![app_id.clone()];
    principal_repo
        .insert(&sa_principal)
        .await
        .context("insert service-account principal")?;
    service_account_repo
        .insert(&sa)
        .await
        .context("insert service account")?;

    // Attach SA to Application (sets application.service_account_id). The
    // column references iam_principals (migration 028), so it takes the
    // principal id, as Go's fcdev init stores it.
    application.service_account_id = Some(sa_principal.id.clone());
    application.updated_at = Utc::now();
    application_repo
        .update(&application)
        .await
        .context("attach service account to application")?;
    println!("  → service account \"{}\" attached", sa_code);

    // ── 5. OAuth client for the SA (client_credentials grant) ────────
    let (client_secret_plaintext, client_secret_ref) = generate_client_secret()?;
    let oauth_row_id = tsid::generate(EntityType::OAuthClient);
    let public_client_id = tsid::generate(EntityType::OAuthClient);

    let mut oauth_client = OAuthClient::new(
        &public_client_id,
        format!("{} Service Account Client", name),
    );
    oauth_client.id = oauth_row_id;
    oauth_client.client_type = OAuthClientType::Confidential;
    oauth_client.client_secret_ref = Some(client_secret_ref);
    oauth_client.grant_types = vec![GrantType::ClientCredentials];
    oauth_client.application_ids = vec![app_id.clone()];
    // References iam_principals (migration 027): the principal id.
    oauth_client.service_account_principal_id = Some(sa_principal.id.clone());
    oauth_client_repo
        .insert(&oauth_client)
        .await
        .context("insert OAuth client")?;
    println!("  → OAuth client minted (id={})", oauth_client.id);

    // ── 6. Write .env ────────────────────────────────────────────────
    let updates = [
        ("FLOWCATALYST_BASE_URL", args.api_base_url.as_str()),
        ("FLOWCATALYST_APP_CODE", code.as_str()),
        ("FLOWCATALYST_CLIENT_ID", public_client_id.as_str()),
        (
            "FLOWCATALYST_CLIENT_SECRET",
            client_secret_plaintext.as_str(),
        ),
    ];
    let env_path = args.root.join(".env");
    write_env_updates(&env_path, &updates).context("write .env")?;

    println!("\n✓ Application scaffolded.\n");
    println!("  Application:     {} (code={})", name, code);
    println!("  Service account: {}", sa_principal.id);
    println!(
        "  OAuth client:    {} (clientId={})",
        oauth_client.id, public_client_id
    );
    println!("  Default client:  {}", client_id);
    println!();
    println!(
        "  Credentials written to {}. The clientSecret is shown ONLY in",
        env_path.display()
    );
    println!("  the .env — the platform stores only a keyed hash and cannot");
    println!("  return it again. Rotate via the OAuth Clients page if needed.");

    #[cfg(feature = "embedded-db")]
    if let Some(mut e) = _embedded {
        crate::embedded_pg::stop(&mut e).await;
    }
    Ok(())
}

// ─── Prompt helpers ────────────────────────────────────────────────────

fn resolve(
    flag_value: Option<String>,
    question: &str,
    default: Option<String>,
    yes: bool,
) -> Result<String> {
    if let Some(v) = flag_value.filter(|s| !s.is_empty()) {
        return Ok(v);
    }
    if yes {
        return default
            .ok_or_else(|| anyhow::anyhow!("--yes mode requires a flag value for: {}", question));
    }
    let suffix = match default.as_deref() {
        Some(d) if !d.is_empty() => format!(" [{}]", d),
        _ => String::new(),
    };
    print!("{}{}: ", question, suffix);
    stdout().flush().ok();
    let mut input = String::new();
    stdin().lock().read_line(&mut input)?;
    let trimmed = input.trim_end_matches(&['\r', '\n'][..]).to_string();
    if trimmed.is_empty() {
        if let Some(d) = default {
            return Ok(d);
        }
        return Err(anyhow::anyhow!("{} is required", question));
    }
    Ok(trimmed)
}

fn resolve_secret(flag_value: Option<String>, question: &str, yes: bool) -> Result<String> {
    if let Some(v) = flag_value.filter(|s| !s.is_empty()) {
        return Ok(v);
    }
    if yes {
        return Err(anyhow::anyhow!(
            "--yes mode requires --admin-password to be set"
        ));
    }
    // Best-effort password masking: rpassword would be nicer but adds a
    // dep. With no TTY this is just a normal line read.
    print!("{}: ", question);
    stdout().flush().ok();
    let mut input = String::new();
    stdin().lock().read_line(&mut input)?;
    let trimmed = input.trim_end_matches(&['\r', '\n'][..]).to_string();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!("{} is required", question));
    }
    Ok(trimmed)
}

// ─── Admin / identity-provider bootstrap ────────────────────────────────

async fn create_admin(
    pool: &sqlx::PgPool,
    principal_repo: &PrincipalRepository,
    email: &str,
    password: &str,
) -> Result<()> {
    let domain = email.split('@').nth(1).unwrap_or("").to_lowercase();
    if domain.is_empty() {
        return Err(anyhow::anyhow!("invalid email format: {}", email));
    }

    // Internal IDP row (idempotent).
    let idp_repo = IdentityProviderRepository::new(pool);
    let idp_id = match idp_repo
        .find_by_code("internal")
        .await
        .context("look up internal IDP")?
    {
        Some(i) => i.id,
        None => {
            let idp = IdentityProvider::new(
                "internal",
                "Internal Authentication",
                IdentityProviderType::Internal,
            );
            let id = idp.id.clone();
            idp_repo.insert(&idp).await.context("insert internal IDP")?;
            id
        }
    };

    // Anchor email-domain mapping for the admin's domain (idempotent).
    let edm_repo = EmailDomainMappingRepository::new(pool);
    if edm_repo
        .find_by_email_domain(&domain)
        .await
        .context("look up email-domain mapping")?
        .is_none()
    {
        let mapping = EmailDomainMapping::new(&domain, &idp_id, ScopeType::Anchor);
        edm_repo
            .insert(&mapping)
            .await
            .context("insert email-domain mapping")?;
    }

    // Hash password (try the strict policy; relax if the operator picked
    // something the default policy refuses — matches bootstrap_admin's
    // fallback so they can't lock themselves out).
    let password_service = PasswordService::new(Argon2Config::default(), PasswordPolicy::default());
    let password_hash = match password_service.hash_password(password) {
        Ok(h) => h,
        Err(_) => {
            tracing::warn!("password does not meet complexity requirements, hashing anyway");
            password_service
                .hash_password_with_complexity(password, false)
                .map_err(|e| anyhow::anyhow!("hash password: {}", e))?
        }
    };

    let mut principal = Principal::new_user(email, UserScope::Anchor);
    if let Some(identity) = principal.user_identity.as_mut() {
        identity.password_hash = Some(password_hash);
    }
    principal.assign_role_with_source(
        "platform:super-admin",
        fc_platform::AssignmentSource::Bootstrap,
    );
    principal_repo
        .insert(&principal)
        .await
        .context("insert admin principal")?;
    info!(email = %email, "created anchor admin");
    Ok(())
}

// ─── Secret generation ─────────────────────────────────────────────────

/// A fresh client secret and its stored form, the verify-only `hashed:v1:`
/// ref (what the platform's OAuth client API stores).
fn generate_client_secret() -> Result<(String, String)> {
    use base64::Engine;
    let mut secret_bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut secret_bytes);
    let plaintext = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret_bytes);

    let enc = EncryptionService::from_env().ok_or_else(|| {
        anyhow::anyhow!(
            "FLOWCATALYST_APP_KEY not configured — fc-dev sets a default; \
             if you cleared it, set it again and re-run"
        )
    })?;
    let stored_ref = enc.hash_secret(&plaintext);
    Ok((plaintext, stored_ref))
}

/// A bearer token and a signing secret, each stored `encrypted:` (the
/// platform's CreateServiceAccount shapes and seals them the same way).
fn generate_webhook_credentials() -> Result<WebhookCredentials> {
    use base64::Engine;
    let enc = EncryptionService::from_env()
        .ok_or_else(|| anyhow::anyhow!("FLOWCATALYST_APP_KEY not configured"))?;
    let token: String = (0..32)
        .map(|_| {
            let i = rand::Rng::random_range(&mut rand::rng(), 0..36u8);
            if i < 10 {
                (b'0' + i) as char
            } else {
                (b'a' + i - 10) as char
            }
        })
        .collect();
    let mut signing = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut signing);
    let signing = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signing);
    let seal = |v: &str| {
        enc.encrypt_ref(v)
            .map_err(|e| anyhow::anyhow!("encrypting the service account's credentials: {e}"))
    };
    let mut credentials = WebhookCredentials::bearer_token(seal(&format!("fc_{token}"))?);
    credentials.signing_secret = Some(seal(&signing)?);
    Ok(credentials)
}

// ─── .env writer ───────────────────────────────────────────────────────

/// Idempotently update `path` so that every `(key, value)` in `updates`
/// is present. Existing keys are rewritten in place; new keys are
/// appended under a comment block. Values containing whitespace or
/// shell-special characters are single-quoted with embedded quotes
/// escaped. Re-runs with identical input are no-ops.
///
/// Exposed `pub(crate)` so the `outbox init` subcommand (and any other
/// fc-dev subcommand that needs to persist secrets to `.env`) can
/// reuse it — single source of truth for the .env edit semantics.
pub(crate) fn write_env_updates(path: &PathBuf, updates: &[(&str, &str)]) -> Result<()> {
    let original = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = if original.is_empty() {
        Vec::new()
    } else {
        original.split('\n').map(String::from).collect()
    };
    let mut seen: std::collections::HashSet<&str> = Default::default();

    for line in lines.iter_mut() {
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim();
            if let Some((k, v)) = updates.iter().find(|(k, _)| *k == key) {
                *line = format!("{}={}", k, quote_env_value(v));
                seen.insert(k);
            }
        }
    }

    let to_append: Vec<&(&str, &str)> = updates.iter().filter(|(k, _)| !seen.contains(k)).collect();
    if !to_append.is_empty() {
        if !lines.is_empty() && !lines.last().map(|l| l.is_empty()).unwrap_or(true) {
            lines.push(String::new());
        }
        lines.push("# FlowCatalyst (added by `fc-dev init`)".into());
        for (k, v) in &to_append {
            lines.push(format!("{}={}", k, quote_env_value(v)));
        }
    }

    let next = lines.join("\n").trim_end_matches('\n').to_string() + "\n";
    if next == original {
        println!("  → {} already current, no update needed", path.display());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create parent directory for .env")?;
    }
    fs::write(path, next).context("write .env")?;
    println!(
        "  → {} {}",
        path.display(),
        if original.is_empty() {
            "created"
        } else {
            "updated"
        }
    );
    Ok(())
}

fn quote_env_value(value: &str) -> String {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || "#'\"`$".contains(c))
    {
        format!("'{}'", value.replace('\'', "'\\''"))
    } else {
        value.to_string()
    }
}
