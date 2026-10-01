//! Stored secret references: how a secret column's value is written and how
//! it is opened for use.
//!
//! A stored secret is one of (Go `shared/encryption/secretref.go` and
//! `internal/secrets/provider.go`):
//!
//! - `encrypted:<blob>`: ciphertext from [`EncryptionService`], opened with
//!   the application key;
//! - a secret-manager reference, stored verbatim and resolved when read:
//!   `aws-sm://<secret id or ARN>` (AWS Secrets Manager), `aws-ps://<name>`
//!   (AWS SSM Parameter Store), `gcp-sm://…`, `vault://path#field`, and
//!   `env://VAR` (a process environment variable);
//! - `literal:<value>`: a dev bypass that is its own plaintext.
//!
//! Writing ([`seal_secret_ref`]) is Go's `EncryptSecretRef`: a reference or
//! an existing `encrypted:` value is kept as sent, an unknown `<scheme>://`
//! is refused (it is a mistyped reference, never a secret to seal), and
//! anything else is plaintext and is encrypted (an `encrypt:` prefix forces
//! that for a secret that looks like a URL).
//!
//! An *opaque* secret (a function's secret or database connection, owner
//! decision #54) is written by [`classify_opaque_secret`] instead: an
//! `aws-sm://` reference or an `encrypted:` value is kept as sent (a
//! malformed one is `INVALID_SECRET_REF`), and everything else, a
//! `postgres://` DSN included, is plaintext to encrypt.
//!
//! Reading ([`SecretResolver::resolve`]) opens each form. A secret-manager
//! value is fetched through the [`SecretStore`] registered for its scheme
//! and cached for [`DEFAULT_CACHE_TTL`], so a rotation in the secret manager
//! is picked up without a restart. This platform registers `env` and
//! `aws-sm` (the AWS SDK it already depends on); `aws-ps`, `gcp-sm` and
//! `vault` are accepted on write, as Go accepts them, but resolve to
//! [`SecretRefError::NoProvider`] until a store is registered, as Go's
//! secrets registry answers for a scheme with no provider. Go itself opens
//! a stored IdP secret with `Decrypt` only, so a reference there fails at
//! login; the platform resolves it instead (the capability production will
//! need; today it holds only `encrypted:` values).
//!
//! No error or log line carries a secret value.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use dashmap::DashMap;

use super::encryption_service::{EncryptionError, EncryptionService, ENCRYPTED_PREFIX};
use super::log_throttle::LogThrottle;
use aws_sdk_secretsmanager::error::DisplayErrorContext;
use std::env;
use tokio::sync::OnceCell;

/// The reference prefixes stored verbatim and resolved at read time (Go's
/// `externalSecretSchemes`).
pub const EXTERNAL_SECRET_SCHEMES: [&str; 6] = [
    "aws-sm://",
    "aws-ps://",
    "gcp-sm://",
    "vault://",
    "env://",
    "literal:",
];

/// The dev-bypass prefix: the rest of the value is the plaintext.
pub const LITERAL_PREFIX: &str = "literal:";

/// Forces a value that looks like a URL to be encrypted as plaintext.
pub const ENCRYPT_DIRECTIVE: &str = "encrypt:";

/// How long a secret fetched from a secret manager is reused.
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300);

/// Why a secret could not be sealed or opened. Never carries a secret value.
#[derive(Debug, thiserror::Error)]
pub enum SecretRefError {
    #[error(
        "unsupported secret-manager scheme \"{scheme}://\"; supported: {supported} \
         (prefix the value with \"encrypt:\" to store it as an encrypted plaintext secret instead)"
    )]
    UnsupportedScheme { scheme: String, supported: String },
    #[error("FLOWCATALYST_APP_KEY is not configured; secrets cannot be encrypted or decrypted")]
    NotConfigured,
    #[error(transparent)]
    Encryption(#[from] EncryptionError),
    #[error("no secret provider for scheme \"{0}\"")]
    NoProvider(String),
    #[error("secret \"{key}\" not found in {scheme}")]
    NotFound { scheme: String, key: String },
    #[error("{scheme} lookup of \"{key}\" failed: {message}")]
    Provider {
        scheme: String,
        key: String,
        message: String,
    },
    #[error("stored secret is neither an `encrypted:` value nor a secret reference")]
    NotAReference,
}

