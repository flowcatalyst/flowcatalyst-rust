//! `cargo run --example fc-sync --features client`
//!
//! Rust analogue of the TypeScript SDK's `"fc:sync": "tsx scripts/fc-sync.ts"`.
//!
//! Pushes role + event-type + subscription definitions to the platform for a
//! single application. Idempotent — re-run after editing the in-code
//! definitions to apply changes.
//!
//! ## Run
//!
//! ```bash
//! export FC_BASE_URL=http://localhost:8080
//! export FC_TOKEN=<a-platform-access-token>
//! export FC_APP_CODE=billing
//! cargo run --example fc-sync --features client
//! ```
//!
//! For a tighter inner loop, build once and invoke the binary directly:
//!
//! ```bash
//! cargo build --release --example fc-sync --features client
//! ./target/release/examples/fc-sync     # ~20ms cold-start, no cargo tax
//! ```
//!
//! ## Bind to `just`
//!
//! ```just
//! fc-sync:
//!     cargo run --example fc-sync --features client
//! ```

use fc_sdk::client::FlowCatalystClient;
use fc_sdk::sync::{
    DefinitionSet, DefinitionSynchronizer, EventTypeDefinition, RoleDefinition,
    SubscriptionBinding, SubscriptionDefinition, SyncOptions,
};
use std::env;
use std::error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let base_url = env::var("FC_BASE_URL")?;
    let token = env::var("FC_TOKEN")?;
    let app = env::var("FC_APP_CODE").unwrap_or_else(|_| "billing".to_string());

    let client = FlowCatalystClient::new(base_url).with_token(token);

    let set = DefinitionSet::for_application(&app)
        // ── Roles ───────────────────────────────────────────────────────
        .add_role(
            RoleDefinition::make(format!("{app}:admin"))
                .with_display_name("Billing Admin")
                .with_permissions(vec![
                    "invoice:create".into(),
                    "invoice:read".into(),
                    "invoice:void".into(),
                ]),
        )
        .add_role(
            RoleDefinition::make(format!("{app}:viewer"))
                .with_display_name("Billing Viewer")
                .with_permissions(vec!["invoice:read".into()]),
        )
        // ── Event types ─────────────────────────────────────────────────
        .add_event_type(EventTypeDefinition::make(
            format!("{app}:invoices:invoice:created"),
            "Invoice Created",
        ))
        .add_event_type(EventTypeDefinition::make(
            format!("{app}:invoices:invoice:voided"),
            "Invoice Voided",
        ))
        // ── Subscriptions ───────────────────────────────────────────────
        .add_subscription(
            SubscriptionDefinition::make(
                format!("{app}-internal"),
                format!("{app} → internal"),
                format!("https://{app}.internal/webhook"),
            )
            .with_description("Internal webhook target")
            .add_event_type(SubscriptionBinding::make(format!(
                "{app}:invoices:invoice:created"
            ))),
        );

    let synchronizer = DefinitionSynchronizer::new(client);
    let result = synchronizer.sync(&set, &SyncOptions::default()).await;

    println!("✓ sync complete for application '{app}'");
    println!("  roles:         {:?}", result.roles);
    println!("  event_types:   {:?}", result.event_types);
    println!("  subscriptions: {:?}", result.subscriptions);
    Ok(())
}
