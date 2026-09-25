//! One router, wired from a [`RouterEnv`] — shared by the standalone
//! `fc-router` binary and `fc-server`'s router role, so both honour the same
//! environment the same way (Go: one `newRouterServer` for every caller).

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use fc_queue::QueuePublisher;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use super::env::{dev_router_config, RouterEnv};
use crate::api::{create_router_with_options, AuthMode, RouterDeps, RouterOptions};
use crate::config_sync::{ConfigSyncConfig, ConfigSyncService};
use crate::health::{HealthService, HealthServiceConfig};
use crate::lifecycle::{LifecycleConfig, LifecycleManager};
use crate::manager::{stall_config_for_mediation_timeout, ConsumerFactory, QueueManager};
use crate::mediator::HttpMediatorConfig;
use crate::notification::{
    create_notification_service_with_scheduler, NotificationServiceWithScheduler,
};
use crate::platform_token::PlatformTokenSource;
use crate::settled::HttpSettledReporter;
use crate::standby::StandbyAwareProcessor;
use crate::warning::{WarningService, WarningServiceConfig};

/// What the embedding binary supplies.
pub struct RouterRuntimeOptions {
    /// Builds a consumer for each configured queue (scheme dispatch lives
    /// with the binary's broker clients — see
    /// [`super::SchemeConsumerFactory`] with the `backends` feature).
    pub consumer_factory: Arc<dyn ConsumerFactory>,
    /// The standalone binary's own leader election, if enabled.
    pub standby: Option<Arc<StandbyAwareProcessor>>,
    /// An outside leadership signal (`fc-server`'s election): the router
    /// polls only while it reads `true`.
    pub leadership: Option<watch::Receiver<bool>>,
}

/// A running router: services, queue manager, config watcher, lifecycle
/// tasks.
pub struct RouterRuntime {
    pub queue_manager: Arc<QueueManager>,
    pub warning_service: Arc<WarningService>,
    pub health_service: Arc<HealthService>,
    pub config_sync: Option<Arc<ConfigSyncService>>,
    pub standby: Option<Arc<StandbyAwareProcessor>>,
    lifecycle: LifecycleManager,
    drain_timeout: Duration,
    manager_handle: Option<JoinHandle<()>>,
    /// Keeps the notification batch scheduler alive for the runtime's life.
    _notifications: Option<NotificationServiceWithScheduler>,
}

impl RouterRuntime {
    /// Build and start the router. Never waits for a configuration: the
    /// watcher retries in the background (Go `Watch`), so the caller binds
    /// HTTP straight away whatever state the config sources are in.
    pub async fn start(env: &RouterEnv, opts: RouterRuntimeOptions) -> crate::Result<Self> {
        let notifications = create_notification_service_with_scheduler(&env.notification);
        let warning_service = Arc::new(match notifications {
            Some(ref ns) => {
                info!(
                    batch_interval = env.notification.batch_interval_seconds,
                    min_severity = ?env.notification.min_severity,
                    "Notification service enabled (Teams webhook with batching)"
                );
                WarningService::with_notification(
                    WarningServiceConfig::default(),
                    ns.service.clone(),
                )
            }
            None => {
                info!("Notification service disabled - no webhook configured");
                WarningService::new(WarningServiceConfig::default())
            }
        });
        let health_service = Arc::new(HealthService::new(
            HealthServiceConfig::default(),
            warning_service.clone(),
        ));

        info!(
            strict_routing = env.strict_routing,
            "Strict routing gate set"
        );
        let mut builder = QueueManager::builder(HttpMediatorConfig::production())
            // Go's DefaultStallConfig: derived from the mediation timeout,
            // force-NACK off.
            .stall_config(stall_config_for_mediation_timeout(
                HttpMediatorConfig::production().timeout,
            ))
            .warning_service(warning_service.clone())
            .health_service(health_service.clone())
            .consumer_factory(opts.consumer_factory)
            .strict_routing(env.strict_routing)
            .deferral_budget(env.deferral_budget);
        // A-01: a platform to report settled BLOCK_ON_ERROR siblings to.
        if let Some(url) = env.platform_url.as_deref().filter(|u| !u.trim().is_empty()) {
            let reporter = HttpSettledReporter::new(url, None);
            info!(url = %reporter.url(), "Router: settled-message hook enabled");
            builder = builder.settled_reporter(Arc::new(reporter));
        }
        let queue_manager = Arc::new(builder.build());

        // Leadership only pauses polling (owner ruling); HTTP is served
        // whatever it says (H14).
        match opts.leadership {
            Some(mut rx) => {
                queue_manager.set_leader(*rx.borrow());
                let follower = queue_manager.clone();
                tokio::spawn(async move {
                    while rx.changed().await.is_ok() {
                        let leader = *rx.borrow();
                        follower.set_leader(leader);
                    }
                });
            }
            None => {
                queue_manager.set_leader(opts.standby.as_ref().is_none_or(|s| s.is_leader()));
            }
        }

        // Configuration: dev mode's built-in document, else the watched
        // sources (Go: one code path — the config URL, and nothing else).
        let config_sync = if env.dev_mode {
            let config = dev_router_config();
            info!(
                queues = config.queues.len(),
                pools = config.processing_pools.len(),
                "Development mode: built-in LocalStack configuration"
            );
            queue_manager.reload_config(config).await?;
            None
        } else if env.config_urls.is_empty() {
            warn!(
                "FLOWCATALYST_CONFIG_URL is not set: the router runs with no queues and no pools"
            );
            None
        } else {
            let mut sync_config = ConfigSyncConfig::new(env.config_urls.join(","));
            sync_config.sync_interval = env.config_interval;
            let mut service =
                ConfigSyncService::new(sync_config, queue_manager.clone(), warning_service.clone());
            if let (Some(creds), Some(platform_url)) =
                (env.credentials.as_ref(), env.platform_url.as_deref())
            {
                let token = Arc::new(PlatformTokenSource::new(
                    platform_url,
                    creds.client_id.clone(),
                    creds.client_secret.clone(),
                    service.http_client().clone(),
                ));
                info!(
                    platform_url = %platform_url,
                    client_id = %creds.client_id,
                    "Router: config document fetched with client credentials"
                );
                service = service.with_credentials(token, platform_url);
            }
            info!(
                urls = ?env.config_urls,
                interval = ?env.config_interval,
                "Configuration sync enabled"
            );
            Some(Arc::new(service))
        };

        let mut lifecycle_config = LifecycleConfig::default();
        // R-59: a negative idle TTL disables the synthesised-pool sweep.
        if let Some(secs) = env.synth_pool_idle_secs {
            lifecycle_config.synth_pool_idle_ttl = if secs < 0 {
                Duration::ZERO
            } else {
                Duration::from_secs(secs as u64)
            };
        }
        let cb_max_idle = lifecycle_config.circuit_breaker_max_idle;
        let mut lifecycle = LifecycleManager::start_with_features(
            queue_manager.clone(),
            warning_service.clone(),
            health_service.clone(),
            lifecycle_config,
            config_sync.clone(),
            opts.standby.clone(),
        );
        lifecycle.spawn_circuit_breaker_eviction(
            queue_manager.circuit_breaker_registry().clone(),
            cb_max_idle,
        );

        // Consumers created by config sync start their own poll loops; this
        // runs the in-pipeline reaper and any consumer registered up front.
        let manager = queue_manager.clone();
        let manager_handle = tokio::spawn(async move {
            if let Err(e) = manager.start().await {
                tracing::error!(error = %e, "QueueManager error");
            }
        });

        Ok(Self {
            queue_manager,
            warning_service,
            health_service,
            config_sync,
            standby: opts.standby,
            lifecycle,
            drain_timeout: env.drain_timeout,
            manager_handle: Some(manager_handle),
            _notifications: notifications,
        })
    }

