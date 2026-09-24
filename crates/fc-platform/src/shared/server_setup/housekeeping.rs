//! Periodic housekeeping the platform runs alongside its HTTP server.

use std::sync::Arc;
use std::time::Duration;

use crate::OAuthClientRepository;

/// Clear OAuth secret-rotation overlaps once their window has closed, every
/// minute, so a superseded secret isn't kept at rest. Verification already
/// refuses an expired previous secret, so this is hygiene, not enforcement.
/// Go runs the same purge on its auth purger's one-minute tick
/// (internal/server/subsystems.go:538-580).
pub fn spawn_lapsed_previous_secret_purge(repo: Arc<OAuthClientRepository>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        tick.tick().await; // skip the immediate-fire tick
        loop {
            tick.tick().await;
            match repo.purge_lapsed_previous_secrets().await {
                Ok(n) if n > 0 => {
                    tracing::debug!(cleared = n, "lapsed oauth previous-secret purge")
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "lapsed oauth previous-secret purge failed"),
            }
        }
    });
}
