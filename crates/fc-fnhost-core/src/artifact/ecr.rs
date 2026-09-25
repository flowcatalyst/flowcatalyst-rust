//! AWS ECR authorization for private `oci://<account>.dkr.ecr.<region>
//! .amazonaws.com/<repository>` registries (owner decision #14,
//! `docs/owner-decisions-2026-09-25.md`).
//!
//! ECR answers an anonymous blob request with `401` and
//! `WWW-Authenticate: Basic realm="..."` — [`OciSource`](super::OciSource)
//! already runs the Basic-challenge branch of its normal flow, so the only
//! new thing an ECR host needs is a way to produce that Basic credential:
//! `ecr:GetAuthorizationToken` returns a base64 `user:password` pair, valid
//! 12 hours, on whatever IAM identity the host already runs as (the default
//! AWS credential chain — a task or instance role, no secret configured
//! anywhere in this crate). [`EcrTokenCache`] mints one per region and
//! reuses it until shortly before it expires; nothing here ever logs the
//! decoded password, the encoded token, or the `Basic` header.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use chrono::{DateTime, Utc};

use super::ArtifactError;
use crate::clock::SharedClock;

/// ECR tokens are valid 12 hours; refresh this long before the real expiry
/// so a token already in flight never goes stale mid-pull.
const EXPIRY_MARGIN: chrono::Duration = chrono::Duration::minutes(5);

/// Mints a fresh ECR authorization token for a region. The only
/// implementation the host wires up is [`AwsEcrAuthorizer`] (behind the
/// `ecr` feature); tests fake this trait instead of the AWS SDK.
#[async_trait]
pub trait EcrAuthorizer: Send + Sync {
    /// The decoded `(user, password)` pair for a fresh token in `region`,
    /// and when it expires.
    async fn authorize(
        &self,
        region: &str,
    ) -> Result<(String, String, DateTime<Utc>), ArtifactError>;
}

/// Caches one `Basic` auth header per ECR region, minted through an
/// [`EcrAuthorizer`] and reused until shortly before it expires.
pub struct EcrTokenCache {
    authorizer: Arc<dyn EcrAuthorizer>,
    clock: SharedClock,
    /// Held across a mint for a given region, so concurrent callers for
    /// that region mint once. A mint for one region does not block another.
    cached: tokio::sync::Mutex<HashMap<String, (String, DateTime<Utc>)>>,
}

impl EcrTokenCache {
    pub fn new(authorizer: Arc<dyn EcrAuthorizer>, clock: SharedClock) -> Self {
        Self {
            authorizer,
            clock,
            cached: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// The cached `Basic <base64>` header for `region`, minting a fresh
    /// token when there is none cached or it is within 5 minutes of
    /// expiring.
    pub async fn basic_header(&self, region: &str) -> Result<String, ArtifactError> {
        let mut cached = self.cached.lock().await;
        if let Some((header, expiry)) = cached.get(region) {
            if self.clock.now() < *expiry - EXPIRY_MARGIN {
                return Ok(header.clone());
            }
        }
        let (user, password, expiry) = self.authorizer.authorize(region).await?;
        let raw = format!("{user}:{password}");
        let header = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(raw)
        );
        cached.insert(region.to_owned(), (header.clone(), expiry));
        Ok(header)
    }
}

impl fmt::Debug for EcrTokenCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EcrTokenCache[headers=***]")
    }
}

/// The AWS region encoded in a private ECR registry hostname —
/// `<account>.dkr.ecr[-fips].<region>.amazonaws.com[.cn]` — or `None` when
/// the host isn't shaped like one (every other registry, including public
/// ECR's `public.ecr.aws`, which is a different, always-anonymous service).
pub fn ecr_region(host: &str) -> Option<String> {
    let lower = host.to_ascii_lowercase();
    let rest = lower
        .strip_suffix(".amazonaws.com.cn")
        .or_else(|| lower.strip_suffix(".amazonaws.com"))?;
    let mut labels = rest.split('.');
    let account = labels.next().unwrap_or("");
    let dkr = labels.next().unwrap_or("");
    let service = labels.next().unwrap_or("");
    let region = labels.next().unwrap_or("");
    let extra = labels.next();
    let shaped = !account.is_empty()
        && dkr == "dkr"
        && (service == "ecr" || service == "ecr-fips")
        && !region.is_empty()
        && extra.is_none();
    shaped.then(|| region.to_owned())
}

