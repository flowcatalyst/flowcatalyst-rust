//! Where a claimed dispatch job is published, and under which pool code.
//!
//! A port of Go's dispatch naming rules (`internal/platform/dispatch/settings.go`,
//! `internal/platform/shared/dispatchqueue`, `scheduler/destination.go`,
//! `scheduler/poolcode.go`). Every job goes to its tenant's queue for its
//! priority — `{prefix}-{tenant}-{priority}` (`.fifo`-suffixed on SQS) — and
//! carries a client-namespaced pool code. Composing exactly as Go does is
//! what lets a Go router (or a Rust one fed the same queue list) consume what
//! this scheduler publishes.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use sqlx::PgPool;
use tracing::{debug, warn};

/// The tenant segment for client-less jobs.
pub const TENANT_PLATFORM: &str = "platform";
/// SQS's cap on a queue name, `.fifo` included.
pub const SQS_MAX_NAME_LENGTH: usize = 80;
/// The router's global fallback pool.
pub const DEFAULT_POOL_CODE: &str = "DEFAULT-POOL";
/// The suffix of every per-tenant fallback pool.
pub const DEFAULT_POOL_SUFFIX: &str = "-DEFAULT-POOL";

/// A dispatch priority: which of a tenant's two queues a job goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Default,
    HighPriority,
}

impl Priority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::Default => "DEFAULT",
            Priority::HighPriority => "HIGH_PRIORITY",
        }
    }

    fn recognise(stored: &str) -> Option<Priority> {
        match stored.trim().to_ascii_uppercase().as_str() {
            "DEFAULT" => Some(Priority::Default),
            "HIGH_PRIORITY" => Some(Priority::HighPriority),
            _ => None,
        }
    }

    /// The job's own claim (`msg_dispatch_jobs.queue`): `None` when absent,
    /// blank or unrecognised, so the subscription's is consulted instead
    /// (Go `dispatchqueue.ForJob`).
    pub fn for_job(stored: Option<&str>) -> Option<Priority> {
        stored.and_then(Priority::recognise)
    }

    /// A subscription's stored value on the publish path: lenient, anything
    /// unusable is DEFAULT (Go `dispatchqueue.ForPublishing`).
    pub fn for_publishing(stored: Option<&str>) -> Priority {
        match stored.and_then(Priority::recognise) {
            Some(Priority::HighPriority) => Priority::HighPriority,
            _ => Priority::Default,
        }
    }
}

/// Why a queue name could not be composed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ComposeError {
    #[error("a tenant is required to compose a dispatch queue name")]
    TenantRequired,
    #[error("dispatch queue name {composed:?} ({} chars) exceeds SQS's {SQS_MAX_NAME_LENGTH}-character limit for tenant {tenant:?}", composed.len())]
    NameTooLong { tenant: String, composed: String },
}

/// `{prefix}-{tenant}-{priority}`, `.fifo`-suffixed and length-capped when
/// `sqs`. A blank prefix omits its segment. Only the tenant is sanitised
/// (`_`, `.` and space become `-`). Go `dispatchqueue.ComposeName`.
pub fn compose_name(
    prefix: &str,
    tenant: &str,
    priority: Priority,
    sqs: bool,
) -> Result<String, ComposeError> {
    if tenant.trim().is_empty() {
        return Err(ComposeError::TenantRequired);
    }
    let mut name = String::new();
    if !prefix.trim().is_empty() {
        name.push_str(prefix);
        name.push('-');
    }
    name.extend(tenant.chars().map(|c| match c {
        '_' | '.' | ' ' => '-',
        other => other,
    }));
    name.push('-');
    name.push_str(priority.as_str());
    if !sqs {
        return Ok(name);
    }
    name.push_str(".fifo");
    if name.len() > SQS_MAX_NAME_LENGTH {
        return Err(ComposeError::NameTooLong {
            tenant: tenant.to_string(),
            composed: name,
        });
    }
    Ok(name)
}

/// `{clientIdentifier}-{poolCode}` for a client-owned pool,
/// `platform-{poolCode}` for a platform-level one.
pub fn compose_pool_code(pool_code: &str, client_identifier: Option<&str>) -> String {
    let tenant = client_identifier
        .filter(|s| !s.is_empty())
        .unwrap_or(TENANT_PLATFORM);
    format!("{tenant}-{pool_code}")
}

