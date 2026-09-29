//! A function's declared database connection (Java `fnhost/context/Dsn.java`):
//! the value of the secret its manifest `db[].secretRef` names, normalised.
//!
//! Accepted, **PostgreSQL only** (anything else is the load failure
//! `DB_UNSUPPORTED`, as Java):
//!
//! - `postgres://user:pass@host[:port]/db[?params]` or `postgresql://…`
//!   (libpq form, percent-decoded);
//! - `jdbc:postgresql://host[:port]/db[?user=…&password=…&…]` (Java's form);
//! - a secret-manager reference such as `aws-sm://<secret id or ARN>`
//!   (when the host has a resolver for its scheme), whose value is one of
//!   the above or an RDS-style JSON secret (`username`, `password`, `host`,
//!   `port`, `dbname`). A reference is re-read periodically, so a rotated
//!   password reaches the pool without a redeploy.
//!
//! The password never appears in `Debug`, `Display` or a log line: only in
//! the connect options and in [`Dsn::identity`], the map key that lets two
//! functions naming the same connection share one pool, which is never
//! printed.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
#[cfg(feature = "aws-secrets")]
use aws_sdk_secretsmanager::error::DisplayErrorContext;
use sha2::Digest as _;
use sqlx::postgres::PgConnectOptions;
#[cfg(feature = "aws-secrets")]
use std::collections::HashMap;
#[cfg(feature = "aws-secrets")]
use tokio::sync::Mutex;

/// The load-failure code for a DSN this host cannot use (Java's).
pub const DB_UNSUPPORTED: &str = "DB_UNSUPPORTED";
/// The load-failure code for a secret-manager reference that could not be
/// read (a Rust extension: Java has no references).
pub const DB_SECRET_UNRESOLVED: &str = "DB_SECRET_UNRESOLVED";

/// Where a pool's connection comes from.
#[derive(Clone, PartialEq, Eq)]
pub enum DsnSource {
    /// A connection string, as the secret holds it.
    Literal(String),
    /// A secret-manager reference, resolved (and re-resolved) by a
    /// [`SecretResolver`].
    Reference(String),
}

impl DsnSource {
    /// Reads a secret value: a reference when its scheme is one `resolver`
    /// knows, else a literal connection string.
    pub fn of(value: &str, resolver: &dyn SecretResolver) -> DsnSource {
        let value = value.trim();
        match value.split_once("://") {
            Some((scheme, _)) if resolver.handles(scheme) => DsnSource::Reference(value.to_owned()),
            _ => DsnSource::Literal(value.to_owned()),
        }
    }
}

impl fmt::Debug for DsnSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DsnSource::Literal(_) => f.write_str("Literal(<redacted>)"),
            DsnSource::Reference(r) => write!(f, "Reference({r})"),
        }
    }
}

/// Resolves secret-manager references to a connection string (or an RDS
/// JSON secret). The host's is AWS Secrets Manager (`aws-sm://`, feature
/// `aws-secrets`); tests plug in their own.
#[async_trait]
pub trait SecretResolver: Send + Sync {
    /// Whether references with this scheme (`aws-sm`) are this resolver's.
    fn handles(&self, scheme: &str) -> bool;
    /// The secret's current value. The error names what failed, never a
    /// secret value.
    async fn resolve(&self, reference: &str) -> Result<String, String>;
}

/// Resolves nothing: every value is a literal connection string.
pub struct NoResolver;

#[async_trait]
impl SecretResolver for NoResolver {
    fn handles(&self, _scheme: &str) -> bool {
        false
    }

    async fn resolve(&self, reference: &str) -> Result<String, String> {
        Err(format!("no resolver for {reference}"))
    }
}

/// The resolvers the deployed host has: AWS Secrets Manager with the
/// `aws-secrets` feature, none otherwise.
pub fn default_resolver() -> Arc<dyn SecretResolver> {
    #[cfg(feature = "aws-secrets")]
    {
        Arc::new(AwsSecretsManager::default())
    }
    #[cfg(not(feature = "aws-secrets"))]
    {
        Arc::new(NoResolver)
    }
}

/// A parsed, validated connection.
#[derive(Clone)]
pub struct Dsn {
    options: PgConnectOptions,
    identity: String,
    shown: String,
}

