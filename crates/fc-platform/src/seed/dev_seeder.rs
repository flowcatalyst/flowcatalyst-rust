//! Development Data Seeder
//!
//! Seeds development data on application startup.
//! Matches Java DevDataSeeder for cross-platform compatibility.
//!
//! Default credentials:
//!   Platform Admin: admin@flowcatalyst.local / DevPassword123!
//!   Client Admin:   alice@acme.com / DevPassword123!
//!   Regular User:   bob@acme.com / DevPassword123!

use sqlx::PgPool;
use tracing::info;

use crate::{
    AnchorDomain, Application, AuthRole, Client, ClientAuthConfig, ClientStatus,
    ClientAccessGrant, EventType, Principal, RoleSource, UserScope, AuthProvider,
};
use crate::{
    AnchorDomainRepository, ApplicationRepository, ClientRepository,
    ClientAuthConfigRepository, ClientAccessGrantRepository, EventTypeRepository,
    PrincipalRepository, RoleRepository,
};
use crate::identity_provider::entity::IdentityProvider;
use crate::identity_provider::repository::IdentityProviderRepository;
use crate::email_domain_mapping::entity::{EmailDomainMapping, ScopeType};
use crate::email_domain_mapping::repository::EmailDomainMappingRepository;
use crate::auth::password_service::{PasswordService, Argon2Config, PasswordPolicy};

const DEV_PASSWORD: &str = "DevPassword123!";

/// Development data seeder
pub struct DevDataSeeder {
    pg_pool: PgPool,
    password_service: PasswordService,
}

