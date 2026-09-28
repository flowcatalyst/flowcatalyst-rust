//! `cargo run --example scheduled-jobs-runner --features scheduled-jobs-runner,axum`
//!
//! End-to-end example of consuming scheduled-job firings from the
//! FlowCatalyst platform. Boots an Axum HTTP server that:
//!
//!   1. Registers handlers for two job codes (`daily-rollup`, `cleanup`).
//!   2. Wraps them in a `ScheduledJobRunner` with a `MemoryLockProvider`
//!      so concurrent fires (within this process) are serialised.
//!   3. Exposes a single `POST /scheduled-jobs` endpoint that hands every
//!      platform-fired envelope to the runner.
//!
//! Point your scheduled-job definitions' `target_url` at
//! `http://your-host:4001/scheduled-jobs` and the platform will call this
//! server on each cron tick.
//!
//! ## Run
//!
//! ```bash
//! export FC_BASE_URL=http://localhost:8080
//! export FC_TOKEN=<a-platform-access-token>
//! cargo run --example scheduled-jobs-runner --features scheduled-jobs-runner,axum
//! ```
//!
//! ## What gets reported back to the platform
//!
//! For jobs declared with `tracksCompletion: true`, the runner POSTs a
//! `complete_instance` call after the handler finishes (SUCCESS with the
//! returned JSON, or FAILURE with the error message). Lock contention
//! reports as FAILURE with `{"skipped": true, "reason": "lock-held"}`.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use fc_sdk::client::FlowCatalystClient;
use fc_sdk::lock::MemoryLockProvider;
use fc_sdk::scheduled_jobs::{
    HandlerError, HandlerFuture, LogOptions, RunResult, ScheduledJobRunner,
};
use std::env;
use std::error;
use tokio::net::TcpListener;

#[derive(Clone)]
struct AppState {
    runner: Arc<ScheduledJobRunner>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let base_url = env::var("FC_BASE_URL")?;
    let token = env::var("FC_TOKEN")?;

    let client = FlowCatalystClient::new(base_url).with_token(token);

    let runner = ScheduledJobRunner::builder(client, Arc::new(MemoryLockProvider::new()))
        .handler("daily-rollup", |ctx| {
            Box::pin(async move {
                ctx.log("starting daily rollup", LogOptions::default())
                    .await;

                // ── your work goes here ──
                let processed = 42usize;

                ctx.log(
                    "rollup complete",
                    LogOptions {
                        metadata: Some(serde_json::json!({ "processed": processed })),
                        ..Default::default()
                    },
                )
                .await;
                Ok::<_, HandlerError>(serde_json::json!({ "processed": processed }))
            }) as HandlerFuture
        })
        .handler("cleanup", |_ctx| {
            Box::pin(async move {
                // Return an error to demonstrate the FAILURE completion path.
                Err::<serde_json::Value, _>(HandlerError::msg("nothing to clean up"))
            }) as HandlerFuture
        })
        .on_error(|err, env| {
            eprintln!("scheduled-job runner error on {}: {err:?}", env.instance_id,);
        })
        .build();

    let state = AppState {
        runner: Arc::new(runner),
    };
    let app = Router::new()
        .route("/scheduled-jobs", post(handle_firing))
        .with_state(state);

    let listener = TcpListener::bind("0.0.0.0:4001").await?;
    println!("scheduled-jobs runner listening on http://0.0.0.0:4001");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_firing(
    State(state): State<AppState>,
    Json(envelope): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.runner.process(envelope) {
        RunResult::Accepted => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "ok": true })),
        ),
        RunResult::BadRequest(msg) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        ),
        RunResult::NotFound(msg) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": msg })),
        ),
    }
}