#[cfg(feature = "ecr")]
mod aws_authorizer {
    use super::*;
    use tokio::sync::OnceCell;

    /// Mints tokens via `ecr:GetAuthorizationToken`, on the default AWS
    /// credential chain — the host's own task or instance role. Built from
    /// the environment lazily, once, on first use.
    pub struct AwsEcrAuthorizer {
        config: OnceCell<aws_config::SdkConfig>,
    }

    impl Default for AwsEcrAuthorizer {
        fn default() -> Self {
            Self {
                config: OnceCell::new(),
            }
        }
    }

    impl AwsEcrAuthorizer {
        async fn base_config(&self) -> &aws_config::SdkConfig {
            self.config
                .get_or_init(|| aws_config::load_defaults(aws_config::BehaviorVersion::latest()))
                .await
        }
    }

    #[async_trait]
    impl EcrAuthorizer for AwsEcrAuthorizer {
        async fn authorize(
            &self,
            region: &str,
        ) -> Result<(String, String, DateTime<Utc>), ArtifactError> {
            let base = self.base_config().await;
            let conf = aws_sdk_ecr::config::Builder::from(base)
                .region(aws_sdk_ecr::config::Region::new(region.to_owned()))
                .build();
            let client = aws_sdk_ecr::Client::from_conf(conf);
            let output = client.get_authorization_token().send().await.map_err(|e| {
                ArtifactError::Transport(format!("ECR GetAuthorizationToken: {e:?}"))
            })?;
            let data = output.authorization_data().first().ok_or_else(|| {
                ArtifactError::Transport("ECR returned no authorization data".into())
            })?;
            let token = data.authorization_token().ok_or_else(|| {
                ArtifactError::Transport("ECR authorization data carried no token".into())
            })?;
            let (user, password) = decode_token(token)?;
            let expiry = data
                .expires_at()
                .and_then(|t| DateTime::from_timestamp(t.secs(), 0))
                // ECR tokens are valid 12h; fall back to a conservative
                // estimate if the SDK ever omits the field.
                .unwrap_or_else(|| Utc::now() + chrono::Duration::hours(11));
            Ok((user, password, expiry))
        }
    }

    /// `base64(user:password)` -> `(user, password)`.
    fn decode_token(token: &str) -> Result<(String, String), ArtifactError> {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(token)
            .map_err(ArtifactError::transport)?;
        let text = String::from_utf8(decoded).map_err(ArtifactError::transport)?;
        let (user, password) = text
            .split_once(':')
            .ok_or_else(|| ArtifactError::Transport("ECR token was not user:password".into()))?;
        Ok((user.to_owned(), password.to_owned()))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn decodes_a_base64_user_password_pair() {
            let token = base64::engine::general_purpose::STANDARD.encode("AWS:s3cr3t-p4ss");
            let (user, password) = decode_token(&token).unwrap();
            assert_eq!(user, "AWS");
            assert_eq!(password, "s3cr3t-p4ss");
        }

        #[test]
        fn rejects_a_token_with_no_colon() {
            let token = base64::engine::general_purpose::STANDARD.encode("not-a-pair");
            assert!(decode_token(&token).is_err());
        }

        #[test]
        fn rejects_non_base64() {
            assert!(decode_token("not base64!!").is_err());
        }
    }
}