impl DevDataSeeder {
    pub fn new(pg_pool: PgPool) -> Self {
        // Use testing config for faster seeding, but still Argon2id
        let password_service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::lenient(),
        );
        Self { pg_pool, password_service }
    }

    /// Seed all development data
    pub async fn seed(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("=== DEV DATA SEEDER ===");
        info!("Seeding development data...");

        self.seed_anchor_domain().await?;
        let internal_idp_id = self.seed_internal_identity_provider().await?;
        let clients = self.seed_clients().await?;
        self.seed_email_domain_mappings(&internal_idp_id, &clients).await?;
        self.seed_auth_configs(&clients).await?;
        // Application roles must exist before users reference them in role assignments.
        self.seed_application_roles().await?;
        self.seed_users(&clients).await?;
        self.seed_applications().await?;
        self.seed_event_types().await?;

        info!("Development data seeded successfully!");
        info!("");
        info!("Default logins:");
        info!("  Platform Admin: admin@flowcatalyst.local / {}", DEV_PASSWORD);
        info!("  Client Admin:   alice@acme.com / {}", DEV_PASSWORD);
        info!("  Regular User:   bob@acme.com / {}", DEV_PASSWORD);
        info!("=======================");

        Ok(())
    }

    async fn seed_anchor_domain(&self) -> Result<(), Box<dyn std::error::Error>> {
        let repo = AnchorDomainRepository::new(&self.pg_pool);

        if repo.find_by_domain("flowcatalyst.local").await?.is_some() {
            return Ok(());
        }

        let anchor = AnchorDomain::new("flowcatalyst.local");
        repo.insert(&anchor).await?;
        info!("Created anchor domain: flowcatalyst.local");

        Ok(())
    }

    /// Ensure the "internal" identity provider exists (for password-based auth).
    async fn seed_internal_identity_provider(&self) -> Result<String, Box<dyn std::error::Error>> {
        let repo = IdentityProviderRepository::new(&self.pg_pool);

        if let Some(existing) = repo.find_by_code("internal").await? {
            return Ok(existing.id);
        }

        let idp = IdentityProvider::new("internal", "Internal Authentication", crate::identity_provider::entity::IdentityProviderType::Internal);
        repo.insert(&idp).await?;
        info!("Created internal identity provider");
        Ok(idp.id)
    }

    /// Create email domain mappings linking domains to the internal IDP.
    async fn seed_email_domain_mappings(
        &self,
        internal_idp_id: &str,
        clients: &SeedClients,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let repo = EmailDomainMappingRepository::new(&self.pg_pool);

        // Anchor domain
        self.create_edm_if_not_exists(&repo, "flowcatalyst.local", internal_idp_id, ScopeType::Anchor, None).await?;

        // Client domains
        if let Some(ref acme) = clients.acme {
            self.create_edm_if_not_exists(&repo, "acme.com", internal_idp_id, ScopeType::Client, Some(&acme.id)).await?;
        }
        if let Some(ref globex) = clients.globex {
            self.create_edm_if_not_exists(&repo, "globex.com", internal_idp_id, ScopeType::Client, Some(&globex.id)).await?;
        }

        // Partner domain
        self.create_edm_if_not_exists(&repo, "partner.io", internal_idp_id, ScopeType::Partner, None).await?;

        Ok(())
    }

    async fn create_edm_if_not_exists(
        &self,
        repo: &EmailDomainMappingRepository,
        domain: &str,
        idp_id: &str,
        scope_type: ScopeType,
        primary_client_id: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if repo.find_by_email_domain(domain).await?.is_some() {
            return Ok(());
        }

        let mut mapping = EmailDomainMapping::new(domain, idp_id, scope_type);
        if let Some(client_id) = primary_client_id {
            mapping.primary_client_id = Some(client_id.to_string());
        }
        repo.insert(&mapping).await?;
        info!("Created email domain mapping: {} → internal ({})", domain, scope_type.as_str());

        Ok(())
    }

    async fn seed_clients(&self) -> Result<SeedClients, Box<dyn std::error::Error>> {
        let repo = ClientRepository::new(&self.pg_pool);

        let acme = self.create_client_if_not_exists(&repo, "Acme Corporation", "acme", ClientStatus::Active).await?;
        let globex = self.create_client_if_not_exists(&repo, "Globex Industries", "globex", ClientStatus::Active).await?;
        let _initech = self.create_client_if_not_exists(&repo, "Initech Solutions", "initech", ClientStatus::Active).await?;
        let _umbrella = self.create_client_if_not_exists(&repo, "Umbrella Corp", "umbrella", ClientStatus::Suspended).await?;

        Ok(SeedClients { acme, globex })
    }

    async fn create_client_if_not_exists(
        &self,
        repo: &ClientRepository,
        name: &str,
        identifier: &str,
        status: ClientStatus,
    ) -> Result<Option<Client>, Box<dyn std::error::Error>> {
        if let Some(existing) = repo.find_by_identifier(identifier).await? {
            return Ok(Some(existing));
        }

        let mut client = Client::new(name, identifier);
        client.status = status;
        repo.insert(&client).await?;
        info!("Created client: {} ({})", name, identifier);

        Ok(Some(client))
    }

    async fn seed_auth_configs(&self, _clients: &SeedClients) -> Result<(), Box<dyn std::error::Error>> {
        let repo = ClientAuthConfigRepository::new(&self.pg_pool);

        self.create_auth_config_if_not_exists(&repo, "flowcatalyst.local").await?;
        self.create_auth_config_if_not_exists(&repo, "acme.com").await?;
        self.create_auth_config_if_not_exists(&repo, "globex.com").await?;
        self.create_auth_config_if_not_exists(&repo, "initech.com").await?;
        self.create_auth_config_if_not_exists(&repo, "partner.io").await?;

        Ok(())
    }

    async fn create_auth_config_if_not_exists(
        &self,
        repo: &ClientAuthConfigRepository,
        domain: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if repo.find_by_email_domain(domain).await?.is_some() {
            return Ok(());
        }

        let mut config = ClientAuthConfig::new_partner(domain);
        config.auth_provider = AuthProvider::Internal;
        repo.insert(&config).await?;

        Ok(())
    }

    /// Seed the application-scoped roles that dev users are assigned.
    /// Without these, editing a seeded user's roles fails with ROLE_NOT_FOUND
    /// because `assign_roles` validates every name against `iam_roles`.
    async fn seed_application_roles(&self) -> Result<(), Box<dyn std::error::Error>> {
        let repo = RoleRepository::new(&self.pg_pool);

        let roles: &[(&str, &str, &str, &str)] = &[
            ("acme", "client-admin", "Acme Client Admin", "Client-level administration for Acme"),
            ("dispatch", "admin",   "Dispatch Admin",   "Full control over dispatch operations"),
            ("dispatch", "user",    "Dispatch User",    "Create and manage dispatch jobs"),
            ("dispatch", "viewer",  "Dispatch Viewer",  "Read-only access to dispatch data"),
        ];

        for (app, name, display, description) in roles {
            let full_name = format!("{}:{}", app, name);
            if repo.find_by_name(&full_name).await?.is_some() {
                continue;
            }
            let mut role = AuthRole::new(*app, *name, *display);
            role.description = Some((*description).to_string());
            role.source = RoleSource::Sdk;
            repo.insert(&role).await?;
            info!("Created dev role: {}", full_name);
        }

        Ok(())
    }

    async fn seed_users(&self, clients: &SeedClients) -> Result<(), Box<dyn std::error::Error>> {
        let principal_repo = PrincipalRepository::new(&self.pg_pool);
        let grant_repo = ClientAccessGrantRepository::new(&self.pg_pool);

        let password_hash = self.password_service.hash_password(DEV_PASSWORD)
            .map_err(|e| format!("Failed to hash password: {}", e))?;

        // Platform admin (anchor domain - access to all clients)
        self.create_user_if_not_exists(
            &principal_repo,
            "admin@flowcatalyst.local",
            "Platform Administrator",
            None,
            &password_hash,
            UserScope::Anchor,
            vec!["platform:super-admin"],
        ).await?;

        // Acme client admin
        if let Some(ref acme) = clients.acme {
            self.create_user_if_not_exists(
                &principal_repo,
                "alice@acme.com",
                "Alice Johnson",
                Some(&acme.id),
                &password_hash,
                UserScope::Client,
                vec!["acme:client-admin", "dispatch:admin"],
            ).await?;

            // Acme regular user
            self.create_user_if_not_exists(
                &principal_repo,
                "bob@acme.com",
                "Bob Smith",
                Some(&acme.id),
                &password_hash,
                UserScope::Client,
                vec!["dispatch:user"],
            ).await?;
        }

        // Globex user
        if let Some(ref globex) = clients.globex {
            self.create_user_if_not_exists(
                &principal_repo,
                "charlie@globex.com",
                "Charlie Brown",
                Some(&globex.id),
                &password_hash,
                UserScope::Client,
                vec!["dispatch:admin"],
            ).await?;
        }

        // Partner user (cross-client access)
        let partner = self.create_user_if_not_exists(
            &principal_repo,
            "diana@partner.io",
            "Diana Partner",
            None,
            &password_hash,
            UserScope::Partner,
            vec!["dispatch:viewer"],
        ).await?;

        // Grant partner access to Acme and Globex
        if let (Some(partner), Some(ref acme)) = (&partner, &clients.acme) {
            self.create_grant_if_not_exists(&grant_repo, &partner.id, &acme.id).await?;
        }
        if let (Some(partner), Some(ref globex)) = (&partner, &clients.globex) {
            self.create_grant_if_not_exists(&grant_repo, &partner.id, &globex.id).await?;
        }

        Ok(())
    }

    async fn create_user_if_not_exists(
        &self,
        repo: &PrincipalRepository,
        email: &str,
        name: &str,
        client_id: Option<&str>,
        password_hash: &str,
        scope: UserScope,
        roles: Vec<&str>,
    ) -> Result<Option<Principal>, Box<dyn std::error::Error>> {
        if let Some(existing) = repo.find_by_email(email).await? {
            return Ok(Some(existing));
        }

        let mut user = Principal::new_user(email, scope);
        user.name = name.to_string();
        if let Some(cid) = client_id {
            user.client_id = Some(cid.to_string());
        }

        // Set password hash
        if let Some(ref mut identity) = user.user_identity {
            identity.password_hash = Some(password_hash.to_string());
        }

        // Add roles
        for role in roles {
            user.assign_role_with_source(role, "DEV_SEEDER");
        }

        repo.insert(&user).await?;
        info!("Created user: {} ({})", name, email);

        Ok(Some(user))
    }

    async fn create_grant_if_not_exists(
        &self,
        repo: &ClientAccessGrantRepository,
        principal_id: &str,
        client_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if repo.find_by_principal_and_client(principal_id, client_id).await?.is_some() {
            return Ok(());
        }

        let grant = ClientAccessGrant::new(principal_id, client_id, "system");
        repo.insert(&grant).await?;

        Ok(())
    }

    async fn seed_applications(&self) -> Result<(), Box<dyn std::error::Error>> {
        let repo = ApplicationRepository::new(&self.pg_pool);

        self.create_application_if_not_exists(&repo, "tms", "Transport Management System",
            "End-to-end transportation planning, execution, and optimization").await?;
        self.create_application_if_not_exists(&repo, "wms", "Warehouse Management System",
            "Inventory control, picking, packing, and warehouse operations").await?;
        self.create_application_if_not_exists(&repo, "oms", "Order Management System",
            "Order processing, fulfillment orchestration, and customer service").await?;
        self.create_application_if_not_exists(&repo, "track", "Shipment Tracking",
            "Real-time visibility and tracking for shipments and assets").await?;
        self.create_application_if_not_exists(&repo, "yard", "Yard Management System",
            "Dock scheduling, trailer tracking, and yard operations").await?;
        self.create_application_if_not_exists(&repo, "platform", "Platform Services",
            "Core platform infrastructure and shared services").await?;

        Ok(())
    }

    async fn create_application_if_not_exists(
        &self,
        repo: &ApplicationRepository,
        code: &str,
        name: &str,
        description: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if repo.find_by_code(code).await?.is_some() {
            return Ok(());
        }

        let app = Application::new(code, name).with_description(description);
        repo.insert(&app).await?;
        info!("Created application: {} ({})", name, code);

        Ok(())
    }

    async fn seed_event_types(&self) -> Result<(), Box<dyn std::error::Error>> {
        let repo = EventTypeRepository::new(&self.pg_pool);

        // TMS - Transport Management Events
        self.create_event_type(&repo, "tms:planning:load:created", "Load Created",
            "A new load has been created in the system").await?;
        self.create_event_type(&repo, "tms:planning:load:updated", "Load Updated",
            "Load details have been modified").await?;
        self.create_event_type(&repo, "tms:planning:load:tendered", "Load Tendered",
            "Load has been tendered to a carrier").await?;
        self.create_event_type(&repo, "tms:planning:load:accepted", "Load Accepted",
            "Carrier has accepted the load tender").await?;
        self.create_event_type(&repo, "tms:planning:load:rejected", "Load Rejected",
            "Carrier has rejected the load tender").await?;
        self.create_event_type(&repo, "tms:planning:route:optimized", "Route Optimized",
            "Route optimization completed for shipments").await?;

        self.create_event_type(&repo, "tms:execution:shipment:dispatched", "Shipment Dispatched",
            "Shipment has been dispatched for delivery").await?;
        self.create_event_type(&repo, "tms:execution:shipment:in-transit", "Shipment In Transit",
            "Shipment is currently in transit").await?;
        self.create_event_type(&repo, "tms:execution:shipment:delivered", "Shipment Delivered",
            "Shipment has been successfully delivered").await?;
        self.create_event_type(&repo, "tms:execution:shipment:exception", "Shipment Exception",
            "An exception occurred during shipment execution").await?;
        self.create_event_type(&repo, "tms:execution:driver:assigned", "Driver Assigned",
            "Driver has been assigned to a shipment").await?;
        self.create_event_type(&repo, "tms:execution:driver:checked-in", "Driver Checked In",
            "Driver has checked in at a facility").await?;

        self.create_event_type(&repo, "tms:billing:invoice:generated", "Invoice Generated",
            "Freight invoice has been generated").await?;
        self.create_event_type(&repo, "tms:billing:invoice:approved", "Invoice Approved",
            "Freight invoice has been approved for payment").await?;
        self.create_event_type(&repo, "tms:billing:payment:processed", "Payment Processed",
            "Payment has been processed for carrier").await?;

        // WMS - Warehouse Management Events
        self.create_event_type(&repo, "wms:inventory:receipt:completed", "Receipt Completed",
            "Inbound receipt has been completed").await?;
        self.create_event_type(&repo, "wms:inventory:putaway:completed", "Putaway Completed",
            "Inventory putaway has been completed").await?;
        self.create_event_type(&repo, "wms:inventory:adjustment:made", "Inventory Adjusted",
            "Inventory adjustment has been recorded").await?;
        self.create_event_type(&repo, "wms:inventory:cycle-count:completed", "Cycle Count Completed",
            "Inventory cycle count has been completed").await?;
        self.create_event_type(&repo, "wms:inventory:transfer:completed", "Transfer Completed",
            "Inventory transfer between locations completed").await?;

        self.create_event_type(&repo, "wms:outbound:wave:released", "Wave Released",
            "Outbound wave has been released for picking").await?;
        self.create_event_type(&repo, "wms:outbound:pick:completed", "Pick Completed",
            "Order picking has been completed").await?;
        self.create_event_type(&repo, "wms:outbound:pack:completed", "Pack Completed",
            "Order packing has been completed").await?;
        self.create_event_type(&repo, "wms:outbound:ship:confirmed", "Ship Confirmed",
            "Shipment has been confirmed and loaded").await?;

        self.create_event_type(&repo, "wms:labor:task:assigned", "Task Assigned",
            "Work task has been assigned to associate").await?;
        self.create_event_type(&repo, "wms:labor:task:completed", "Task Completed",
            "Work task has been completed by associate").await?;

        // OMS - Order Management Events
        self.create_event_type(&repo, "oms:order:order:created", "Order Created",
            "New customer order has been created").await?;
        self.create_event_type(&repo, "oms:order:order:confirmed", "Order Confirmed",
            "Order has been confirmed and validated").await?;
        self.create_event_type(&repo, "oms:order:order:cancelled", "Order Cancelled",
            "Order has been cancelled").await?;
        self.create_event_type(&repo, "oms:order:order:modified", "Order Modified",
            "Order has been modified after creation").await?;

        self.create_event_type(&repo, "oms:fulfillment:allocation:completed", "Allocation Completed",
            "Inventory allocation for order completed").await?;
        self.create_event_type(&repo, "oms:fulfillment:backorder:created", "Backorder Created",
            "Backorder created for unavailable items").await?;
        self.create_event_type(&repo, "oms:fulfillment:split:occurred", "Order Split",
            "Order has been split into multiple shipments").await?;

        self.create_event_type(&repo, "oms:returns:return:initiated", "Return Initiated",
            "Customer return has been initiated").await?;
        self.create_event_type(&repo, "oms:returns:return:received", "Return Received",
            "Returned items have been received").await?;
        self.create_event_type(&repo, "oms:returns:refund:processed", "Refund Processed",
            "Refund has been processed for return").await?;

        // Track - Shipment Tracking Events
        self.create_event_type(&repo, "track:visibility:checkpoint:recorded", "Checkpoint Recorded",
            "Shipment checkpoint has been recorded").await?;
        self.create_event_type(&repo, "track:visibility:eta:updated", "ETA Updated",
            "Estimated time of arrival has been updated").await?;
        self.create_event_type(&repo, "track:visibility:delay:detected", "Delay Detected",
            "Shipment delay has been detected").await?;
        self.create_event_type(&repo, "track:visibility:geofence:entered", "Geofence Entered",
            "Asset has entered a geofence area").await?;
        self.create_event_type(&repo, "track:visibility:geofence:exited", "Geofence Exited",
            "Asset has exited a geofence area").await?;

        self.create_event_type(&repo, "track:alerts:exception:raised", "Exception Raised",
            "Tracking exception has been raised").await?;
        self.create_event_type(&repo, "track:alerts:temperature:breach", "Temperature Breach",
            "Temperature threshold has been breached").await?;
        self.create_event_type(&repo, "track:alerts:tamper:detected", "Tamper Detected",
            "Potential tampering has been detected").await?;

        // Yard - Yard Management Events
        self.create_event_type(&repo, "yard:gate:check-in:completed", "Gate Check-In",
            "Vehicle has completed gate check-in").await?;
        self.create_event_type(&repo, "yard:gate:check-out:completed", "Gate Check-Out",
            "Vehicle has completed gate check-out").await?;

        self.create_event_type(&repo, "yard:dock:appointment:scheduled", "Appointment Scheduled",
            "Dock appointment has been scheduled").await?;
        self.create_event_type(&repo, "yard:dock:appointment:arrived", "Appointment Arrived",
            "Vehicle has arrived for dock appointment").await?;
        self.create_event_type(&repo, "yard:dock:door:assigned", "Door Assigned",
            "Dock door has been assigned to trailer").await?;
        self.create_event_type(&repo, "yard:dock:loading:started", "Loading Started",
            "Loading/unloading has started at dock").await?;
        self.create_event_type(&repo, "yard:dock:loading:completed", "Loading Completed",
            "Loading/unloading has been completed").await?;

        self.create_event_type(&repo, "yard:yard:trailer:spotted", "Trailer Spotted",
            "Trailer has been spotted at location").await?;
        self.create_event_type(&repo, "yard:yard:trailer:moved", "Trailer Moved",
            "Trailer has been moved within yard").await?;
        self.create_event_type(&repo, "yard:yard:trailer:sealed", "Trailer Sealed",
            "Trailer has been sealed").await?;

        // Platform - Admin Events (event types, connections, dispatch pools, subscriptions)
        self.create_event_type(&repo, "platform:admin:eventtype:created", "Event Type Created",
            "A new event type has been registered in the platform").await?;
        self.create_event_type(&repo, "platform:admin:eventtype:updated", "Event Type Updated",
            "Event type metadata has been updated").await?;
        self.create_event_type(&repo, "platform:admin:eventtype:archived", "Event Type Archived",
            "Event type has been archived").await?;
        self.create_event_type(&repo, "platform:admin:eventtype:deleted", "Event Type Deleted",
            "Event type has been deleted from the platform").await?;
        self.create_event_type(&repo, "platform:admin:eventtype:schema-added", "Event Type Schema Added",
            "A new schema version has been added to an event type").await?;
        self.create_event_type(&repo, "platform:admin:eventtype:schema-deprecated", "Event Type Schema Deprecated",
            "A schema version has been marked as deprecated").await?;
        self.create_event_type(&repo, "platform:admin:eventtype:schema-finalised", "Event Type Schema Finalised",
            "A schema version has been finalised as current").await?;

        // Platform - IAM Events (applications, roles)
        self.create_event_type(&repo, "platform:iam:application:created", "Application Created",
            "A new application has been registered in the platform").await?;
        self.create_event_type(&repo, "platform:iam:application:updated", "Application Updated",
            "Application details have been updated").await?;
        self.create_event_type(&repo, "platform:iam:application:activated", "Application Activated",
            "Application has been activated").await?;
        self.create_event_type(&repo, "platform:iam:application:deactivated", "Application Deactivated",
            "Application has been deactivated").await?;
        self.create_event_type(&repo, "platform:iam:application:deleted", "Application Deleted",
            "Application has been deleted from the platform").await?;

        self.create_event_type(&repo, "platform:iam:role:created", "Role Created",
            "A new role has been created").await?;
        self.create_event_type(&repo, "platform:iam:role:updated", "Role Updated",
            "Role details or permissions have been updated").await?;
        self.create_event_type(&repo, "platform:iam:role:deleted", "Role Deleted",
            "Role has been deleted").await?;
        self.create_event_type(&repo, "platform:iam:roles:synced", "Roles Synced",
            "Roles have been bulk synced from an external application").await?;

        info!("Event types seeded successfully");

        Ok(())
    }

    async fn create_event_type(
        &self,
        repo: &EventTypeRepository,
        code: &str,
        name: &str,
        description: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if repo.find_by_code(code).await?.is_some() {
            return Ok(());
        }

        let mut event_type = EventType::new(code, name)
            .map_err(|e| format!("Invalid event type code {}: {}", code, e))?;
        event_type.description = Some(description.to_string());
        repo.insert(&event_type).await?;

        Ok(())
    }
}

/// Helper struct to hold seeded clients
struct SeedClients {
    acme: Option<Client>,
    globex: Option<Client>,
}
