//! The router's environment, read with Go's names and semantics
//! (`internal/server/envcfg.go` `LoadEnv` + `run.go` `newRouterServer`),
//! plus this crate's historical names as fallbacks.
//!
//! The deployed task definition (`inhance/iac/compute/fc-router.ts`) is the
//! contract; `docs/parity/router-env-vs-go.md` maps every variable it sets.

use std::time::Duration;

use fc_common::config::parse_go_bool;
use fc_common::WarningSeverity;

use crate::notification::NotificationConfig;
use crate::standby::StandbyRouterConfig;

/// Go `ServerConfig.ConfigPollInterval`'s fallback for an unset, zero or
/// negative interval.
pub const DEFAULT_CONFIG_INTERVAL: Duration = Duration::from_secs(300);

/// Go `defaultNotifyBatchInterval`.
pub const DEFAULT_NOTIFY_BATCH_INTERVAL_SECS: u64 = 300;

/// Go `ServerConfig.DrainTimeout`'s fallback.
pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);

/// A router environment Go refuses to start with.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouterEnvError {
    #[error(
        "FC_ROUTER_CLIENT_ID and FC_ROUTER_CLIENT_SECRET must be set together (one without the other \
         cannot authenticate to the platform's router-config document)"
    )]
    HalfCredential,
    #[error(
        "FC_ROUTER_CLIENT_ID/SECRET need FC_ROUTER_PLATFORM_URL: the credential belongs to one platform, \
         and a comma-separated FLOWCATALYST_CONFIG_URL may list third-party config services the \
         credential must never be sent to"
    )]
    CredentialWithoutPlatform,
}

/// The router's OAuth client for its own platform.
#[derive(Clone, PartialEq, Eq)]
pub struct RouterCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for RouterCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouterCredentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

/// Everything the router reads from its environment.
#[derive(Debug, Clone)]
pub struct RouterEnv {
    /// `FLOWCATALYST_CONFIG_URL`, split on commas. Empty: the router runs
    /// with no queues and no pools (Go: no config source).
    pub config_urls: Vec<String>,
    /// `FC_ROUTER_CONFIG_INTERVAL_SECONDS`, alias
    /// `FLOWCATALYST_CONFIG_INTERVAL`; unset, unparseable or non-positive
    /// → 300s.
    pub config_interval: Duration,
    /// `FC_ROUTER_PLATFORM_URL`, then `FC_API_BASE_URL`, then
    /// `FLOWCATALYST_URL` (Go `RouterPlatformURL`). Turns on settle
    /// reporting and names the origin the credential may be sent to.
    pub platform_url: Option<String>,
    /// `FC_ROUTER_CLIENT_ID` + `FC_ROUTER_CLIENT_SECRET`.
    pub credentials: Option<RouterCredentials>,
    /// `FLOWCATALYST_DEV_MODE`.
    pub dev_mode: bool,
    /// `FC_ROUTER_STRICT_ROUTING`.
    pub strict_routing: bool,
    /// `FC_DRAIN_TIMEOUT_SECONDS`; non-positive → 60s.
    pub drain_timeout: Duration,
    /// `FC_ROUTER_SYNTH_POOL_IDLE_SECS`: `None` (unset, unparseable or 0)
    /// keeps the 1h default; negative disables the sweep.
    pub synth_pool_idle_secs: Option<i64>,
    /// `FC_ROUTER_DEFERRAL_BUDGET`; 0 keeps the default.
    pub deferral_budget: usize,
    /// `FC_ROUTER_HTTP_PREFIX` as given (`fc-server` defaults it to
    /// `/router`, the standalone binary to root-only).
    pub http_prefix: Option<String>,
    /// Standby / leader election for the standalone binary.
    pub standby: StandbyRouterConfig,
    /// Warning notifications.
    pub notification: NotificationConfig,
}

