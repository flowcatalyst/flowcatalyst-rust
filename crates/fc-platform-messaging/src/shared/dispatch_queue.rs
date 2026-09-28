//! Dispatch queue and pool naming, and the queue settings, as Go composes
//! them (`internal/platform/shared/dispatchqueue/{name,priority}.go`,
//! `internal/platform/dispatch/settings.go`, `internal/server/envcfg.go`).
//!
//! One queue per tenant and priority: `[{prefix}-]{tenant}-{DEFAULT|HIGH_PRIORITY}`,
//! with `.fifo` appended on SQS (at most 80 characters). A pool's code is
//! composed per tenant: `{clientIdentifier|platform}-{code}`. Public so the
//! scheduler's publisher and the router-config document share one naming.

// The naming is the scheduler's (`scheduler::destination`), so the queues
// and pools the router-config document advertises are exactly the ones the
// scheduler publishes to and stamps.
pub use crate::scheduler::destination::{
    compose_name, compose_pool_code, ComposeError as NameError, Priority, DEFAULT_POOL_CODE,
    DEFAULT_POOL_SUFFIX, SQS_MAX_NAME_LENGTH, TENANT_PLATFORM,
};

/// The tenant a client identifier names: the identifier, or `platform`
/// when there is none.
pub fn tenant_for(client_identifier: Option<&str>) -> &str {
    match client_identifier {
        Some(c) if !c.is_empty() => c,
        _ => TENANT_PLATFORM,
    }
}

/// Where the dispatch queues live (Go `dispatch.Settings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSettings {
    pub sqs: bool,
    pub prefix: String,
    pub sqs_account_id: String,
    pub sqs_region: String,
    /// The Postgres queue's URI (the platform database), in Postgres mode.
    pub database_url: String,
}

impl QueueSettings {
    /// Go `ResolveSettings`.
    pub fn resolve(
        queue_type: &str,
        queue_url: &str,
        queue_region: &str,
        prefix: &str,
        database_url: &str,
    ) -> Result<Self, String> {
        let sqs = queue_type.trim().eq_ignore_ascii_case("SQS");
        if !sqs {
            return Ok(Self {
                sqs: false,
                prefix: prefix.trim().to_string(),
                sqs_account_id: String::new(),
                sqs_region: String::new(),
                database_url: normalise_postgres_scheme(database_url),
            });
        }
        if prefix.trim().is_empty() {
            return Err(
                "FC_DISPATCH_QUEUE_PREFIX is required when FC_DISPATCH_QUEUE_TYPE=SQS: every \
                 tenant queue name is composed from it"
                    .to_string(),
            );
        }
        let mut region = queue_region.trim().to_string();
        if region.is_empty() {
            region = region_from_sqs_url(queue_url);
        }
        let account = account_from_sqs_url(queue_url);
        if region.is_empty() || account.is_empty() {
            return Err(format!(
                "FC_DISPATCH_QUEUE_TYPE=SQS needs an account id and region to compose queue URLs \
                 (account={account:?}, region={region:?}, FC_DISPATCH_QUEUE_URL={queue_url:?})"
            ));
        }
        Ok(Self {
            sqs: true,
            prefix: prefix.to_string(),
            sqs_account_id: account,
            sqs_region: region,
            database_url: String::new(),
        })
    }

    /// From the environment, as Go's `envcfg` reads it: the first non-empty
    /// of `FC_DISPATCH_QUEUE_TYPE`/`DISPATCH_QUEUE_TYPE`,
    /// `FC_DISPATCH_QUEUE_URL`/`DISPATCH_QUEUE_URL`,
    /// `FC_DISPATCH_QUEUE_REGION`/`DISPATCH_QUEUE_REGION`, and
    /// `FC_DISPATCH_QUEUE_PREFIX`; the database URL as Go resolves it.
    pub fn from_env() -> Result<Self, String> {
        Self::resolve(
            &first_env(&["FC_DISPATCH_QUEUE_TYPE", "DISPATCH_QUEUE_TYPE"]),
            &first_env(&["FC_DISPATCH_QUEUE_URL", "DISPATCH_QUEUE_URL"]),
            &first_env(&["FC_DISPATCH_QUEUE_REGION", "DISPATCH_QUEUE_REGION"]),
            &first_env(&["FC_DISPATCH_QUEUE_PREFIX"]),
            &resolve_database_url(),
        )
    }

    /// Go `QueueURIFor`: the SQS queue URL, or the database URL.
    pub fn queue_uri_for(&self, name: &str) -> String {
        if self.sqs {
            format!(
                "https://sqs.{}.amazonaws.com/{}/{}",
                self.sqs_region, self.sqs_account_id, name
            )
        } else {
            self.database_url.clone()
        }
    }
}

fn first_env(names: &[&str]) -> String {
    names
        .iter()
        .filter_map(|n| std::env::var(n).ok())
        .find(|v| !v.is_empty())
        .unwrap_or_default()
}

