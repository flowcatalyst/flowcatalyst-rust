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
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;

use super::encryption_service::{EncryptionError, EncryptionService, ENCRYPTED_PREFIX};

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
        match std::env::var(key) {
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
    client: tokio::sync::OnceCell<aws_sdk_secretsmanager::Client>,
}

impl AwsSecretsManagerStore {
    pub fn new() -> Self {
        Self {
            client: tokio::sync::OnceCell::new(),
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
                message: aws_sdk_secretsmanager::error::DisplayErrorContext(e).to_string(),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
        std::env::set_var("FC_SECRET_REF_TEST_VAR", "from-env");
        assert_eq!(
            r.resolve("env://FC_SECRET_REF_TEST_VAR").await.unwrap(),
            "from-env"
        );
        assert!(r.resolve("env://FC_SECRET_REF_TEST_UNSET").await.is_err());
    }
}