/// Whether `value` is a secret-manager reference (or `literal:`), i.e. one
/// of [`EXTERNAL_SECRET_SCHEMES`].
pub fn is_secret_reference(value: &str) -> bool {
    let v = value.trim();
    EXTERNAL_SECRET_SCHEMES.iter().any(|s| v.starts_with(s))
}

/// An RFC 3986 scheme: ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ).
fn is_scheme_token(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The scheme of a `<scheme>://…` value whose scheme is not a supported
/// secret manager (Go's `unsupportedScheme`); `None` when there is nothing to
/// refuse (a supported reference, or a value that merely contains `://`).
pub fn unsupported_scheme(value: &str) -> Option<&str> {
    let v = value.trim();
    let i = v.find("://")?;
    let scheme = &v[..i];
    if i == 0 || !is_scheme_token(scheme) {
        return None;
    }
    if EXTERNAL_SECRET_SCHEMES
        .iter()
        .any(|s| s.strip_suffix("://") == Some(scheme))
    {
        return None;
    }
    Some(scheme)
}

/// Whether a stored value is a reference of any kind, supported or not:
/// something to resolve (or refuse), never a plaintext to encrypt.
pub fn looks_like_reference(value: &str) -> bool {
    is_secret_reference(value) || unsupported_scheme(value).is_some()
}

fn supported_schemes() -> String {
    EXTERNAL_SECRET_SCHEMES
        .iter()
        .filter(|s| s.ends_with("://"))
        .copied()
        .collect::<Vec<_>>()
        .join(", ")
}

/// The stored form of a secret the caller sent (Go's `EncryptSecretRef`):
/// an `encrypted:` value or a secret reference is kept as sent (trimmed); an
/// unknown `<scheme>://` is refused; anything else (without a leading
/// `encrypt:` directive) is encrypted, which needs `enc`.
pub fn seal_secret_ref(
    enc: Option<&EncryptionService>,
    value: &str,
) -> Result<String, SecretRefError> {
    let v = value.trim();
    if v.starts_with(ENCRYPTED_PREFIX) || is_secret_reference(v) {
        return Ok(v.to_string());
    }
    if let Some(scheme) = unsupported_scheme(v) {
        return Err(SecretRefError::UnsupportedScheme {
            scheme: scheme.to_string(),
            supported: supported_schemes(),
        });
    }
    let plaintext = v.strip_prefix(ENCRYPT_DIRECTIVE).unwrap_or(v);
    let enc = enc.ok_or(SecretRefError::NotConfigured)?;
    Ok(enc.encrypt_ref(plaintext)?)
}

/// The secret-manager references an opaque secret keeps as sent: the ones
/// the platform's resolver ([`SecretResolver::platform`]) can open for
/// someone other than the platform itself (owner decision #54). `env://` is
/// deliberately absent: it would read the platform's own environment (its
/// app key, its database URL) on behalf of whoever set the secret, so it is
/// plaintext here. `aws-ps://`, `gcp-sm://` and `vault://` have no store yet
/// and are plaintext too.
pub const OPAQUE_SECRET_REFERENCE_SCHEMES: [&str; 1] = ["aws-sm://"];

/// The longest secret id or ARN AWS Secrets Manager accepts.
const AWS_SECRET_ID_MAX: usize = 2048;

/// How a value sent for an opaque secret is stored: a function's secret,
/// including the connection string a `db[]` entry names (owner decision
/// #54, the middle road of `docs/plans/go-function-service-fixes.md` §1.2).
///
/// Unlike [`seal_secret_ref`], an unknown `<scheme>://` is not refused: a
/// `postgres://` DSN or an `https://…?token=` URL is the secret itself.
pub enum OpaqueSecret<'a> {
    /// A reference in [`OPAQUE_SECRET_REFERENCE_SCHEMES`], trimmed: stored
    /// as sent and resolved when the secret is delivered.
    Reference(&'a str),
    /// An `encrypted:` value whose payload is base64, trimmed: stored as
    /// sent. Whether it opens with this platform's key is the caller's
    /// check (it needs the key).
    Encrypted(&'a str),
    /// Plaintext to encrypt: the trimmed value, without a leading
    /// `encrypt:` directive. May be empty.
    Plaintext(&'a str),
}

impl fmt::Debug for OpaqueSecret<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpaqueSecret::Reference(r) => f.debug_tuple("Reference").field(r).finish(),
            OpaqueSecret::Encrypted(_) => f.write_str("Encrypted(***)"),
            OpaqueSecret::Plaintext(_) => f.write_str("Plaintext(***)"),
        }
    }
}

