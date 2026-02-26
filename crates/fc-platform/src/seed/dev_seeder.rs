//! Development Data Seeder
//!
//! Seeds development data on application startup.
//! Matches Java DevDataSeeder for cross-platform compatibility.
//!
//! Default credentials:
//!   Platform Admin: admin@flowcatalyst.local / DevPassword123!
//!   Client Admin:   alice@acme.com / DevPassword123!
//!   Regular User:   bob@acme.com / DevPassword123!

use mongodb::Database;
use sea_orm::DatabaseConnection;
use tracing::info;

use crate::{
    AnchorDomain, Application, Client, ClientAuthConfig, ClientStatus,
    ClientAccessGrant, EventType, Principal, UserScope, AuthProvider,
};
use crate::{
    AnchorDomainRepository, ApplicationRepository, ClientRepository,
    ClientAuthConfigRepository, ClientAccessGrantRepository, EventTypeRepository,
    PrincipalRepository,
};
use crate::auth::password_service::{PasswordService, Argon2Config, PasswordPolicy};

const DEV_PASSWORD: &str = "DevPassword123!";

/// Development data seeder
///
/// During the MongoDB → PostgreSQL migration, the seeder holds both database connections.
/// Migrated repositories (Client) use `pg_db`, others still use MongoDB `db`.
pub struct DevDataSeeder {
    db: Database,
    pg_db: DatabaseConnection,
    password_service: PasswordService,
}

impl DevDataSeeder {
    pub fn new(db: Database, pg_db: DatabaseConnection) -> Self {
        // Use testing config for faster seeding, but still Argon2id
        let password_service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::lenient(),
        );
        Self { db, pg_db, password_service }
    }

    /// Seed all development data
    pub async fn seed(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("=== DEV DATA SEEDER ===");
        info!("Seeding development data...");

        self.seed_anchor_domain().await?;
        let clients = self.seed_clients().await?;
        self.seed_auth_configs(&clients).await?;
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
        let repo = AnchorDomainRepository::new(&self.db);

        if repo.find_by_domain("flowcatalyst.local").await?.is_some() {
            return Ok(());
        }

        let anchor = AnchorDomain::new("flowcatalyst.local");
        repo.insert(&anchor).await?;
        info!("Created anchor domain: flowcatalyst.local");

        Ok(())
    }

    async fn seed_clients(&self) -> Result<SeedClients, Box<dyn std::error::Error>> {
        let repo = ClientRepository::new(&self.pg_db);

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
        let repo = ClientAuthConfigRepository::new(&self.db);

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

    async fn seed_users(&self, clients: &SeedClients) -> Result<(), Box<dyn std::error::Error>> {
        let principal_repo = PrincipalRepository::new(&self.pg_db);
        let grant_repo = ClientAccessGrantRepository::new(&self.pg_db);

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

        let grant = ClientAccessGrant::new(principal_id, client_id);
        repo.insert(&grant).await?;

        Ok(())
    }

    async fn seed_applications(&self) -> Result<(), Box<dyn std::error::Error>> {
        let repo = ApplicationRepository::new(&self.db);

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
        let repo = EventTypeRepository::new(&self.db);

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

        // Platform - Infrastructure Events
        self.create_event_type(&repo, "platform:integration:webhook:delivered", "Webhook Delivered",
            "Outbound webhook has been successfully delivered").await?;
        self.create_event_type(&repo, "platform:integration:webhook:failed", "Webhook Failed",
            "Outbound webhook delivery has failed").await?;
        self.create_event_type(&repo, "platform:integration:sync:completed", "Sync Completed",
            "Data synchronization has been completed").await?;

        self.create_event_type(&repo, "platform:audit:login:success", "Login Success",
            "User has successfully logged in").await?;
        self.create_event_type(&repo, "platform:audit:login:failed", "Login Failed",
            "User login attempt has failed").await?;
        self.create_event_type(&repo, "platform:audit:permission:changed", "Permission Changed",
            "User permissions have been modified").await?;

        // Platform - Control Plane Events
        self.create_event_type(&repo, "platform:control-plane:event-type:created", "Event Type Created",
            "A new event type has been registered in the platform").await?;
        self.create_event_type(&repo, "platform:control-plane:event-type:updated", "Event Type Updated",
            "Event type metadata has been updated").await?;
        self.create_event_type(&repo, "platform:control-plane:event-type:archived", "Event Type Archived",
            "Event type has been archived").await?;
        self.create_event_type(&repo, "platform:control-plane:event-type:deleted", "Event Type Deleted",
            "Event type has been deleted from the platform").await?;
        self.create_event_type(&repo, "platform:control-plane:event-type:schema-added", "Event Type Schema Added",
            "A new schema version has been added to an event type").await?;
        self.create_event_type(&repo, "platform:control-plane:event-type:schema-deprecated", "Event Type Schema Deprecated",
            "A schema version has been marked as deprecated").await?;
        self.create_event_type(&repo, "platform:control-plane:event-type:schema-activated", "Event Type Schema Activated",
            "A schema version has been activated as current").await?;

        self.create_event_type(&repo, "platform:control-plane:application:created", "Application Created",
            "A new application has been registered in the platform").await?;
        self.create_event_type(&repo, "platform:control-plane:application:updated", "Application Updated",
            "Application details have been updated").await?;
        self.create_event_type(&repo, "platform:control-plane:application:activated", "Application Activated",
            "Application has been activated").await?;
        self.create_event_type(&repo, "platform:control-plane:application:deactivated", "Application Deactivated",
            "Application has been deactivated").await?;
        self.create_event_type(&repo, "platform:control-plane:application:deleted", "Application Deleted",
            "Application has been deleted from the platform").await?;

        self.create_event_type(&repo, "platform:control-plane:role:created", "Role Created",
            "A new role has been created").await?;
        self.create_event_type(&repo, "platform:control-plane:role:updated", "Role Updated",
            "Role details or permissions have been updated").await?;
        self.create_event_type(&repo, "platform:control-plane:role:deleted", "Role Deleted",
            "Role has been deleted").await?;
        self.create_event_type(&repo, "platform:control-plane:role:synced", "Roles Synced",
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