/// Where dispatch queues live, resolved once at startup (Go
/// `dispatch.ResolveSettings`, minus Go's "anything else is Postgres":
/// here an unset type refuses to start the scheduler rather than publish
/// where no router may be listening).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchQueueSettings {
    pub kind: DispatchQueueKind,
    /// `FC_DISPATCH_QUEUE_PREFIX`, e.g. `FC-staging`.
    pub prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchQueueKind {
    /// Per-tenant SQS FIFO queues in this account and region.
    Sqs { region: String, account_id: String },
    /// Per-tenant rows in the platform database's `queue_messages`.
    Postgres,
}

impl DispatchQueueSettings {
    pub fn is_sqs(&self) -> bool {
        matches!(self.kind, DispatchQueueKind::Sqs { .. })
    }

    /// Resolve from the raw settings. `queue_type` is `SQS` or `POSTGRES`
    /// (any case); blank is "no queue configured", an error.
    pub fn resolve(
        queue_type: &str,
        queue_url: &str,
        queue_region: &str,
        prefix: &str,
    ) -> Result<Self, String> {
        let kind = queue_type.trim().to_ascii_uppercase();
        match kind.as_str() {
            "" => Err(
                "no dispatch queue is configured: set FC_DISPATCH_QUEUE_TYPE \
                 (alias DISPATCH_QUEUE_TYPE) to SQS or POSTGRES; the scheduler will not \
                 claim jobs it has nowhere to publish"
                    .to_string(),
            ),
            "POSTGRES" | "POSTGRESQL" => Ok(Self {
                kind: DispatchQueueKind::Postgres,
                prefix: prefix.trim().to_string(),
            }),
            "SQS" => {
                if prefix.trim().is_empty() {
                    return Err(
                        "FC_DISPATCH_QUEUE_PREFIX is required when FC_DISPATCH_QUEUE_TYPE=SQS: \
                         without it, dispatch queues would be named literally \"FC-{env}-...\" \
                         instead of a real per-deployment prefix"
                            .to_string(),
                    );
                }
                let region = if queue_region.trim().is_empty() {
                    region_from_sqs_url(queue_url)
                } else {
                    queue_region.trim().to_string()
                };
                let account_id = account_from_sqs_url(queue_url);
                if region.is_empty() || account_id.is_empty() {
                    return Err(format!(
                        "FC_DISPATCH_QUEUE_TYPE=SQS needs an account id and region to compose \
                         queue URLs (account={account_id:?}, region={region:?}, \
                         FC_DISPATCH_QUEUE_URL={queue_url:?})"
                    ));
                }
                Ok(Self {
                    kind: DispatchQueueKind::Sqs { region, account_id },
                    prefix: prefix.to_string(),
                })
            }
            other => Err(format!(
                "unknown FC_DISPATCH_QUEUE_TYPE {other:?}: expected SQS or POSTGRES"
            )),
        }
    }

    /// Resolve from the environment, with Go's names and aliases:
    /// `FC_DISPATCH_QUEUE_TYPE`/`DISPATCH_QUEUE_TYPE`,
    /// `FC_DISPATCH_QUEUE_URL`/`DISPATCH_QUEUE_URL`,
    /// `FC_DISPATCH_QUEUE_REGION`/`DISPATCH_QUEUE_REGION`,
    /// `FC_DISPATCH_QUEUE_PREFIX`.
    pub fn from_env() -> Result<Self, String> {
        let first = |a: &str, b: &str| {
            std::env::var(a)
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(|| std::env::var(b).ok().filter(|v| !v.is_empty()))
                .unwrap_or_default()
        };
        Self::resolve(
            &first("FC_DISPATCH_QUEUE_TYPE", "DISPATCH_QUEUE_TYPE"),
            &first("FC_DISPATCH_QUEUE_URL", "DISPATCH_QUEUE_URL"),
            &first("FC_DISPATCH_QUEUE_REGION", "DISPATCH_QUEUE_REGION"),
            &std::env::var("FC_DISPATCH_QUEUE_PREFIX").unwrap_or_default(),
        )
    }
}