/// A value that claims to be a reference or an `encrypted:` value and is
/// not a well-formed one (`INVALID_SECRET_REF`). The message never carries
/// the value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct InvalidSecretRef(pub String);

/// The `INVALID_SECRET_REF` message for an `encrypted:` value that does
/// not open with this platform's key.
pub const ENCRYPTED_DOES_NOT_DECRYPT: &str =
    "encrypted: payload does not decrypt with this platform's key";

/// Classifies a value sent for an opaque secret (see [`OpaqueSecret`]):
/// leading and trailing whitespace is dropped, as [`seal_secret_ref`] (and
/// Java's `SecretRef.parse`) does.
pub fn classify_opaque_secret(value: &str) -> Result<OpaqueSecret<'_>, InvalidSecretRef> {
    let v = value.trim();
    if let Some(payload) = v.strip_prefix(ENCRYPTED_PREFIX) {
        // Java's `SecretRef.parse` message; strict, padded base64.
        if BASE64.decode(payload).is_err() {
            return Err(InvalidSecretRef(
                "encrypted: payload is not base64".to_string(),
            ));
        }
        if payload.is_empty() {
            return Err(InvalidSecretRef("encrypted: payload is empty".to_string()));
        }
        return Ok(OpaqueSecret::Encrypted(v));
    }
    if let Some(id) = v.strip_prefix("aws-sm://") {
        check_aws_secret_id(id)?;
        return Ok(OpaqueSecret::Reference(v));
    }
    Ok(OpaqueSecret::Plaintext(
        v.strip_prefix(ENCRYPT_DIRECTIVE).unwrap_or(v),
    ))
}

/// `aws-sm://<secret name or ARN>`: what `GetSecretValue` accepts as a
/// `SecretId`. A name is 1 to 2048 of `A-Z a-z 0-9 / _ + = . @ -`; an ARN is
/// `arn:<partition>:secretsmanager:<region>:<account>:secret:<name>`.
fn check_aws_secret_id(id: &str) -> Result<(), InvalidSecretRef> {
    let malformed = |why: &str| {
        Err(InvalidSecretRef(format!(
            "aws-sm:// reference is malformed: {why}; expected aws-sm://<secret name or ARN>"
        )))
    };
    if id.is_empty() {
        return malformed("it names no secret");
    }
    if id.len() > AWS_SECRET_ID_MAX {
        return malformed("a secret id is at most 2048 characters");
    }
    let name_char = |c: char| c.is_ascii_alphanumeric() || "/_+=.@-".contains(c);
    if let Some(arn) = id.strip_prefix("arn:") {
        let parts: Vec<&str> = arn.splitn(6, ':').collect();
        let well_formed = parts.len() == 6
            && parts[..4].iter().all(|p| !p.is_empty())
            && parts[1] == "secretsmanager"
            && parts[4] == "secret"
            && parts[0..4]
                .iter()
                .all(|p| p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            && !parts[5].is_empty()
            && parts[5].chars().all(name_char);
        if !well_formed {
            return malformed(
                "an ARN is arn:<partition>:secretsmanager:<region>:<account>:secret:<name>",
            );
        }
        return Ok(());
    }
    if !id.chars().all(name_char) {
        return malformed("a secret name has only letters, digits and / _ + = . @ -");
    }
    Ok(())
}

/// A secret manager that answers one scheme's references.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// The secret stored under `key` (the reference without its scheme).
    async fn get(&self, key: &str) -> Result<String, SecretRefError>;
}

/// `env://VAR`: a process environment variable (Go's `EnvProvider`); an
/// unset or empty variable is not found.
pub struct EnvSecretStore;

#[async_trait]
impl SecretStore for EnvSecretStore {
    async fn get(&self, key: &str) -> Result<String, SecretRefError> {
        match env::var(key) {
            Ok(v) if !v.is_empty() => Ok(v),
            _ => Err(SecretRefError::NotFound {
                scheme: "env".to_string(),
                key: key.to_string(),
            }),
        }
    }
}

/// `aws-sm://<secret id or ARN>`: the secret's string value from AWS Secrets
/// Manager, read with the host's AWS credentials (the default provider
/// chain). The client is built on first use.
pub struct AwsSecretsManagerStore {
    client: OnceCell<aws_sdk_secretsmanager::Client>,
}

