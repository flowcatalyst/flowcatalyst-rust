pub mod config;
pub mod dispatch_job_projection;
pub mod event_fan_out;
pub mod event_projection;
pub mod health;
pub mod partition_manager;

pub use config::StreamProcessorConfig;
pub use event_fan_out::EventFanOutConfig;
pub use health::{
    AggregatedHealth, StreamHealth, StreamHealthService, StreamHealthSnapshot, StreamHealthStatus,
    StreamProcessorHealth, StreamStatus,
};
pub use partition_manager::PartitionManagerConfig;

use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Handle returned by `start_stream_processor` to control the running projections.
pub struct StreamProcessorHandle {
    cancel: CancellationToken,
    tasks: TaskTracker,
}

impl StreamProcessorHandle {
    /// Signal all projection loops to stop and wait for them to finish.
    pub async fn stop(self) {
        self.cancel.cancel();
        self.tasks.wait().await;
    }
}

/// Dropping the handle without `stop()` still signals the loops to stop
/// (without waiting for them), as dropping the old per-service shutdown
/// senders did.
impl Drop for StreamProcessorHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Start the stream processor projection loops.
///
/// Returns a `StreamProcessorHandle` to stop them, and a `StreamHealthService`
/// pre-populated with health trackers for all enabled projections.
pub fn start_stream_processor(
    pool: sqlx::PgPool,
    config: StreamProcessorConfig,
) -> (StreamProcessorHandle, StreamHealthService) {
    let mut health_service = StreamHealthService::new();
    let cancel = CancellationToken::new();
    let tasks = TaskTracker::new();

    let mut register = |name: &str| {
        let health = Arc::new(StreamHealth::new(name.to_string()));
        health_service.register(health.clone());
        health
    };

    if config.events_enabled {
        let health = register(event_projection::HEALTH_NAME);
        tasks.spawn(event_projection::run(
            pool.clone(),
            config.events_batch_size,
            health,
            cancel.clone(),
        ));
    }

    if config.dispatch_jobs_enabled {
        let health = register(dispatch_job_projection::HEALTH_NAME);
        tasks.spawn(dispatch_job_projection::run(
            pool.clone(),
            config.dispatch_jobs_batch_size,
            health,
            cancel.clone(),
        ));
    }

    if config.fan_out_enabled {
        let health = register(event_fan_out::HEALTH_NAME);
        let fan_out_config = EventFanOutConfig {
            batch_size: config.fan_out_batch_size,
            subscription_refresh: std::time::Duration::from_secs(
                config.fan_out_subscription_refresh_secs,
            ),
        };
        tasks.spawn(event_fan_out::run(
            pool.clone(),
            fan_out_config,
            health,
            cancel.clone(),
        ));
    }

    if config.partition_manager_enabled {
        let health = register(partition_manager::HEALTH_NAME);
        tasks.spawn(partition_manager::run(
            pool,
            PartitionManagerConfig::default(),
            health,
            cancel.clone(),
        ));
    }

    tasks.close();
    (StreamProcessorHandle { cancel, tasks }, health_service)
}