#[cfg(feature = "ecr")]
pub use aws_authorizer::AwsEcrAuthorizer;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{Clock, ManualClock};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn matches_the_standard_regional_host() {
        assert_eq!(
            ecr_region("123456789012.dkr.ecr.us-east-1.amazonaws.com"),
            Some("us-east-1".to_owned())
        );
    }

    #[test]
    fn matches_case_insensitively() {
        assert_eq!(
            ecr_region("123456789012.DKR.ECR.us-east-1.AMAZONAWS.COM"),
            Some("us-east-1".to_owned())
        );
    }

    #[test]
    fn matches_the_china_partition() {
        assert_eq!(
            ecr_region("123456789012.dkr.ecr.cn-north-1.amazonaws.com.cn"),
            Some("cn-north-1".to_owned())
        );
    }

    #[test]
    fn matches_the_fips_variant() {
        assert_eq!(
            ecr_region("123456789012.dkr.ecr-fips.us-gov-west-1.amazonaws.com"),
            Some("us-gov-west-1".to_owned())
        );
    }

    #[test]
    fn rejects_other_registries() {
        assert_eq!(ecr_region("ghcr.io"), None);
        assert_eq!(ecr_region("localhost"), None);
        assert_eq!(ecr_region("docker.io"), None);
        // Public ECR is a different, always-anonymous service.
        assert_eq!(ecr_region("public.ecr.aws"), None);
        // No account label.
        assert_eq!(ecr_region("dkr.ecr.us-east-1.amazonaws.com"), None);
        // Wrong middle label.
        assert_eq!(
            ecr_region("123456789012.dkr.s3.us-east-1.amazonaws.com"),
            None
        );
        // An extra label the shape doesn't allow.
        assert_eq!(
            ecr_region("x.123456789012.dkr.ecr.us-east-1.amazonaws.com"),
            None
        );
    }

    struct FakeAuthorizer {
        calls: AtomicUsize,
        user: &'static str,
        password: &'static str,
        expires_in: chrono::Duration,
        clock: ManualClock,
    }

    #[async_trait]
    impl EcrAuthorizer for FakeAuthorizer {
        async fn authorize(
            &self,
            _region: &str,
        ) -> Result<(String, String, DateTime<Utc>), ArtifactError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok((
                self.user.to_owned(),
                self.password.to_owned(),
                self.clock.now() + self.expires_in,
            ))
        }
    }

    #[tokio::test]
    async fn caches_the_header_until_five_minutes_before_expiry() {
        let clock = ManualClock::new(Utc::now());
        let fake = Arc::new(FakeAuthorizer {
            calls: AtomicUsize::new(0),
            user: "AWS",
            password: "tok3n-1",
            expires_in: chrono::Duration::hours(12),
            clock: clock.clone(),
        });
        let cache = EcrTokenCache::new(fake.clone(), Arc::new(clock.clone()));

        let first = cache.basic_header("us-east-1").await.unwrap();
        assert_eq!(
            first,
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("AWS:tok3n-1")
            )
        );
        assert_eq!(cache.basic_header("us-east-1").await.unwrap(), first);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);

        clock.advance(chrono::Duration::hours(11) + chrono::Duration::minutes(54));
        assert_eq!(cache.basic_header("us-east-1").await.unwrap(), first);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);

        clock.advance(chrono::Duration::minutes(2));
        let refreshed = cache.basic_header("us-east-1").await.unwrap();
        assert_eq!(refreshed, first);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn caches_independently_per_region() {
        let clock = ManualClock::new(Utc::now());
        let fake = Arc::new(FakeAuthorizer {
            calls: AtomicUsize::new(0),
            user: "AWS",
            password: "tok3n",
            expires_in: chrono::Duration::hours(12),
            clock: clock.clone(),
        });
        let cache = EcrTokenCache::new(fake.clone(), Arc::new(clock));
        cache.basic_header("us-east-1").await.unwrap();
        cache.basic_header("eu-west-1").await.unwrap();
        cache.basic_header("us-east-1").await.unwrap();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn debug_never_shows_a_header() {
        let clock = ManualClock::new(Utc::now());
        let fake = Arc::new(FakeAuthorizer {
            calls: AtomicUsize::new(0),
            user: "AWS",
            password: "super-secret-password",
            expires_in: chrono::Duration::hours(12),
            clock: clock.clone(),
        });
        let cache = EcrTokenCache::new(fake, Arc::new(clock));
        cache.basic_header("us-east-1").await.unwrap();
        assert!(!format!("{cache:?}").contains("super-secret-password"));
    }
}