impl AwsSecretsManagerStore {
    pub fn new() -> Self {
        Self {
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> &aws_sdk_secretsmanager::Client {
        self.client
            .get_or_init(|| async {
                let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
                aws_sdk_secretsmanager::Client::new(&config)
            })
            .await
    }
}

impl Default for AwsSecretsManagerStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SecretStore for AwsSecretsManagerStore {
    async fn get(&self, key: &str) -> Result<String, SecretRefError> {
        let out = self
            .client()
            .await
            .get_secret_value()
            .secret_id(key)
            .send()
            .await
            .map_err(|e| SecretRefError::Provider {
                scheme: "aws-sm".to_string(),
                key: key.to_string(),
                message: DisplayErrorContext(e).to_string(),
            })?;
        out.secret_string()
            .map(str::to_string)
            .ok_or_else(|| SecretRefError::NotFound {
                scheme: "aws-sm".to_string(),
                key: key.to_string(),
            })
    }
}

/// Opens stored secrets: decrypts `encrypted:` values, returns a `literal:`
/// value's plaintext, and resolves a secret-manager reference through the
/// store registered for its scheme, caching the answer.
pub struct SecretResolver {
    encryption: Option<Arc<EncryptionService>>,
    stores: HashMap<String, Arc<dyn SecretStore>>,
    cache: DashMap<String, (String, Instant)>,
    ttl: Duration,
}

impl SecretResolver {
    /// A resolver with no secret-manager stores: it opens `encrypted:` and
    /// `literal:` values only.
    pub fn new(encryption: Option<Arc<EncryptionService>>) -> Self {
        Self {
            encryption,
            stores: HashMap::new(),
            cache: DashMap::new(),
            ttl: DEFAULT_CACHE_TTL,
        }
    }

    /// The platform's resolver: `env://` and `aws-sm://` references resolve.
    pub fn platform(encryption: Option<Arc<EncryptionService>>) -> Self {
        Self::new(encryption)
            .with_store("env", Arc::new(EnvSecretStore))
            .with_store("aws-sm", Arc::new(AwsSecretsManagerStore::new()))
    }

    /// Resolve `scheme://` references through `store`.
    pub fn with_store(mut self, scheme: &str, store: Arc<dyn SecretStore>) -> Self {
        self.stores.insert(scheme.to_string(), store);
        self
    }

    /// How long a secret-manager answer is reused.
    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// The plaintext of a stored secret.
    pub async fn resolve(&self, stored: &str) -> Result<String, SecretRefError> {
        let v = stored.trim();
        if let Some(plain) = v.strip_prefix(LITERAL_PREFIX) {
            return Ok(plain.to_string());
        }
        if v.starts_with(ENCRYPTED_PREFIX) {
            let enc = self
                .encryption
                .as_deref()
                .ok_or(SecretRefError::NotConfigured)?;
            return Ok(enc.decrypt_ref(v)?);
        }
        let Some((scheme, key)) = v.split_once("://") else {
            return Err(SecretRefError::NotAReference);
        };
        if !is_scheme_token(scheme) {
            return Err(SecretRefError::NotAReference);
        }
        let store = self
            .stores
            .get(scheme)
            .ok_or_else(|| SecretRefError::NoProvider(scheme.to_string()))?;
        if let Some(hit) = self.cache.get(v) {
            if hit.1.elapsed() < self.ttl {
                return Ok(hit.0.clone());
            }
        }
        let secret = store.get(key).await?;
        self.cache
            .insert(v.to_string(), (secret.clone(), Instant::now()));
        Ok(secret)
    }

    /// [`Self::resolve`], except that when a secret manager fails on a
    /// reference this resolver has read before, it answers the last value it
    /// read, however old, rather than the error. For a caller to whom a
    /// missing secret does more harm than a stale one: a function host
    /// handed a document without the secret reloads the function without
    /// it. The failure is logged; the value never is.
    pub async fn resolve_or_last_known(&self, stored: &str) -> Result<String, SecretRefError> {
        match self.resolve(stored).await {
            Err(e @ (SecretRefError::NotFound { .. } | SecretRefError::Provider { .. })) => {
                let Some(last) = self.cache.get(stored.trim()) else {
                    return Err(e);
                };
                // A caller on a poll path asks again every few seconds.
                static STALE: LogThrottle = LogThrottle::new(Duration::from_secs(60));
                if let Some(suppressed) = STALE.admit() {
                    tracing::warn!(
                        error = %e,
                        age_secs = last.1.elapsed().as_secs(),
                        suppressed,
                        "secret manager lookup failed; using the last value read"
                    );
                }
                Ok(last.0.clone())
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn enc() -> EncryptionService {
        EncryptionService::new(&EncryptionService::generate_key()).unwrap()
    }

    /// A secret manager that counts its lookups; never touches AWS.
    struct FakeStore {
        values: HashMap<String, String>,
        calls: AtomicUsize,
    }

    impl FakeStore {
        fn with(key: &str, value: &str) -> Arc<Self> {
            Arc::new(Self {
                values: HashMap::from([(key.to_string(), value.to_string())]),
                calls: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait]
    impl SecretStore for FakeStore {
        async fn get(&self, key: &str) -> Result<String, SecretRefError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.values
                .get(key)
                .cloned()
                .ok_or_else(|| SecretRefError::NotFound {
                    scheme: "fake".into(),
                    key: key.into(),
                })
        }
    }

    #[test]
    fn references_and_ciphertext_are_stored_as_sent() {
        let e = enc();
        for v in [
            "aws-sm://prod/idp/entra",
            "aws-ps:///fc/idp",
            "gcp-sm://projects/p/secrets/s",
            "vault://kv/idp#secret",
            "env://IDP_SECRET",
            "literal:dev-secret",
            "encrypted:AAAA",
        ] {
            assert_eq!(seal_secret_ref(Some(&e), v).unwrap(), v);
            assert_eq!(seal_secret_ref(None, &format!("  {v} ")).unwrap(), v);
        }
    }

    #[test]
    fn a_plaintext_is_encrypted_and_needs_a_key() {
        let e = enc();
        let sealed = seal_secret_ref(Some(&e), "s3cret").unwrap();
        assert!(sealed.starts_with("encrypted:"));
        assert_eq!(e.decrypt_ref(&sealed).unwrap(), "s3cret");
        assert!(matches!(
            seal_secret_ref(None, "s3cret"),
            Err(SecretRefError::NotConfigured)
        ));
        // The directive forces encryption of a URL-shaped secret.
        let sealed = seal_secret_ref(Some(&e), "encrypt:https://x.example/s").unwrap();
        assert_eq!(e.decrypt_ref(&sealed).unwrap(), "https://x.example/s");
        // A value that merely contains "://" is a secret, not a reference.
        let sealed = seal_secret_ref(Some(&e), "p@ss://word").unwrap();
        assert_eq!(e.decrypt_ref(&sealed).unwrap(), "p@ss://word");
    }

    #[test]
    fn an_unknown_scheme_is_refused_naming_the_supported_ones() {
        let err = seal_secret_ref(Some(&enc()), "aws-smm://prod/idp").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("\"aws-smm://\""), "{msg}");
        assert!(
            msg.contains("aws-sm://, aws-ps://, gcp-sm://, vault://, env://"),
            "{msg}"
        );
        assert!(!msg.contains("literal:"), "{msg}");
    }

    /// Owner decision #54: what an opaque secret's value is stored as.
    #[test]
    fn an_opaque_secret_keeps_an_aws_reference_or_ciphertext_and_encrypts_the_rest() {
        let kept = |v: &str| match classify_opaque_secret(v).unwrap() {
            OpaqueSecret::Reference(r) => format!("ref:{r}"),
            OpaqueSecret::Encrypted(r) => format!("enc:{r}"),
            OpaqueSecret::Plaintext(p) => format!("plain:{p}"),
        };
        // An AWS Secrets Manager reference, a name or an ARN, trimmed.
        assert_eq!(
            kept("aws-sm://prod/orders-db"),
            "ref:aws-sm://prod/orders-db"
        );
        assert_eq!(
            kept("  aws-sm://a_b+c=d.e@f-g \n"),
            "ref:aws-sm://a_b+c=d.e@f-g"
        );
        let arn = "aws-sm://arn:aws:secretsmanager:eu-west-1:123456789012:secret:prod/db-AbCdEf";
        assert_eq!(kept(arn), format!("ref:{arn}"));
        // An existing ciphertext, as sent.
        let sealed = enc().encrypt_ref("x").unwrap();
        assert_eq!(kept(&sealed), format!("enc:{sealed}"));
        // Everything else is the secret itself, a URL-shaped one included.
        for v in [
            "s3cret",
            "postgres://app:p%40ss@db.internal:5432/orders?sslmode=require",
            "jdbc:postgresql://db/orders?user=a&password=b",
            "https://hooks.example.com/x?token=abc",
            "env://FLOWCATALYST_APP_KEY",
            "aws-ps:///fc/db",
            "vault://kv/x#f",
            "gcp-sm://projects/p/secrets/s",
            "aws-smm://prod/db",
            "AWS-SM://prod/db",
            "literal:dev",
            "p@ss://word",
        ] {
            assert_eq!(kept(v), format!("plain:{v}"), "{v}");
        }
        // The `encrypt:` directive (Java's) is stripped: what follows is
        // plaintext, even when it looks like a reference.
        assert_eq!(
            kept("encrypt:aws-sm://not/a/ref"),
            "plain:aws-sm://not/a/ref"
        );
        assert_eq!(kept("encrypt:encrypted:AAAA"), "plain:encrypted:AAAA");
        assert_eq!(kept(" encrypt:mysecret "), "plain:mysecret");
        assert_eq!(kept("encrypt:"), "plain:");
        assert_eq!(kept("   "), "plain:");
    }

    #[test]
    fn a_malformed_reference_or_ciphertext_is_refused_without_echoing_it() {
        let refused = |v: &str| classify_opaque_secret(v).unwrap_err().to_string();
        // Java's message for the base64 case.
        assert_eq!(
            refused("encrypted:not base64"),
            "encrypted: payload is not base64"
        );
        // Base64 but no envelope: classified, and refused by the caller
        // that holds the key (ENCRYPTED_DOES_NOT_DECRYPT).
        assert!(matches!(
            classify_opaque_secret("encrypted:QUJD"),
            Ok(OpaqueSecret::Encrypted("encrypted:QUJD"))
        ));
        assert_eq!(refused("encrypted:QUI"), "encrypted: payload is not base64");
        assert_eq!(refused("encrypted:"), "encrypted: payload is empty");
        let expected = "; expected aws-sm://<secret name or ARN>";
        for (v, why) in [
            ("aws-sm://", "it names no secret"),
            ("aws-sm://  ", "it names no secret"),
            (
                "aws-sm://prod/db password",
                "a secret name has only letters, digits and / _ + = . @ -",
            ),
            (
                "aws-sm://prod/db#field",
                "a secret name has only letters, digits and / _ + = . @ -",
            ),
            (
                "aws-sm://arn:aws:secretsmanager:eu-west-1:1234:prod/db",
                "an ARN is arn:<partition>:secretsmanager:<region>:<account>:secret:<name>",
            ),
            (
                "aws-sm://arn:aws:ssm:eu-west-1:1234:secret:prod/db",
                "an ARN is arn:<partition>:secretsmanager:<region>:<account>:secret:<name>",
            ),
            (
                "aws-sm://arn:aws:secretsmanager::1234:secret:prod",
                "an ARN is arn:<partition>:secretsmanager:<region>:<account>:secret:<name>",
            ),
            (
                "aws-sm://arn:aws:secretsmanager:eu-west-1:1234:secret:",
                "an ARN is arn:<partition>:secretsmanager:<region>:<account>:secret:<name>",
            ),
        ] {
            assert_eq!(
                refused(v),
                format!("aws-sm:// reference is malformed: {why}{expected}"),
                "{v}"
            );
        }
        let long = format!("aws-sm://{}", "a".repeat(2049));
        assert_eq!(
            refused(&long),
            format!("aws-sm:// reference is malformed: a secret id is at most 2048 characters{expected}")
        );
        assert!(classify_opaque_secret(&format!("aws-sm://{}", "a".repeat(2048))).is_ok());
        assert!(!refused("aws-sm://prod/hunter2 x").contains("hunter2"));
    }

    #[test]
    fn opaque_debug_hides_the_value() {
        assert_eq!(
            format!("{:?}", OpaqueSecret::Plaintext("hunter2")),
            "Plaintext(***)"
        );
        assert_eq!(
            format!("{:?}", OpaqueSecret::Encrypted("encrypted:AAAA")),
            "Encrypted(***)"
        );
    }

    #[tokio::test]
    async fn the_last_value_read_outlives_a_failing_secret_manager() {
        struct Flaky {
            fail: AtomicBool,
        }
        #[async_trait]
        impl SecretStore for Flaky {
            async fn get(&self, key: &str) -> Result<String, SecretRefError> {
                if self.fail.load(Ordering::SeqCst) {
                    return Err(SecretRefError::Provider {
                        scheme: "aws-sm".into(),
                        key: key.into(),
                        message: "throttled".into(),
                    });
                }
                Ok("v1".into())
            }
        }
        let store = Arc::new(Flaky {
            fail: AtomicBool::new(false),
        });
        let r = SecretResolver::new(None)
            .with_store("aws-sm", store.clone())
            .with_cache_ttl(Duration::ZERO);
        // Never read: the failure is the answer.
        store.fail.store(true, Ordering::SeqCst);
        assert!(r.resolve_or_last_known("aws-sm://k").await.is_err());
        store.fail.store(false, Ordering::SeqCst);
        assert_eq!(r.resolve_or_last_known("aws-sm://k").await.unwrap(), "v1");
        store.fail.store(true, Ordering::SeqCst);
        assert!(
            r.resolve("aws-sm://k").await.is_err(),
            "resolve itself is strict"
        );
        assert_eq!(r.resolve_or_last_known(" aws-sm://k ").await.unwrap(), "v1");
        // A form with no secret manager behind it is never papered over.
        assert!(matches!(
            r.resolve_or_last_known("vault://x").await,
            Err(SecretRefError::NoProvider(_))
        ));
    }

    #[test]
    fn reference_detection() {
        assert!(looks_like_reference("aws-sm://x"));
        assert!(looks_like_reference("literal:x"));
        assert!(looks_like_reference("aws-smm://x"));
        assert!(!looks_like_reference("plain"));
        assert!(!looks_like_reference("p@ss://word"));
        assert!(!looks_like_reference("encrypted:AAAA"));
    }

    #[tokio::test]
    async fn resolves_each_stored_form() {
        let e = Arc::new(enc());
        let store = FakeStore::with("prod/idp", "from-sm");
        let r = SecretResolver::new(Some(e.clone())).with_store("aws-sm", store.clone());

        let sealed = e.encrypt_ref("from-key").unwrap();
        assert_eq!(r.resolve(&sealed).await.unwrap(), "from-key");
        assert_eq!(r.resolve("literal:dev").await.unwrap(), "dev");
        assert_eq!(r.resolve("aws-sm://prod/idp").await.unwrap(), "from-sm");
        assert!(matches!(
            r.resolve("vault://kv/x#f").await,
            Err(SecretRefError::NoProvider(s)) if s == "vault"
        ));
        assert!(matches!(
            r.resolve("plain").await,
            Err(SecretRefError::NotAReference)
        ));
        assert!(matches!(
            r.resolve("aws-sm://missing").await,
            Err(SecretRefError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn a_secret_manager_answer_is_cached_until_the_ttl() {
        let store = FakeStore::with("k", "v");
        let r = SecretResolver::new(None).with_store("aws-sm", store.clone());
        r.resolve("aws-sm://k").await.unwrap();
        r.resolve("aws-sm://k").await.unwrap();
        assert_eq!(store.calls.load(Ordering::SeqCst), 1);

        let r = SecretResolver::new(None)
            .with_store("aws-sm", store.clone())
            .with_cache_ttl(Duration::ZERO);
        r.resolve("aws-sm://k").await.unwrap();
        r.resolve("aws-sm://k").await.unwrap();
        assert_eq!(store.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_ciphertext_needs_the_key() {
        let sealed = enc().encrypt_ref("x").unwrap();
        assert!(matches!(
            SecretResolver::new(None).resolve(&sealed).await,
            Err(SecretRefError::NotConfigured)
        ));
    }

    #[tokio::test]
    async fn env_references_read_the_environment() {
        let r = SecretResolver::platform(None);
        env::set_var("FC_SECRET_REF_TEST_VAR", "from-env");
        assert_eq!(
            r.resolve("env://FC_SECRET_REF_TEST_VAR").await.unwrap(),
            "from-env"
        );
        assert!(r.resolve("env://FC_SECRET_REF_TEST_UNSET").await.is_err());
    }
}
