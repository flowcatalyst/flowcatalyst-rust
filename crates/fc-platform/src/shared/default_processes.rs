//! Seed the platform's built-in example Process: an on-demand fulfilment
//! workflow rendered as a single Mermaid flowchart. Operators land on the
//! Processes page with one well-formed example showing the conventions
//! (three-segment code, Mermaid body, tags, application=`platform`).
//!
//! Falls under the "platform infrastructure processing" exception in
//! CLAUDE.md — bootstrap-only, runs before HTTP serving begins, no
//! executing principal, so writes go directly to the repository rather
//! than through `UseCase` / `UnitOfWork`. Same exception used by
//! `seed_builtin_roles`, `seed_platform_application`, and
//! `bootstrap_admin_user`.
//!
//! Idempotent: skips if a row with `code = platform:fulfilment:on-demand-flow`
//! already exists. Once seeded, the row is editable like any other Process —
//! re-running the seeder does not overwrite operator edits.

use sqlx::PgPool;
use tracing::info;

use crate::process::entity::{Process, ProcessSource};
use crate::process::repository::ProcessRepository;

const EXAMPLE_CODE: &str = "platform:fulfilment:on-demand-flow";
const EXAMPLE_NAME: &str = "On-Demand Fulfilment Flow";
const EXAMPLE_DESCRIPTION: &str =
    "Reference workflow covering order placement, geocoding, picking, packing, trip creation, \
     vehicle assignment, execution, and the cancel / abort branches. Edit or archive freely — \
     the seeder only writes if no row with this code exists.";

/// Mermaid source for the example workflow. Edit here and bump a fresh DB
/// to re-seed (the seeder is one-shot per code).
const EXAMPLE_BODY: &str = r#"flowchart TD
    Start([Customer places order]) --> OrderCreated[OrderCreated]
    OrderCreated --> GeoCheck{Address geocoded?}

    GeoCheck -- No --> GeoJob[Dispatch geocoding job]
    GeoJob --> GeoResult{Resolved?}
    GeoResult -- No --> GeoHold[Order on hold — geocoding failed]
    GeoHold --> NotifyCustomer[Notify customer]
    NotifyCustomer --> EndHold([Awaiting address fix])
    GeoResult -- Yes --> Reserve
    GeoCheck -- Yes --> Reserve[Reserve inventory at WMS]

    Reserve --> Stock{Stock available?}
    Stock -- No --> Backorder[Backorder created]
    Backorder --> EndBackorder([Backordered])
    Stock -- Yes --> Pick[Pick goods]

    Pick --> CancelEarly{Cancellation requested?}
    CancelEarly -- Yes --> ReleaseInv[Release inventory]
    ReleaseInv --> Refund[Refund customer]
    Refund --> EndCancel([Order cancelled])
    CancelEarly -- No --> Pack[Pack parcels]

    Pack --> Ready[FulfilmentReady]
    Ready --> CreateTrip[Create trip]
    CreateTrip --> AssignVehicle[Assign vehicle and driver]
    AssignVehicle --> Load[Load vehicle at depot]
    Load --> Depart[Depart depot]
    Depart --> EnRoute[En route]

    EnRoute --> AbortCheck{Abort signal?}
    AbortCheck -- Yes --> AbortReturn[Return to depot]
    AbortReturn --> ReverseLogistics[Restock inventory]
    ReverseLogistics --> EndAbort([Trip aborted])
    AbortCheck -- No --> Arrive[Arrive at customer]

    Arrive --> POD{Proof of delivery captured?}
    POD -- No --> Failed[Delivery failed]
    Failed --> Reschedule{Reschedule attempt?}
    Reschedule -- Yes --> CreateTrip
    Reschedule -- No --> ReverseLogistics
    POD -- Yes --> Complete[FulfilmentCompleted]
    Complete --> Invoice[Generate invoice]
    Invoice --> EndDelivered([Delivered])

    classDef happy fill:#d4edda,stroke:#28a745,color:#155724;
    classDef exception fill:#f8d7da,stroke:#dc3545,color:#721c24;
    classDef terminal fill:#e2e3e5,stroke:#6c757d,color:#383d41;
    class OrderCreated,Reserve,Pick,Pack,Ready,CreateTrip,AssignVehicle,Load,Depart,EnRoute,Arrive,Complete,Invoice happy;
    class GeoJob,GeoHold,NotifyCustomer,Backorder,ReleaseInv,Refund,AbortReturn,ReverseLogistics,Failed exception;
    class Start,EndHold,EndBackorder,EndCancel,EndAbort,EndDelivered terminal;
"#;

/// Seed the example on-demand fulfilment Process. No-op once seeded.
pub async fn seed_default_processes(pool: &PgPool) -> Result<(), sqlx::Error> {
    let repo = ProcessRepository::new(pool);

    let exists = repo
        .exists_by_code(EXAMPLE_CODE)
        .await
        .map_err(|e| sqlx::Error::Protocol(format!("exists_by_code({}): {}", EXAMPLE_CODE, e)))?;
    if exists {
        return Ok(());
    }

    let mut process = Process::new(EXAMPLE_CODE, EXAMPLE_NAME)
        .map_err(|e| sqlx::Error::Protocol(format!("Process::new: {}", e)))?;
    process.description = Some(EXAMPLE_DESCRIPTION.to_string());
    process.source = ProcessSource::Code;
    process.body = EXAMPLE_BODY.to_string();
    process.tags = vec![
        "example".to_string(),
        "fulfilment".to_string(),
        "platform".to_string(),
    ];

    repo.insert(&process)
        .await
        .map_err(|e| sqlx::Error::Protocol(format!("insert(example process): {}", e)))?;

    info!(
        code = EXAMPLE_CODE,
        "Seeded example on-demand fulfilment process"
    );
    Ok(())
}