    /// The router's HTTP surface (monitoring, health, dashboard, publish),
    /// authenticated per `AUTH_MODE` / `FC_ROUTER_AUTH_*`. `http_prefix`
    /// additionally nests it under that path.
    pub fn api_router(
        &self,
        publisher: Arc<dyn QueuePublisher>,
        metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
        http_prefix: Option<String>,
    ) -> Router {
        let auth_config = crate::api::AuthConfig::from_env();
        let auth_state = if auth_config.mode != AuthMode::None {
            info!(mode = ?auth_config.mode, "Router API authentication configured");
            Some(crate::api::create_auth_state(auth_config))
        } else {
            info!("Router API authentication disabled (AUTH_MODE=NONE or no credentials)");
            None
        };
        let config_reloader = self
            .config_sync
            .clone()
            .map(|s| s as Arc<dyn crate::api::ConfigReloader>);
        create_router_with_options(
            RouterDeps {
                publisher,
                queue_manager: self.queue_manager.clone(),
                warning_service: self.warning_service.clone(),
                health_service: self.health_service.clone(),
                circuit_breaker_registry: self.queue_manager.circuit_breaker_registry().clone(),
            },
            RouterOptions {
                standby_enabled: self.standby.is_some(),
                instance_id: self
                    .standby
                    .as_ref()
                    .map(|s| s.instance_id().to_string())
                    .unwrap_or_else(|| "default".to_string()),
                metrics_handle,
                auth_state,
                router_http_prefix: http_prefix,
                config_reloader,
                ..RouterOptions::default()
            },
        )
    }

    /// Whether this instance currently processes messages.
    pub fn is_leader(&self) -> bool {
        self.lifecycle.is_leader()
    }

    /// Go's order: stop polling, drain the pools (bounded by
    /// `FC_DRAIN_TIMEOUT_SECONDS`), tear the manager down; only then stop
    /// the lifecycle tasks and release leadership.
    pub async fn shutdown(mut self) {
        self.queue_manager
            .shutdown_with_timeout(self.drain_timeout)
            .await;
        self.lifecycle.shutdown().await;
        if let Some(handle) = self.manager_handle.take() {
            match tokio::time::timeout(Duration::from_secs(30), handle).await {
                Ok(_) => info!("Manager task completed gracefully"),
                Err(_) => warn!("Manager task did not complete within 30s timeout"),
            }
        }
    }
}