/// Go `ResolveDatabaseURL`: `FC_DATABASE_URL`, `DATABASE_URL`, then
/// `DB_HOST`/`DB_PORT`/`DB_NAME`/`DB_USERNAME`/`DB_PASSWORD`, then the local
/// default.
pub fn resolve_database_url() -> String {
    let url = first_env(&["FC_DATABASE_URL", "DATABASE_URL"]);
    if !url.is_empty() {
        return url;
    }
    let host = first_env(&["DB_HOST"]);
    if host.is_empty() {
        return "postgresql://postgres@localhost:5432/flowcatalyst".to_string();
    }
    let or = |name: &str, default: &str| {
        let v = first_env(&[name]);
        if v.is_empty() {
            default.to_string()
        } else {
            v
        }
    };
    let user = or("DB_USERNAME", "postgres");
    let password = first_env(&["DB_PASSWORD"]);
    let auth = if password.is_empty() {
        user
    } else {
        format!("{user}:{}", urlencoding_minimal(&password))
    };
    let host_port = if host.contains(':') {
        host
    } else {
        format!("{host}:{}", or("DB_PORT", "5432"))
    };
    format!(
        "postgresql://{auth}@{host_port}/{}",
        or("DB_NAME", "flowcatalyst")
    )
}

/// Go `url.QueryEscape`.
fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~' => out.push(c),
            ' ' => out.push('+'),
            _ => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}

/// Go `normalisePostgresScheme`: `postgresql://` → `postgres://`.
fn normalise_postgres_scheme(url: &str) -> String {
    match url.strip_prefix("postgresql://") {
        Some(rest) => format!("postgres://{rest}"),
        None => url.to_string(),
    }
}

fn url_host_and_path(url: &str) -> (&str, &str) {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, p),
        None => (rest, ""),
    };
    let authority = authority.split(['?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    (host, path.split(['?', '#']).next().unwrap_or(""))
}

/// Go `regionFromSQSURL`: `sqs.<region>.amazonaws.com`.
fn region_from_sqs_url(url: &str) -> String {
    let (host, _) = url_host_and_path(url.trim());
    let host = host.split(':').next().unwrap_or("");
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 4 && parts[0].starts_with("sqs") && parts[2] == "amazonaws" {
        parts[1].to_string()
    } else {
        String::new()
    }
}

/// Go `accountFromSQSURL`: the first non-blank path segment.
fn account_from_sqs_url(url: &str) -> String {
    let (_, path) = url_host_and_path(url.trim());
    path.split('/')
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compose_as_go_does() {
        assert_eq!(
            compose_name("FC-staging", "acme", Priority::Default, true).unwrap(),
            "FC-staging-acme-DEFAULT.fifo"
        );
        assert_eq!(
            compose_name("FC-staging", "platform", Priority::HighPriority, true).unwrap(),
            "FC-staging-platform-HIGH_PRIORITY.fifo"
        );
        assert_eq!(
            compose_name("", "platform", Priority::Default, false).unwrap(),
            "platform-DEFAULT"
        );
        assert_eq!(
            compose_name("p", "a_b.c d", Priority::Default, false).unwrap(),
            "p-a-b-c-d-DEFAULT"
        );
        assert_eq!(
            compose_name("p", " ", Priority::Default, false),
            Err(NameError::TenantRequired)
        );
        let long = "t".repeat(80);
        assert!(matches!(
            compose_name("p", &long, Priority::Default, true),
            Err(NameError::NameTooLong { .. })
        ));
        // Postgres mode has no length limit.
        assert!(compose_name("p", &long, Priority::Default, false).is_ok());
    }

    #[test]
    fn pools_compose_per_tenant() {
        assert_eq!(
            compose_pool_code("DEFAULT-POOL", None),
            "platform-DEFAULT-POOL"
        );
        assert_eq!(compose_pool_code("fast", Some("")), "platform-fast");
        assert_eq!(compose_pool_code("fast", Some("acme")), "acme-fast");
    }

    #[test]
    fn priority_is_high_only_when_named() {
        assert_eq!(
            Priority::for_publishing(Some(" high_priority ")),
            Priority::HighPriority
        );
        assert_eq!(Priority::for_publishing(Some("fast")), Priority::Default);
        assert_eq!(Priority::for_publishing(None), Priority::Default);
    }

    #[test]
    fn settings_resolve_as_go_does() {
        let pg = QueueSettings::resolve("", "", "", "", "postgresql://u@h:5432/db").unwrap();
        assert!(!pg.sqs);
        assert_eq!(
            pg.queue_uri_for("platform-DEFAULT"),
            "postgres://u@h:5432/db"
        );

        let sqs = QueueSettings::resolve(
            "sqs",
            "https://sqs.eu-west-1.amazonaws.com/123456789012/whatever.fifo",
            "",
            "FC-prod",
            "",
        )
        .unwrap();
        assert_eq!(sqs.sqs_region, "eu-west-1");
        assert_eq!(sqs.sqs_account_id, "123456789012");
        assert_eq!(
            sqs.queue_uri_for("FC-prod-acme-DEFAULT.fifo"),
            "https://sqs.eu-west-1.amazonaws.com/123456789012/FC-prod-acme-DEFAULT.fifo"
        );

        assert!(QueueSettings::resolve(
            "SQS",
            "https://sqs.eu-west-1.amazonaws.com/1/q",
            "",
            "",
            ""
        )
        .is_err());
        assert!(QueueSettings::resolve("SQS", "", "eu-west-1", "FC", "").is_err());
        let region = QueueSettings::resolve(
            "SQS",
            "https://sqs.eu-west-1.amazonaws.com/1/q",
            "us-east-1",
            "FC",
            "",
        )
        .unwrap();
        assert_eq!(region.sqs_region, "us-east-1");
    }
}