impl RouterEnv {
    /// Read the process environment.
    pub fn from_env() -> Result<Self, RouterEnvError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Read an environment through `get` (a test seam).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, RouterEnvError> {
        let env = Lookup(&get);

        let config_urls = env
            .first(&["FLOWCATALYST_CONFIG_URL"])
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|u| !u.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let config_interval = match env.int_alias(
            "FC_ROUTER_CONFIG_INTERVAL_SECONDS",
            "FLOWCATALYST_CONFIG_INTERVAL",
        ) {
            Some(secs) if secs > 0 => Duration::from_secs(secs as u64),
            _ => DEFAULT_CONFIG_INTERVAL,
        };

        let platform_url = env.first(&[
            "FC_ROUTER_PLATFORM_URL",
            "FC_API_BASE_URL",
            "FLOWCATALYST_URL",
        ]);

        // Go newRouterServer: refused at the composition root rather than
        // failing on every fetch.
        let client_id = env.first(&["FC_ROUTER_CLIENT_ID"]);
        let client_secret = env.first(&["FC_ROUTER_CLIENT_SECRET"]);
        let present = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.trim().is_empty());
        let credentials = match (present(&client_id), present(&client_secret)) {
            (false, false) => None,
            (true, true) => {
                if !platform_url
                    .as_deref()
                    .is_some_and(|u| !u.trim().is_empty())
                {
                    return Err(RouterEnvError::CredentialWithoutPlatform);
                }
                Some(RouterCredentials {
                    client_id: client_id.unwrap_or_default(),
                    client_secret: client_secret.unwrap_or_default(),
                })
            }
            _ => return Err(RouterEnvError::HalfCredential),
        };

        let drain_timeout = match env.int("FC_DRAIN_TIMEOUT_SECONDS") {
            Some(secs) if secs > 0 => Duration::from_secs(secs as u64),
            _ => DEFAULT_DRAIN_TIMEOUT,
        };

        let synth_pool_idle_secs = env
            .int("FC_ROUTER_SYNTH_POOL_IDLE_SECS")
            .filter(|&secs| secs != 0);

        let deferral_budget = env
            .int("FC_ROUTER_DEFERRAL_BUDGET")
            .filter(|&n| n > 0)
            .map_or(0, |n| n as usize);

        Ok(Self {
            config_urls,
            config_interval,
            platform_url,
            credentials,
            dev_mode: env.bool_first(&["FLOWCATALYST_DEV_MODE"], false),
            strict_routing: env.bool_first(&["FC_ROUTER_STRICT_ROUTING"], false),
            drain_timeout,
            synth_pool_idle_secs,
            deferral_budget,
            http_prefix: env.first(&["FC_ROUTER_HTTP_PREFIX"]),
            standby: standby_config(&env),
            notification: notification_config(&env),
        })
    }
}

/// Standby for the standalone binary. Go's names first (`FC_STANDBY_*`,
/// then its legacy `STANDBY_ENABLED` / `REDIS_URL`); this binary's own
/// `FLOWCATALYST_STANDBY_*` names stay as fallbacks. Leader election is how
/// HA is enforced, so every name that ever turned it on still does.
fn standby_config(env: &Lookup<'_>) -> StandbyRouterConfig {
    let defaults = StandbyRouterConfig::default();
    StandbyRouterConfig {
        enabled: env.bool_first(
            &[
                "FC_STANDBY_ENABLED",
                "STANDBY_ENABLED",
                "FLOWCATALYST_STANDBY_ENABLED",
            ],
            false,
        ),
        redis_url: env
            .first(&[
                "FC_STANDBY_REDIS_URL",
                "REDIS_URL",
                "FLOWCATALYST_STANDBY_REDIS_URL",
                "FLOWCATALYST_REDIS_URL",
            ])
            .unwrap_or(defaults.redis_url),
        lock_key: env
            .first(&["FC_STANDBY_LOCK_KEY", "FLOWCATALYST_STANDBY_LOCK_KEY"])
            .unwrap_or(defaults.lock_key),
        lock_ttl_seconds: env
            .int("FLOWCATALYST_STANDBY_LOCK_TTL")
            .filter(|&n| n > 0)
            .map_or(30, |n| n as u64),
        heartbeat_interval_seconds: env
            .int("FLOWCATALYST_STANDBY_HEARTBEAT_INTERVAL")
            .filter(|&n| n > 0)
            .map_or(10, |n| n as u64),
        instance_id: env
            .first(&["FLOWCATALYST_INSTANCE_ID", "HOSTNAME"])
            .unwrap_or_default(),
    }
}

