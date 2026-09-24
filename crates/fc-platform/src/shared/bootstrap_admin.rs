//! Bootstrap admin user seeder.
//!
//! On startup, if no `USER` principal with `scope = ANCHOR` exists, look at
//! `FLOWCATALYST_BOOTSTRAP_ADMIN_EMAIL` / `_PASSWORD` / `_NAME` and create
//! one. Idempotent: if any anchor user already exists (or the named user
//! already exists), do nothing.
//!
//! Ports `packages/platform/src/bootstrap/bootstrap-service.ts::bootstrapAdminUser`
//! from the TS platform.
//!
//! Falls under the "platform infrastructure processing" exception in
//! CLAUDE.md — bootstrap-only, runs before HTTP serving begins, no
//! executing principal, so writes go directly to the repositories rather
//! than through `UseCase` / `UnitOfWork`. Same exception that
//! `seed_builtin_roles` and `seed_platform_application` use.

use sqlx::PgPool;
use tracing::{info, warn};

use crate::auth::password_service::{Argon2Config, PasswordPolicy, PasswordService};
use crate::email_domain_mapping::entity::{EmailDomainMapping, ScopeType};
use crate::email_domain_mapping::repository::EmailDomainMappingRepository;
use crate::identity_provider::entity::{IdentityProvider, IdentityProviderType};
use crate::identity_provider::repository::IdentityProviderRepository;
use crate::principal::entity::{Principal, UserScope};
use crate::principal::repository::PrincipalRepository;
use crate::shared::error::Result;

const ENV_EMAIL: &str = "FLOWCATALYST_BOOTSTRAP_ADMIN_EMAIL";
const ENV_PASSWORD: &str = "FLOWCATALYST_BOOTSTRAP_ADMIN_PASSWORD";
const ENV_NAME: &str = "FLOWCATALYST_BOOTSTRAP_ADMIN_NAME";
const DEFAULT_NAME: &str = "Bootstrap Admin";
const ROLE_SUPER_ADMIN: &str = "platform:super-admin";
const ROLE_SOURCE: &str = "BOOTSTRAP";

/// Bootstrap an initial admin if no anchor `USER` exists. See module docs.
pub async fn bootstrap_admin_user(pool: &PgPool) -> Result<()> {
    // Cheap existence check: any anchor USER already present means we're
    // not on a freshly-deployed environment.
    let existing: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM iam_principals WHERE type = 'USER' AND scope = 'ANCHOR'",
    )
    .fetch_one(pool)
    .await?;

    if existing.0 > 0 {
        return Ok(());
    }

    info!("No anchor users found, checking for bootstrap configuration...");

    let email = match std::env::var(ENV_EMAIL).ok().filter(|v| !v.is_empty()) {
        Some(e) => e,
        None => {
            warn!(
                "No bootstrap admin configured. Set {} and {} to create an initial admin.",
                ENV_EMAIL, ENV_PASSWORD
            );
            return Ok(());
        }
    };

    let password = match std::env::var(ENV_PASSWORD).ok().filter(|v| !v.is_empty()) {
        Some(p) => p,
        None => {
            warn!(
                "No bootstrap admin configured. Set {} and {} to create an initial admin.",
                ENV_EMAIL, ENV_PASSWORD
            );
            return Ok(());
        }
    };

    let name = std::env::var(ENV_NAME)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_NAME.to_string());

    if !email.contains('@') {
        warn!(email = %email, "Invalid bootstrap email format");
        return Ok(());
    }

    let principal_repo = PrincipalRepository::new(pool);

    // Idempotency: someone may have created this user via SQL before
    // bootstrap ran (no anchor existed yet, but the named user does).
    if principal_repo.find_by_email(&email).await?.is_some() {
        info!(email = %email, "Bootstrap user already exists");
        return Ok(());
    }

    let email_domain = email.split('@').nth(1).unwrap_or("").to_string();

    let internal_idp_id = ensure_internal_identity_provider(pool).await?;

    ensure_anchor_email_domain_mapping(pool, &email_domain, &internal_idp_id).await?;

    // Hash password. Try the strict configured policy first; if the
    // bootstrap password doesn't satisfy it, fall back to the relaxed
    // policy with a warning — matching the TS `validateAndHash → hash`
    // fallback so operators can't lock themselves out with a non-compliant
    // bootstrap password.
    let password_service = PasswordService::new(Argon2Config::default(), PasswordPolicy::default());
    let password_hash = match password_service.hash_password(&password) {
        Ok(h) => h,
        Err(_) => {
            warn!("Bootstrap password does not meet complexity requirements, hashing anyway");
            match password_service.hash_password_with_complexity(&password, false) {
                Ok(h) => h,
                Err(e) => {
                    warn!(error = %e, "Failed to hash bootstrap password");
                    return Ok(());
                }
            }
        }
    };

    let mut principal = Principal::new_user(&email, UserScope::Anchor);
    principal.name = name.clone();
    if let Some(ref mut identity) = principal.user_identity {
        identity.password_hash = Some(password_hash);
    }
    principal.assign_role_with_source(ROLE_SUPER_ADMIN, ROLE_SOURCE);

    principal_repo.insert(&principal).await?;

    info!(
        name = %name,
        email = %email,
        "Created bootstrap admin with platform:super-admin role and ANCHOR scope"
    );

    Ok(())
}

async fn ensure_internal_identity_provider(pool: &PgPool) -> Result<String> {
    let repo = IdentityProviderRepository::new(pool);

    if let Some(existing) = repo.find_by_code("internal").await? {
        return Ok(existing.id);
    }

    let idp = IdentityProvider::new(
        "internal",
        "Internal Authentication",
        IdentityProviderType::Internal,
    );
    repo.insert(&idp).await?;
    info!("Created internal identity provider");
    Ok(idp.id)
}

async fn ensure_anchor_email_domain_mapping(
    pool: &PgPool,
    email_domain: &str,
    idp_id: &str,
) -> Result<()> {
    let repo = EmailDomainMappingRepository::new(pool);

    if repo.find_by_email_domain(email_domain).await?.is_some() {
        return Ok(());
    }

    let mapping = EmailDomainMapping::new(email_domain, idp_id, ScopeType::Anchor);
    repo.insert(&mapping).await?;
    info!(email_domain = %email_domain, "Created anchor domain mapping");
    Ok(())
}