/// The region of an SQS URL whose host is `sqs[-fips].<region>.amazonaws.com[.cn]`.
fn region_from_sqs_url(raw: &str) -> String {
    let Some(host) = url_host(raw) else {
        return String::new();
    };
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 4 && parts[0].starts_with("sqs") && parts[2] == "amazonaws" {
        parts[1].to_string()
    } else {
        String::new()
    }
}

/// The first non-blank path segment of an SQS URL (the account id).
fn account_from_sqs_url(raw: &str) -> String {
    let after_scheme = raw.split_once("://").map(|(_, r)| r).unwrap_or(raw);
    let path = after_scheme.split_once('/').map(|(_, p)| p).unwrap_or("");
    let path = path.split(['?', '#']).next().unwrap_or("");
    path.split('/')
        .find(|s| !s.trim().is_empty())
        .unwrap_or("")
        .to_string()
}

fn url_host(raw: &str) -> Option<&str> {
    let (_, rest) = raw.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

// ── Cached lookups ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct PoolRef {
    code: String,
    client_identifier: Option<String>,
}

#[derive(Default)]
struct PoolCodeSnapshot {
    pools: HashMap<String, PoolRef>,
    clients: HashMap<String, String>,
    refreshed: Option<Instant>,
}

/// Composes the pool code a job publishes, and a job's tenant, from cached
/// `msg_dispatch_pools` and `tnt_clients` snapshots (Go `PoolCodeResolver`).
/// Never fails: a refresh failure serves the stale snapshot.
pub struct PoolCodeResolver {
    pool: PgPool,
    ttl: Duration,
    snapshot: RwLock<PoolCodeSnapshot>,
    refresh_lock: tokio::sync::Mutex<()>,
}