/// Warning notifications.
///
/// - Webhook: Go's `FC_NOTIFY_WEBHOOK_URL` enables notifications on its own.
///   The deployed task sets this crate's historical
///   `NOTIFICATION_TEAMS_WEBHOOK_URL` + `NOTIFICATION_TEAMS_ENABLED` instead
///   (Go reads neither, so Go's router sends none): honoured unless
///   `NOTIFICATION_TEAMS_ENABLED` is explicitly false.
/// - Floor: `FC_NOTIFY_MIN_SEVERITY`, then `NOTIFICATION_MIN_SEVERITY`; an
///   unrecognised value keeps the WARNING floor (Go: a typo must never
///   reopen INFO).
/// - Batch interval: `FC_NOTIFY_BATCH_INTERVAL_SECONDS`, alias
///   `NOTIFICATION_BATCH_INTERVAL` (Go reads both); non-positive → 300s.
fn notification_config(env: &Lookup<'_>) -> NotificationConfig {
    let canonical = env.first(&["FC_NOTIFY_WEBHOOK_URL"]);
    let teams_flag = env
        .first(&["NOTIFICATION_TEAMS_ENABLED"])
        .and_then(|v| parse_go_bool(&v));
    let legacy = env
        .first(&["NOTIFICATION_TEAMS_WEBHOOK_URL"])
        .filter(|_| teams_flag != Some(false));
    let teams_webhook_url = canonical.or(legacy);

    let min_severity = env
        .first(&["FC_NOTIFY_MIN_SEVERITY", "NOTIFICATION_MIN_SEVERITY"])
        .and_then(|s| crate::warning::parse_severity(s.trim()))
        .unwrap_or(WarningSeverity::Warn);

    let batch_interval_seconds = env
        .int_alias(
            "FC_NOTIFY_BATCH_INTERVAL_SECONDS",
            "NOTIFICATION_BATCH_INTERVAL",
        )
        .filter(|&n| n > 0)
        .map_or(DEFAULT_NOTIFY_BATCH_INTERVAL_SECS, |n| n as u64);

    NotificationConfig {
        teams_enabled: teams_webhook_url.is_some(),
        teams_webhook_url,
        min_severity,
        batch_interval_seconds,
        #[cfg(feature = "email")]
        email_config: None,
    }
}

/// Go's env helpers over an injectable lookup.
struct Lookup<'a>(&'a dyn Fn(&str) -> Option<String>);

impl Lookup<'_> {
    /// Go `envFirst`: the first non-empty value.
    fn first(&self, keys: &[&str]) -> Option<String> {
        keys.iter()
            .filter_map(|k| (self.0)(k))
            .find(|v| !v.is_empty())
    }

    /// Go `envInt`: `None` when unset or unparseable.
    fn int(&self, key: &str) -> Option<i64> {
        (self.0)(key).and_then(|v| v.parse().ok())
    }

    /// Go `envIntAlias`: the primary when it parses, else the alias.
    fn int_alias(&self, key: &str, alias: &str) -> Option<i64> {
        self.int(key).or_else(|| self.int(alias))
    }

    /// Go `envBoolAlias`, N-way: the first non-empty name decides.
    fn bool_first(&self, keys: &[&str], default: bool) -> bool {
        self.first(keys)
            .map_or(default, |v| parse_go_bool(&v).unwrap_or(default))
    }
}

