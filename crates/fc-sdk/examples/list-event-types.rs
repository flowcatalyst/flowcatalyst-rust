//! `cargo run --example list-event-types --features client`
//!
//! Smallest possible smoke test of the FlowCatalyst API client. Lists the
//! event types currently registered on the platform. Useful as the
//! "everything's wired correctly" check before reaching for a bigger
//! example.
//!
//! ## Run
//!
//! ```bash
//! export FC_BASE_URL=http://localhost:8080
//! export FC_TOKEN=<a-platform-access-token>
//! cargo run --example list-event-types --features client
//! ```

use fc_sdk::client::FlowCatalystClient;
use std::env;
use std::error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let base_url = env::var("FC_BASE_URL")?;
    let token = env::var("FC_TOKEN")?;

    let client = FlowCatalystClient::new(base_url).with_token(token);
    let event_types = client.event_types().list(None, None, None).await?;

    println!("Found {} event type(s):", event_types.items.len());
    for et in event_types.items {
        println!("  {:<48}  {}", et.code, et.name);
    }
    Ok(())
}
