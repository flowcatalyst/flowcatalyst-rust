//! Shared shutdown signal handling.

use tokio::signal;
use tracing::info;

/// Wait for ctrl_c or SIGTERM, logging which was received.
/// Shared across fc-server and fc-dev.
#[expect(
    clippy::expect_used,
    reason = "installing a signal handler at start-up: the process cannot shut down cleanly without it"
)]
pub async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => { info!("Received Ctrl+C, shutting down"); }
        _ = terminate => { info!("Received SIGTERM, shutting down"); }
    }
}