/// The built-in configuration `FLOWCATALYST_DEV_MODE` runs with: LocalStack
/// SQS queues (`LOCALSTACK_SQS_HOST`) and three pools.
pub fn dev_router_config() -> fc_common::RouterConfig {
    use fc_common::{PoolConfig, QueueConfig};
    let sqs_host = std::env::var("LOCALSTACK_SQS_HOST")
        .unwrap_or_else(|_| "http://sqs.eu-west-1.localhost.localstack.cloud:4566".to_string());
    let pool = |code: &str, concurrency, rate_limit_per_minute| PoolConfig {
        code: code.to_string(),
        concurrency,
        rate_limit_per_minute,
    };
    let queue = |name: &str, connections| QueueConfig {
        name: name.to_string(),
        uri: format!("{sqs_host}/000000000000/{name}"),
        connections,
        visibility_timeout: 120,
    };
    fc_common::RouterConfig {
        processing_pools: vec![
            pool("DEFAULT", 10, None),
            pool("HIGH", 20, None),
            pool("LOW", 5, Some(60)),
        ],
        queues: vec![
            queue("fc-high-priority.fifo", 2),
            queue("fc-default.fifo", 2),
            queue("fc-low-priority.fifo", 1),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> Result<RouterEnv, RouterEnvError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        RouterEnv::from_lookup(move |k| map.get(k).cloned())
    }

    /// The deployed task definition's variables (fake values).
    fn production() -> Vec<(&'static str, &'static str)> {
        vec![
            ("RUST_LOG", "info"),
            ("API_PORT", "8080"),
            ("AWS_REGION", "eu-west-1"),
            ("MESSAGE_ROUTER_ENABLED", "true"),
            ("PLATFORM_ENABLED", "false"),
            (
                "FLOWCATALYST_CONFIG_URL",
                "https://a.example/api/config,https://b.example/api/config,http://fc-platform:8080/api/dispatch/router-config",
            ),
            ("FLOWCATALYST_CONFIG_INTERVAL", "300"),
            ("FC_ROUTER_PLATFORM_URL", "http://fc-platform:8080"),
            ("FLOWCATALYST_STANDBY_ENABLED", "false"),
            ("AUTH_MODE", "NONE"),
            ("NOTIFICATION_TEAMS_ENABLED", "true"),
            ("NOTIFICATION_TEAMS_WEBHOOK_URL", "https://hooks.example/fake"),
            ("NOTIFICATION_MIN_SEVERITY", "WARNING"),
            ("NOTIFICATION_BATCH_INTERVAL", "60"),
            ("FC_ROUTER_CLIENT_ID", "oac_fake"),
            ("FC_ROUTER_CLIENT_SECRET", "fake-secret"),
        ]
    }

    #[test]
    fn the_production_task_definition_reads_as_intended() {
        let e = env(&production()).unwrap();
        assert_eq!(e.config_urls.len(), 3);
        assert_eq!(
            e.config_urls[2],
            "http://fc-platform:8080/api/dispatch/router-config"
        );
        assert_eq!(e.config_interval, Duration::from_secs(300));
        assert_eq!(e.platform_url.as_deref(), Some("http://fc-platform:8080"));
        assert_eq!(
            e.credentials,
            Some(RouterCredentials {
                client_id: "oac_fake".into(),
                client_secret: "fake-secret".into()
            })
        );
        assert!(!e.standby.enabled);
        assert!(e.notification.teams_enabled);
        assert_eq!(
            e.notification.teams_webhook_url.as_deref(),
            Some("https://hooks.example/fake")
        );
        assert_eq!(e.notification.min_severity, WarningSeverity::Warn);
        assert_eq!(e.notification.batch_interval_seconds, 60);
        assert_eq!(e.drain_timeout, Duration::from_secs(60));
        assert!(!e.dev_mode);
        assert!(
            !format!("{e:?}").contains("fake-secret"),
            "the secret is never printed"
        );
    }

    #[test]
    fn half_a_credential_or_one_without_a_platform_is_refused() {
        let mut half = production();
        half.retain(|(k, _)| *k != "FC_ROUTER_CLIENT_SECRET");
        assert_eq!(env(&half).unwrap_err(), RouterEnvError::HalfCredential);

        let mut blank_secret = production();
        blank_secret.push(("FC_ROUTER_CLIENT_SECRET", "  "));
        blank_secret.retain(|(k, v)| *k != "FC_ROUTER_CLIENT_SECRET" || *v == "  ");
        assert_eq!(
            env(&blank_secret).unwrap_err(),
            RouterEnvError::HalfCredential
        );

        let mut no_platform = production();
        no_platform.retain(|(k, _)| *k != "FC_ROUTER_PLATFORM_URL");
        assert_eq!(
            env(&no_platform).unwrap_err(),
            RouterEnvError::CredentialWithoutPlatform
        );

        let mut none = production();
        none.retain(|(k, _)| !k.starts_with("FC_ROUTER_CLIENT"));
        assert_eq!(env(&none).unwrap().credentials, None);
    }

    #[test]
    fn go_names_win_and_zero_or_bad_values_fall_back_to_go_defaults() {
        let e = env(&[
            ("FC_ROUTER_CONFIG_INTERVAL_SECONDS", "0"),
            ("FLOWCATALYST_CONFIG_INTERVAL", "30"),
            ("FC_NOTIFY_BATCH_INTERVAL_SECONDS", "15"),
            ("NOTIFICATION_BATCH_INTERVAL", "60"),
            ("FC_NOTIFY_MIN_SEVERITY", "loud"),
            ("FC_DRAIN_TIMEOUT_SECONDS", "0"),
            ("FC_API_BASE_URL", "http://platform:8080"),
        ])
        .unwrap();
        // Go envIntAlias: a parseable primary wins, and 0 then means default.
        assert_eq!(e.config_interval, DEFAULT_CONFIG_INTERVAL);
        assert_eq!(e.notification.batch_interval_seconds, 15);
        assert_eq!(e.notification.min_severity, WarningSeverity::Warn);
        assert_eq!(e.drain_timeout, DEFAULT_DRAIN_TIMEOUT);
        assert_eq!(e.platform_url.as_deref(), Some("http://platform:8080"));
        assert!(e.config_urls.is_empty());

        let e = env(&[("FLOWCATALYST_CONFIG_INTERVAL", "30")]).unwrap();
        assert_eq!(e.config_interval, Duration::from_secs(30));
        assert_eq!(e.notification.batch_interval_seconds, 300);
        assert!(!e.notification.teams_enabled);
    }

    #[test]
    fn notification_webhook_names() {
        // Go's name alone enables.
        let e = env(&[("FC_NOTIFY_WEBHOOK_URL", "https://go")]).unwrap();
        assert_eq!(
            e.notification.teams_webhook_url.as_deref(),
            Some("https://go")
        );
        // The legacy URL is honoured unless its flag is explicitly off.
        let e = env(&[("NOTIFICATION_TEAMS_WEBHOOK_URL", "https://legacy")]).unwrap();
        assert!(e.notification.teams_enabled);
        let e = env(&[
            ("NOTIFICATION_TEAMS_WEBHOOK_URL", "https://legacy"),
            ("NOTIFICATION_TEAMS_ENABLED", "false"),
        ])
        .unwrap();
        assert!(!e.notification.teams_enabled);
        assert_eq!(e.notification.teams_webhook_url, None);
    }

    #[test]
    fn standby_names() {
        assert!(
            env(&[("FC_STANDBY_ENABLED", "TRUE")])
                .unwrap()
                .standby
                .enabled
        );
        assert!(env(&[("STANDBY_ENABLED", "yes")]).unwrap().standby.enabled);
        assert!(
            env(&[("FLOWCATALYST_STANDBY_ENABLED", "true")])
                .unwrap()
                .standby
                .enabled
        );
        // The first set name decides (Go envBoolAlias).
        assert!(
            !env(&[
                ("FC_STANDBY_ENABLED", "false"),
                ("FLOWCATALYST_STANDBY_ENABLED", "true")
            ])
            .unwrap()
            .standby
            .enabled
        );
    }
}