impl Dsn {
    /// Parses a literal connection string (or a resolved secret's value).
    /// The error never contains the value.
    pub fn parse(raw: &str) -> Result<Dsn, String> {
        let raw = raw.trim();
        if raw.starts_with('{') {
            return Self::from_secret_json(raw);
        }
        let (scheme, rest) = raw
            .split_once("://")
            .ok_or_else(|| "unsupported or malformed database url".to_owned())?;
        let url = match scheme.to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" => url::Url::parse(&format!("postgresql://{rest}")),
            "jdbc:postgresql" => url::Url::parse(&format!("postgresql://{rest}")),
            s if s.starts_with("jdbc:") => {
                return Err("only PostgreSQL is supported on this host".to_owned())
            }
            _ => return Err("unsupported or malformed database url".to_owned()),
        }
        .map_err(|_| "unsupported or malformed database url".to_owned())?;
        let jdbc = scheme.to_ascii_lowercase().starts_with("jdbc:");
        let mut params: BTreeMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let decode = |s: &str| {
            urlencoding::decode(s)
                .map(|c| c.into_owned())
                .map_err(|_| "unsupported or malformed database url".to_owned())
        };
        let mut user = decode(url.username())?;
        let mut password = url.password().map(decode).transpose()?;
        if jdbc {
            // JDBC carries the credentials as parameters.
            if let Some(u) = params.remove("user") {
                user = u;
            }
            if let Some(p) = params.remove("password") {
                password = Some(p);
            }
        }
        let host = url
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| "the database url names no host".to_owned())?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned();
        let port = url.port().unwrap_or(5432);
        let database = url.path().trim_start_matches('/');
        let database = decode(database)?;
        Self::build(host, port, database, user, password, params)
    }

    /// An RDS-style secret: `username`, `password`, `host`, `port`
    /// (default 5432), `dbname` (or `database`).
    fn from_secret_json(raw: &str) -> Result<Dsn, String> {
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|_| "the database secret is not JSON".to_owned())?;
        let field = |k: &str| {
            value[k]
                .as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let engine = field("engine").unwrap_or_else(|| "postgres".into());
        if !engine.starts_with("postgres") {
            return Err("only PostgreSQL is supported on this host".to_owned());
        }
        let (Some(user), Some(password), Some(host)) =
            (field("username"), field("password"), field("host"))
        else {
            return Err("the database secret needs username, password and host".to_owned());
        };
        let port = value["port"]
            .as_u64()
            .or_else(|| value["port"].as_str().and_then(|p| p.parse().ok()))
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(5432);
        let database = field("dbname")
            .or_else(|| field("database"))
            .unwrap_or_default();
        Self::build(host, port, database, user, Some(password), BTreeMap::new())
    }

    fn build(
        host: String,
        port: u16,
        database: String,
        user: String,
        password: Option<String>,
        params: BTreeMap<String, String>,
    ) -> Result<Dsn, String> {
        // libpq's URL, rebuilt from the parts, is what sqlx reads the
        // parameters (sslmode, options, application_name, …) from.
        let mut url = url::Url::parse("postgresql://placeholder/").expect("a valid base");
        url.set_host(Some(&host))
            .map_err(|_| "the database url's host is malformed".to_owned())?;
        url.set_port(Some(port))
            .expect("a postgresql url takes a port");
        url.set_path(&database);
        if !user.is_empty() {
            url.set_username(&user)
                .expect("a url with a host takes a user");
        }
        if let Some(p) = &password {
            url.set_password(Some(p))
                .expect("a url with a host takes a password");
        }
        if !params.is_empty() {
            let mut query = url.query_pairs_mut();
            for (k, v) in &params {
                query.append_pair(k, v);
            }
        }
        let mut options = PgConnectOptions::from_str(url.as_str())
            .map_err(|_| "unsupported database url parameters".to_owned())?
            // DISCARD ALL on every release deallocates server-side prepared
            // statements, so the statement cache is per borrow: see
            // `pools::reset_session`.
            .statement_cache_capacity(100);
        if !params.contains_key("application_name") {
            options = options.application_name("flowcatalyst-function");
        }
        let mut identity = format!("{host}|{port}|{database}|{user}|");
        identity.push_str(password.as_deref().unwrap_or(""));
        for (k, v) in &params {
            identity.push_str(&format!("|{k}={v}"));
        }
        let shown = format!("postgresql://{host}:{port}/{database} user={user}");
        Ok(Dsn {
            options,
            identity,
            shown,
        })
    }

    /// What pools are shared by: every part that makes two connections
    /// genuinely different, the password included. Never print it.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn connect_options(&self) -> &PgConnectOptions {
        &self.options
    }
}

/// The connection without its password or parameters (Java `Dsn.toString`).
impl fmt::Display for Dsn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.shown)
    }
}

impl fmt::Debug for Dsn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Dsn[{}]", self.shown)
    }
}

/// A short, stable, non-reversible name for a pool identity (for logs).
pub fn pool_id(identity: &str) -> String {
    hex::encode(&sha2::Sha256::digest(identity.as_bytes())[..6])
}

/// AWS Secrets Manager (`aws-sm://<secret id or ARN>`), on the host's own
/// IAM role (the default credential chain); a secret is read from its
/// ARN's region.
#[cfg(feature = "aws-secrets")]
#[derive(Default)]
pub struct AwsSecretsManager {
    clients: Mutex<HashMap<Option<String>, aws_sdk_secretsmanager::Client>>,
}

#[cfg(feature = "aws-secrets")]
#[async_trait]
impl SecretResolver for AwsSecretsManager {
    fn handles(&self, scheme: &str) -> bool {
        scheme == "aws-sm"
    }

