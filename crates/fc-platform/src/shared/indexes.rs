//! MongoDB Index Initialization
//!
//! Creates indexes for all collections on application startup.
//! Matches Java MongoIndexInitializer for cross-platform compatibility.

use mongodb::{Database, IndexModel, bson::doc, options::IndexOptions};
use tracing::info;

/// TTL for high-volume transactional data: 30 days
const TTL_30_DAYS_SECONDS: u64 = 30 * 24 * 60 * 60;

/// Initialize all MongoDB indexes
pub async fn initialize_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    info!("Initializing MongoDB indexes...");

    create_principal_indexes(db).await?;
    create_client_indexes(db).await?;
    create_application_indexes(db).await?;
    create_role_indexes(db).await?;
    create_oauth_indexes(db).await?;
    create_event_indexes(db).await?;
    create_dispatch_job_indexes(db).await?;
    create_event_type_indexes(db).await?;
    create_subscription_indexes(db).await?;
    create_audit_log_indexes(db).await?;
    create_misc_indexes(db).await?;

    info!("MongoDB indexes initialized successfully");
    Ok(())
}

async fn create_principal_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    let collection = db.collection::<mongodb::bson::Document>("principals");

    // Client filtering
    collection.create_index(
        IndexModel::builder()
            .keys(doc! { "client_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // Type filtering
    collection.create_index(
        IndexModel::builder()
            .keys(doc! { "type": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // Email lookup (unique, sparse for users only)
    collection.create_index(
        IndexModel::builder()
            .keys(doc! { "user_identity.email": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .sparse(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // Service account lookup (unique, sparse for service accounts only)
    collection.create_index(
        IndexModel::builder()
            .keys(doc! { "service_account_id": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .sparse(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    info!("Created indexes on principals");
    Ok(())
}

async fn create_client_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    let clients = db.collection::<mongodb::bson::Document>("clients");

    // Identifier lookup (unique)
    clients.create_index(
        IndexModel::builder()
            .keys(doc! { "identifier": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // Status filtering
    clients.create_index(
        IndexModel::builder()
            .keys(doc! { "status": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // Client access grants
    let grants = db.collection::<mongodb::bson::Document>("client_access_grants");

    grants.create_index(
        IndexModel::builder()
            .keys(doc! { "principal_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    grants.create_index(
        IndexModel::builder()
            .keys(doc! { "client_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    grants.create_index(
        IndexModel::builder()
            .keys(doc! { "principal_id": 1, "client_id": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // Auth configs
    let auth_configs = db.collection::<mongodb::bson::Document>("auth_configs");

    auth_configs.create_index(
        IndexModel::builder()
            .keys(doc! { "email_domain": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    info!("Created indexes on clients, client_access_grants, auth_configs");
    Ok(())
}

async fn create_application_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    let applications = db.collection::<mongodb::bson::Document>("applications");

    // Code lookup (unique)
    applications.create_index(
        IndexModel::builder()
            .keys(doc! { "code": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // Active filtering
    applications.create_index(
        IndexModel::builder()
            .keys(doc! { "active": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // Application-client config
    let app_config = db.collection::<mongodb::bson::Document>("application_client_config");

    app_config.create_index(
        IndexModel::builder()
            .keys(doc! { "application_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    app_config.create_index(
        IndexModel::builder()
            .keys(doc! { "client_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    app_config.create_index(
        IndexModel::builder()
            .keys(doc! { "application_id": 1, "client_id": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    info!("Created indexes on applications, application_client_config");
    Ok(())
}

async fn create_role_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    let roles = db.collection::<mongodb::bson::Document>("roles");

    // Code lookup (unique)
    roles.create_index(
        IndexModel::builder()
            .keys(doc! { "code": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // Application filtering
    roles.create_index(
        IndexModel::builder()
            .keys(doc! { "application_code": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // Source filtering
    roles.create_index(
        IndexModel::builder()
            .keys(doc! { "source": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // IdP role mappings
    let idp_mappings = db.collection::<mongodb::bson::Document>("idp_role_mappings");

    idp_mappings.create_index(
        IndexModel::builder()
            .keys(doc! { "idp_role_name": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    idp_mappings.create_index(
        IndexModel::builder()
            .keys(doc! { "internal_role_name": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    info!("Created indexes on roles, idp_role_mappings");
    Ok(())
}

async fn create_oauth_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    let oauth_clients = db.collection::<mongodb::bson::Document>("oauth_clients");

    // Client ID lookup (unique)
    oauth_clients.create_index(
        IndexModel::builder()
            .keys(doc! { "client_id": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // Owner client filtering
    oauth_clients.create_index(
        IndexModel::builder()
            .keys(doc! { "owner_client_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // Authorization codes (_id is the code itself)
    let auth_codes = db.collection::<mongodb::bson::Document>("authorization_codes");

    // Principal lookup for cleanup
    auth_codes.create_index(
        IndexModel::builder()
            .keys(doc! { "principal_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // Client lookup for cleanup
    auth_codes.create_index(
        IndexModel::builder()
            .keys(doc! { "client_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // TTL index - auto-delete expired authorization codes
    auth_codes.create_index(
        IndexModel::builder()
            .keys(doc! { "expires_at": 1 })
            .options(IndexOptions::builder()
                .expire_after(std::time::Duration::from_secs(0))
                .background(true)
                .build())
            .build(),
    ).await?;

    // Refresh tokens
    let refresh_tokens = db.collection::<mongodb::bson::Document>("refresh_tokens");

    refresh_tokens.create_index(
        IndexModel::builder()
            .keys(doc! { "token_hash": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    refresh_tokens.create_index(
        IndexModel::builder()
            .keys(doc! { "principal_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    refresh_tokens.create_index(
        IndexModel::builder()
            .keys(doc! { "token_family": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    refresh_tokens.create_index(
        IndexModel::builder()
            .keys(doc! { "expires_at": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    info!("Created indexes on oauth_clients, authorization_codes, refresh_tokens");
    Ok(())
}

async fn create_event_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    let events = db.collection::<mongodb::bson::Document>("events");

    // Idempotency - essential for deduplication
    events.create_index(
        IndexModel::builder()
            .keys(doc! { "deduplication_id": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .sparse(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // TTL index - auto-delete events after 30 days
    events.create_index(
        IndexModel::builder()
            .keys(doc! { "time": 1 })
            .options(IndexOptions::builder()
                .expire_after(std::time::Duration::from_secs(TTL_30_DAYS_SECONDS))
                .background(true)
                .build())
            .build(),
    ).await?;

    info!("Created minimal indexes on events (write-optimized)");
    Ok(())
}

async fn create_dispatch_job_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    let jobs = db.collection::<mongodb::bson::Document>("dispatch_jobs");

    // Idempotency - essential for deduplication
    jobs.create_index(
        IndexModel::builder()
            .keys(doc! { "idempotency_key": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .sparse(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // Scheduler - find pending jobs to dispatch
    jobs.create_index(
        IndexModel::builder()
            .keys(doc! { "status": 1, "scheduled_for": 1, "client_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // FIFO ordering within client context
    jobs.create_index(
        IndexModel::builder()
            .keys(doc! { "client_id": 1, "message_group": 1, "status": 1 })
            .options(IndexOptions::builder()
                .sparse(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // TTL index - auto-delete dispatch jobs after 30 days
    jobs.create_index(
        IndexModel::builder()
            .keys(doc! { "created_at": 1 })
            .options(IndexOptions::builder()
                .expire_after(std::time::Duration::from_secs(TTL_30_DAYS_SECONDS))
                .background(true)
                .build())
            .build(),
    ).await?;

    info!("Created minimal indexes on dispatch_jobs (write-optimized)");
    Ok(())
}

async fn create_event_type_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    let event_types = db.collection::<mongodb::bson::Document>("event_types");

    // Code lookup (unique)
    event_types.create_index(
        IndexModel::builder()
            .keys(doc! { "code": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // Status filtering
    event_types.create_index(
        IndexModel::builder()
            .keys(doc! { "status": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    info!("Created indexes on event_types");
    Ok(())
}

async fn create_subscription_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    let subscriptions = db.collection::<mongodb::bson::Document>("subscriptions");

    // Code + client lookup (unique)
    subscriptions.create_index(
        IndexModel::builder()
            .keys(doc! { "code": 1, "client_id": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // Status filtering
    subscriptions.create_index(
        IndexModel::builder()
            .keys(doc! { "status": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    info!("Created indexes on subscriptions");
    Ok(())
}

async fn create_audit_log_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    let audit_logs = db.collection::<mongodb::bson::Document>("audit_logs");

    // Entity lookup
    audit_logs.create_index(
        IndexModel::builder()
            .keys(doc! { "entity_type": 1, "entity_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // Principal lookup
    audit_logs.create_index(
        IndexModel::builder()
            .keys(doc! { "principal_id": 1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    // Time-ordered listing
    audit_logs.create_index(
        IndexModel::builder()
            .keys(doc! { "created_at": -1 })
            .options(IndexOptions::builder().background(true).build())
            .build(),
    ).await?;

    info!("Created indexes on audit_logs");
    Ok(())
}

async fn create_misc_indexes(db: &Database) -> Result<(), mongodb::error::Error> {
    // Anchor domains
    let anchor_domains = db.collection::<mongodb::bson::Document>("anchor_domains");

    anchor_domains.create_index(
        IndexModel::builder()
            .keys(doc! { "domain": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // OIDC login states
    let oidc_states = db.collection::<mongodb::bson::Document>("oidc_login_states");

    oidc_states.create_index(
        IndexModel::builder()
            .keys(doc! { "state": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    // TTL for OIDC states - 10 minutes
    oidc_states.create_index(
        IndexModel::builder()
            .keys(doc! { "created_at": 1 })
            .options(IndexOptions::builder()
                .expire_after(std::time::Duration::from_secs(10 * 60))
                .background(true)
                .build())
            .build(),
    ).await?;

    // Dispatch pools
    let dispatch_pools = db.collection::<mongodb::bson::Document>("dispatch_pools");

    dispatch_pools.create_index(
        IndexModel::builder()
            .keys(doc! { "code": 1 })
            .options(IndexOptions::builder()
                .unique(true)
                .background(true)
                .build())
            .build(),
    ).await?;

    info!("Created indexes on anchor_domains, oidc_login_states, dispatch_pools");
    Ok(())
}