impl PoolCodeResolver {
    pub fn new(pool: PgPool, ttl: Duration) -> Self {
        Self {
            pool,
            ttl,
            snapshot: RwLock::new(PoolCodeSnapshot::default()),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    fn is_fresh(&self) -> bool {
        self.snapshot
            .read()
            .refreshed
            .is_some_and(|t| t.elapsed() < self.ttl)
    }

    async fn ensure_fresh(&self) {
        if self.is_fresh() {
            return;
        }
        let _guard = self.refresh_lock.lock().await;
        if self.is_fresh() {
            return;
        }
        if let Err(e) = self.refresh().await {
            warn!(error = %e, "pool code cache refresh failed; resolving from stale cache");
        }
    }

    async fn refresh(&self) -> Result<(), sqlx::Error> {
        let pools: Vec<(String, String, Option<String>)> =
            sqlx::query_as("SELECT id, code, client_identifier FROM msg_dispatch_pools")
                .fetch_all(&self.pool)
                .await?;
        let clients: Vec<(String, String)> =
            sqlx::query_as("SELECT id, identifier FROM tnt_clients")
                .fetch_all(&self.pool)
                .await?;
        let mut snap = self.snapshot.write();
        snap.pools = pools
            .into_iter()
            .map(|(id, code, client_identifier)| {
                (
                    id,
                    PoolRef {
                        code,
                        client_identifier,
                    },
                )
            })
            .collect();
        snap.clients = clients.into_iter().collect();
        snap.refreshed = Some(Instant::now());
        debug!(
            pools = snap.pools.len(),
            clients = snap.clients.len(),
            "pool code cache refreshed"
        );
        Ok(())
    }

    /// The pool code for a job, per Go's chain:
    /// pool with a client → `{client}-{code}`; platform pool →
    /// `platform-{code}`; no (known) pool but a known client →
    /// `{client}-DEFAULT-POOL`; neither → `platform-DEFAULT-POOL`.
    pub async fn resolve(&self, pool_id: Option<&str>, client_id: Option<&str>) -> String {
        self.ensure_fresh().await;
        let snap = self.snapshot.read();
        if let Some(p) = pool_id
            .filter(|s| !s.is_empty())
            .and_then(|id| snap.pools.get(id))
        {
            if !p.code.is_empty() {
                return compose_pool_code(&p.code, p.client_identifier.as_deref());
            }
        }
        if let Some(identifier) = client_id
            .filter(|s| !s.is_empty())
            .and_then(|id| snap.clients.get(id))
            .filter(|s| !s.is_empty())
        {
            return format!("{identifier}{DEFAULT_POOL_SUFFIX}");
        }
        format!("{TENANT_PLATFORM}{DEFAULT_POOL_SUFFIX}")
    }

    /// The client's identifier, `None` when absent or unknown.
    pub async fn client_identifier(&self, client_id: Option<&str>) -> Option<String> {
        let client_id = client_id.filter(|s| !s.is_empty())?;
        self.ensure_fresh().await;
        self.snapshot
            .read()
            .clients
            .get(client_id)
            .filter(|s| !s.is_empty())
            .cloned()
    }
}

#[derive(Default)]
struct PrioritySnapshot {
    stored: HashMap<String, Option<String>>,
    refreshed: Option<Instant>,
}

/// `msg_subscriptions.queue` by subscription id, cached (Go
/// `SubscriptionPriorityCache`). Never fails.
pub struct SubscriptionPriorityCache {
    pool: PgPool,
    ttl: Duration,
    snapshot: RwLock<PrioritySnapshot>,
    refresh_lock: tokio::sync::Mutex<()>,
}

impl SubscriptionPriorityCache {
    pub fn new(pool: PgPool, ttl: Duration) -> Self {
        Self {
            pool,
            ttl,
            snapshot: RwLock::new(PrioritySnapshot::default()),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    fn is_fresh(&self) -> bool {
        self.snapshot
            .read()
            .refreshed
            .is_some_and(|t| t.elapsed() < self.ttl)
    }

    pub async fn priority_for(&self, subscription_id: Option<&str>) -> Priority {
        let Some(id) = subscription_id.filter(|s| !s.is_empty()) else {
            return Priority::Default;
        };
        if !self.is_fresh() {
            let _guard = self.refresh_lock.lock().await;
            if !self.is_fresh() {
                match sqlx::query_as::<_, (String, Option<String>)>(
                    "SELECT id, queue FROM msg_subscriptions",
                )
                .fetch_all(&self.pool)
                .await
                {
                    Ok(rows) => {
                        let mut snap = self.snapshot.write();
                        snap.stored = rows.into_iter().collect();
                        snap.refreshed = Some(Instant::now());
                    }
                    Err(e) => warn!(error = %e,
                        "subscription priority cache refresh failed; resolving from stale cache"),
                }
            }
        }
        let snap = self.snapshot.read();
        Priority::for_publishing(snap.stored.get(id).and_then(|q| q.as_deref()))
    }
}

/// A claimed job's routing inputs, as the destination resolver needs them.
#[derive(Debug, Clone, Default)]
pub struct DestinationInput<'a> {
    pub client_id: Option<&'a str>,
    pub subscription_id: Option<&'a str>,
    /// The job's own `queue` column, raw.
    pub queue: Option<&'a str>,
}

/// Resolves a job's destination queue name (Go `DestinationResolver`):
/// tenant from the client (platform when none or unknown); priority from the
/// job's own recognised queue, else its subscription's, else DEFAULT.
pub struct DestinationResolver {
    tenants: std::sync::Arc<PoolCodeResolver>,
    priorities: SubscriptionPriorityCache,
    prefix: String,
    sqs: bool,
}

impl DestinationResolver {
    pub fn new(
        tenants: std::sync::Arc<PoolCodeResolver>,
        priorities: SubscriptionPriorityCache,
        settings: &DispatchQueueSettings,
    ) -> Self {
        Self {
            tenants,
            priorities,
            prefix: settings.prefix.clone(),
            sqs: settings.is_sqs(),
        }
    }

    pub async fn destination(&self, input: &DestinationInput<'_>) -> Result<String, ComposeError> {
        let tenant = self
            .tenants
            .client_identifier(input.client_id)
            .await
            .unwrap_or_else(|| TENANT_PLATFORM.to_string());
        let priority = match Priority::for_job(input.queue) {
            Some(p) => p,
            None => self.priorities.priority_for(input.subscription_id).await,
        };
        compose_name(&self.prefix, &tenant, priority, self.sqs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_name_matches_go() {
        let cases = [
            (
                "FC-staging",
                "acme",
                Priority::Default,
                true,
                "FC-staging-acme-DEFAULT.fifo",
            ),
            (
                "FC-staging",
                "acme",
                Priority::HighPriority,
                true,
                "FC-staging-acme-HIGH_PRIORITY.fifo",
            ),
            (
                "FC-staging",
                TENANT_PLATFORM,
                Priority::Default,
                true,
                "FC-staging-platform-DEFAULT.fifo",
            ),
            ("", "acme", Priority::Default, false, "acme-DEFAULT"),
            (
                "FC-dev",
                "a_b.c d",
                Priority::Default,
                false,
                "FC-dev-a-b-c-d-DEFAULT",
            ),
        ];
        for (prefix, tenant, p, sqs, want) in cases {
            assert_eq!(compose_name(prefix, tenant, p, sqs).unwrap(), want);
        }
    }

    #[test]
    fn compose_name_refuses_blank_tenants_and_long_sqs_names() {
        assert_eq!(
            compose_name("FC", " ", Priority::Default, true),
            Err(ComposeError::TenantRequired)
        );
        let long = "a".repeat(80);
        assert!(matches!(
            compose_name("FC-staging", &long, Priority::HighPriority, true),
            Err(ComposeError::NameTooLong { .. })
        ));
        assert!(compose_name("FC-staging", &long, Priority::HighPriority, false).is_ok());
    }

    #[test]
    fn priority_parsing_matches_go() {
        assert_eq!(Priority::for_job(None), None);
        assert_eq!(Priority::for_job(Some("  ")), None);
        assert_eq!(Priority::for_job(Some("workers-high")), None);
        assert_eq!(Priority::for_job(Some("Default")), Some(Priority::Default));
        assert_eq!(
            Priority::for_job(Some("high_priority")),
            Some(Priority::HighPriority)
        );
        assert_eq!(Priority::for_publishing(None), Priority::Default);
        assert_eq!(
            Priority::for_publishing(Some("workers-high")),
            Priority::Default
        );
        assert_eq!(
            Priority::for_publishing(Some("HIGH_PRIORITY")),
            Priority::HighPriority
        );
    }

    #[test]
    fn pool_code_composition() {
        assert_eq!(compose_pool_code("FAST", Some("acme")), "acme-FAST");
        assert_eq!(compose_pool_code("FAST", None), "platform-FAST");
        assert_eq!(compose_pool_code("FAST", Some("")), "platform-FAST");
    }

    #[test]
    fn settings_resolve_like_go() {
        let s = DispatchQueueSettings::resolve(
            "sqs",
            "https://sqs.eu-west-1.amazonaws.com/123456789012/inhance-fc-np-dispatch.fifo",
            "",
            "FC-np",
        )
        .unwrap();
        assert_eq!(
            s.kind,
            DispatchQueueKind::Sqs {
                region: "eu-west-1".into(),
                account_id: "123456789012".into()
            }
        );
        // An explicit region wins over the URL's.
        let s = DispatchQueueSettings::resolve(
            "SQS",
            "https://sqs.eu-west-1.amazonaws.com/123456789012/q.fifo",
            "ap-southeast-2",
            "FC-np",
        )
        .unwrap();
        assert!(
            matches!(s.kind, DispatchQueueKind::Sqs { ref region, .. } if region == "ap-southeast-2")
        );
        // SQS without a prefix, or without an addressable account, refuses.
        assert!(DispatchQueueSettings::resolve(
            "SQS",
            "https://sqs.eu-west-1.amazonaws.com/1/q",
            "",
            ""
        )
        .is_err());
        assert!(DispatchQueueSettings::resolve("SQS", "", "eu-west-1", "FC-np").is_err());
        // Nothing configured refuses; Postgres is explicit.
        assert!(DispatchQueueSettings::resolve("", "", "", "").is_err());
        assert!(DispatchQueueSettings::resolve("kafka", "", "", "").is_err());
        assert_eq!(
            DispatchQueueSettings::resolve("postgres", "", "", "")
                .unwrap()
                .kind,
            DispatchQueueKind::Postgres
        );
    }
}