    async fn resolve(&self, reference: &str) -> Result<String, String> {
        let id = reference
            .strip_prefix("aws-sm://")
            .filter(|id| !id.is_empty())
            .ok_or_else(|| "not an aws-sm:// reference".to_owned())?;
        let parts: Vec<&str> = id.split(':').collect();
        let region = (parts.len() >= 4 && parts[0] == "arn" && !parts[3].is_empty())
            .then(|| parts[3].to_owned());
        let client = {
            let mut clients = self.clients.lock().await;
            match clients.get(&region) {
                Some(client) => client.clone(),
                None => {
                    let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
                    if let Some(region) = &region {
                        loader = loader.region(aws_config::Region::new(region.clone()));
                    }
                    let client = aws_sdk_secretsmanager::Client::new(&loader.load().await);
                    clients.insert(region, client.clone());
                    client
                }
            }
        };
        let secret = client
            .get_secret_value()
            .secret_id(id)
            .send()
            .await
            .map_err(|e| format!("reading {reference} failed: {}", DisplayErrorContext(&e)))?;
        secret
            .secret_string()
            .map(str::to_owned)
            .ok_or_else(|| format!("{reference} has no string value"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libpq_and_jdbc_forms_of_one_connection_share_an_identity() {
        let a =
            Dsn::parse("postgres://app:p%40ss@db.internal:5433/orders?sslmode=require").unwrap();
        let b =
            Dsn::parse("postgresql://app:p%40ss@db.internal:5433/orders?sslmode=require").unwrap();
        let c = Dsn::parse(
            "jdbc:postgresql://db.internal:5433/orders?user=app&password=p%40ss&sslmode=require",
        )
        .unwrap();
        assert_eq!(a.identity(), b.identity());
        assert_eq!(a.identity(), c.identity());
        assert_eq!(a.connect_options().get_host(), "db.internal");
        assert_eq!(a.connect_options().get_port(), 5433);
        assert_eq!(a.connect_options().get_username(), "app");
        assert_eq!(a.connect_options().get_database(), Some("orders"));
        let other_password =
            Dsn::parse("postgres://app:other@db.internal:5433/orders?sslmode=require").unwrap();
        assert_ne!(a.identity(), other_password.identity());
        let default_port = Dsn::parse("postgres://app:x@db.internal/orders").unwrap();
        assert_eq!(default_port.connect_options().get_port(), 5432);
    }

    #[test]
    fn nothing_prints_the_password() {
        let dsn = Dsn::parse("postgres://app:hunter2@db/orders?password=hunter3").unwrap();
        for shown in [dsn.to_string(), format!("{dsn:?}")] {
            assert!(!shown.contains("hunter"), "{shown}");
            assert!(shown.contains("db"), "{shown}");
        }
        assert!(!format!(
            "{:?}",
            DsnSource::Literal("postgres://a:hunter2@b/c".into())
        )
        .contains("hunter2"));
        assert!(!pool_id(dsn.identity()).contains("hunter"));
    }

    #[test]
    fn anything_but_postgres_is_refused_without_echoing_it() {
        for raw in [
            "mysql://u:secretpw@h/d",
            "jdbc:mysql://h/d?password=secretpw",
            "secretpw",
            "",
            "postgres://",
            r#"{"engine":"mysql","username":"u","password":"secretpw","host":"h"}"#,
            r#"{"username":"u"}"#,
        ] {
            let err = Dsn::parse(raw).unwrap_err();
            assert!(!err.contains("secretpw"), "{err}");
        }
    }

    #[test]
    fn an_rds_json_secret_is_a_connection() {
        let dsn = Dsn::parse(
            r#"{"engine":"postgres","username":"app","password":"p@ss:w/rd","host":"db.rds","port":5433,"dbname":"orders"}"#,
        )
        .unwrap();
        assert_eq!(dsn.connect_options().get_host(), "db.rds");
        assert_eq!(dsn.connect_options().get_port(), 5433);
        assert_eq!(dsn.connect_options().get_database(), Some("orders"));
        assert_eq!(
            dsn.identity(),
            Dsn::parse("postgres://app:p%40ss%3Aw%2Frd@db.rds:5433/orders")
                .unwrap()
                .identity()
        );
    }

    #[test]
    fn a_reference_is_one_only_for_a_scheme_the_resolver_handles() {
        struct Fake;
        #[async_trait]
        impl SecretResolver for Fake {
            fn handles(&self, scheme: &str) -> bool {
                scheme == "aws-sm"
            }
            async fn resolve(&self, _: &str) -> Result<String, String> {
                unreachable!()
            }
        }
        assert_eq!(
            DsnSource::of(" aws-sm://prod/db ", &Fake),
            DsnSource::Reference("aws-sm://prod/db".into())
        );
        assert_eq!(
            DsnSource::of("postgres://a@b/c", &Fake),
            DsnSource::Literal("postgres://a@b/c".into())
        );
        assert_eq!(
            DsnSource::of("aws-sm://prod/db", &NoResolver),
            DsnSource::Literal("aws-sm://prod/db".into())
        );
    }
}
